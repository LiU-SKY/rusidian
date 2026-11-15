// Open notes, one `Doc` per file however many tabs show it: the text, the
// unsaved flag, autosave and the parsed properties. Editors bind to
// `doc.content`, so a note open in two panes stays in sync.

import { SvelteMap } from "svelte/reactivity";
import { movedPath, parseFrontmatter, readNote, syncStamp, writeNote, type Frontmatter } from "./api";

const AUTOSAVE_MS = 800;
const PROPS_MS = 300;

/**
 * The editor works in `\n` (CodeMirror hands back nothing else), so text from
 * disk is taken in that form too; otherwise a CRLF note would look edited the
 * moment it reloads. Saving puts the file's own line endings back (Rust side).
 */
const lf = (text: string) => (text.includes("\r\n") ? text.replace(/\r\n/g, "\n") : text);

/** A save found the file changed elsewhere; the unsaved text went into `copy`. */
export type ConflictHandler = (path: string, copy: string) => void;

export class Doc {
  path = $state("");
  content = $state("");
  /** Changed since the last save. */
  dirty = $state(false);
  fm = $state<Frontmatter | null>(null);
  loaded = $state(false);
  /** Could not be read (missing or not text). */
  failed = $state<string | null>(null);

  #saveTimer: ReturnType<typeof setTimeout> | null = null;
  #propsTimer: ReturnType<typeof setTimeout> | null = null;
  #propsSeq = 0;
  /** The text last read from or saved to disk: a save checks the file still holds it. */
  #base: string | null = null;
  /** Saves run one after another, so each checks against the one before. */
  #saving: Promise<void> = Promise.resolve();
  #onerror: (message: string) => void;
  #onconflict: ConflictHandler;

  constructor(path: string, onerror: (message: string) => void, onconflict: ConflictHandler) {
    this.path = path;
    this.#onerror = onerror;
    this.#onconflict = onconflict;
  }

  get isNote() {
    return this.path.toLowerCase().endsWith(".md");
  }

  async load() {
    try {
      const text = lf(await readNote(this.path));
      this.#base = text;
      this.content = text;
      this.loaded = true;
      this.failed = null;
      this.refreshProps();
    } catch (e) {
      this.failed = String(e);
    }
  }

  /** The user (or a property edit) changed the text. */
  edit(text: string) {
    if (text === this.content) return;
    this.content = text;
    this.dirty = true;
    if (this.#saveTimer) clearTimeout(this.#saveTimer);
    this.#saveTimer = setTimeout(() => this.save(), AUTOSAVE_MS);
    if (this.#propsTimer) clearTimeout(this.#propsTimer);
    this.#propsTimer = setTimeout(() => this.refreshProps(), PROPS_MS);
  }

  /** The file changed on disk (and we had nothing unsaved): take its text. */
  replaceFromDisk(text: string) {
    text = lf(text);
    this.#base = text;
    if (text === this.content) return;
    this.content = text;
    this.dirty = false;
    this.refreshProps();
  }

  /** Save now if there are unsaved changes (waits for a save already running). */
  save(): Promise<void> {
    if (this.#saveTimer) clearTimeout(this.#saveTimer);
    this.#saveTimer = null;
    const run = this.#saving.then(() => this.#write());
    this.#saving = run.catch(() => {});
    return run;
  }

  async #write() {
    if (!this.dirty) return;
    const { path, content } = this;
    try {
      const res = await writeNote(path, content, this.#base ?? undefined, syncStamp());
      if (res.conflict !== null) {
        // Changed elsewhere (a sync pull, Obsidian) since we read it: the file
        // keeps that version, which the editor now shows; ours is in the copy.
        if (this.content !== content) await writeNote(res.conflict, this.content);
        if (this.path === path) {
          this.#base = lf(res.disk ?? "");
          this.content = this.#base;
          this.dirty = false;
          this.refreshProps();
        }
        this.#onconflict(path, res.conflict);
        return;
      }
      this.#base = content;
      // Keep the flag if more was typed while saving.
      if (content === this.content) this.dirty = false;
    } catch (e) {
      this.#onerror(String(e));
    }
  }

  /** Re-parse frontmatter in Rust; stale responses (older text) are dropped. */
  async refreshProps() {
    if (this.#propsTimer) clearTimeout(this.#propsTimer);
    this.#propsTimer = null;
    if (!this.isNote) return;
    const seq = ++this.#propsSeq;
    try {
      const parsed = await parseFrontmatter(this.content);
      if (seq === this.#propsSeq) this.fm = parsed;
    } catch (e) {
      if (seq === this.#propsSeq) this.#onerror(String(e));
    }
  }

  /** Stop timers without saving (the file is gone). */
  dispose() {
    if (this.#saveTimer) clearTimeout(this.#saveTimer);
    if (this.#propsTimer) clearTimeout(this.#propsTimer);
    this.#propsSeq++;
  }
}

export class DocStore {
  /** Reactive, so tabs pick up a doc as soon as it is opened. */
  #docs = new SvelteMap<string, Doc>();
  #onerror: (message: string) => void;
  #onconflict: ConflictHandler;

  constructor(onerror: (message: string) => void, onconflict: ConflictHandler) {
    this.#onerror = onerror;
    this.#onconflict = onconflict;
  }

  /** The doc for `path`, loading it the first time. */
  open(path: string): Doc {
    let doc = this.#docs.get(path);
    if (!doc) {
      doc = new Doc(path, this.#onerror, this.#onconflict);
      this.#docs.set(path, doc);
      doc.load();
    }
    return doc;
  }

  find(path: string): Doc | undefined {
    return this.#docs.get(path);
  }

  all(): Doc[] {
    return [...this.#docs.values()];
  }

  /** Save every unsaved doc now (before moves, vault switches, …). */
  async flushAll() {
    await Promise.all(this.all().map((d) => d.save()));
  }

  /** `from` (file or folder) moved to `to`: re-key the docs under it. */
  moved(from: string, to: string) {
    for (const doc of this.all()) {
      const next = movedPath(doc.path, from, to);
      if (next === null) continue;
      this.#docs.delete(doc.path);
      doc.path = next;
      this.#docs.set(next, doc);
    }
  }

  /** `path` (file or folder) is gone: drop its docs without saving. */
  removed(path: string) {
    for (const doc of this.all()) {
      if (movedPath(doc.path, path, "") === null) continue;
      doc.dispose();
      this.#docs.delete(doc.path);
    }
  }

  /** Drop docs no tab shows any more (saving them first; kept if the save failed). */
  async keepOnly(paths: Set<string>) {
    for (const doc of this.all()) {
      if (paths.has(doc.path)) continue;
      await doc.save();
      if (!paths.has(doc.path) && !doc.dirty) {
        doc.dispose();
        this.#docs.delete(doc.path);
      }
    }
  }

  async clear() {
    await this.flushAll();
    for (const doc of this.all()) doc.dispose();
    this.#docs.clear();
  }
}
