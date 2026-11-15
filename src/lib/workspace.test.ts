import { describe, expect, it } from "vitest";
import { Leaf, MAX_PANES, Workspace } from "./workspace.svelte";

const all = () => true;

/** Two panes: notes a.md (reading) + tasks | graph (local) centred on b.md. */
function sample(): Workspace {
  const ws = new Workspace();
  ws.pane.tabs = [new Leaf("file", "a.md"), new Leaf("tasks")];
  ws.pane.tabs[0].reading = true;
  ws.pane.active = 1;
  ws.splitRight();
  const graph = new Leaf("graph", "b.md");
  graph.graphLocal = true;
  ws.pane.tabs = [graph];
  return ws;
}

const shape = (ws: Workspace) => ({
  activePane: ws.activePane,
  panes: ws.panes.map((p) => ({
    active: p.active,
    tabs: p.tabs.map((l) => [l.kind, l.path, l.reading, l.graphLocal]),
  })),
});

describe("Workspace serialize/restore", () => {
  it("round-trips panes, tabs, active indices and per-tab flags", () => {
    const ws = sample();
    const json = ws.serialize();
    const back = new Workspace();
    expect(back.restore(json, all)).toBe(true);
    expect(shape(back)).toEqual(shape(ws));
    expect(shape(back)).toEqual({
      activePane: 1,
      panes: [
        {
          active: 1,
          tabs: [
            ["file", "a.md", true, false],
            ["tasks", null, false, false],
          ],
        },
        { active: 0, tabs: [["graph", "b.md", false, true]] },
      ],
    });
  });

  it("gives restored leaves fresh ids", () => {
    const ws = sample();
    const back = new Workspace();
    back.restore(ws.serialize(), all);
    const ids = new Set([...ws.leaves(), ...back.leaves()].map((l) => l.id));
    expect(ids.size).toBe(ws.leaves().length + back.leaves().length);
  });

  it("drops tabs whose file no longer exists and clamps the active tab", () => {
    const ws = new Workspace();
    ws.pane.tabs = [new Leaf("calendar"), new Leaf("file", "gone.md"), new Leaf("file", "kept.md")];
    ws.pane.active = 2;
    const back = new Workspace();
    expect(back.restore(ws.serialize(), (p) => p !== "gone.md")).toBe(true);
    expect(back.pane.tabs.map((l) => l.path)).toEqual([null, "kept.md"]);
    expect(back.pane.active).toBe(1);
  });

  it("drops a pane left with no tabs and clamps the active pane", () => {
    const ws = sample();
    ws.panes[1].tabs = [new Leaf("file", "gone.md")];
    const back = new Workspace();
    expect(back.restore(ws.serialize(), (p) => p !== "gone.md")).toBe(true);
    expect(back.panes).toHaveLength(1);
    expect(back.activePane).toBe(0);
  });

  it("does not ask whether non-file tabs exist", () => {
    const ws = new Workspace();
    ws.pane.tabs = [new Leaf("graph", "centre.md")];
    const asked: string[] = [];
    const back = new Workspace();
    back.restore(ws.serialize(), (p) => (asked.push(p), false));
    expect(asked).toEqual([]);
    expect(back.leaf.kind).toBe("graph");
  });

  it("keeps at most MAX_PANES panes", () => {
    const saved = {
      panes: Array.from({ length: MAX_PANES + 2 }, (_, i) => ({ active: 0, tabs: [{ kind: "file", path: `${i}.md` }] })),
      activePane: MAX_PANES + 1,
    };
    const back = new Workspace();
    expect(back.restore(JSON.stringify(saved), all)).toBe(true);
    expect(back.panes).toHaveLength(MAX_PANES);
    expect(back.activePane).toBe(MAX_PANES - 1);
  });

  it("rejects bad input and leaves the layout alone", () => {
    const ws = sample();
    const before = shape(ws);
    for (const json of ["", "not json", "null", "{}", '{"panes":"x"}', '{"panes":[]}', '{"panes":[{"tabs":[]}]}']) {
      expect(ws.restore(json, all), json).toBe(false);
      expect(shape(ws)).toEqual(before);
    }
  });

  it("tolerates junk entries and missing fields", () => {
    const json = JSON.stringify({
      panes: [{ tabs: [null, 3, { path: "x.md" }, { kind: "file" }, { kind: "file", path: "ok.md" }], active: "zz" }],
      activePane: -4,
    });
    const back = new Workspace();
    expect(back.restore(json, all)).toBe(true);
    expect(back.pane.tabs.map((l) => [l.kind, l.path, l.reading])).toEqual([["file", "ok.md", false]]);
    expect(back.pane.active).toBe(0);
    expect(back.activePane).toBe(0);
  });
});

describe("Workspace tab bookkeeping", () => {
  it("follows moves of a file or a folder", () => {
    const ws = new Workspace();
    ws.pane.tabs = [new Leaf("file", "notes/a.md"), new Leaf("file", "notes-old/b.md"), new Leaf("file", "c.md")];
    ws.moved("notes", "archive/notes");
    ws.moved("c.md", "d.md");
    expect(ws.pane.tabs.map((l) => l.path)).toEqual(["archive/notes/a.md", "notes-old/b.md", "d.md"]);
  });

  it("empties tabs of removed files and counts them", () => {
    const ws = new Workspace();
    ws.pane.tabs = [new Leaf("file", "x/a.md"), new Leaf("graph", "x/a.md"), new Leaf("file", "y.md")];
    ws.pane.tabs[0].reading = true;
    expect(ws.removed("x")).toBe(1);
    expect(ws.pane.tabs.map((l) => [l.kind, l.path, l.reading])).toEqual([
      ["empty", null, false],
      ["graph", "x/a.md", false],
      ["file", "y.md", false],
    ]);
  });

  it("closing the last tab of a split pane closes the pane", () => {
    const ws = sample();
    ws.closeTab(ws.panes[1], 0);
    expect(ws.panes).toHaveLength(1);
    expect(ws.activePane).toBe(0);
  });

  it("unsplit keeps the other pane's non-empty tabs", () => {
    const ws = sample();
    ws.panes[1].tabs.push(new Leaf());
    ws.focus(ws.panes[0]);
    ws.unsplit();
    expect(ws.panes).toHaveLength(1);
    expect(ws.pane.tabs.map((l) => l.kind)).toEqual(["file", "tasks", "graph"]);
  });
});
