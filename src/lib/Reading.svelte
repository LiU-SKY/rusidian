<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { displayName, mediaKind, readNote, resolveLink, vaultUrl } from "./api";
  import { bodyOf, findHeading, headings, sectionOf } from "./outline";
  import { linkTags, makeCallouts, renderMarkdown, taskLines } from "./render";
  import { renderMath, renderMermaid } from "./mathMermaid";
  import { settings } from "./settings.svelte";

  interface Props {
    content: string;
    /** Path of the note shown (links resolve from here). */
    path: string;
    /** A link was clicked; `source` is the note it is written in (differs inside embeds). */
    onlink: (target: string, heading: string | null, source: string) => void;
    ontag: (tag: string) => void;
    /** A task checkbox of this note was clicked (0-based source line). */
    ontoggle: (line: number) => void;
    /** Rendered for printing: light colours, nothing to click. */
    print?: boolean;
  }

  let { content, path, onlink, ontag, ontoggle, print = false }: Props = $props();

  /** `![[A]]` inside `![[B]]` inside the note: deeper embeds become links. */
  const MAX_DEPTH = 2;
  let host: HTMLElement | undefined = $state();
  let seq = 0;

  $effect(() => {
    const text = content;
    const source = path;
    settings.resolved; // diagrams take the theme's colours: redraw when it changes
    if (!host) return;
    const s = ++seq;
    host.innerHTML = renderMarkdown(bodyOf(text));
    enhance(host, source, 0, s);
    if (print) return;
    // Only this note's own checkboxes edit it (embedded ones are read-only).
    const lines = taskLines(text);
    host.querySelectorAll<HTMLInputElement>("input[type=checkbox]").forEach((box, i) => {
      if (box.closest(".embed")) return;
      if (lines[i] === undefined) return;
      box.disabled = false;
      box.dataset.line = String(lines[i]);
    });
  });

  /** Scroll to the heading on 0-based source `line`. */
  export function gotoLine(line: number) {
    if (!host) return;
    const index = headings(content).findIndex((h) => h.line === line);
    const els = host.querySelectorAll(":scope > :is(h1, h2, h3, h4, h5, h6)");
    els[index]?.scrollIntoView({ block: "start" });
  }

  const current = (s: number) => s === seq;

  /**
   * Resolves once math, diagrams, embeds and images have finished loading
   * (or after `timeout` ms), so a printout has them all.
   */
  export async function settled(timeout = 10_000) {
    const end = Date.now() + timeout;
    const busy = () =>
      !!host &&
      (!!host.querySelector(".math-pending, .mermaid-pending, span.embed[data-done]:not([class*='embed-'])") ||
        !!host.querySelector("img[data-loading]") ||
        [...host.querySelectorAll("img")].some((img) => img.src && !img.complete));
    await new Promise((r) => setTimeout(r, 50));
    while (busy() && Date.now() < end) await new Promise((r) => setTimeout(r, 100));
  }

  /** Tags, callouts, local images and embeds for HTML rendered from `source`. */
  function enhance(root: HTMLElement, source: string, depth: number, s: number) {
    makeCallouts(root);
    linkTags(root);
    // After sanitizing: KaTeX and Mermaid draw from the source text themselves.
    for (const el of root.querySelectorAll<HTMLElement>(".math[data-tex]")) {
      renderMath(el, el.dataset.tex ?? "", el.dataset.display === "1");
      el.removeAttribute("data-tex");
    }
    for (const code of root.querySelectorAll<HTMLElement>("pre > code.language-mermaid")) {
      const box = document.createElement("div");
      code.parentElement!.replaceWith(box);
      renderMermaid(box, code.textContent ?? "", print);
    }
    for (const a of root.querySelectorAll<HTMLAnchorElement>("a.internal-link")) a.dataset.source = source;
    for (const a of root.querySelectorAll<HTMLAnchorElement>("a[href]:not(.internal-link):not(.tag)")) {
      const href = a.getAttribute("href") ?? "";
      if (/^[a-z][a-z0-9+.-]*:/i.test(href)) a.classList.add("external-link");
      else if (href && !href.startsWith("#")) {
        // [text](Other%20Note.md): an internal link written markdown-style.
        const [target, ...sub] = safeDecode(href).split("#");
        a.classList.add("internal-link");
        a.dataset.target = target;
        a.dataset.sub = sub.join("#");
        a.dataset.source = source;
      }
    }
    for (const img of root.querySelectorAll<HTMLImageElement>("img[src]")) {
      const src = img.getAttribute("src") ?? "";
      if (/^(https?:|data:|blob:)/i.test(src)) continue;
      img.removeAttribute("src");
      img.dataset.loading = "";
      resolveLink(source, safeDecode(src))
        .then((p) => {
          if (p && current(s)) img.src = vaultUrl(p);
        })
        .catch(() => {})
        .finally(() => delete img.dataset.loading);
    }
    for (const el of root.querySelectorAll<HTMLElement>("span.embed:not([data-done])")) {
      el.dataset.done = "";
      embed(el, source, depth, s);
    }
  }

  function safeDecode(s: string): string {
    try {
      return decodeURIComponent(s);
    } catch {
      return s;
    }
  }

  /** `|300` or `|300x200` sets an embedded image's size. */
  function applySize(el: HTMLElement, alias: string) {
    const m = /^(\d+)(?:x(\d+))?$/.exec(alias.trim());
    if (!m) return false;
    el.style.width = `${m[1]}px`;
    if (m[2]) el.style.height = `${m[2]}px`;
    return true;
  }

  async function embed(el: HTMLElement, source: string, depth: number, s: number) {
    const { target, sub, alias } = el.dataset as { target: string; sub: string; alias: string };
    const missing = (why: string) => {
      el.className = "embed embed-missing";
      el.textContent = why;
    };
    let file: string | null;
    try {
      file = target ? await resolveLink(source, target) : source;
    } catch {
      file = null;
    }
    if (!current(s)) return;
    if (!file) return missing(`"${target}" 파일이 없습니다`);

    const kind = mediaKind(file);
    const url = vaultUrl(file);
    if (kind === "image") {
      const img = document.createElement("img");
      img.src = url;
      img.alt = alias && !applySize(img, alias) ? alias : displayName(file.split("/").pop()!);
      el.replaceChildren(img);
      el.className = "embed embed-image";
      return;
    }
    if (kind === "audio" || kind === "video") {
      const media = document.createElement(kind);
      media.src = url;
      media.controls = true;
      el.replaceChildren(media);
      el.className = `embed embed-${kind}`;
      return;
    }
    if (kind === "pdf") {
      const frame = document.createElement("iframe");
      frame.src = sub ? `${url}#${sub}` : url;
      frame.title = file;
      el.replaceChildren(frame);
      el.className = "embed embed-pdf";
      return;
    }
    if (!file.toLowerCase().endsWith(".md")) return missing(`${file} 은(는) 미리 볼 수 없는 형식입니다`);

    // A note (or one heading / block of it).
    if (depth >= MAX_DEPTH || (file === path && !sub)) {
      const a = document.createElement("a");
      a.className = "internal-link";
      a.href = "#";
      Object.assign(a.dataset, { target, sub, source });
      a.textContent = alias || target;
      el.replaceWith(a);
      return;
    }
    let text: string;
    try {
      text = await readNote(file);
    } catch {
      if (current(s)) missing(`"${target}"을(를) 읽을 수 없습니다`);
      return;
    }
    if (!current(s)) return;
    let part = bodyOf(text);
    if (sub.startsWith("^")) {
      const line = text.split(/\r?\n/).find((l) => l.trimEnd().endsWith(sub));
      if (!line) return missing(`"${target}#${sub}" 블록이 없습니다`);
      part = line.trimEnd().slice(0, -sub.length);
    } else if (sub) {
      const line = findHeading(text, sub);
      if (line === null) return missing(`"${target}#${sub}" 제목이 없습니다`);
      part = sectionOf(text, line);
    }

    const box = document.createElement("div");
    box.className = "embed embed-note";
    const head = document.createElement("a");
    head.className = "internal-link embed-title";
    head.href = "#";
    Object.assign(head.dataset, { target, sub, source });
    head.textContent = displayName(file.split("/").pop()!) + (sub ? ` > ${sub.replace(/^\^/, "^")}` : "");
    const body = document.createElement("div");
    body.className = "embed-body";
    body.innerHTML = renderMarkdown(part);
    box.append(head, body);
    el.replaceWith(box);
    enhance(body, file, depth + 1, s);
  }

  function onclick(e: MouseEvent) {
    const target = e.target as HTMLElement;
    const box = target.closest<HTMLInputElement>("input[type=checkbox]");
    if (box) {
      if (box.dataset.line) ontoggle(Number(box.dataset.line));
      else e.preventDefault();
      return;
    }
    const a = target.closest<HTMLAnchorElement>("a");
    if (!a) return;
    e.preventDefault();
    if (a.classList.contains("tag")) {
      ontag(a.dataset.tag!);
    } else if (a.classList.contains("internal-link")) {
      const sub = a.dataset.sub ?? "";
      onlink(a.dataset.target ?? "", sub && !sub.startsWith("^") ? sub : null, a.dataset.source ?? path);
    } else if (a.classList.contains("external-link")) {
      openUrl(a.href).catch(() => {});
    } else if (a.dataset.fn || a.dataset.fnback) {
      // Footnote ↔ its reference, inside this rendering (embeds have their own).
      const id = a.dataset.fn ? `fn-${a.dataset.fn}` : `fnref-${a.dataset.fnback}`;
      const scope = a.closest(".embed-body") ?? host;
      scope?.querySelector(`[id="${id}"]`)?.scrollIntoView({ block: "center", behavior: "smooth" });
    } else if (a.getAttribute("href")?.startsWith("#")) {
      // [text](#Heading) in the same note
      const heading = safeDecode(a.getAttribute("href")!.slice(1));
      if (heading) onlink("", heading, path);
    }
  }
</script>

<div class="reading-scroll">
  <!-- Clicks are delegated to the links and checkboxes inside, which are focusable themselves. -->
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
  <article class={["reading", { print }]} bind:this={host} {onclick} aria-label="읽기 보기"></article>
</div>

<style>
  .reading-scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .reading {
    box-sizing: border-box;
    max-width: var(--line-width);
    margin: 0 auto;
    padding: 24px 24px 40vh;
    font-size: var(--editor-font-size);
    line-height: 1.7;
    color: var(--text);
    overflow-wrap: break-word;
    user-select: text;
  }
  .reading :global(:is(h1, h2, h3, h4, h5, h6)) {
    line-height: 1.3;
    margin: 1.2em 0 0.5em;
  }
  /* ------------------------------------------------ math, diagrams, footnotes */
  .reading :global(.math-block) {
    display: block;
    margin: 1em 0;
    overflow-x: auto;
    overflow-y: hidden;
    text-align: center;
  }
  .reading :global(:is(.math-pending, .mermaid-pending)) {
    color: var(--text-faint);
    font-family: var(--font-mono);
    font-size: 0.9em;
    white-space: pre-wrap;
  }
  .reading :global(.math-error) {
    color: var(--danger);
    font-family: var(--font-mono);
  }
  .reading :global(.mermaid-block) {
    margin: 1em 0;
    text-align: center;
    overflow-x: auto;
  }
  .reading :global(.mermaid-block svg) {
    max-width: 100%;
    height: auto;
  }
  .reading :global(.mermaid-error) {
    margin-bottom: 6px;
    color: var(--danger);
    font-size: 0.9em;
    text-align: left;
  }
  .reading :global(.fn-ref) {
    font-size: 0.75em;
    line-height: 0;
  }
  .reading :global(.fn-ref a),
  .reading :global(.fn-back) {
    color: var(--accent);
    text-decoration: none;
  }
  .reading :global(.footnotes) {
    margin-top: 2.5em;
    padding-top: 0.8em;
    border-top: 1px solid var(--border);
    font-size: 0.9em;
    color: var(--text-muted);
  }
  .reading :global(.footnotes li:target),
  .reading :global(.fn-missing) {
    color: var(--text-faint);
  }
  .reading :global(h1) { font-size: 1.6em; }
  .reading :global(h2) { font-size: 1.4em; }
  .reading :global(h3) { font-size: 1.25em; }
  .reading :global(:is(h4, h5, h6)) { font-size: 1.1em; }
  .reading > :global(:first-child) {
    margin-top: 0;
  }
  .reading :global(p) {
    margin: 0 0 0.8em;
  }
  .reading :global(a) {
    color: var(--accent);
    text-decoration: underline;
    text-decoration-color: var(--accent-soft);
    cursor: pointer;
  }
  .reading :global(a.external-link::after) {
    content: "↗";
    font-size: 0.75em;
    margin-left: 1px;
    text-decoration: none;
    display: inline-block;
  }
  .reading :global(a.tag) {
    text-decoration: none;
    background: var(--accent-bg);
    border-radius: 10px;
    padding: 0 6px;
  }
  .reading :global(mark) {
    background: var(--highlight-bg);
    color: inherit;
    border-radius: 2px;
    padding: 0 1px;
  }
  .reading :global(code) {
    font-family: var(--font-mono);
    font-size: 0.88em;
    background: var(--code-bg);
    border-radius: 3px;
    padding: 1px 4px;
  }
  .reading :global(pre) {
    background: var(--code-bg);
    border-radius: 6px;
    padding: 12px 14px;
    overflow: auto;
  }
  .reading :global(pre code) {
    background: none;
    padding: 0;
  }
  .reading :global(blockquote) {
    margin: 0 0 0.8em;
    padding: 0 0 0 14px;
    border-left: 3px solid var(--accent-soft);
    color: var(--text-muted);
  }
  .reading :global(:is(ul, ol)) {
    padding-left: 1.6em;
    margin: 0 0 0.8em;
  }
  .reading :global(li:has(> input[type="checkbox"])) {
    list-style: none;
    margin-left: -1.4em;
  }
  .reading :global(input[type="checkbox"]) {
    accent-color: var(--accent);
    margin: 0 6px 0 0;
    vertical-align: -2px;
  }
  .reading :global(li:has(> input:checked)) {
    color: var(--text-faint);
    text-decoration: line-through;
  }
  .reading :global(table) {
    border-collapse: collapse;
    margin: 0 0 0.8em;
    display: block;
    overflow: auto;
  }
  .reading :global(:is(th, td)) {
    border: 1px solid var(--border);
    padding: 4px 10px;
  }
  .reading :global(th) {
    background: var(--bg-side);
  }
  .reading :global(hr) {
    border: 0;
    border-top: 1px solid var(--border);
    margin: 1.5em 0;
  }
  .reading :global(img) {
    max-width: 100%;
    border-radius: 4px;
  }
  /* ------------------------------------------------------------ embeds */
  .reading :global(.embed-image),
  .reading :global(.embed-audio),
  .reading :global(.embed-video) {
    display: block;
    margin: 0.4em 0;
  }
  .reading :global(.embed-video video) {
    max-width: 100%;
  }
  .reading :global(.embed-pdf iframe) {
    width: 100%;
    height: 70vh;
    border: 1px solid var(--border);
    border-radius: 6px;
  }
  .reading :global(.embed-note) {
    margin: 0.4em 0 0.8em;
    padding: 6px 14px 2px;
    border-left: 3px solid var(--accent);
    background: var(--bg-side);
    border-radius: 0 6px 6px 0;
  }
  .reading :global(.embed-title) {
    display: inline-block;
    font-size: 0.85em;
    font-weight: 600;
    text-decoration: none;
    margin-bottom: 4px;
  }
  .reading :global(.embed-missing) {
    color: var(--text-faint);
    font-style: italic;
    font-size: 0.9em;
  }
  /* ---------------------------------------------------------- callouts */
  .reading :global(.callout) {
    --c: 68 138 255;
    margin: 0 0 0.8em;
    padding: 10px 14px;
    border-radius: 6px;
    background: rgb(var(--c) / 0.1);
    border-left: 3px solid rgb(var(--c));
  }
  .reading :global(.callout-title) {
    font-weight: 600;
    color: rgb(var(--c));
  }
  .reading :global(summary.callout-title) {
    cursor: pointer;
  }
  .reading :global(.callout-content > :last-child) {
    margin-bottom: 0;
  }
  .reading :global(.callout-content > :first-child) {
    margin-top: 0.4em;
  }
  .reading :global(.callout:is([data-callout="tip"], [data-callout="hint"], [data-callout="important"], [data-callout="abstract"], [data-callout="summary"], [data-callout="tldr"])) { --c: 0 168 168; }
  .reading :global(.callout:is([data-callout="success"], [data-callout="check"], [data-callout="done"])) { --c: 8 160 70; }
  .reading :global(.callout:is([data-callout="question"], [data-callout="help"], [data-callout="faq"], [data-callout="warning"], [data-callout="caution"], [data-callout="attention"])) { --c: 230 130 20; }
  .reading :global(.callout:is([data-callout="failure"], [data-callout="fail"], [data-callout="missing"], [data-callout="danger"], [data-callout="error"], [data-callout="bug"])) { --c: 225 60 60; }
  .reading :global(.callout[data-callout="example"]) { --c: 124 92 219; }
  .reading :global(.callout:is([data-callout="quote"], [data-callout="cite"])) { --c: 140 140 140; }
</style>
