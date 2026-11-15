// Panes and tabs of the main area. Each tab ("leaf", as Obsidian calls it)
// shows a file or a vault-wide view. Up to two panes side by side (split).
// The layout is remembered per vault in localStorage.

import { movedPath } from "./api";

export type LeafKind = "empty" | "file" | "tasks" | "calendar" | "finance" | "graph";
export type ViewKind = Exclude<LeafKind, "empty" | "file">;

let nextId = 0;
const newId = () => `leaf-${++nextId}`;

export class Leaf {
  readonly id = newId();
  kind = $state<LeafKind>("empty");
  /** The file shown (kind "file"), or the graph's centre note. */
  path = $state<string | null>(null);
  /** Rendered (reading view) instead of the editor. */
  reading = $state(false);
  /** Graph shows only `path`'s neighbourhood. */
  graphLocal = $state(false);

  constructor(kind: LeafKind = "empty", path: string | null = null) {
    this.kind = kind;
    this.path = path;
  }
}

export class Pane {
  readonly id = newId();
  tabs = $state<Leaf[]>([new Leaf()]);
  active = $state(0);

  get leaf(): Leaf {
    return this.tabs[Math.min(this.active, this.tabs.length - 1)];
  }
}

export const MAX_PANES = 2;

interface SavedLeaf {
  kind: LeafKind;
  path: string | null;
  reading?: boolean;
  graphLocal?: boolean;
}
interface Saved {
  panes: { tabs: SavedLeaf[]; active: number }[];
  activePane: number;
}

export class Workspace {
  panes = $state<Pane[]>([new Pane()]);
  activePane = $state(0);

  get pane(): Pane {
    return this.panes[Math.min(this.activePane, this.panes.length - 1)];
  }

  get leaf(): Leaf {
    return this.pane.leaf;
  }

  /** Every leaf, for "is this file still shown anywhere?" questions. */
  leaves(): Leaf[] {
    return this.panes.flatMap((p) => p.tabs);
  }

  focus(pane: Pane, index = pane.active) {
    this.activePane = Math.max(0, this.panes.indexOf(pane));
    pane.active = Math.max(0, Math.min(index, pane.tabs.length - 1));
  }

  /** A new tab after the active one in `pane`; becomes active. */
  newTab(pane = this.pane, leaf = new Leaf()): Leaf {
    pane.tabs.splice(pane.active + 1, 0, leaf);
    this.focus(pane, pane.active + 1);
    return leaf;
  }

  closeTab(pane: Pane, index: number) {
    if (pane.tabs.length > 1) {
      pane.tabs.splice(index, 1);
      if (pane.active >= index && pane.active > 0) pane.active--;
      this.focus(pane);
      return;
    }
    // The last tab of a pane: close the pane if split, else leave it empty.
    if (this.panes.length > 1) {
      this.panes.splice(this.panes.indexOf(pane), 1);
      this.activePane = Math.min(this.activePane, this.panes.length - 1);
    } else {
      pane.tabs = [new Leaf()];
      pane.active = 0;
    }
  }

  /** Split right: the new pane starts with a copy of the active tab. */
  splitRight(): boolean {
    if (this.panes.length >= MAX_PANES) return false;
    const src = this.leaf;
    const pane = new Pane();
    const copy = new Leaf(src.kind, src.path);
    copy.reading = src.reading;
    copy.graphLocal = src.graphLocal;
    pane.tabs = [copy];
    this.panes.splice(this.activePane + 1, 0, pane);
    this.activePane++;
    return true;
  }

  /** Close the other pane, keeping its tabs (moved into this one). */
  unsplit() {
    if (this.panes.length < 2) return;
    const keep = this.pane;
    const others = this.panes.filter((p) => p !== keep).flatMap((p) => p.tabs);
    keep.tabs.push(...others.filter((l) => l.kind !== "empty"));
    this.panes = [keep];
    this.activePane = 0;
  }

  /** The first tab showing `kind` (and `path`), if any. */
  find(kind: LeafKind, path?: string | null): { pane: Pane; index: number } | null {
    for (const pane of this.panes) {
      const index = pane.tabs.findIndex((l) => l.kind === kind && (path === undefined || l.path === path));
      if (index >= 0) return { pane, index };
    }
    return null;
  }

  /** `from` moved to `to`: point tabs at the new path. */
  moved(from: string, to: string) {
    for (const leaf of this.leaves()) {
      const next = leaf.path ? movedPath(leaf.path, from, to) : null;
      if (next !== null) leaf.path = next;
    }
  }

  /** `path` is gone: tabs that showed it become empty. Returns how many. */
  removed(path: string): number {
    let count = 0;
    for (const leaf of this.leaves()) {
      if (leaf.kind === "file" && leaf.path && movedPath(leaf.path, path, "") !== null) {
        leaf.kind = "empty";
        leaf.path = null;
        leaf.reading = false;
        count++;
      }
    }
    return count;
  }

  reset() {
    this.panes = [new Pane()];
    this.activePane = 0;
  }

  // ------------------------------------------------------------- persistence
  serialize(): string {
    const saved: Saved = {
      panes: this.panes.map((p) => ({
        active: p.active,
        tabs: p.tabs.map((l) => ({ kind: l.kind, path: l.path, reading: l.reading, graphLocal: l.graphLocal })),
      })),
      activePane: this.activePane,
    };
    return JSON.stringify(saved);
  }

  /** Restore a saved layout; tabs whose file no longer exists are dropped. */
  restore(json: string, exists: (path: string) => boolean): boolean {
    let saved: Saved;
    try {
      saved = JSON.parse(json);
    } catch {
      return false;
    }
    if (!Array.isArray(saved?.panes)) return false;
    const panes: Pane[] = [];
    for (const sp of saved.panes.slice(0, MAX_PANES)) {
      const tabs = (Array.isArray(sp.tabs) ? sp.tabs : [])
        .filter((t) => t && typeof t.kind === "string")
        .filter((t) => t.kind !== "file" || (typeof t.path === "string" && exists(t.path)))
        .map((t) => {
          const leaf = new Leaf(t.kind, t.path ?? null);
          leaf.reading = !!t.reading;
          leaf.graphLocal = !!t.graphLocal;
          return leaf;
        });
      if (!tabs.length) continue;
      const pane = new Pane();
      pane.tabs = tabs;
      pane.active = Math.min(Math.max(0, Number(sp.active) || 0), tabs.length - 1);
      panes.push(pane);
    }
    if (!panes.length) return false;
    this.panes = panes;
    this.activePane = Math.min(Math.max(0, Number(saved.activePane) || 0), panes.length - 1);
    return true;
  }
}
