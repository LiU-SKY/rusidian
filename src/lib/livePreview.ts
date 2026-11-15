// Live preview: Obsidian's default editing mode. Markdown markup is hidden on
// every line the cursor is not on, task boxes become clickable checkboxes,
// list markers become bullets and `![[image]]` embeds show the image. Lines
// with the cursor (or a selection) show the raw text so it stays editable.

import { syntaxTree } from "@codemirror/language";
import { type EditorState, type Extension, type Range, StateField } from "@codemirror/state";
import type { SyntaxNode } from "@lezer/common";
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate, WidgetType } from "@codemirror/view";
import { mediaKind } from "./api";
import { INLINE_MATH, renderMath, renderMermaid } from "./mathMermaid";
import { parseWiki } from "./render";

export interface LivePreviewOptions {
  /** URL of an embedded image (`![[x.png]]` target), or null if it doesn't exist. */
  imageUrl: (target: string) => Promise<string | null>;
}

class CheckboxWidget extends WidgetType {
  constructor(readonly checked: boolean, readonly from: number) {
    super();
  }
  eq(o: CheckboxWidget) {
    return o.checked === this.checked && o.from === this.from;
  }
  toDOM(view: EditorView) {
    const box = document.createElement("input");
    box.type = "checkbox";
    box.className = "cm-lp-task";
    box.checked = this.checked;
    box.setAttribute("aria-label", this.checked ? "완료 취소" : "완료");
    box.addEventListener("mousedown", (e) => e.preventDefault());
    box.addEventListener("click", (e) => {
      e.preventDefault();
      // `[ ]` ↔ `[x]`: the character between the brackets.
      view.dispatch({ changes: { from: this.from + 1, to: this.from + 2, insert: this.checked ? " " : "x" } });
    });
    return box;
  }
  ignoreEvent() {
    return true;
  }
}

class BulletWidget extends WidgetType {
  eq() {
    return true;
  }
  toDOM() {
    const s = document.createElement("span");
    s.className = "cm-lp-bullet";
    s.textContent = "•";
    return s;
  }
}

class RuleWidget extends WidgetType {
  eq() {
    return true;
  }
  toDOM() {
    const s = document.createElement("span");
    s.className = "cm-lp-hr";
    return s;
  }
}

class ImageWidget extends WidgetType {
  constructor(readonly target: string, readonly size: string | null, readonly opts: LivePreviewOptions) {
    super();
  }
  eq(o: ImageWidget) {
    return o.target === this.target && o.size === this.size;
  }
  toDOM() {
    const img = document.createElement("img");
    img.className = "cm-lp-image";
    img.alt = this.target;
    const m = this.size && /^(\d+)(?:x(\d+))?$/.exec(this.size);
    if (m) {
      img.style.width = `${m[1]}px`;
      if (m[2]) img.style.height = `${m[2]}px`;
    }
    this.opts.imageUrl(this.target).then((url) => {
      if (url) img.src = url;
      else img.replaceWith(Object.assign(document.createElement("span"), {
        className: "cm-lp-missing", textContent: `"${this.target}" 파일이 없습니다`,
      }));
    });
    return img;
  }
}

/** `$tex$` (inline) or `$$tex$$` (display) drawn by KaTeX. */
class MathWidget extends WidgetType {
  constructor(readonly tex: string, readonly display: boolean) {
    super();
  }
  eq(o: MathWidget) {
    return o.tex === this.tex && o.display === this.display;
  }
  toDOM() {
    const el = document.createElement(this.display ? "div" : "span");
    el.className = "cm-lp-math";
    renderMath(el, this.tex, this.display);
    return el;
  }
}

/** A ```mermaid block drawn as a diagram. */
class MermaidWidget extends WidgetType {
  constructor(readonly code: string) {
    super();
  }
  eq(o: MermaidWidget) {
    return o.code === this.code;
  }
  toDOM() {
    const el = document.createElement("div");
    el.className = "cm-lp-mermaid";
    renderMermaid(el, this.code);
    return el;
  }
}

const hide = Decoration.replace({});
const bullet = Decoration.replace({ widget: new BulletWidget() });
const rule = Decoration.replace({ widget: new RuleWidget() });
const highlightMark = Decoration.mark({ class: "cm-lp-highlight" });
const quoteLine = Decoration.line({ class: "cm-lp-quote" });

const WIKI = /(!?)\[\[([^[\]\n]+?)\]\]/g;
const HIGHLIGHT = /==(?=\S)([^\n]*?\S)==/g;
const INLINE_DISPLAY_MATH = /\$\$([^\n$]+?)\$\$/g;
const CODE_NODES = new Set(["InlineCode", "FencedCode", "CodeBlock", "CodeText", "HTMLBlock", "Comment"]);

/** End of the frontmatter block (0 if none): the YAML part is never previewed. */
function frontmatterEnd(state: EditorState): number {
  const doc = state.doc;
  if (doc.line(1).text.replace(/^﻿/, "").trimEnd() !== "---") return 0;
  for (let n = 2; n <= Math.min(doc.lines, 500); n++) {
    if (doc.line(n).text.trimEnd() === "---") return doc.line(n).to;
  }
  return 0;
}

function inCode(state: EditorState, pos: number): boolean {
  for (let node: SyntaxNode | null = syntaxTree(state).resolveInner(pos, 1); node; node = node.parent) {
    if (CODE_NODES.has(node.name)) return true;
  }
  return false;
}

function build(view: EditorView, opts: LivePreviewOptions): DecorationSet {
  const { state } = view;
  const doc = state.doc;
  const decos: Range<Decoration>[] = [];

  // Lines the cursor or a selection touches keep their markup visible.
  const active = new Set<number>();
  for (const r of state.selection.ranges) {
    for (let n = doc.lineAt(r.from).number; n <= doc.lineAt(r.to).number; n++) active.add(n);
  }
  const fmEnd = frontmatterEnd(state);
  /** Wiki links found by regex; syntax nodes inside them are left alone. */
  const wikiRanges: [number, number][] = [];
  const inWiki = (from: number, to: number) => wikiRanges.some(([a, b]) => from < b && to > a);

  for (const { from, to } of view.visibleRanges) {
    // 1. Regex syntax the markdown parser doesn't know: [[links]], ![[embeds]], ==highlights==.
    for (let pos = Math.max(from, fmEnd); pos <= to; ) {
      const line = doc.lineAt(pos);
      pos = line.to + 1;
      if (active.has(line.number)) continue;
      for (const m of line.text.matchAll(WIKI)) {
        const start = line.from + m.index!;
        const end = start + m[0].length;
        if (inCode(state, start)) continue;
        wikiRanges.push([start, end]);
        const parts = parseWiki(m[2]);
        if (m[1] && mediaKind(parts.target) === "image") {
          decos.push(Decoration.replace({ widget: new ImageWidget(parts.target, parts.alias, opts) }).range(start, end));
          continue;
        }
        const open = m[1].length + 2;
        // Show only the alias when there is one (`\|` is the pipe inside tables).
        const pipe = /\\?\|/.exec(m[2]);
        const shownFrom = pipe ? start + open + pipe.index + pipe[0].length : start + open;
        decos.push(hide.range(start, shownFrom), hide.range(end - 2, end));
      }
      for (const m of line.text.matchAll(HIGHLIGHT)) {
        const start = line.from + m.index!;
        const end = start + m[0].length;
        if (inCode(state, start) || inWiki(start, end)) continue;
        decos.push(hide.range(start, start + 2), highlightMark.range(start + 2, end - 2), hide.range(end - 2, end));
      }
      // `$$…$$` inside a line (a line that is only `$$…$$` is a block, see mathBlocks).
      if (!/^\s*\$\$.*\$\$\s*$/.test(line.text)) {
        for (const m of line.text.matchAll(INLINE_DISPLAY_MATH)) {
          const start = line.from + m.index!;
          if (inCode(state, start)) continue;
          decos.push(Decoration.replace({ widget: new MathWidget(m[1].trim(), true) }).range(start, start + m[0].length));
        }
      }
      for (const m of line.text.matchAll(INLINE_MATH)) {
        const start = line.from + m.index!;
        const end = start + m[0].length;
        if (inCode(state, start) || inWiki(start, end)) continue;
        decos.push(Decoration.replace({ widget: new MathWidget(m[1], false) }).range(start, end));
      }
    }

    // 2. Markdown syntax from the parse tree.
    syntaxTree(state).iterate({
      from: Math.max(from, fmEnd),
      to,
      enter: (node) => {
        if (node.name === "FencedCode" || node.name === "CodeBlock" || node.name === "HTMLBlock") return false;
        const line = doc.lineAt(node.from);
        if (node.name === "Blockquote") {
          for (let n = line.number; n <= doc.lineAt(node.to).number; n++) {
            if (!active.has(n)) decos.push(quoteLine.range(doc.line(n).from));
          }
          return;
        }
        if (active.has(line.number) || inWiki(node.from, node.to)) return;
        const parent = node.node.parent?.name;
        const next = doc.sliceString(node.to, node.to + 1);
        switch (node.name) {
          case "HeaderMark":
            // ATX `## ` only; a setext underline stays visible.
            if (parent?.startsWith("ATXHeading")) {
              if (node.from === line.from) decos.push(hide.range(node.from, next === " " ? node.to + 1 : node.to));
              else decos.push(hide.range(node.from, node.to)); // closing #s
            }
            break;
          case "EmphasisMark":
          case "StrikethroughMark":
            decos.push(hide.range(node.from, node.to));
            break;
          case "CodeMark":
            if (parent === "InlineCode") decos.push(hide.range(node.from, node.to));
            break;
          case "LinkMark":
          case "URL":
          case "LinkTitle":
            // Only `[text](url)`; a bare `[text]` (e.g. a callout's `[!note]`) stays as written.
            if (parent === "Link" && node.node.parent?.getChild("URL")) decos.push(hide.range(node.from, node.to));
            break;
          case "QuoteMark":
            decos.push(hide.range(node.from, next === " " ? node.to + 1 : node.to));
            break;
          case "ListMark": {
            const task = node.node.nextSibling?.name === "Task";
            const end = next === " " ? node.to + 1 : node.to;
            if (task) decos.push(hide.range(node.from, end));
            else if (/^[-*+]$/.test(doc.sliceString(node.from, node.to))) decos.push(bullet.range(node.from, node.to));
            break;
          }
          case "TaskMarker": {
            const checked = /x/i.test(doc.sliceString(node.from + 1, node.from + 2));
            decos.push(Decoration.replace({ widget: new CheckboxWidget(checked, node.from) }).range(node.from, node.to));
            break;
          }
          case "HorizontalRule":
            decos.push(rule.range(node.from, node.to));
            break;
        }
      },
    });
  }
  return Decoration.set(decos, true);
}

/**
 * `$$` math blocks and ```mermaid blocks, drawn in place of their lines while
 * the cursor is outside them. Replacing whole lines needs block decorations,
 * which only a state field (not the view plugin above) may provide.
 */
function blockWidgets(state: EditorState): DecorationSet {
  const doc = state.doc;
  const decos: Range<Decoration>[] = [];
  const touched = (from: number, to: number) => state.selection.ranges.some((r) => r.from <= to && r.to >= from);
  const fmEnd = frontmatterEnd(state);

  // Mermaid: fenced code whose info string is "mermaid".
  syntaxTree(state).iterate({
    from: fmEnd,
    enter: (node) => {
      if (node.name !== "FencedCode") return;
      const info = node.node.getChild("CodeInfo");
      if (!info || doc.sliceString(info.from, info.to).trim().toLowerCase() !== "mermaid") return false;
      const first = doc.lineAt(node.from);
      const last = doc.lineAt(node.to);
      if (last.number === first.number || touched(first.from, last.to)) return false;
      const closed = /^\s*(```|~~~)/.test(last.text);
      const code = doc.sliceString(first.to + 1, Math.max(first.to + 1, closed ? last.from - 1 : last.to));
      decos.push(Decoration.replace({ widget: new MermaidWidget(code), block: true }).range(first.from, last.to));
      return false;
    },
  });

  // `$$` blocks: a line starting with `$$` up to the line ending with `$$`.
  for (let n = doc.lineAt(fmEnd).number + (fmEnd > 0 ? 1 : 0); n <= doc.lines; n++) {
    const line = doc.line(n);
    const text = line.text.trim();
    if (!text.startsWith("$$") || inCode(state, line.from)) continue;
    let end = line;
    if (!(text.length > 4 && text.endsWith("$$"))) {
      let m = n + 1;
      while (m <= doc.lines && !doc.line(m).text.trim().endsWith("$$")) m++;
      if (m > doc.lines) break; // never closed: leave it as text
      end = doc.line(m);
    }
    n = end.number;
    if (touched(line.from, end.to)) continue;
    const tex = doc.sliceString(line.from, end.to).trim().slice(2, -2).trim();
    if (tex) decos.push(Decoration.replace({ widget: new MathWidget(tex, true), block: true }).range(line.from, end.to));
  }
  return Decoration.set(decos, true);
}

const mathBlocks = StateField.define<DecorationSet>({
  create: blockWidgets,
  update: (value, tr) => (tr.docChanged || tr.selection || syntaxTree(tr.startState) !== syntaxTree(tr.state) ? blockWidgets(tr.state) : value),
  provide: (f) => EditorView.decorations.from(f),
});

export function livePreview(opts: LivePreviewOptions): Extension {
  return [mathBlocks, inlinePreview(opts)];
}

function inlinePreview(opts: LivePreviewOptions) {
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = build(view, opts);
      }
      update(u: ViewUpdate) {
        if (u.docChanged || u.viewportChanged || u.selectionSet || syntaxTree(u.startState) !== syntaxTree(u.state)) {
          this.decorations = build(u.view, opts);
        }
      }
    },
    { decorations: (v) => v.decorations },
  );
}
