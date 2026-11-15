import { movedPath } from "./api";

/** File tree state shared by every level of the (recursive) tree. */
export class TreeUi {
  /** Open folders, by path. */
  expanded = $state<Record<string, boolean>>({});
  /** Path whose name is being edited inline. */
  renaming = $state<string | null>(null);
  /** Path being dragged. */
  dragging = $state<string | null>(null);
  /** Folder a drop would land in ("" = vault root). */
  dropTarget = $state<string | null>(null);

  /** Open every folder above `path` so it is visible. */
  reveal(path: string) {
    const parts = path.split("/");
    for (let i = 1; i < parts.length; i++) this.expanded[parts.slice(0, i).join("/")] = true;
  }

  /** Keep folders open (and edits pointed right) after `from` moved to `to`. */
  moved(from: string, to: string) {
    const next: Record<string, boolean> = {};
    for (const [path, open] of Object.entries(this.expanded)) next[movedPath(path, from, to) ?? path] = open;
    this.expanded = next;
  }

  /** Can `path` be dropped into `folder`? Not onto itself, its own folder, or inside itself. */
  canDrop(path: string, folder: string): boolean {
    const parent = path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "";
    return folder !== parent && folder !== path && !folder.startsWith(`${path}/`);
  }
}

export interface TreeActions {
  /** Ctrl+click / middle click: in a new tab. */
  open: (path: string, newTab?: boolean) => void;
  /** `name` is what the user typed (notes without ".md"). */
  rename: (path: string, isDir: boolean, name: string) => void;
  move: (path: string, folder: string) => void;
  trash: (path: string, isDir: boolean) => void;
  newNote: (folder: string) => void;
  newFolder: (parent: string) => void;
  newCanvas: (folder: string) => void;
  reveal: (path: string) => void;
  /** Earlier versions of a note (file recovery). */
  history: (path: string) => void;
}
