import { describe, expect, it } from "vitest";
import { fuzzyMatch } from "./fuzzy";

const score = (q: string, t: string) => fuzzyMatch(q, t)?.score ?? -Infinity;

describe("fuzzyMatch", () => {
  it("matches everything with an empty or blank query", () => {
    expect(fuzzyMatch("", "anything")).toEqual({ score: 0, indices: [] });
    expect(fuzzyMatch("   ", "anything")).toEqual({ score: 0, indices: [] });
  });

  it("returns null when the characters are not all there in order", () => {
    expect(fuzzyMatch("xyz", "daily note")).toBeNull();
    expect(fuzzyMatch("ba", "ab")).toBeNull();
  });

  it("ignores case and spaces in the query", () => {
    const m = fuzzyMatch("D N", "daily/Note");
    expect(m).not.toBeNull();
    expect(m!.indices).toEqual([0, 6]);
  });

  it("gives the indices of a substring match", () => {
    expect(fuzzyMatch("note", "my note.md")!.indices).toEqual([3, 4, 5, 6]);
  });

  it("gives in-order indices for a scattered match", () => {
    expect(fuzzyMatch("mnt", "my note")!.indices).toEqual([0, 3, 5]);
  });

  it("works on non-ASCII text", () => {
    expect(fuzzyMatch("회의", "2026 회의록")!.indices).toEqual([5, 6]);
    expect(fuzzyMatch("회록", "2026 회의록")!.indices).toEqual([5, 7]);
  });

  it("ranks a substring above any scattered match", () => {
    expect(score("note", "a very long path/with notes in it")).toBeGreaterThan(score("note", "n-o-t-e"));
  });

  it("ranks a match at the start above one later on", () => {
    expect(score("plan", "plan.md")).toBeGreaterThan(score("plan", "the plan"));
  });

  it("ranks a word start above a mid-word match", () => {
    expect(score("log", "work/log")).toBeGreaterThan(score("log", "worklog."));
  });

  it("ranks contiguous runs above spread-out characters", () => {
    expect(score("abc", "abxc")).toBeGreaterThan(score("abc", "axbxc"));
  });

  it("prefers the shorter text when otherwise equal", () => {
    expect(score("idea", "idea.md")).toBeGreaterThan(score("idea", "idea-backlog.md"));
  });

  it("sorts quick-switcher candidates sensibly", () => {
    const files = ["archive/old meeting.md", "meetings/2026-11.md", "Meeting.md", "m-e-e-t.md"];
    const ranked = files
      .map((f) => ({ f, m: fuzzyMatch("meet", f) }))
      .filter((x) => x.m)
      .sort((a, b) => b.m!.score - a.m!.score)
      .map((x) => x.f);
    expect(ranked[0]).toBe("Meeting.md");
    expect(ranked.at(-1)).toBe("m-e-e-t.md");
  });
});
