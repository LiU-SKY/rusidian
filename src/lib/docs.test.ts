// The autosave doc store: the part of the app that decides what reaches disk,
// so a regression here loses notes. `./api` (Tauri commands) is replaced by an
// in-memory disk that saves like the Rust side: with `expected` given, a file
// that no longer holds that text is left alone and the save is a conflict.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readNote, writeNote, parseFrontmatter, type WriteResult } from "./api";
import { Doc, DocStore } from "./docs.svelte";

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  readNote: vi.fn(),
  writeNote: vi.fn(),
  parseFrontmatter: vi.fn(),
}));

const PATH = "notes/a.md";
const COPY = "notes/a (충돌 2026-11-15 0900 이 기기).md";

const lf = (s: string) => s.replace(/\r\n/g, "\n");

/** Files by path, as the Rust side would see them. */
let disk: Map<string, string>;
/** When set, each write waits for the test to call the returned release. */
let gate: (() => Promise<void>) | null;

const writes = () => vi.mocked(writeNote).mock.calls;

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
  vi.setSystemTime(new Date(2026, 10, 15, 9, 0));
  disk = new Map();
  gate = null;
  vi.mocked(readNote).mockImplementation(async (path) => {
    const text = disk.get(path);
    if (text === undefined) throw new Error(`cannot read ${path}`);
    return text;
  });
  vi.mocked(writeNote).mockImplementation(async (path, content, expected) => {
    if (gate) await gate();
    const now = disk.get(path);
    if (now !== undefined && expected !== undefined && lf(now) !== lf(expected)) {
      disk.set(COPY, content);
      return { conflict: COPY, disk: now } satisfies WriteResult;
    }
    // Keep the file's line endings, like keep_line_endings in core.
    disk.set(path, now?.includes("\r\n") && !content.includes("\r") ? content.replace(/\n/g, "\r\n") : content);
    return { conflict: null, disk: null } satisfies WriteResult;
  });
  vi.mocked(parseFrontmatter).mockResolvedValue({} as Awaited<ReturnType<typeof parseFrontmatter>>);
});

afterEach(() => {
  vi.useRealTimers();
  vi.clearAllMocks();
});

/** Let pending promise chains run (timers stay put). */
async function flush() {
  for (let i = 0; i < 20; i++) await Promise.resolve();
}

/** Writes wait until released, one release per write, in order. */
function holdWrites() {
  const waiting: (() => void)[] = [];
  gate = () => new Promise<void>((r) => waiting.push(r));
  return {
    get waiting() {
      return waiting.length;
    },
    async release() {
      waiting.shift()?.();
      await flush();
    },
  };
}

async function openDoc(text: string, handlers = { onerror: vi.fn(), onconflict: vi.fn() }) {
  disk.set(PATH, text);
  const doc = new Doc(PATH, handlers.onerror, handlers.onconflict);
  await doc.load();
  return { doc, ...handlers };
}

describe("Doc autosave", () => {
  it("saves 0.8s after the last edit, not before", async () => {
    const { doc } = await openDoc("one");
    doc.edit("two");
    expect(doc.dirty).toBe(true);

    await vi.advanceTimersByTimeAsync(500);
    doc.edit("three"); // restarts the wait
    await vi.advanceTimersByTimeAsync(799);
    expect(writeNote).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(1);
    await flush();
    expect(writes()).toEqual([[PATH, "three", "one", "2026-11-15 0900"]]);
    expect(disk.get(PATH)).toBe("three");
    expect(doc.dirty).toBe(false);
  });

  it("does not write when nothing changed", async () => {
    const { doc } = await openDoc("same");
    doc.edit("same");
    expect(doc.dirty).toBe(false);
    await doc.save();
    await vi.advanceTimersByTimeAsync(2000);
    expect(writeNote).not.toHaveBeenCalled();
  });

  it("an explicit save cancels the pending autosave", async () => {
    const { doc } = await openDoc("one");
    doc.edit("two");
    await doc.save();
    await vi.advanceTimersByTimeAsync(2000);
    expect(writes()).toHaveLength(1);
  });

  it("stays dirty when more is typed while a save is running", async () => {
    const { doc } = await openDoc("one");
    const hold = holdWrites();
    doc.edit("two");
    const saving = doc.save();
    await flush();
    doc.edit("two and more");
    await hold.release();
    await saving;
    expect(disk.get(PATH)).toBe("two");
    expect(doc.dirty).toBe(true);
  });

  it("reports a failed save and keeps the text unsaved", async () => {
    const { doc, onerror } = await openDoc("one");
    vi.mocked(writeNote).mockRejectedValueOnce("disk full");
    doc.edit("two");
    await doc.save();
    expect(onerror).toHaveBeenCalledWith("disk full");
    expect(doc.dirty).toBe(true);
    expect(doc.content).toBe("two");
  });
});

describe("Doc saves run one at a time", () => {
  it("overlapping saves do not overlap on disk or conflict with each other", async () => {
    const { doc, onconflict } = await openDoc("v0");
    const hold = holdWrites();

    doc.edit("v1");
    const first = doc.save();
    await flush();
    doc.edit("v2");
    const second = doc.save();
    await flush();
    // The second write has not started while the first is still running.
    expect(writes()).toHaveLength(1);
    expect(hold.waiting).toBe(1);

    await hold.release();
    await hold.release();
    await Promise.all([first, second]);

    // Each save checked against the text the one before it left on disk.
    expect(writes().map(([, content, expected]) => [content, expected])).toEqual([
      ["v1", "v0"],
      ["v2", "v1"],
    ]);
    expect(onconflict).not.toHaveBeenCalled();
    expect(disk.get(PATH)).toBe("v2");
    expect(doc.content).toBe("v2");
    expect(doc.dirty).toBe(false);
  });

  it("autosave firing during a manual save waits for it", async () => {
    const { doc, onconflict } = await openDoc("v0");
    const hold = holdWrites();
    doc.edit("v1");
    const manual = doc.save();
    await flush();
    doc.edit("v2");
    await vi.advanceTimersByTimeAsync(800);
    expect(writes()).toHaveLength(1);
    await hold.release();
    await hold.release();
    await manual;
    await flush();
    expect(onconflict).not.toHaveBeenCalled();
    expect(disk.get(PATH)).toBe("v2");
  });

  it("a failed save does not block the next one", async () => {
    const { doc } = await openDoc("v0");
    vi.mocked(writeNote).mockRejectedValueOnce("locked");
    doc.edit("v1");
    const first = doc.save();
    const second = doc.save();
    await Promise.all([first, second]);
    expect(writes()).toHaveLength(2);
    expect(disk.get(PATH)).toBe("v1");
    expect(doc.dirty).toBe(false);
  });
});

describe("Doc line endings", () => {
  it("takes CRLF text from disk as LF and does not mark it dirty", async () => {
    const { doc } = await openDoc("# Title\r\nline\r\n");
    expect(doc.content).toBe("# Title\nline\n");
    expect(doc.dirty).toBe(false);
    await vi.advanceTimersByTimeAsync(2000);
    await doc.save();
    expect(writeNote).not.toHaveBeenCalled();
  });

  it("an editor echoing the LF text back is not an edit", async () => {
    const { doc } = await openDoc("a\r\nb");
    doc.edit("a\nb");
    expect(doc.dirty).toBe(false);
  });

  it("a CRLF reload of the same text changes nothing", async () => {
    const { doc } = await openDoc("a\nb");
    doc.replaceFromDisk("a\r\nb");
    expect(doc.content).toBe("a\nb");
    expect(doc.dirty).toBe(false);
  });

  it("saving an edited CRLF note is not a conflict and keeps its CRLF", async () => {
    const { doc, onconflict } = await openDoc("a\r\nb\r\n");
    doc.edit("a\nb\nc\n");
    await doc.save();
    expect(writes()[0][2]).toBe("a\nb\n");
    expect(onconflict).not.toHaveBeenCalled();
    expect(disk.get(PATH)).toBe("a\r\nb\r\nc\r\n");
    expect(doc.dirty).toBe(false);
  });
});

describe("Doc save conflicts", () => {
  it("shows the file's text, clears dirty and reports the copy", async () => {
    const { doc, onconflict } = await openDoc("base");
    disk.set(PATH, "theirs\r\nedit\r\n"); // a sync pull or Obsidian, unnoticed
    doc.edit("mine");
    await doc.save();

    expect(disk.get(PATH)).toBe("theirs\r\nedit\r\n");
    expect(disk.get(COPY)).toBe("mine");
    expect(doc.content).toBe("theirs\nedit\n");
    expect(doc.dirty).toBe(false);
    expect(onconflict).toHaveBeenCalledExactlyOnceWith(PATH, COPY);
  });

  it("uses the file's text as the base for the next save", async () => {
    const { doc, onconflict } = await openDoc("base");
    disk.set(PATH, "theirs");
    doc.edit("mine");
    await doc.save();
    doc.edit("theirs, then mine");
    await doc.save();
    expect(onconflict).toHaveBeenCalledOnce();
    expect(writes()[1][2]).toBe("theirs");
    expect(disk.get(PATH)).toBe("theirs, then mine");
  });

  it("text typed while the conflicting save ran goes into the copy too", async () => {
    const { doc } = await openDoc("base");
    disk.set(PATH, "theirs");
    const hold = holdWrites();
    doc.edit("mine");
    const saving = doc.save();
    await flush();
    doc.edit("mine, more");
    await hold.release(); // the conflicting save
    await hold.release(); // the newer text written into the copy
    await saving;
    expect(disk.get(COPY)).toBe("mine, more");
    expect(doc.content).toBe("theirs");
  });

  it("a reload from disk resets the base", async () => {
    const { doc, onconflict } = await openDoc("base");
    disk.set(PATH, "theirs");
    doc.replaceFromDisk("theirs");
    doc.edit("theirs + mine");
    await doc.save();
    expect(onconflict).not.toHaveBeenCalled();
    expect(disk.get(PATH)).toBe("theirs + mine");
  });
});

describe("DocStore", () => {
  it("opens one doc per path and loads it", async () => {
    disk.set(PATH, "x\r\n");
    const store = new DocStore(vi.fn(), vi.fn());
    const doc = store.open(PATH);
    expect(store.open(PATH)).toBe(doc);
    await flush();
    expect(readNote).toHaveBeenCalledOnce();
    expect(doc.content).toBe("x\n");
  });

  it("flushAll saves every dirty doc", async () => {
    disk.set("a.md", "a");
    disk.set("b.md", "b");
    const store = new DocStore(vi.fn(), vi.fn());
    const a = store.open("a.md");
    const b = store.open("b.md");
    await flush();
    a.edit("a2");
    b.edit("b2");
    await store.flushAll();
    expect([disk.get("a.md"), disk.get("b.md")]).toEqual(["a2", "b2"]);
  });

  it("keepOnly keeps a doc whose save failed", async () => {
    disk.set("a.md", "a");
    const store = new DocStore(vi.fn(), vi.fn());
    store.open("a.md");
    await flush();
    store.find("a.md")!.edit("a2");
    vi.mocked(writeNote).mockRejectedValueOnce("locked");
    await store.keepOnly(new Set());
    expect(store.find("a.md")?.content).toBe("a2");
  });

  it("removed drops docs without saving them", async () => {
    disk.set("dir/a.md", "a");
    const store = new DocStore(vi.fn(), vi.fn());
    store.open("dir/a.md");
    await flush();
    store.find("dir/a.md")!.edit("a2");
    store.removed("dir");
    await vi.advanceTimersByTimeAsync(2000);
    expect(store.find("dir/a.md")).toBeUndefined();
    expect(writeNote).not.toHaveBeenCalled();
  });
});
