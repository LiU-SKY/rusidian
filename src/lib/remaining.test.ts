import { describe, expect, it } from "vitest";
import type { BalanceItem, FinanceItem } from "./api";
import { remainingAt, spendableSnapshot } from "./remaining";

const bal = (account: string, won: number, group: string | null, date: string | null): BalanceItem => ({
  path: "자산.md", title: "자산", line: 0, raw: "", amount: won * 100, account, group, date,
});
const rec = (kind: "income" | "expense", won: number, date: string, category: string | null = null): FinanceItem => ({
  path: "가계부.md", title: "가계부", line: 0, raw: "", kind, amount: won * 100, memo: "", category, date,
  explicit_date: true, parent_line: null,
});
const isSaving = (g: string | null) => g === "저축" || !!g?.startsWith("저축/");

describe("spendableSnapshot", () => {
  it("sums each non-saving account at its latest balance", () => {
    const snap = spendableSnapshot([
      bal("카카오페이머니", 9_000, "페이", "2026-11-01"),
      bal("카카오페이머니", 5_300, "페이", "2026-11-14"),
      bal("토스뱅크 입출금", 3_183, "입출금", "2026-11-14"),
      bal("토스뱅크 외화통장", 737, "외화", "2026-11-14"),
      bal("주택청약종합저축", 1_060_000, "저축", "2026-11-14"),
    ], isSaving);
    expect(snap).toEqual({ amount: 9_220_00, date: "2026-11-14" });
  });

  it("is null without a dated non-saving balance", () => {
    expect(spendableSnapshot([], isSaving)).toBeNull();
    expect(spendableSnapshot([bal("청약", 100, "저축", "2026-11-14")], isSaving)).toBeNull();
    expect(spendableSnapshot([bal("지갑", 100, null, null)], isSaving)).toBeNull();
  });
});

describe("remainingAt", () => {
  const snap = { amount: 9_220_00, date: "2026-11-14" };

  it("is the snapshot on its own day (its records are already in the balances)", () => {
    expect(remainingAt(snap, "2026-11-14", [rec("expense", 1_000, "2026-11-14")])).toBe(9_220_00);
  });

  it("adds income and takes spending and savings after the snapshot", () => {
    const later = [rec("income", 889_200, "2026-11-23"), rec("expense", 10_000, "2026-11-24"), rec("expense", 300_000, "2026-11-26", "저축/청년도약")];
    expect(remainingAt(snap, "2026-11-30", later)).toBe((9_220 + 889_200 - 10_000 - 300_000) * 100);
    expect(remainingAt(snap, "2026-11-23", later)).toBe((9_220 + 889_200) * 100);
  });

  it("goes back from the snapshot for an earlier month's end", () => {
    const nov = [rec("expense", 300_000, "2026-11-09"), rec("income", 21, "2026-11-01"), rec("expense", 5_000, "2026-10-31")];
    // Oct 31 is the end itself, so its record stays in.
    expect(remainingAt(snap, "2026-10-31", nov)).toBe((9_220 + 300_000 - 21) * 100);
  });
});
