// Money records grouped for display: a payment together with the
// settlements indented under it (money back from people who shared the
// bill, refunds), and memo text split into what is shown where.

import type { FinanceItem } from "./api";

export const recordKey = (e: { path: string; line: number }) => `${e.path}:${e.line}`;

/** A record and the settlements written under it. */
export interface TxGroup {
  item: FinanceItem;
  children: FinanceItem[];
  /** Amount after settlements (× 100); equals item.amount without children. */
  net: number;
  /** This record is itself a settlement whose payment is not in the list (another month). */
  orphan: boolean;
}

/** Index every record's settlements by the parent's key. */
export function childrenByParent(all: FinanceItem[]): Map<string, FinanceItem[]> {
  const map = new Map<string, FinanceItem[]>();
  for (const e of all) {
    if (e.parent_line === null) continue;
    const key = recordKey({ path: e.path, line: e.parent_line });
    const list = map.get(key);
    if (list) list.push(e);
    else map.set(key, [e]);
  }
  return map;
}

/**
 * `shown` grouped: payments carry their settlements (looked up in `all`, so a
 * settlement from another month still counts toward its payment); a
 * settlement whose payment is not in `shown` stands on its own.
 */
export function groupRecords(shown: FinanceItem[], all: FinanceItem[]): TxGroup[] {
  const kids = childrenByParent(all);
  const shownKeys = new Set(shown.map(recordKey));
  const out: TxGroup[] = [];
  for (const e of shown) {
    if (e.parent_line !== null && shownKeys.has(recordKey({ path: e.path, line: e.parent_line }))) continue;
    const children = kids.get(recordKey(e)) ?? [];
    out.push({
      item: e,
      children,
      net: e.amount + children.reduce((n, c) => n + c.amount, 0),
      orphan: e.parent_line !== null,
    });
  }
  return out;
}

/** The parent record of a settlement, if it is loaded. */
export function parentOf(e: FinanceItem, all: FinanceItem[]): FinanceItem | undefined {
  if (e.parent_line === null) return undefined;
  return all.find((p) => p.path === e.path && p.line === e.parent_line);
}

/**
 * "김민준에게서 돌려받음 — 맘스터치 30,600원의 1/3 [카카오페이]" →
 * title "김민준에게서 돌려받음", detail "맘스터치 30,600원의 1/3", account "카카오페이".
 */
export function splitMemo(memo: string): { title: string; detail: string; account: string } {
  let text = memo.trim();
  let account = "";
  const acc = /\s*\[([^\]]+)\]$/.exec(text);
  if (acc) {
    account = acc[1].trim();
    text = text.slice(0, acc.index).trim();
  }
  const [title, ...rest] = text.split(" — ");
  return { title: title.trim(), detail: rest.join(" — ").trim(), account };
}
