// The money left to spend, from account balances (the 자산 tab's 🏦 records)
// moved by the finance records written since.

import type { BalanceItem, FinanceItem } from "./api";

export interface Snapshot {
  /** Each non-saving account at its latest balance, summed (× 100). */
  amount: number;
  /** The newest of those balances' dates. */
  date: string;
}

/**
 * Money outside savings at the latest balance records, like the 자산 tab's
 * "쓸 수 있는 돈". `items` is oldest first (as `balances` returns it). Null
 * when no non-saving account has a dated balance.
 */
export function spendableSnapshot(items: BalanceItem[], isSaving: (group: string | null) => boolean): Snapshot | null {
  const latest = new Map<string, BalanceItem>();
  for (const b of items) latest.set(b.account, b);
  let amount = 0;
  let date: string | null = null;
  for (const b of latest.values()) {
    if (isSaving(b.group)) continue;
    amount += b.amount;
    if (b.date && (!date || b.date > date)) date = b.date;
  }
  return date ? { amount, date } : null;
}

/** Money in minus money out; savings are money out of these accounts too. */
export const netFlow = (items: FinanceItem[]) =>
  items.reduce((n, e) => n + (e.kind === "income" ? e.amount : -e.amount), 0);

/**
 * Money left at the end of `end` (YYYY-MM-DD): the snapshot plus the records
 * after it, or, for a day before it, minus the records between. Records on the
 * snapshot's own day are already in its balances.
 */
export function remainingAt(snap: Snapshot, end: string, records: FinanceItem[]): number {
  const between = (from: string, to: string) => records.filter((e) => !!e.date && e.date > from && e.date <= to);
  return end >= snap.date
    ? snap.amount + netFlow(between(snap.date, end))
    : snap.amount - netFlow(between(end, snap.date));
}
