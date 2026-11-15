<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { displayName, filePaths, mediaKind, readNote, refreshTree, resolveLink, vaultUrl } from "./api";
  import {
    anchor, bounds, edgePath, edgeSides, insideGroup, newId, parseCanvas, PRESET_COLORS, rgbOf, serializeCanvas, SIDES,
    sideFacing, type CanvasData, type CanvasEdge, type CanvasNode, type Point, type Side,
  } from "./canvas";
  import type { Doc } from "./docs.svelte";
  import Icon from "./Icon.svelte";
  import { bodyOf } from "./outline";
  import { linkTags, makeCallouts, renderMarkdown } from "./render";
  import { renderMath } from "./mathMermaid";

  interface Props {
    doc: Doc;
    /** Open a vault file (in a new tab when `newTab`). */
    onopen: (path: string, newTab?: boolean) => void;
    onlink: (target: string, heading: string | null, source: string) => void;
    onerror: (message: string) => void;
  }

  let { doc, onopen, onlink, onerror }: Props = $props();
  /** Keeps this canvas's SVG ids apart from another open canvas's. */
  const uid = `cv${newId().slice(0, 8)}`;

  let data = $state<CanvasData>({ nodes: [], edges: [] });
  let broken = $state<string | null>(null);
  /** The text last read or written, to tell our own saves from outside changes. */
  let lastText: string | null = null;

  let host: HTMLDivElement | undefined = $state();
  let width = $state(0);
  let height = $state(0);
  let view = $state({ x: 0, y: 0, k: 1 });
  let fitted = false;

  let selected = $state<Set<string>>(new Set());
  let selectedEdge = $state<string | null>(null);
  let editing = $state<string | null>(null);
  let editingEdge = $state<string | null>(null);
  let hovered = $state<string | null>(null);
  /** An arrow being drawn from a card's side to the pointer. */
  let linking = $state<{ from: string; side: Side; to: Point } | null>(null);
  let picker = $state<{ query: string; files: string[] } | null>(null);

  // ------------------------------------------------------------ file sync
  $effect(() => {
    const text = doc.content;
    if (!doc.loaded || text === lastText) return;
    untrack(() => {
      try {
        data = parseCanvas(text);
        broken = null;
      } catch (e) {
        broken = `캔버스를 읽을 수 없습니다: ${String((e as Error)?.message ?? e)}`;
      }
      lastText = text;
      selected = new Set([...selected].filter((id) => data.nodes.some((n) => n.id === id)));
      if (!fitted && width > 0) fit(false);
    });
  });

  /** Save the canvas (the doc autosaves shortly after). */
  function commit() {
    if (broken) return;
    const text = serializeCanvas($state.snapshot(data) as CanvasData);
    lastText = text;
    doc.edit(text);
  }

  // ------------------------------------------------------------ view
  const toWorld = (sx: number, sy: number): Point => ({ x: (sx - view.x) / view.k, y: (sy - view.y) / view.k });

  function screenXY(e: { clientX: number; clientY: number }): [number, number] {
    const r = host!.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  }

  function fit(animate = true) {
    if (width === 0) return;
    fitted = true;
    const b = bounds(data.nodes);
    const target = b
      ? (() => {
          const pad = 60;
          const k = Math.min(1, (width - pad * 2) / Math.max(b.width, 1), (height - pad * 2 - 50) / Math.max(b.height, 1));
          const kk = Math.max(0.1, k);
          return { k: kk, x: width / 2 - (b.x + b.width / 2) * kk, y: (height - 50) / 2 - (b.y + b.height / 2) * kk };
        })()
      : { k: 1, x: width / 2, y: height / 2 };
    if (!animate || matchMedia("(prefers-reduced-motion: reduce)").matches) {
      zoomTo = null;
      view = target;
      return;
    }
    glideTo(target);
  }

  // Zooming eases toward its target, like the graph view.
  const ZOOM_TAU = 55;
  let zoomTo: { k: number; sx: number; sy: number; last: number } | null = null;
  let glide: { from: typeof view; to: typeof view; t0: number } | null = null;

  function glideTo(to: typeof view) {
    zoomTo = null;
    glide = { from: { ...view }, to, t0: performance.now() };
    const step = (now: number) => {
      if (!glide) return;
      const t = Math.min(1, (now - glide.t0) / 380);
      const e = 1 - (1 - t) ** 3;
      const { from, to } = glide;
      view = { k: from.k * Math.pow(to.k / from.k, e), x: from.x + (to.x - from.x) * e, y: from.y + (to.y - from.y) * e };
      if (t < 1) requestAnimationFrame(step);
      else glide = null;
    };
    requestAnimationFrame(step);
  }

  function zoomAt(k: number, sx: number, sy: number) {
    const w = toWorld(sx, sy);
    view = { k, x: sx - w.x * k, y: sy - w.y * k };
  }

  function zoomBy(factor: number, sx = width / 2, sy = height / 2) {
    glide = null;
    const running = zoomTo !== null;
    const k = Math.min(4, Math.max(0.1, (running ? zoomTo!.k : view.k) * factor));
    if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
      zoomAt(k, sx, sy);
      return;
    }
    if (running) {
      Object.assign(zoomTo!, { k, sx, sy });
      return;
    }
    zoomTo = { k, sx, sy, last: performance.now() };
    const step = (now: number) => {
      const z = zoomTo;
      if (!z) return;
      const dt = Math.min(64, Math.max(0, now - z.last));
      z.last = now;
      let k = view.k * Math.pow(z.k / view.k, 1 - Math.exp(-dt / ZOOM_TAU));
      if (Math.abs(Math.log(z.k / k)) < 0.003) k = z.k;
      zoomAt(k, z.sx, z.sy);
      if (k === z.k) zoomTo = null;
      else requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  }

  /** Scroll pans (Shift: sideways); Ctrl+scroll or a pinch zooms, as in Obsidian. */
  function onwheel(e: WheelEvent) {
    if ((e.target as HTMLElement).closest("textarea, .picker")) return;
    e.preventDefault();
    if (e.ctrlKey || e.metaKey) {
      const [sx, sy] = screenXY(e);
      zoomBy(Math.exp(-e.deltaY * (e.deltaMode === 1 ? 0.05 : 0.0025)), sx, sy);
      return;
    }
    glide = null;
    const scale = e.deltaMode === 1 ? 20 : 1;
    const dx = e.shiftKey && !e.deltaX ? e.deltaY : e.deltaX;
    const dy = e.shiftKey && !e.deltaX ? 0 : e.deltaY;
    view = { ...view, x: view.x - dx * scale, y: view.y - dy * scale };
  }

  onMount(() => {
    const ro = new ResizeObserver(() => {
      if (!host) return;
      width = host.clientWidth;
      height = host.clientHeight;
      if (!fitted && width > 0 && doc.loaded) fit(false);
    });
    if (host) ro.observe(host);
    return () => ro.disconnect();
  });

  // ------------------------------------------------------------ gestures
  type Drag =
    | { kind: "pan"; sx: number; sy: number; vx: number; vy: number; moved: boolean }
    | { kind: "move"; start: Point; items: { n: CanvasNode; x: number; y: number }[]; moved: boolean }
    | { kind: "resize"; n: CanvasNode; start: Point; w: number; h: number }
    | { kind: "link" };
  let drag: Drag | null = null;

  const byId = (id: string) => data.nodes.find((n) => n.id === id);

  /** The card under a world point (the smallest; groups last). */
  function nodeAt(p: Point, except?: string): CanvasNode | null {
    const hits = data.nodes.filter(
      (n) => n.id !== except && p.x >= n.x && p.y >= n.y && p.x <= n.x + n.width && p.y <= n.y + n.height,
    );
    hits.sort((a, b) => (a.type === "group" ? 1 : 0) - (b.type === "group" ? 1 : 0) || a.width * a.height - b.width * b.height);
    return hits[0] ?? null;
  }

  function onBackgroundDown(e: PointerEvent) {
    if (e.button !== 0 && e.button !== 1) return;
    host?.focus();
    finishEditing();
    const [sx, sy] = screenXY(e);
    drag = { kind: "pan", sx, sy, vx: view.x, vy: view.y, moved: false };
    host!.setPointerCapture(e.pointerId);
  }

  function onNodeDown(e: PointerEvent, n: CanvasNode) {
    if (e.button !== 0 || editing === n.id) return;
    e.stopPropagation();
    host?.focus();
    if (editing) finishEditing();
    selectedEdge = null;
    if (e.shiftKey) {
      const next = new Set(selected);
      if (next.has(n.id)) next.delete(n.id);
      else next.add(n.id);
      selected = next;
    } else if (!selected.has(n.id)) {
      selected = new Set([n.id]);
    }
    // Moving a group takes the cards inside it along.
    const moving = new Map<string, CanvasNode>();
    for (const id of selected) {
      const m = byId(id);
      if (!m) continue;
      moving.set(m.id, m);
      if (m.type === "group") for (const c of insideGroup(m, data.nodes)) moving.set(c.id, c);
    }
    const [sx, sy] = screenXY(e);
    drag = { kind: "move", start: toWorld(sx, sy), items: [...moving.values()].map((m) => ({ n: m, x: m.x, y: m.y })), moved: false };
    host!.setPointerCapture(e.pointerId);
  }

  function onResizeDown(e: PointerEvent, n: CanvasNode) {
    if (e.button !== 0) return;
    e.stopPropagation();
    const [sx, sy] = screenXY(e);
    drag = { kind: "resize", n, start: toWorld(sx, sy), w: n.width, h: n.height };
    host!.setPointerCapture(e.pointerId);
  }

  function onHandleDown(e: PointerEvent, n: CanvasNode, side: Side) {
    if (e.button !== 0) return;
    e.stopPropagation();
    const [sx, sy] = screenXY(e);
    linking = { from: n.id, side, to: toWorld(sx, sy) };
    drag = { kind: "link" };
    host!.setPointerCapture(e.pointerId);
  }

  function onpointermove(e: PointerEvent) {
    if (!drag) return;
    const [sx, sy] = screenXY(e);
    const p = toWorld(sx, sy);
    if (drag.kind === "pan") {
      if (Math.abs(sx - drag.sx) + Math.abs(sy - drag.sy) > 3) drag.moved = true;
      glide = null;
      zoomTo = null;
      view = { ...view, x: drag.vx + sx - drag.sx, y: drag.vy + sy - drag.sy };
    } else if (drag.kind === "move") {
      const dx = p.x - drag.start.x;
      const dy = p.y - drag.start.y;
      if (Math.abs(dx) + Math.abs(dy) > 2 / view.k) drag.moved = true;
      if (!drag.moved) return;
      for (const it of drag.items) {
        it.n.x = Math.round(it.x + dx);
        it.n.y = Math.round(it.y + dy);
      }
    } else if (drag.kind === "resize") {
      drag.n.width = Math.max(60, Math.round(drag.w + p.x - drag.start.x));
      drag.n.height = Math.max(40, Math.round(drag.h + p.y - drag.start.y));
    } else if (linking) {
      linking = { ...linking, to: p };
    }
  }

  function onpointerup(e: PointerEvent) {
    const d = drag;
    drag = null;
    if (!d) return;
    if (d.kind === "pan" && !d.moved) {
      selected = new Set();
      selectedEdge = null;
    } else if (d.kind === "move" && d.moved) {
      commit();
    } else if (d.kind === "resize") {
      commit();
    } else if (d.kind === "link" && linking) {
      const l = linking;
      linking = null;
      const from = byId(l.from);
      if (!from) return;
      let to = nodeAt(l.to, l.from);
      if (!to) {
        // Dropped on empty space: a new card there, as in Obsidian.
        to = addNode({ type: "text", text: "", x: l.to.x - 125, y: l.to.y - 30, width: 250, height: 60 }, false);
        editing = to.id;
      }
      data.edges.push({ id: newId(), fromNode: from.id, fromSide: l.side, toNode: to.id, toSide: sideFacing(to, l.to) });
      commit();
    }
    if (host?.hasPointerCapture(e.pointerId)) host.releasePointerCapture(e.pointerId);
  }

  // ------------------------------------------------------------ editing
  function addNode(fields: Partial<CanvasNode> & { type: string }, save = true): CanvasNode {
    data.nodes.push({ id: newId(), x: 0, y: 0, width: 250, height: 60, ...fields } as CanvasNode);
    const n = data.nodes[data.nodes.length - 1];
    selected = new Set([n.id]);
    selectedEdge = null;
    if (save) commit();
    return n;
  }

  /** Where new cards go: the middle of the screen. */
  const centre = () => toWorld(width / 2, (height - 50) / 2);

  async function addText(at?: Point) {
    const c = at ?? centre();
    const n = addNode({ type: "text", text: "", x: Math.round(c.x - 125), y: Math.round(c.y - 30), width: 250, height: 60 });
    editing = n.id;
  }

  function addGroup() {
    const chosen = data.nodes.filter((n) => selected.has(n.id));
    const b = bounds(chosen);
    const pad = 30;
    const box = b
      ? { x: b.x - pad, y: b.y - pad - 10, width: b.width + pad * 2, height: b.height + pad * 2 + 10 }
      : (() => {
          const c = centre();
          return { x: Math.round(c.x - 200), y: Math.round(c.y - 150), width: 400, height: 300 };
        })();
    // Groups go first so cards draw on top of them.
    const g = { id: newId(), type: "group", label: "그룹", ...box } as CanvasNode;
    data.nodes.unshift(g);
    selected = new Set([g.id]);
    commit();
    editing = g.id;
  }

  async function openPicker() {
    try {
      const files = filePaths(await refreshTree()).filter((p) => !p.toLowerCase().endsWith(".canvas"));
      picker = { query: "", files };
    } catch (e) {
      onerror(String(e));
    }
  }

  function addFile(path: string, at?: Point) {
    picker = null;
    const c = at ?? centre();
    const media = mediaKind(path) === "image";
    const [w, h] = path.toLowerCase().endsWith(".md") ? [400, 400] : media ? [400, 300] : [300, 80];
    addNode({ type: "file", file: path, x: Math.round(c.x - w / 2), y: Math.round(c.y - h / 2), width: w, height: h });
  }

  function deleteSelection() {
    if (selectedEdge) {
      data.edges = data.edges.filter((e) => e.id !== selectedEdge);
      selectedEdge = null;
      commit();
      return;
    }
    if (!selected.size) return;
    data.nodes = data.nodes.filter((n) => !selected.has(n.id));
    data.edges = data.edges.filter((e) => !selected.has(e.fromNode) && !selected.has(e.toNode));
    selected = new Set();
    commit();
  }

  function setColor(color: string | null) {
    const apply = (o: { color?: string }) => {
      if (color) o.color = color;
      else delete o.color;
    };
    if (selectedEdge) {
      const e = data.edges.find((x) => x.id === selectedEdge);
      if (e) apply(e);
    }
    for (const n of data.nodes) if (selected.has(n.id)) apply(n);
    commit();
  }

  function finishEditing() {
    if (editing) {
      const n = byId(editing);
      editing = null;
      // An empty new card is dropped, as in Obsidian.
      if (n && n.type === "text" && !n.text?.trim() && !data.edges.some((e) => e.fromNode === n.id || e.toNode === n.id)) {
        data.nodes = data.nodes.filter((x) => x !== n);
        selected = new Set();
      }
      commit();
    }
    if (editingEdge) {
      editingEdge = null;
      commit();
    }
  }

  function onNodeDblClick(e: MouseEvent, n: CanvasNode) {
    e.stopPropagation();
    if (n.type === "text" || n.type === "group") editing = n.id;
    else if (n.type === "file" && n.file) onopen(n.subpath ? n.file : n.file, e.ctrlKey || e.metaKey);
    else if (n.type === "link" && n.url) openUrl(n.url).catch((err) => onerror(String(err)));
  }

  function onBackgroundDblClick(e: MouseEvent) {
    const [sx, sy] = screenXY(e);
    addText(toWorld(sx, sy));
  }

  function onkeydown(e: KeyboardEvent) {
    if (editing || editingEdge || picker) {
      if (e.key === "Escape") {
        e.preventDefault();
        picker = null;
        finishEditing();
        host?.focus();
      }
      return;
    }
    if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      deleteSelection();
    } else if (e.key === "Escape") {
      selected = new Set();
      selectedEdge = null;
    } else if (e.key === "Enter" && selected.size === 1) {
      const n = byId([...selected][0]);
      if (n && (n.type === "text" || n.type === "group")) {
        e.preventDefault();
        editing = n.id;
      }
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a") {
      e.preventDefault();
      selected = new Set(data.nodes.map((n) => n.id));
    }
  }

  // Files dragged from the file tree become file cards where they land.
  function ondragover(e: DragEvent) {
    if (!e.dataTransfer?.types.includes("text/plain")) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
  }

  function ondrop(e: DragEvent) {
    const path = e.dataTransfer?.getData("text/plain");
    if (!path) return;
    e.preventDefault();
    const [sx, sy] = screenXY(e);
    addFile(path, toWorld(sx, sy));
  }

  // ------------------------------------------------------------ content
  /** Note texts shown in file cards. */
  let noteTexts = $state<Record<string, string | null>>({});
  $effect(() => {
    for (const n of data.nodes) {
      const f = n.file;
      if (n.type !== "file" || !f || !f.toLowerCase().endsWith(".md") || f in untrack(() => noteTexts)) continue;
      noteTexts[f] = null;
      readNote(f)
        .then((t) => (noteTexts[f] = t))
        .catch(() => (noteTexts[f] = ""));
    }
  });

  /** Callouts, tags, math and images inside rendered markdown. */
  function enhance(el: HTMLElement, arg: [string, string | null | undefined]) {
    let source = arg[0];
    const run = () => {
      makeCallouts(el);
      linkTags(el);
      for (const m of el.querySelectorAll<HTMLElement>(".math[data-tex]")) {
        renderMath(m, m.dataset.tex ?? "", m.dataset.display === "1");
        m.removeAttribute("data-tex");
      }
      for (const img of el.querySelectorAll<HTMLImageElement>("img[src]")) {
        const src = img.getAttribute("src") ?? "";
        if (/^(https?:|data:|blob:)/i.test(src)) continue;
        img.removeAttribute("src");
        resolveLink(source, decodeURIComponent(src)).then((p) => p && (img.src = vaultUrl(p))).catch(() => {});
      }
      for (const span of el.querySelectorAll<HTMLElement>("span.embed")) {
        const target = span.dataset.target ?? "";
        resolveLink(source, target)
          .then((p) => {
            if (p && mediaKind(p) === "image") {
              const img = document.createElement("img");
              img.src = vaultUrl(p);
              span.replaceWith(img);
            } else span.textContent = target;
          })
          .catch(() => (span.textContent = target));
      }
    };
    run();
    return {
      update(next: [string, string | null | undefined]) {
        source = next[0];
        run();
      },
    };
  }

  function onContentClick(e: MouseEvent, source: string) {
    const a = (e.target as HTMLElement).closest<HTMLAnchorElement>("a");
    if (!a) return;
    e.preventDefault();
    if (a.classList.contains("internal-link")) {
      const sub = a.dataset.sub ?? "";
      onlink(a.dataset.target ?? "", sub && !sub.startsWith("^") ? sub : null, source);
    } else if (/^https?:/i.test(a.getAttribute("href") ?? "")) {
      openUrl(a.href).catch(() => {});
    }
  }

  const nodeStyle = (n: CanvasNode) => {
    const rgb = rgbOf(n.color);
    return `left:${n.x}px;top:${n.y}px;width:${n.width}px;height:${n.height}px;${rgb ? `--card-rgb:${rgb};` : ""}`;
  };
  const fileName = (p: string) => displayName(p.split("/").pop()!);

  // Groups first (under), then cards.
  const ordered = $derived([...data.nodes.filter((n) => n.type === "group"), ...data.nodes.filter((n) => n.type !== "group")]);

  interface EdgeShape {
    e: CanvasEdge;
    d: string;
    mid: Point;
    rgb: string | null;
  }
  const shapes = $derived.by((): EdgeShape[] => {
    const map = new Map(data.nodes.map((n) => [n.id, n]));
    const out: EdgeShape[] = [];
    for (const e of data.edges) {
      const a = map.get(e.fromNode);
      const b = map.get(e.toNode);
      if (!a || !b) continue;
      const [fs, ts] = edgeSides(e, a, b);
      const { d, mid } = edgePath(anchor(a, fs), fs, anchor(b, ts), ts);
      out.push({ e, d, mid, rgb: rgbOf(e.color) });
    }
    return out;
  });
  const linkShape = $derived.by(() => {
    if (!linking) return null;
    const a = byId(linking.from);
    return a ? edgePath(anchor(a, linking.side), linking.side, linking.to, null).d : null;
  });

  const shownFiles = $derived.by(() => {
    if (!picker) return [];
    const q = picker.query.trim().toLowerCase();
    return (q ? picker.files.filter((f) => f.toLowerCase().includes(q)) : picker.files).slice(0, 60);
  });

  const hasSelection = $derived(selected.size > 0 || !!selectedEdge);
  const COLORS = Object.keys(PRESET_COLORS);

  /** Focus a freshly shown input, cursor at the end. */
  function focusInput(el: HTMLElement) {
    tick().then(() => {
      el.focus();
      if (el instanceof HTMLTextAreaElement || el instanceof HTMLInputElement) {
        el.setSelectionRange(el.value.length, el.value.length);
      }
    });
  }
</script>

<div class="canvas-wrap">
  {#if broken}
    <div class="broken"><p>{broken}</p><p class="hint">파일 내용은 그대로 둡니다. 소스로 보려면 다른 편집기에서 여세요.</p></div>
  {:else}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_static_element_interactions, a11y_no_noninteractive_element_interactions -->
    <div class="canvas" bind:this={host} tabindex="0" role="application" aria-label="캔버스"
      style:--k={view.k}
      style:background-position="{view.x}px {view.y}px"
      style:background-size="{20 * view.k}px {20 * view.k}px"
      {onwheel} onpointerdown={onBackgroundDown} {onpointermove} {onpointerup} ondblclick={onBackgroundDblClick}
      {onkeydown} {ondragover} {ondrop}>
      <div class="world" style:transform="translate({view.x}px, {view.y}px) scale({view.k})">
        {#each ordered as n (n.id)}
          {@const sel = selected.has(n.id)}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class={["node", n.type, { sel, colored: !!rgbOf(n.color) }]} style={nodeStyle(n)}
            onpointerdown={(e) => onNodeDown(e, n)} ondblclick={(e) => onNodeDblClick(e, n)}
            onpointerenter={() => (hovered = n.id)} onpointerleave={() => hovered === n.id && (hovered = null)}>
            {#if n.type === "group"}
              {#if editing === n.id}
                <input class="group-label editing" value={n.label ?? ""} use:focusInput
                  oninput={(e) => (n.label = e.currentTarget.value)} onblur={finishEditing}
                  onkeydown={(e) => e.key === "Enter" && e.currentTarget.blur()} onpointerdown={(e) => e.stopPropagation()} />
              {:else if n.label}
                <div class="group-label">{n.label}</div>
              {/if}
            {:else if n.type === "text"}
              {#if editing === n.id}
                <textarea class="edit" value={n.text ?? ""} use:focusInput placeholder="텍스트를 입력하세요"
                  oninput={(e) => (n.text = e.currentTarget.value)} onblur={finishEditing}
                  onpointerdown={(e) => e.stopPropagation()}></textarea>
              {:else}
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <div class="md" use:enhance={[doc.path, n.text]} onclick={(e) => onContentClick(e, doc.path)}>
                  {@html renderMarkdown(n.text ?? "")}
                </div>
              {/if}
            {:else if n.type === "file" && n.file}
              {@const kind = mediaKind(n.file)}
              {#if kind === "image"}
                <img class="file-img" src={vaultUrl(n.file)} alt={fileName(n.file)} draggable="false" />
              {:else if n.file.toLowerCase().endsWith(".md")}
                <div class="file-title"><Icon name="file" size={12} />{fileName(n.file)}</div>
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <div class="md" use:enhance={[n.file, noteTexts[n.file]]} onclick={(e) => onContentClick(e, n.file!)}>
                  {#if noteTexts[n.file] === null}
                    <p class="muted">불러오는 중…</p>
                  {:else}
                    {@html renderMarkdown(bodyOf(noteTexts[n.file] ?? ""))}
                  {/if}
                </div>
              {:else}
                <div class="file-card"><Icon name="file" size={16} /><span>{fileName(n.file)}</span></div>
              {/if}
            {:else if n.type === "link" && n.url}
              <div class="file-card link"><Icon name="arrow-up-right" size={16} /><span>{n.url}</span></div>
            {:else}
              <div class="file-card"><span class="muted">{n.type} 카드</span></div>
            {/if}

            {#if sel && !editing}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div class="resize" onpointerdown={(e) => onResizeDown(e, n)}></div>
            {/if}
            {#if (sel || hovered === n.id) && !editing && n.type !== "group"}
              {#each SIDES as side (side)}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div class={["handle", side]} onpointerdown={(e) => onHandleDown(e, n, side)} title="끌어서 화살표 잇기"></div>
              {/each}
            {/if}
          </div>
        {/each}

        <svg class="edges" aria-hidden="true">
          <defs>
            <marker id="{uid}-pending" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="5" markerHeight="5" orient="auto-start-reverse">
              <path class="tip pending" d="M0 0L10 5L0 10z" />
            </marker>
          </defs>
          {#each shapes as s (s.e.id)}
            {@const mid = `${uid}-${s.e.id}`}
            <g class={["edge", { sel: selectedEdge === s.e.id }]} style={s.rgb ? `--edge-rgb:${s.rgb}` : ""}>
              <!-- Each arrow has its own head: SVG can't colour a shared one per line. -->
              <marker id={mid} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="5" markerHeight="5" orient="auto-start-reverse">
                <path class="tip" d="M0 0L10 5L0 10z" />
              </marker>
              <path class="line" d={s.d}
                marker-end={(s.e.toEnd ?? "arrow") === "arrow" ? `url(#${mid})` : undefined}
                marker-start={(s.e.fromEnd ?? "none") === "arrow" ? `url(#${mid})` : undefined} />
              <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
              <path class="hit" d={s.d}
                onpointerdown={(e) => { e.stopPropagation(); host?.focus(); finishEditing(); selected = new Set(); selectedEdge = s.e.id; }}
                ondblclick={(e) => { e.stopPropagation(); editingEdge = s.e.id; }} />
            </g>
          {/each}
          {#if linkShape}<path class="line pending" d={linkShape} marker-end="url(#{uid}-pending)" />{/if}
        </svg>

        {#each shapes as s (s.e.id)}
          {#if editingEdge === s.e.id}
            <input class="edge-label editing" style:left="{s.mid.x}px" style:top="{s.mid.y}px" value={s.e.label ?? ""}
              use:focusInput placeholder="라벨" oninput={(e) => (s.e.label = e.currentTarget.value)} onblur={finishEditing}
              onkeydown={(e) => e.key === "Enter" && e.currentTarget.blur()} onpointerdown={(e) => e.stopPropagation()} />
          {:else if s.e.label}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="edge-label" style:left="{s.mid.x}px" style:top="{s.mid.y}px"
              ondblclick={(e) => { e.stopPropagation(); editingEdge = s.e.id; }}>{s.e.label}</div>
          {/if}
        {/each}
      </div>

      {#if !data.nodes.length && doc.loaded}
        <p class="empty-hint">빈 곳을 두 번 눌러 카드를 만들거나, 파일 트리에서 노트를 끌어다 놓으세요.</p>
      {/if}

      <div class="toolbar" onpointerdown={(e) => e.stopPropagation()} ondblclick={(e) => e.stopPropagation()} role="toolbar" aria-label="캔버스 도구" tabindex="-1">
        <button onclick={() => addText()} title="텍스트 카드 추가 (빈 곳 두 번 클릭)"><Icon name="note-pen" size={16} /></button>
        <button onclick={openPicker} title="노트·파일 카드 추가 (파일 트리에서 끌어다 놓아도 됨)"><Icon name="file-plus" size={16} /></button>
        <button onclick={addGroup} title="그룹 추가 (고른 카드를 감쌈)"><Icon name="columns" size={16} /></button>
        {#if hasSelection}
          <span class="sep"></span>
          <button class="swatch none" onclick={() => setColor(null)} title="기본 색" aria-label="기본 색"></button>
          {#each COLORS as c (c)}
            <button class="swatch" style:--sw="rgb({PRESET_COLORS[c]})" onclick={() => setColor(c)} aria-label="색 {c}"></button>
          {/each}
          <button onclick={deleteSelection} title="삭제 (Delete)"><Icon name="trash" size={16} /></button>
        {/if}
      </div>

      <div class="zoom" onpointerdown={(e) => e.stopPropagation()} ondblclick={(e) => e.stopPropagation()} role="toolbar" aria-label="확대·축소" tabindex="-1">
        <button onclick={() => zoomBy(1.25)} aria-label="확대" title="확대 (Ctrl+스크롤)"><Icon name="plus" size={15} /></button>
        <button onclick={() => zoomBy(1 / 1.25)} aria-label="축소" title="축소"><Icon name="minus" size={15} /></button>
        <button onclick={() => fit()} aria-label="화면에 맞추기" title="화면에 맞추기"><Icon name="fit" size={15} /></button>
      </div>

      {#if picker}
        <div class="picker" onpointerdown={(e) => e.stopPropagation()} ondblclick={(e) => e.stopPropagation()} role="dialog" aria-label="파일 고르기" tabindex="-1">
          <input placeholder="노트나 파일 찾기" value={picker.query} use:focusInput
            oninput={(e) => picker && (picker.query = e.currentTarget.value)}
            onkeydown={(e) => { if (e.key === "Enter" && shownFiles[0]) addFile(shownFiles[0]); }} />
          <ul>
            {#each shownFiles as f (f)}
              <li><button onclick={() => addFile(f)} title={f}>{fileName(f)}<span>{f}</span></button></li>
            {:else}
              <li class="muted">일치하는 파일 없음</li>
            {/each}
          </ul>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .canvas-wrap {
    flex: 1;
    min-height: 0;
    display: flex;
    position: relative;
  }
  .canvas {
    flex: 1;
    position: relative;
    overflow: hidden;
    outline: none;
    cursor: grab;
    touch-action: none;
    background-color: var(--bg);
    background-image: radial-gradient(circle, var(--border) 1px, transparent 1.2px);
    user-select: none;
  }
  .canvas:active {
    cursor: grabbing;
  }
  .world {
    position: absolute;
    left: 0;
    top: 0;
    transform-origin: 0 0;
  }
  .node {
    --card-rgb: 160 160 160;
    position: absolute;
    box-sizing: border-box;
    border: 2px solid rgb(var(--card-rgb) / 0.55);
    border-radius: 8px;
    background: var(--bg);
    cursor: default;
  }
  .node.colored {
    background: color-mix(in srgb, rgb(var(--card-rgb)) 8%, var(--bg));
    border-color: rgb(var(--card-rgb));
  }
  .node.sel {
    outline: calc(2px / var(--k)) solid var(--accent);
    outline-offset: calc(2px / var(--k));
  }
  .node.group {
    background: rgb(var(--card-rgb) / 0.06);
    border-style: solid;
    border-color: rgb(var(--card-rgb) / 0.5);
  }
  .group-label {
    position: absolute;
    left: 0;
    bottom: calc(100% + 6px);
    max-width: 100%;
    padding: 2px 8px;
    border-radius: 6px;
    background: rgb(var(--card-rgb) / 0.18);
    color: var(--text);
    font: inherit;
    font-size: 16px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  input.group-label {
    border: 1px solid var(--accent);
    outline: none;
    min-width: 160px;
  }
  .md {
    height: 100%;
    box-sizing: border-box;
    overflow: hidden;
    padding: 10px 14px;
    font-size: 15px;
    line-height: 1.55;
    color: var(--text);
    overflow-wrap: break-word;
  }
  .file-title + .md {
    height: calc(100% - 28px);
  }
  .md :global(:first-child) {
    margin-top: 0;
  }
  .md :global(p),
  .md :global(:is(ul, ol)) {
    margin: 0 0 0.6em;
  }
  .md :global(:is(ul, ol)) {
    padding-left: 1.4em;
  }
  .md :global(:is(h1, h2, h3, h4)) {
    margin: 0.4em 0 0.4em;
    line-height: 1.3;
  }
  .md :global(h1) {
    font-size: 1.5em;
  }
  .md :global(h2) {
    font-size: 1.3em;
  }
  .md :global(a) {
    color: var(--accent);
    text-decoration: none;
  }
  .md :global(code) {
    font-family: var(--font-mono);
    font-size: 0.9em;
    background: var(--code-bg);
    padding: 0 3px;
    border-radius: 3px;
  }
  .md :global(pre) {
    background: var(--code-bg);
    padding: 8px;
    border-radius: 6px;
    overflow: hidden;
  }
  .md :global(img) {
    max-width: 100%;
  }
  .md :global(mark) {
    background: var(--highlight-bg);
    color: inherit;
  }
  .md :global(blockquote) {
    margin: 0 0 0.6em;
    padding-left: 10px;
    border-left: 3px solid var(--border);
    color: var(--text-muted);
  }
  .file-title {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 12px;
    border-bottom: 1px solid var(--border);
    font-size: 13px;
    color: var(--text-muted);
    overflow: hidden;
    white-space: nowrap;
  }
  .file-img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: contain;
    border-radius: 6px;
  }
  .file-card {
    height: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 14px;
    font-size: 14px;
    color: var(--text);
    overflow: hidden;
  }
  .file-card span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .muted {
    color: var(--text-faint);
  }
  textarea.edit {
    width: 100%;
    height: 100%;
    box-sizing: border-box;
    border: 0;
    border-radius: 6px;
    padding: 10px 14px;
    background: transparent;
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 14px;
    line-height: 1.55;
    resize: none;
    outline: none;
    user-select: text;
  }
  .resize {
    position: absolute;
    right: calc(-6px / var(--k));
    bottom: calc(-6px / var(--k));
    width: calc(12px / var(--k));
    height: calc(12px / var(--k));
    border-radius: 50%;
    background: var(--accent);
    cursor: nwse-resize;
  }
  .handle {
    position: absolute;
    width: calc(12px / var(--k));
    height: calc(12px / var(--k));
    border-radius: 50%;
    background: var(--bg);
    border: calc(2px / var(--k)) solid var(--accent);
    box-sizing: border-box;
    cursor: crosshair;
    transform: translate(-50%, -50%);
  }
  .handle.top {
    left: 50%;
    top: 0;
  }
  .handle.bottom {
    left: 50%;
    top: 100%;
  }
  .handle.left {
    left: 0;
    top: 50%;
  }
  .handle.right {
    left: 100%;
    top: 50%;
  }
  .edges {
    position: absolute;
    left: 0;
    top: 0;
    width: 1px;
    height: 1px;
    overflow: visible;
    pointer-events: none;
  }
  .edge {
    --edge-rgb: 150 150 150;
  }
  .line {
    fill: none;
    stroke: rgb(var(--edge-rgb));
    stroke-width: 3;
    stroke-linecap: round;
  }
  .line.pending {
    stroke: var(--accent);
    stroke-dasharray: 8 6;
  }
  .edge.sel .line {
    stroke: var(--accent);
  }
  .tip {
    fill: rgb(var(--edge-rgb));
  }
  .edge.sel .tip,
  .tip.pending {
    fill: var(--accent);
  }
  .hit {
    fill: none;
    stroke: transparent;
    stroke-width: 16;
    pointer-events: stroke;
    cursor: pointer;
  }
  .edge-label {
    position: absolute;
    transform: translate(-50%, -50%);
    padding: 2px 8px;
    border-radius: 6px;
    background: var(--bg);
    border: 1px solid var(--border);
    font-size: 14px;
    color: var(--text);
    white-space: nowrap;
  }
  input.edge-label {
    font: inherit;
    outline: none;
    border-color: var(--accent);
  }
  .empty-hint {
    position: absolute;
    left: 50%;
    top: 45%;
    transform: translate(-50%, -50%);
    margin: 0;
    font-size: 13px;
    color: var(--text-faint);
    pointer-events: none;
  }
  .toolbar,
  .zoom {
    position: absolute;
    bottom: 14px;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
    box-shadow: 0 2px 8px rgb(0 0 0 / 0.12);
    cursor: default;
  }
  .toolbar {
    left: 50%;
    transform: translateX(-50%);
  }
  .zoom {
    right: 14px;
  }
  .toolbar button,
  .zoom button {
    display: inline-flex;
    border: 0;
    border-radius: 5px;
    background: none;
    color: var(--text-muted);
    padding: 6px;
    cursor: pointer;
  }
  .toolbar button:hover,
  .zoom button:hover {
    background: var(--hover);
    color: var(--text);
  }
  .sep {
    width: 1px;
    height: 18px;
    margin: 0 4px;
    background: var(--border);
  }
  .toolbar .swatch {
    width: 18px;
    height: 18px;
    padding: 0;
    margin: 0 2px;
    border-radius: 50%;
    background: var(--sw);
    border: 2px solid var(--bg);
    box-shadow: 0 0 0 1px var(--border);
  }
  .toolbar .swatch.none {
    background: linear-gradient(135deg, var(--bg) 45%, var(--danger) 45% 55%, var(--bg) 55%);
  }
  .picker {
    position: absolute;
    left: 50%;
    bottom: 64px;
    transform: translateX(-50%);
    width: min(420px, calc(100% - 32px));
    max-height: 50%;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.18);
    cursor: default;
  }
  .picker input {
    margin: 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    padding: 6px 8px;
    outline: none;
    user-select: text;
  }
  .picker input:focus {
    border-color: var(--accent);
  }
  .picker ul {
    list-style: none;
    margin: 0;
    padding: 0 6px 6px;
    overflow: auto;
  }
  .picker li button {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 1px;
    border: 0;
    border-radius: 5px;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 13px;
    text-align: left;
    padding: 5px 8px;
    cursor: pointer;
  }
  .picker li button:hover {
    background: var(--hover);
  }
  .picker li button span {
    font-size: 11px;
    color: var(--text-faint);
  }
  .picker .muted {
    padding: 6px 8px;
    font-size: 12px;
  }
  .broken {
    margin: auto;
    text-align: center;
    color: var(--danger);
    font-size: 14px;
  }
  .broken .hint {
    color: var(--text-faint);
    font-size: 12px;
  }
</style>
