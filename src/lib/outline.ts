// Headings of a note, for the outline pane and `[[Note#Heading]]` links.

export interface Heading {
  level: number;
  /** Shown text: link brackets and emphasis markers removed. */
  text: string;
  /** 0-based line. */
  line: number;
}

const ATX = /^ {0,3}(#{1,6})(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*$/;
const FENCE = /^ {0,3}(`{3,}|~{3,})/;

/** Plain text of a heading as Obsidian shows it in the outline. */
export function headingText(raw: string): string {
  return raw
    .replace(/!?\[\[([^[\]|]*?)(?:\|([^[\]]*?))?\]\]/g, (_, target, alias) => alias ?? target)
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/(\*\*|__|\*|_|~~|==|`)(.+?)\1/g, "$2")
    .trim();
}

/** Number of lines the frontmatter block takes (0 if none). */
export function frontmatterLines(lines: string[]): number {
  if (lines[0]?.replace(/^﻿/, "").trimEnd() !== "---") return 0;
  const close = lines.findIndex((l, n) => n > 0 && l.trimEnd() === "---");
  return close > 0 ? close + 1 : 0;
}

/** The note body, with frontmatter lines blanked (so line numbers stay the same). */
export function bodyOf(text: string): string {
  const lines = text.split(/\r?\n/);
  const n = frontmatterLines(lines);
  return "\n".repeat(n) + lines.slice(n).join("\n");
}

/** Lines `from` heading up to the next heading of the same or a higher level. */
export function sectionOf(text: string, line: number): string {
  const lines = text.split(/\r?\n/);
  const all = headings(text);
  const h = all.find((x) => x.line === line);
  const next = h ? all.find((x) => x.line > line && x.level <= h.level) : undefined;
  return lines.slice(line, next ? next.line : lines.length).join("\n");
}

export function headings(text: string): Heading[] {
  const lines = text.split(/\r?\n/);
  const out: Heading[] = [];
  let i = frontmatterLines(lines);
  let fence: string | null = null;
  for (; i < lines.length; i++) {
    const f = lines[i].match(FENCE);
    if (f) {
      const marker = f[1];
      if (fence === null) fence = marker;
      else if (marker[0] === fence[0] && marker.length >= fence.length) fence = null;
      continue;
    }
    if (fence !== null) continue;
    const m = lines[i].match(ATX);
    if (!m) continue;
    const text = headingText(m[2] ?? "");
    if (text) out.push({ level: m[1].length, text, line: i });
  }
  return out;
}

/** Line of the heading a `#subpath` link points to (Obsidian matches the text, case-insensitively). */
export function findHeading(text: string, subpath: string): number | null {
  // `[[Note#A#B]]` points to B under A; the last part is enough here.
  const wanted = subpath.split("#").filter(Boolean).pop()?.trim().toLowerCase();
  if (!wanted) return null;
  const norm = (s: string) => s.toLowerCase().replace(/\s+/g, " ");
  return headings(text).find((h) => norm(h.text) === norm(wanted))?.line ?? null;
}
