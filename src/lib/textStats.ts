// Word and character counts for the note's status bar.

export interface TextStats {
  words: number;
  /** Characters, spaces included (line breaks not). */
  chars: number;
  /** Characters without any whitespace (as Korean writing counts often want). */
  nonSpace: number;
}

/** Counts for `text`; `skipFrontmatter` leaves out a leading `---` YAML block. */
export function textStats(text: string, skipFrontmatter = true): TextStats {
  const body = skipFrontmatter ? text.replace(/^﻿?---\r?\n[\s\S]*?\r?\n---[ \t]*(?:\r?\n|$)/, "") : text;
  return {
    words: body.match(/\S+/g)?.length ?? 0,
    chars: body.replace(/\r?\n/g, "").length,
    nonSpace: body.replace(/\s/g, "").length,
  };
}
