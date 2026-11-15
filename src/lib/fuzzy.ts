// Fuzzy matching for the quick switcher and command palette: the query's
// characters must appear in order; contiguous runs, word starts and
// early matches score higher. Spaces in the query are ignored.

export interface FuzzyMatch {
  score: number;
  /** Indices of matched characters in `text` (for highlighting). */
  indices: number[];
}

const BOUNDARY = /[\s/_\-.()[\]]/;

export function fuzzyMatch(query: string, text: string): FuzzyMatch | null {
  const q = query.toLowerCase().replace(/\s+/g, "");
  if (!q) return { score: 0, indices: [] };
  const t = text.toLowerCase();

  // A plain substring beats any scattered match.
  const at = t.indexOf(q);
  if (at >= 0) {
    const wordStart = at === 0 || BOUNDARY.test(t[at - 1]);
    return {
      score: 100 + (wordStart ? 20 : 0) + (at === 0 ? 10 : 0) - at * 0.5 - t.length * 0.1,
      indices: Array.from({ length: q.length }, (_, k) => at + k),
    };
  }

  const indices: number[] = [];
  let score = 0;
  let from = 0;
  for (const ch of q) {
    const found = t.indexOf(ch, from);
    if (found < 0) return null;
    const prev = indices[indices.length - 1];
    if (prev !== undefined && found === prev + 1) score += 6;
    if (found === 0 || BOUNDARY.test(t[found - 1])) score += 4;
    score -= Math.min(found - from, 8) * 0.5;
    indices.push(found);
    from = found + 1;
  }
  return { score: score - t.length * 0.1, indices };
}
