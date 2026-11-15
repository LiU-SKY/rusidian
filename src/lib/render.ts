// Markdown → sanitized HTML for the reading view, with Obsidian syntax:
// [[links]], ![[embeds]], ==highlights==, %%comments%%, $math$, footnotes.
// Tags, callouts, task checkboxes, embeds, math and Mermaid diagrams are
// finished on the DOM after sanitizing (see Reading.svelte).

import DOMPurify from "dompurify";
import { Marked, type TokenizerAndRendererExtension, type Tokens } from "marked";

const esc = (s: string) =>
  s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

export interface WikiParts {
  target: string;
  /** `#Heading` / `#^block` part without the first `#`, or "". */
  sub: string;
  alias: string | null;
}

/** `Note#Heading|alias` (a `\|` from inside a table counts as the pipe). */
export function parseWiki(inner: string): WikiParts {
  const [left, ...rest] = inner.replace(/\\\|/g, "|").split("|");
  const alias = rest.length ? rest.join("|").trim() : null;
  const hash = left.indexOf("#");
  const target = (hash < 0 ? left : left.slice(0, hash)).trim();
  const sub = hash < 0 ? "" : left.slice(hash + 1).trim();
  return { target, sub, alias: alias || null };
}

/** What a link shows: the alias, else `Note > Heading` like Obsidian. */
export function wikiLabel({ target, sub, alias }: WikiParts): string {
  if (alias) return alias;
  const heading = sub.replace(/#/g, " > ");
  return target && heading ? `${target} > ${heading}` : target || heading;
}

const wikilink: TokenizerAndRendererExtension = {
  name: "wikilink",
  level: "inline",
  start(src) {
    const i = src.indexOf("[[");
    if (i < 0) return undefined;
    return i > 0 && src[i - 1] === "!" ? i - 1 : i;
  },
  tokenizer(src) {
    const m = /^(!?)\[\[([^[\]\n]+?)\]\]/.exec(src);
    if (!m) return undefined;
    return { type: "wikilink", raw: m[0], embed: m[1] === "!", parts: parseWiki(m[2]) };
  },
  renderer(token) {
    const { embed, parts } = token as unknown as { embed: boolean; parts: WikiParts };
    const data = `data-target="${esc(parts.target)}" data-sub="${esc(parts.sub)}"`;
    if (embed) return `<span class="embed" ${data} data-alias="${esc(parts.alias ?? "")}"></span>`;
    return `<a class="internal-link" href="#" ${data}>${esc(wikiLabel(parts))}</a>`;
  },
};

const highlight: TokenizerAndRendererExtension = {
  name: "highlight",
  level: "inline",
  start: (src) => {
    const i = src.indexOf("==");
    return i < 0 ? undefined : i;
  },
  tokenizer(src) {
    const m = /^==(?=\S)([^\n]*?\S)==/.exec(src);
    if (!m) return undefined;
    return { type: "highlight", raw: m[0], tokens: this.lexer.inlineTokens(m[1]) };
  },
  renderer(token) {
    return `<mark>${this.parser.parseInline((token as Tokens.Generic).tokens ?? [])}</mark>`;
  },
};

/** `%%hidden%%` comments, inline or spanning lines. */
const commentInline: TokenizerAndRendererExtension = {
  name: "comment",
  level: "inline",
  start: (src) => {
    const i = src.indexOf("%%");
    return i < 0 ? undefined : i;
  },
  tokenizer(src) {
    const m = /^%%[\s\S]*?%%/.exec(src);
    return m ? { type: "comment", raw: m[0] } : undefined;
  },
  renderer: () => "",
};
const commentBlock: TokenizerAndRendererExtension = {
  name: "commentBlock",
  level: "block",
  // Only a comment that starts a line and ends its last line; anything
  // else (`text %%x%% more`) is left to the inline rule.
  start: (src) => {
    const m = /(^|\n)%%/.exec(src);
    return m ? m.index + m[1].length : undefined;
  },
  tokenizer(src) {
    const m = /^%%[\s\S]*?%%[ \t]*(?:\n|$)/.exec(src);
    return m ? { type: "commentBlock", raw: m[0] } : undefined;
  },
  renderer: () => "",
};

// ------------------------------------------------------------------ math
// Only placeholders here (TeX in data-tex); KaTeX fills them in after
// sanitizing, so its markup never goes through the sanitizer.

const mathBlock: TokenizerAndRendererExtension = {
  name: "mathBlock",
  level: "block",
  start: (src) => {
    const i = src.indexOf("$$");
    return i < 0 ? undefined : i;
  },
  tokenizer(src) {
    const m = /^\$\$([\s\S]+?)\$\$[ \t]*(?:\n|$)/.exec(src);
    if (m) return { type: "mathBlock", raw: m[0], text: m[1].trim() };
  },
  renderer: (t) => `<div class="math" data-display="1" data-tex="${esc((t as Tokens.Generic).text)}"></div>\n`,
};

const mathInline: TokenizerAndRendererExtension = {
  name: "mathInline",
  level: "inline",
  start: (src) => {
    const i = src.indexOf("$");
    return i < 0 ? undefined : i;
  },
  tokenizer(src) {
    // `$$…$$` inside a line is display math too.
    const d = /^\$\$([^\n]+?)\$\$/.exec(src);
    if (d) return { type: "mathInline", raw: d[0], text: d[1].trim(), display: true };
    const m = /^\$(?![\s$])((?:\\.|[^$\\\n])+?)(?<![\s\\])\$(?![\d$])/.exec(src);
    if (m) return { type: "mathInline", raw: m[0], text: m[1], display: false };
  },
  renderer: (t) => {
    const tok = t as Tokens.Generic;
    return tok.display
      ? `<span class="math" data-display="1" data-tex="${esc(tok.text)}"></span>`
      : `<span class="math" data-tex="${esc(tok.text)}"></span>`;
  },
};

// ------------------------------------------------------------- footnotes
// `text[^note]` + a `[^note]: …` line anywhere, or inline `^[…]`. Numbered
// in the order they are first referenced, listed at the end of the note.

const footnotes = { defs: new Map<string, string>(), order: [] as string[], inline: 0 };
const footnoteNumber = (id: string) => {
  let n = footnotes.order.indexOf(id);
  if (n < 0) n = footnotes.order.push(id) - 1;
  return n + 1;
};
const footnoteRefHtml = (n: number) =>
  `<sup class="fn-ref" id="fnref-${n}"><a href="#fn-${n}" data-fn="${n}">${n}</a></sup>`;

const footnoteDef: TokenizerAndRendererExtension = {
  name: "footnoteDef",
  level: "block",
  start: (src) => {
    const i = src.search(/^\[\^/m);
    return i < 0 ? undefined : i;
  },
  tokenizer(src) {
    // The definition, plus indented lines that continue it.
    const m = /^\[\^([^\]\s]+)\]:[ \t]*([^\n]*(?:\n(?: {2,}|\t)[^\n]*)*)(?:\n|$)/.exec(src);
    if (!m) return;
    footnotes.defs.set(m[1], m[2].replace(/\n[ \t]+/g, "\n").trim());
    return { type: "footnoteDef", raw: m[0] };
  },
  renderer: () => "",
};

const footnoteRef: TokenizerAndRendererExtension = {
  name: "footnoteRef",
  level: "inline",
  start: (src) => {
    const i = src.search(/\^?\[\^?/);
    return i < 0 ? undefined : i;
  },
  tokenizer(src) {
    const m = /^\[\^([^\]\s]+)\](?!:)/.exec(src);
    if (m) return { type: "footnoteRef", raw: m[0], id: m[1] };
    const inline = /^\^\[([^\]\n]+)\]/.exec(src);
    if (inline) {
      const id = `inline-${++footnotes.inline}`;
      footnotes.defs.set(id, inline[1]);
      return { type: "footnoteRef", raw: inline[0], id };
    }
  },
  renderer: (t) => footnoteRefHtml(footnoteNumber((t as Tokens.Generic).id)),
};

const marked = new Marked({
  gfm: true,
  breaks: true, // Obsidian keeps single line breaks ("strict line breaks" off)
  extensions: [commentBlock, commentInline, mathBlock, footnoteDef, wikilink, highlight, mathInline, footnoteRef],
});

/** The footnote list for what was just rendered (referenced ones first, in order). */
function footnoteSection(): string {
  const ids = [...footnotes.order, ...[...footnotes.defs.keys()].filter((id) => !footnotes.order.includes(id))];
  if (ids.length === 0) return "";
  const items = ids.map((id) => {
    const n = footnoteNumber(id);
    const text = footnotes.defs.get(id);
    const body = text === undefined ? `<span class="fn-missing">[^${esc(id)}] 정의가 없습니다</span>` : (marked.parseInline(text, { async: false }) as string);
    return `<li id="fn-${n}">${body} <a href="#fnref-${n}" class="fn-back" data-fnback="${n}" aria-label="본문으로">↩</a></li>`;
  });
  return `<section class="footnotes"><ol>${items.join("")}</ol></section>`;
}

/** Sanitized HTML: notes may contain raw HTML, and this page can reach the app's commands. */
export function renderMarkdown(md: string): string {
  footnotes.defs.clear();
  footnotes.order = [];
  footnotes.inline = 0;
  const html = (marked.parse(md, { async: false }) as string) + footnoteSection();
  return DOMPurify.sanitize(html, { ADD_ATTR: ["target"], FORBID_TAGS: ["style", "form"] });
}

// ------------------------------------------------------------- DOM helpers

const TAG_RE = /(^|\s)#([\p{L}\p{N}_/-]*[\p{L}_/-][\p{L}\p{N}_/-]*)/gu;

/** Turn `#tag` text into tag links (not inside code or links). */
export function linkTags(root: HTMLElement) {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
    acceptNode: (n) =>
      n.parentElement?.closest("code, pre, a, .callout-title-text") || !n.nodeValue?.includes("#")
        ? NodeFilter.FILTER_REJECT
        : NodeFilter.FILTER_ACCEPT,
  });
  const nodes: Text[] = [];
  while (walker.nextNode()) nodes.push(walker.currentNode as Text);
  for (const node of nodes) {
    const text = node.nodeValue!;
    const frag = document.createDocumentFragment();
    let last = 0;
    for (const m of text.matchAll(TAG_RE)) {
      const start = m.index! + m[1].length;
      frag.append(text.slice(last, start));
      const a = document.createElement("a");
      a.className = "tag";
      a.href = "#";
      a.dataset.tag = m[2];
      a.textContent = `#${m[2]}`;
      frag.append(a);
      last = start + 1 + m[2].length;
    }
    if (last === 0) continue;
    frag.append(text.slice(last));
    node.replaceWith(frag);
  }
}

/** `> [!type]± Title` blockquotes → Obsidian callouts (foldable with + / -). */
export function makeCallouts(root: HTMLElement) {
  for (const quote of root.querySelectorAll("blockquote")) {
    const first = quote.firstElementChild;
    if (!(first instanceof HTMLParagraphElement)) continue;
    const m = /^\[!([\w-]+)\]([+-]?)[ \t]*([^\n<]*)(?:<br>)?\n?/.exec(first.innerHTML);
    if (!m) continue;
    const type = m[1].toLowerCase();
    const fold = m[2];
    first.innerHTML = first.innerHTML.slice(m[0].length);
    if (!first.innerHTML.trim()) first.remove();

    const box = document.createElement(fold ? "details" : "div");
    box.className = "callout";
    box.dataset.callout = type;
    if (fold === "+") box.setAttribute("open", "");
    const title = document.createElement(fold ? "summary" : "div");
    title.className = "callout-title";
    const titleText = document.createElement("span");
    titleText.className = "callout-title-text";
    titleText.innerHTML = m[3].trim() || type.charAt(0).toUpperCase() + type.slice(1);
    title.append(titleText);
    const body = document.createElement("div");
    body.className = "callout-content";
    body.append(...quote.childNodes);
    box.append(title, body);
    quote.replaceWith(box);
  }
}

const TASK_LINE = /^\s*(?:>\s*)*(?:[-*+]|\d+[.)])\s+\[( |x|X)\](?:\s|$)/;

/** 0-based source lines of `- [ ]` / `- [x]` tasks, in reading order (skips code blocks). */
export function taskLines(text: string): number[] {
  const out: number[] = [];
  let fence: string | null = null;
  text.split(/\r?\n/).forEach((line, i) => {
    const f = /^ {0,3}(`{3,}|~{3,})/.exec(line);
    if (f) {
      if (fence === null) fence = f[1];
      else if (f[1][0] === fence[0] && f[1].length >= fence.length) fence = null;
      return;
    }
    if (fence === null && TASK_LINE.test(line)) out.push(i);
  });
  return out;
}
