// Obsidian canvases (`.canvas`, the JSON Canvas format): cards (text, a
// vault file, a web link, a group) placed on a plane, and arrows between
// them. Parsed objects are edited in place so fields Rusidian doesn't know
// are written back as they were.

export type Side = "top" | "right" | "bottom" | "left";
export type End = "none" | "arrow";

export interface CanvasNode {
  id: string;
  type: "text" | "file" | "link" | "group" | string;
  x: number;
  y: number;
  width: number;
  height: number;
  /** "1"–"6" (Obsidian's palette) or "#rrggbb". */
  color?: string;
  text?: string;
  file?: string;
  subpath?: string;
  url?: string;
  label?: string;
  [key: string]: unknown;
}

export interface CanvasEdge {
  id: string;
  fromNode: string;
  fromSide?: Side;
  /** Default "none". */
  fromEnd?: End;
  toNode: string;
  toSide?: Side;
  /** Default "arrow". */
  toEnd?: End;
  color?: string;
  label?: string;
  [key: string]: unknown;
}

export interface CanvasData {
  nodes: CanvasNode[];
  edges: CanvasEdge[];
  [key: string]: unknown;
}

export const EMPTY_CANVAS = '{\n\t"nodes":[],\n\t"edges":[]\n}';

/** Read a canvas file; an empty file is an empty canvas. */
export function parseCanvas(text: string): CanvasData {
  if (!text.trim()) return { nodes: [], edges: [] };
  const data = JSON.parse(text);
  if (!data || typeof data !== "object" || Array.isArray(data)) throw new Error("캔버스 파일이 아닙니다.");
  const num = (v: unknown, d: number) => (typeof v === "number" && Number.isFinite(v) ? v : d);
  data.nodes = (Array.isArray(data.nodes) ? data.nodes : []).filter(
    (n: unknown): n is CanvasNode => !!n && typeof n === "object" && typeof (n as CanvasNode).id === "string",
  );
  for (const n of data.nodes as CanvasNode[]) {
    n.x = num(n.x, 0);
    n.y = num(n.y, 0);
    n.width = Math.max(1, num(n.width, 250));
    n.height = Math.max(1, num(n.height, 60));
  }
  const ids = new Set((data.nodes as CanvasNode[]).map((n) => n.id));
  data.edges = (Array.isArray(data.edges) ? data.edges : []).filter(
    (e: unknown): e is CanvasEdge =>
      !!e && typeof e === "object" && ids.has((e as CanvasEdge).fromNode) && ids.has((e as CanvasEdge).toNode),
  );
  return data as CanvasData;
}

/** Written the way Obsidian writes canvases (tab-indented JSON). */
export function serializeCanvas(data: CanvasData): string {
  return JSON.stringify(data, null, "\t");
}

/** A 16-hex-digit id, like Obsidian's. */
export function newId(): string {
  const bytes = new Uint8Array(8);
  crypto.getRandomValues(bytes);
  return [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** Obsidian's six canvas colours (RGB). */
export const PRESET_COLORS: Record<string, string> = {
  "1": "233 49 71",
  "2": "236 117 0",
  "3": "224 172 0",
  "4": "8 185 78",
  "5": "0 191 188",
  "6": "120 82 238",
};

/** "r g b" for a node or edge colour, or null for the default. */
export function rgbOf(color: string | undefined): string | null {
  if (!color) return null;
  if (PRESET_COLORS[color]) return PRESET_COLORS[color];
  const m = /^#?([0-9a-f]{6})$/i.exec(color.trim());
  if (!m) return null;
  const n = parseInt(m[1], 16);
  return `${(n >> 16) & 255} ${(n >> 8) & 255} ${n & 255}`;
}

export interface Point {
  x: number;
  y: number;
}

/** The middle of one side of a card. */
export function anchor(n: CanvasNode, side: Side): Point {
  switch (side) {
    case "top":
      return { x: n.x + n.width / 2, y: n.y };
    case "bottom":
      return { x: n.x + n.width / 2, y: n.y + n.height };
    case "left":
      return { x: n.x, y: n.y + n.height / 2 };
    default:
      return { x: n.x + n.width, y: n.y + n.height / 2 };
  }
}

const NORMAL: Record<Side, Point> = { top: { x: 0, y: -1 }, bottom: { x: 0, y: 1 }, left: { x: -1, y: 0 }, right: { x: 1, y: 0 } };
const SIDES: Side[] = ["top", "right", "bottom", "left"];

/** The side of `n` facing point `p` (for arrows without a side, and drops). */
export function sideFacing(n: CanvasNode, p: Point): Side {
  const cx = n.x + n.width / 2;
  const cy = n.y + n.height / 2;
  // Compare against the card's own proportions, so wide cards prefer top/bottom less.
  const dx = (p.x - cx) / Math.max(1, n.width);
  const dy = (p.y - cy) / Math.max(1, n.height);
  if (Math.abs(dx) > Math.abs(dy)) return dx > 0 ? "right" : "left";
  return dy > 0 ? "bottom" : "top";
}

/** The sides an edge leaves and enters by. */
export function edgeSides(e: CanvasEdge, from: CanvasNode, to: CanvasNode): [Side, Side] {
  const center = (n: CanvasNode) => ({ x: n.x + n.width / 2, y: n.y + n.height / 2 });
  return [e.fromSide ?? sideFacing(from, center(to)), e.toSide ?? sideFacing(to, center(from))];
}

/** A curve from one side to another: path data, its middle (for labels) and the end direction. */
export function edgePath(a: Point, aSide: Side, b: Point, bSide: Side | null): { d: string; mid: Point } {
  const dist = Math.hypot(b.x - a.x, b.y - a.y);
  const reach = Math.max(40, Math.min(200, dist * 0.4));
  const na = NORMAL[aSide];
  const c1 = { x: a.x + na.x * reach, y: a.y + na.y * reach };
  const nb = bSide ? NORMAL[bSide] : { x: 0, y: 0 };
  const c2 = { x: b.x + nb.x * reach, y: b.y + nb.y * reach };
  // Bezier point at t = 0.5.
  const mid = { x: (a.x + 3 * c1.x + 3 * c2.x + b.x) / 8, y: (a.y + 3 * c1.y + 3 * c2.y + b.y) / 8 };
  return { d: `M${a.x} ${a.y} C${c1.x} ${c1.y} ${c2.x} ${c2.y} ${b.x} ${b.y}`, mid };
}

export { SIDES };

/** Cards lying wholly inside a group (they move with it). */
export function insideGroup(group: CanvasNode, nodes: CanvasNode[]): CanvasNode[] {
  return nodes.filter(
    (n) =>
      n !== group &&
      n.x >= group.x &&
      n.y >= group.y &&
      n.x + n.width <= group.x + group.width &&
      n.y + n.height <= group.y + group.height,
  );
}

/** The smallest box around `nodes` (null for none). */
export function bounds(nodes: CanvasNode[]): { x: number; y: number; width: number; height: number } | null {
  if (!nodes.length) return null;
  const x0 = Math.min(...nodes.map((n) => n.x));
  const y0 = Math.min(...nodes.map((n) => n.y));
  const x1 = Math.max(...nodes.map((n) => n.x + n.width));
  const y1 = Math.max(...nodes.map((n) => n.y + n.height));
  return { x: x0, y: y0, width: x1 - x0, height: y1 - y0 };
}
