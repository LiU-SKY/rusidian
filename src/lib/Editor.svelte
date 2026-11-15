<script lang="ts">
  import { onMount } from "svelte";
  import { Compartment, EditorState } from "@codemirror/state";
  import { livePreview } from "./livePreview";
  import {
    Decoration, drawSelection, EditorView, keymap, MatchDecorator, ViewPlugin,
    type DecorationSet, type ViewUpdate,
  } from "@codemirror/view";
  import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
  import { HighlightStyle, indentOnInput, syntaxHighlighting } from "@codemirror/language";
  import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
  import { yamlFrontmatter } from "@codemirror/lang-yaml";
  import { highlightSelectionMatches, searchKeymap } from "@codemirror/search";
  import { tags as t } from "@lezer/highlight";

  interface Props {
    /**
     * The note text. Typing reports it through `onchange`; when it changes
     * from elsewhere (another pane, the file on disk) the editor follows,
     * keeping the cursor and undo history.
     */
    doc: string;
    onchange: (doc: string) => void;
    /** Take keyboard focus when created. */
    autofocus?: boolean;
    /**
     * Ctrl/Cmd+click on a [[wikilink]]: the target without alias ("" for a
     * link inside this note, `[[#Heading]]`) and the heading, if any.
     */
    onlink?: (target: string, heading: string | null) => void;
    /** Hide markup on lines without the cursor (Obsidian's live preview). */
    livePreview?: boolean;
    /** URL of an embedded image target, for live preview. */
    imageUrl?: (target: string) => Promise<string | null>;
    /**
     * Files pasted (a clipboard image) or dropped from outside: saves them
     * and returns the text to insert for each (e.g. `![[Pasted image ….png]]`).
     */
    onfiles?: (files: File[], pasted: boolean) => Promise<string[]>;
    /** The selected text whenever the selection changes ("" when nothing is selected). */
    onselection?: (text: string) => void;
  }

  let {
    doc, onchange, onlink, autofocus = true, livePreview: preview = false, imageUrl = async () => null, onfiles, onselection,
  }: Props = $props();
  const previewSlot = new Compartment();
  const previewExt = () => (preview ? livePreview({ imageUrl: (t) => imageUrl(t) }) : []);
  /** The last text this editor reported; an incoming `doc` equal to it is our own echo. */
  let emitted: string | null = null;

  $effect(() => {
    const ext = previewExt();
    view?.dispatch({ effects: previewSlot.reconfigure(ext) });
  });

  $effect(() => {
    const text = doc;
    if (!view || text === emitted) return;
    applyText(text);
  });

  /** Insert at the cursor (replacing a selection) and focus. */
  export function insertAtCursor(text: string) {
    if (!view) return;
    view.dispatch(view.state.replaceSelection(text));
    view.focus();
  }

  export function focus() {
    view?.focus();
  }

  /** Replace the text with `next`, changing only what differs (keeps undo and the cursor). */
  export function applyText(next: string) {
    if (!view) return;
    const cur = view.state.doc.toString();
    if (cur === next) return;
    let start = 0;
    while (start < cur.length && start < next.length && cur[start] === next[start]) start++;
    let endCur = cur.length;
    let endNext = next.length;
    while (endCur > start && endNext > start && cur[endCur - 1] === next[endNext - 1]) {
      endCur--;
      endNext--;
    }
    view.dispatch({ changes: { from: start, to: endCur, insert: next.slice(start, endNext) } });
  }

  const WIKILINK_RE = /\[\[([^[\]\n]+?)\]\]/g;

  /** The [[link]] under document position `pos`, if any. */
  function linkAt(view: EditorView, pos: number): { target: string; heading: string | null } | null {
    const line = view.state.doc.lineAt(pos);
    const offset = pos - line.from;
    for (const m of line.text.matchAll(WIKILINK_RE)) {
      const start = m.index ?? 0;
      if (offset >= start && offset <= start + m[0].length) {
        const [path, ...sub] = m[1].split("|")[0].replace(/\\$/, "").split("#");
        // Block references (#^id) are not headings.
        const heading = sub.join("#").trim();
        const target = path.trim();
        const useful = heading && !heading.startsWith("^") ? heading : null;
        return target || useful ? { target, heading: useful } : null;
      }
    }
    return null;
  }
  let host: HTMLDivElement;
  let view: EditorView | undefined;

  /** Put the cursor at the start of 0-based `line` and scroll it to the top. */
  export function gotoLine(line: number) {
    if (!view) return;
    const doc = view.state.doc;
    const pos = doc.line(Math.min(Math.max(line + 1, 1), doc.lines)).from;
    view.dispatch({
      selection: { anchor: pos },
      effects: EditorView.scrollIntoView(pos, { y: "start", yMargin: 24 }),
    });
    view.focus();
  }

  // Obsidian-like styling: headings sized, markup characters dimmed.
  const markdownStyle = HighlightStyle.define([
    { tag: t.heading1, fontSize: "1.6em", fontWeight: "700" },
    { tag: t.heading2, fontSize: "1.4em", fontWeight: "700" },
    { tag: t.heading3, fontSize: "1.25em", fontWeight: "700" },
    { tag: [t.heading4, t.heading5, t.heading6], fontSize: "1.1em", fontWeight: "700" },
    { tag: t.strong, fontWeight: "700" },
    { tag: t.emphasis, fontStyle: "italic" },
    { tag: t.strikethrough, textDecoration: "line-through" },
    { tag: t.link, color: "var(--accent)" },
    { tag: t.url, color: "var(--text-faint)", textDecoration: "underline" },
    { tag: t.monospace, fontFamily: "var(--font-mono)", backgroundColor: "var(--code-bg)", borderRadius: "3px" },
    { tag: t.quote, color: "var(--text-muted)", fontStyle: "italic" },
    { tag: [t.processingInstruction, t.contentSeparator, t.meta], color: "var(--text-faint)" },
    // YAML frontmatter
    { tag: [t.propertyName, t.definition(t.propertyName)], color: "var(--accent)" },
    { tag: [t.number, t.bool, t.null], color: "var(--yaml-value)" },
    { tag: t.comment, color: "var(--text-faint)", fontStyle: "italic" },
  ]);

  /** Mark regex matches with `deco`, optionally skipping a leading capture group. */
  function matchHighlighter(regexp: RegExp, deco: Decoration) {
    const matcher = new MatchDecorator({
      regexp,
      decorate: (add, from, _to, match) => {
        const lead = match[1] ?? "";
        const text = match[2] ?? match[0];
        const start = from + lead.length;
        add(start, start + text.length, deco);
      },
    });
    return ViewPlugin.fromClass(
      class {
        decorations: DecorationSet;
        constructor(view: EditorView) {
          this.decorations = matcher.createDeco(view);
        }
        update(u: ViewUpdate) {
          this.decorations = matcher.updateDeco(u, this.decorations);
        }
      },
      { decorations: (v) => v.decorations },
    );
  }

  // [[Note]] / [[Note|alias]] wiki links, and #tags (not purely numeric, Obsidian rule).
  const wikilinks = matchHighlighter(
    /\[\[[^[\]\n]+?\]\]/g,
    Decoration.mark({ class: "cm-wikilink", attributes: { title: "Ctrl+클릭으로 열기" } }),
  );
  const hashtags = matchHighlighter(
    /(^|\s)(#(?![0-9]+(?![\p{L}\p{N}_/-]))[\p{L}\p{N}_/-]+)/gu,
    Decoration.mark({ class: "cm-hashtag" }),
  );

  /** Save files and put their embeds at `pos` (a range: replaced), one per line. */
  async function insertFiles(view: EditorView, files: File[], pasted: boolean, from: number, to: number) {
    if (!onfiles) return;
    const texts = await onfiles(files, pasted);
    if (texts.length === 0) return;
    const insert = texts.join("\n");
    // The document may have changed while saving; keep the spot inside it.
    const len = view.state.doc.length;
    const a = Math.min(from, len);
    const b = Math.min(Math.max(to, a), len);
    view.dispatch({ changes: { from: a, to: b, insert }, selection: { anchor: a + insert.length }, scrollIntoView: true });
    view.focus();
  }

  const fileDrops = EditorView.domEventHandlers({
    paste(e, view) {
      const data = e.clipboardData;
      const files = [...(data?.files ?? [])];
      // Text wins when there is any (copying cells from a spreadsheet also
      // puts a picture on the clipboard); a bare image is saved.
      if (!onfiles || files.length === 0 || data?.getData("text/plain")) return false;
      e.preventDefault();
      const { from, to } = view.state.selection.main;
      insertFiles(view, files, true, from, to);
      return true;
    },
    drop(e, view) {
      const files = [...(e.dataTransfer?.files ?? [])];
      if (!onfiles || files.length === 0) return false; // text or an in-app drag
      e.preventDefault();
      const pos = view.posAtCoords({ x: e.clientX, y: e.clientY }) ?? view.state.selection.main.head;
      insertFiles(view, files, false, pos, pos);
      return true;
    },
  });

  const linkClicks = EditorView.domEventHandlers({
    mousedown(e, view) {
      if (!(e.ctrlKey || e.metaKey) || e.button !== 0 || !onlink) return false;
      const pos = view.posAtCoords({ x: e.clientX, y: e.clientY });
      const link = pos == null ? null : linkAt(view, pos);
      if (!link) return false;
      e.preventDefault();
      onlink(link.target, link.heading);
      return true;
    },
  });

  const theme = EditorView.theme({
    "&": { height: "100%", fontSize: "var(--editor-font-size)", color: "var(--text)", backgroundColor: "var(--bg)" },
    "&.cm-focused": { outline: "none" },
    ".cm-scroller": { fontFamily: "var(--font-text)", lineHeight: "1.7", overflow: "auto" },
    ".cm-content": {
      boxSizing: "border-box",
      width: "100%",
      maxWidth: "var(--line-width)",
      margin: "0 auto",
      padding: "24px 24px 40vh",
      caretColor: "var(--accent)",
    },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--accent)", borderLeftWidth: "2px" },
    "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground":
      { backgroundColor: "var(--selection)" },
    ".cm-selectionMatch": { backgroundColor: "var(--selection-match)" },
    ".cm-wikilink": { color: "var(--accent)", textDecoration: "underline", textDecorationColor: "var(--accent-soft)" },
    ".cm-hashtag": {
      color: "var(--accent)",
      backgroundColor: "var(--accent-bg)",
      borderRadius: "10px",
      padding: "0 6px",
    },
    // live preview
    ".cm-lp-task": { margin: "0 6px 0 0", verticalAlign: "-2px", accentColor: "var(--accent)", cursor: "pointer" },
    ".cm-lp-bullet": { color: "var(--text-muted)", display: "inline-block", width: "1ch" },
    ".cm-lp-hr": {
      display: "inline-block", width: "100%", verticalAlign: "middle", borderTop: "1px solid var(--border)",
    },
    ".cm-lp-quote": { borderLeft: "3px solid var(--accent-soft)", paddingLeft: "14px !important", color: "var(--text-muted)" },
    ".cm-lp-highlight": { backgroundColor: "var(--highlight-bg)", borderRadius: "2px" },
    ".cm-lp-image": { maxWidth: "100%", borderRadius: "4px", verticalAlign: "top" },
    ".cm-lp-missing": { color: "var(--text-faint)", fontStyle: "italic" },
    ".cm-lp-math.math-block": { display: "block", textAlign: "center", padding: "6px 0", overflowX: "auto" },
    ".cm-lp-mermaid": { textAlign: "center", padding: "8px 0", overflowX: "auto" },
    ".cm-lp-mermaid svg": { maxWidth: "100%", height: "auto" },
    ".math-pending, .mermaid-pending": { color: "var(--text-faint)", fontFamily: "var(--font-mono)", whiteSpace: "pre-wrap" },
    ".math-error, .mermaid-error": { color: "var(--danger)" },
    ".mermaid-error + pre": { textAlign: "left", fontFamily: "var(--font-mono)", fontSize: "0.85em", color: "var(--text-muted)" },
    ".cm-panels": { backgroundColor: "var(--bg-side)", color: "var(--text)" },
    ".cm-panels-top": { borderBottom: "1px solid var(--border)" },
    ".cm-textfield": { backgroundColor: "var(--bg)", color: "var(--text)", border: "1px solid var(--border)" },
  });

  onMount(() => {
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc,
        extensions: [
          history(),
          drawSelection(),
          indentOnInput(),
          highlightSelectionMatches(),
          EditorView.lineWrapping,
          yamlFrontmatter({ content: markdown({ base: markdownLanguage }) }),
          syntaxHighlighting(markdownStyle),
          wikilinks,
          hashtags,
          previewSlot.of(previewExt()),
          linkClicks,
          fileDrops,
          theme,
          keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap, indentWithTab]),
          EditorView.updateListener.of((u) => {
            if (u.selectionSet || u.docChanged) {
              const sel = u.state.selection.main;
              onselection?.(sel.empty ? "" : u.state.sliceDoc(sel.from, sel.to));
            }
            if (!u.docChanged) return;
            emitted = u.state.doc.toString();
            onchange(emitted);
          }),
          EditorView.contentAttributes.of({ "aria-label": "노트 편집기", spellcheck: "false" }),
        ],
      }),
    });
    if (autofocus) view.focus();
    return () => view?.destroy();
  });
</script>

<div class="editor-host" bind:this={host}></div>

<style>
  .editor-host {
    flex: 1;
    min-height: 0;
  }
</style>
