// Graph color groups (Obsidian "Groups"): a query picks nodes, the first
// matching group colors them. Supported query terms, all must match:
// `path:folder`, `file:name`, `tag:#tag` (nested tags too), `-term` (not),
// and plain words matched against the file name and path.

export interface GroupNode {
  id: string;
  title: string;
  tags: string[];
}

/** Validated categorical order (dataviz palette), for new groups. */
export const GROUP_COLORS = ["#2a78d6", "#eb6834", "#1baf7a", "#eda100", "#e87ba4", "#008300", "#6250d6", "#e34948"];

function tokens(query: string): string[] {
  const out: string[] = [];
  for (const m of query.matchAll(/(-?[a-z]+:)?"([^"]*)"|(\S+)/gi)) {
    out.push(m[3] ?? `${m[1] ?? ""}${m[2]}`);
  }
  return out.filter(Boolean);
}

type Test = (n: GroupNode) => boolean;

/** A node test for `query`, or null when the query is empty. */
export function compileQuery(query: string): Test | null {
  const tests: Test[] = [];
  for (const raw of tokens(query.trim())) {
    const negate = raw.startsWith("-") && raw.length > 1;
    const term = (negate ? raw.slice(1) : raw).toLowerCase();
    let test: Test;
    if (term.startsWith("path:")) {
      const v = term.slice(5);
      test = (n) => n.id.toLowerCase().includes(v);
    } else if (term.startsWith("file:")) {
      const v = term.slice(5);
      test = (n) => n.title.toLowerCase().includes(v);
    } else if (term.startsWith("tag:")) {
      const v = term.slice(4).replace(/^#/, "");
      test = (n) => n.tags.some((t) => {
        const l = t.toLowerCase();
        return l === v || l.startsWith(`${v}/`);
      });
    } else {
      test = (n) => n.title.toLowerCase().includes(term) || n.id.toLowerCase().includes(term);
    }
    tests.push(negate ? (n) => !test(n) : test);
  }
  return tests.length ? (n) => tests.every((t) => t(n)) : null;
}
