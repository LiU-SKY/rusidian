<script lang="ts">
  import { onMount } from "svelte";
  import type { ForceLink, Simulation, SimulationLinkDatum, SimulationNodeDatum } from "d3-force";
  import { graphGroups, linkGraph, setGraphGroups, type ColorGroup, type GraphData, type GraphNodeKind } from "./api";
  import { GROUP_COLORS, compileQuery } from "./graphGroups";
  import Icon from "./Icon.svelte";
  import { settings } from "./settings.svelte";

  interface Props {
    /** Bumped when vault files change; reloads the graph. */
    version: number;
    currentPath: string | null;
    /** Show only the current note's neighbourhood (Obsidian's "local graph"). */
    local?: boolean;
    onopen: (path: string) => void;
  }

  let { version, currentPath, local = $bindable(false), onopen }: Props = $props();

  interface N extends SimulationNodeDatum {
    id: string;
    title: string;
    kind: GraphNodeKind;
    degree: number;
    tags: string[];
    r: number;
  }
  type L = SimulationLinkDatum<N>;

  /**
   * Above this size the layout is computed off-screen first (time-sliced),
   * because repainting thousands of edges every animation frame is slow.
   */
  const LARGE_NODES = 800;
  const LARGE_EDGES = 1500;
  const WARM_ALPHA = 0.04;
  const FRAME_BUDGET_MS = 12;
  /** The "배치 계산 중" note appears only when the layout takes longer than this. */
  const WARM_NOTICE_MS = 150;
  /** How long "fit to screen" glides. */
  const FIT_MS = 420;
  /** Zooming eases toward its target: about 95% of the way in 3 × this (ms). */
  const ZOOM_TAU = 55;

  // UI state (reactive)
  let raw = $state.raw<GraphData | null>(null);
  let showOrphans = $state(true);
  let showMissing = $state(false);
  let showAttachments = $state(false);
  let depth = $state(1);
  let query = $state("");
  let width = $state(0);
  let height = $state(0);
  let ready = $state(false);
  let counts = $state({ nodes: 0, edges: 0 });
  let tip = $state<{ title: string; kind: GraphNodeKind; degree: number; x: number; y: number } | null>(null);
  let errorText = $state<string | null>(null);
  let warming = $state(false);
  let warmProgress = $state(0);
  let warmNotice = $state(false);

  // Simulation + drawing state: plain objects mutated by d3 every tick, kept
  // out of Svelte's reactivity on purpose (proxies would make each tick slow).
  let canvas: HTMLCanvasElement | undefined = $state();
  let d3: typeof import("d3-force") | null = null;
  let sim: Simulation<N, L> | null = null;
  let nodes: N[] = [];
  let links: L[] = [];
  let neighbors = new Map<string, Set<string>>();
  let view = { k: 1, x: 0, y: 0 };
  let hovered: N | null = null;
  let needsFit = true;
  let large = false;
  /** Something has been drawn: later fits glide instead of jumping. */
  let painted = false;
  /** Bumped to cancel a running fit animation (a gesture takes over). */
  let glide = 0;
  /** What the view is framed on; a change asks for a new fit. */
  let framedOn: string | null = null;
  let colors = { node: "#888", current: "#7c5cdb", edge: "#aaa", label: "#222", attachment: "#b05f00", bg: "#fff" };
  let fontFamily = "sans-serif";

  // The full scene is painted into `base` and reused: hover draws on top of
  // it, pan/zoom transforms it until the gesture settles.
  let base: HTMLCanvasElement | null = null;
  let baseView = { ...view };
  let baseDirty = true;
  let framePending = false;
  let settleTimer: ReturnType<typeof setTimeout> | undefined;

  const radius = (degree: number) => 3 + Math.min(9, Math.sqrt(degree) * 1.6);
  const linkForce = () => sim!.force("link") as ForceLink<N, L>;

  function readColors() {
    if (!canvas) return;
    const s = getComputedStyle(canvas);
    const v = (name: string, fallback: string) => s.getPropertyValue(name).trim() || fallback;
    colors = {
      node: v("--text-muted", colors.node),
      current: v("--accent", colors.current),
      edge: v("--text-faint", colors.edge),
      label: v("--text", colors.label),
      attachment: v("--yaml-value", colors.attachment),
      bg: v("--bg", colors.bg),
    };
    fontFamily = s.fontFamily || fontFamily;
  }

  async function load() {
    try {
      raw = await linkGraph();
      errorText = null;
    } catch (e) {
      errorText = String(e);
    }
  }

  // ------------------------------------------------------- color groups
  /** Obsidian's color groups (`.obsidian/graph.json`), first match wins. */
  let groups = $state<ColorGroup[]>([]);
  let showGroups = $state(false);
  /** Node id → group color, recomputed when nodes or groups change. */
  let groupColor = new Map<string, string>();
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  const SAVE_MS = 400;

  async function loadGroups() {
    try {
      groups = await graphGroups();
    } catch {
      groups = [];
    }
  }

  function recolor() {
    const tests = groups.map((g) => ({ color: g.color, test: compileQuery(g.query) })).filter((g) => g.test);
    groupColor = new Map();
    for (const n of nodes) {
      if (n.kind === "missing") continue;
      const hit = tests.find((g) => g.test!(n));
      if (hit) groupColor.set(n.id, hit.color);
    }
    requestRender(true);
  }

  /** The user changed a group: recolor now, save shortly after. */
  function groupsChanged() {
    recolor();
    clearTimeout(saveTimer);
    const snapshot = groups.map((g) => ({ query: g.query, color: g.color }));
    saveTimer = setTimeout(() => {
      setGraphGroups(snapshot).catch((e) => (errorText = String(e)));
    }, SAVE_MS);
  }

  function addGroup() {
    groups.push({ query: "", color: GROUP_COLORS[groups.length % GROUP_COLORS.length] });
    groupsChanged();
  }

  function removeGroup(i: number) {
    groups.splice(i, 1);
    groupsChanged();
  }

  function moveGroup(i: number, delta: number) {
    const j = i + delta;
    if (j < 0 || j >= groups.length) return;
    [groups[i], groups[j]] = [groups[j], groups[i]];
    groupsChanged();
  }

  // ------------------------------------------------------------ subset
  function rebuild() {
    if (!d3 || !sim || !raw) return;
    const kindOk = (k: GraphNodeKind) =>
      k === "note" || (k === "missing" && showMissing) || (k === "attachment" && showAttachments);
    let keep = new Set(raw.nodes.filter((n) => kindOk(n.kind)).map((n) => n.id));
    let edges = raw.edges.filter((e) => keep.has(e.source) && keep.has(e.target));

    const adj = new Map<string, Set<string>>();
    for (const e of edges) {
      if (!adj.has(e.source)) adj.set(e.source, new Set());
      if (!adj.has(e.target)) adj.set(e.target, new Set());
      adj.get(e.source)!.add(e.target);
      adj.get(e.target)!.add(e.source);
    }

    if (local && currentPath && keep.has(currentPath)) {
      const reached = new Set([currentPath]);
      let frontier = [currentPath];
      for (let d = 0; d < depth; d++) {
        const next: string[] = [];
        for (const id of frontier) for (const n of adj.get(id) ?? []) if (!reached.has(n)) {
          reached.add(n);
          next.push(n);
        }
        frontier = next;
      }
      keep = reached;
    } else if (!showOrphans) {
      keep = new Set([...keep].filter((id) => adj.has(id)));
    }
    edges = edges.filter((e) => keep.has(e.source) && keep.has(e.target));

    const previous = new Map(nodes.map((n) => [n.id, n]));
    const fresh = previous.size === 0;
    nodes = raw.nodes.filter((n) => keep.has(n.id)).map((n) => {
      const p = previous.get(n.id);
      return { ...n, r: radius(n.degree), x: p?.x, y: p?.y, vx: p?.vx, vy: p?.vy };
    });
    // New nodes start next to an already-placed neighbour instead of flying in.
    const placed = new Map(nodes.filter((n) => n.x !== undefined).map((n) => [n.id, n]));
    for (const n of nodes) {
      if (n.x !== undefined) continue;
      const anchor = [...(adj.get(n.id) ?? [])].map((id) => placed.get(id)).find(Boolean);
      if (anchor) {
        n.x = anchor.x! + (Math.random() - 0.5) * 30;
        n.y = anchor.y! + (Math.random() - 0.5) * 30;
      }
    }
    links = edges.map((e) => ({ source: e.source, target: e.target }));
    neighbors = new Map([...adj].filter(([id]) => keep.has(id)).map(([id, s]) => [id, new Set([...s].filter((x) => keep.has(x)))]));
    counts = { nodes: nodes.length, edges: links.length };
    large = nodes.length > LARGE_NODES || links.length > LARGE_EDGES;
    recolor();

    if (hovered && !keep.has(hovered.id)) {
      hovered = null;
      tip = null;
    }
    sim.nodes(nodes);
    linkForce().links(links);
    sim.alpha(fresh ? 1 : 0.4);
    // A new layout or a new frame is settled off-screen first, so it is never
    // drawn mid-explosion and then snapped to fit; small graphs take a few ms.
    if (large || fresh || needsFit) warmUp();
    else sim.restart();
  }

  /** Run the layout off-screen in small slices, then paint once. */
  function warmUp() {
    if (!sim) return;
    sim.stop();
    warming = true;
    warmProgress = 0;
    const noticeTimer = setTimeout(() => (warmNotice = warming), WARM_NOTICE_MS);
    const start = sim.alpha();
    const step = () => {
      if (!sim || !warming) return;
      const t0 = performance.now();
      while (sim.alpha() > WARM_ALPHA && performance.now() - t0 < FRAME_BUDGET_MS) sim.tick();
      warmProgress = Math.round(Math.min(1, (start - sim.alpha()) / (start - WARM_ALPHA)) * 100);
      if (sim.alpha() > WARM_ALPHA) {
        requestAnimationFrame(step);
        return;
      }
      warming = false;
      warmNotice = false;
      clearTimeout(noticeTimer);
      if (needsFit) fit();
      // Small graphs keep settling gently on screen; big ones stay still.
      if (!large) sim.restart();
      requestRender(true);
    };
    requestAnimationFrame(step);
  }

  // ------------------------------------------------------------ drawing
  function requestRender(sceneChanged: boolean) {
    if (sceneChanged) baseDirty = true;
    if (framePending) return;
    framePending = true;
    requestAnimationFrame(() => {
      framePending = false;
      render();
    });
  }

  const sameView = (a: typeof view, b: typeof view) => a.k === b.k && a.x === b.x && a.y === b.y;

  function render() {
    if (!canvas || !base || width === 0) return;
    const ctx = canvas.getContext("2d")!;
    const dpr = window.devicePixelRatio || 1;

    if (baseDirty && !warming) {
      paintScene(base.getContext("2d")!, dpr);
      baseView = { ...view };
      baseDirty = false;
      if (nodes.length > 0) painted = true;
    }

    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    if (sameView(baseView, view)) {
      ctx.drawImage(base, 0, 0);
    } else {
      // Mid-gesture: move/scale the cached picture, repaint sharp once idle.
      const s = view.k / baseView.k;
      const tx = dpr * (width / 2 + view.x) - s * dpr * (width / 2 + baseView.x);
      const ty = dpr * (height / 2 + view.y) - s * dpr * (height / 2 + baseView.y);
      ctx.setTransform(s, 0, 0, s, tx, ty);
      ctx.drawImage(base, 0, 0);
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      clearTimeout(settleTimer);
      settleTimer = setTimeout(() => requestRender(true), 120);
    }
    if (hovered && !warming) paintFocus(ctx, dpr, hovered);
  }

  function applyView(ctx: CanvasRenderingContext2D, dpr: number) {
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.translate(width / 2 + view.x, height / 2 + view.y);
    ctx.scale(view.k, view.k);
  }

  const matchesQuery = (n: N, q: string) => q !== "" && n.title.toLowerCase().includes(q);

  function nodeFill(n: N, q: string, highlight: boolean): string {
    if (highlight || n.id === currentPath || matchesQuery(n, q)) return colors.current;
    return groupColor.get(n.id) ?? (n.kind === "attachment" ? colors.attachment : colors.node);
  }

  function paintNode(ctx: CanvasRenderingContext2D, n: N, fill: string) {
    // 2px surface ring keeps overlapping dots legible
    ctx.beginPath();
    ctx.arc(n.x!, n.y!, n.r, 0, Math.PI * 2);
    ctx.lineWidth = 2 / view.k;
    ctx.strokeStyle = colors.bg;
    ctx.stroke();
    if (n.kind === "missing") {
      ctx.fillStyle = colors.bg;
      ctx.fill();
      ctx.lineWidth = 1 / view.k;
      ctx.strokeStyle = colors.node;
      ctx.stroke();
    } else {
      ctx.fillStyle = fill;
      ctx.fill();
    }
  }

  function paintLabel(ctx: CanvasRenderingContext2D, n: N) {
    ctx.fillText(n.title, n.x!, n.y! + n.r + 3 / view.k);
  }

  function setLabelFont(ctx: CanvasRenderingContext2D) {
    ctx.font = `${11 / view.k}px ${fontFamily}`;
    ctx.textAlign = "center";
    ctx.textBaseline = "top";
    ctx.fillStyle = colors.label;
  }

  /** Everything except the hover highlight. */
  function paintScene(ctx: CanvasRenderingContext2D, dpr: number) {
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, base!.width, base!.height);
    applyView(ctx, dpr);
    const q = query.trim().toLowerCase();

    ctx.lineWidth = 1 / view.k;
    ctx.strokeStyle = colors.edge;
    ctx.globalAlpha = q ? 0.12 : 0.45;
    ctx.beginPath();
    for (const l of links) {
      const s = l.source as N;
      const t = l.target as N;
      ctx.moveTo(s.x!, s.y!);
      ctx.lineTo(t.x!, t.y!);
    }
    ctx.stroke();

    for (const n of nodes) {
      ctx.globalAlpha = q && !matchesQuery(n, q) ? 0.15 : 1;
      paintNode(ctx, n, nodeFill(n, q, false));
    }

    // labels: all when zoomed in, otherwise only the ones that matter
    setLabelFont(ctx);
    for (const n of nodes) {
      const important = n.id === currentPath || matchesQuery(n, q);
      if (!(view.k >= 1.3 || important)) continue;
      ctx.globalAlpha = q && !important ? 0.2 : important ? 1 : 0.75;
      paintLabel(ctx, n);
    }
    ctx.globalAlpha = 1;
  }

  /** Hover: veil the scene, then redraw only the node and its neighbours. */
  function paintFocus(ctx: CanvasRenderingContext2D, dpr: number, center: N) {
    const focus = neighbors.get(center.id) ?? new Set<string>();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.globalAlpha = 0.72;
    ctx.fillStyle = colors.bg;
    ctx.fillRect(0, 0, canvas!.width, canvas!.height);
    applyView(ctx, dpr);

    ctx.globalAlpha = 0.9;
    ctx.strokeStyle = colors.current;
    ctx.lineWidth = 1.5 / view.k;
    ctx.beginPath();
    for (const l of links) {
      const s = l.source as N;
      const t = l.target as N;
      if (s !== center && t !== center) continue;
      ctx.moveTo(s.x!, s.y!);
      ctx.lineTo(t.x!, t.y!);
    }
    ctx.stroke();

    ctx.globalAlpha = 1;
    const shown = nodes.filter((n) => n === center || focus.has(n.id));
    for (const n of shown) paintNode(ctx, n, nodeFill(n, "", n === center));
    setLabelFont(ctx);
    for (const n of shown) paintLabel(ctx, n);
  }

  /** Frame the whole graph. Glides there unless nothing was drawn yet (or motion is reduced). */
  function fit() {
    if (nodes.length === 0 || width === 0) return; // stays pending until there is room
    let [x0, y0, x1, y1] = [Infinity, Infinity, -Infinity, -Infinity];
    for (const n of nodes) {
      x0 = Math.min(x0, n.x! - n.r);
      y0 = Math.min(y0, n.y! - n.r);
      x1 = Math.max(x1, n.x! + n.r);
      y1 = Math.max(y1, n.y! + n.r);
    }
    const pad = 40;
    const bottomBar = 44; // keep nodes clear of the zoom controls in the corner
    const usableH = height - pad * 2 - bottomBar;
    const k = Math.min((width - pad * 2) / Math.max(x1 - x0, 1), usableH / Math.max(y1 - y0, 1), 2.5);
    const target = { k, x: -((x0 + x1) / 2) * k, y: -((y0 + y1) / 2) * k - bottomBar / 2 };
    needsFit = false;
    const id = ++glide;
    if (!painted || matchMedia("(prefers-reduced-motion: reduce)").matches) {
      view = target;
      requestRender(true);
      return;
    }
    const from = { ...view };
    const t0 = performance.now();
    const step = (now: number) => {
      if (id !== glide) return;
      const t = Math.min(1, (now - t0) / FIT_MS);
      const e = 1 - (1 - t) ** 3; // ease-out
      view = {
        k: from.k * Math.pow(target.k / from.k, e), // zoom feels even on a log scale
        x: from.x + (target.x - from.x) * e,
        y: from.y + (target.y - from.y) * e,
      };
      // Repaint every frame: the cached picture only holds what was on screen,
      // so scaling it while zooming out would leave the edges empty until the end.
      requestRender(true);
      if (t < 1) requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  }

  /** Scale to `k`, keeping the graph point under (px, py) where it is. */
  function zoomAt(k: number, px: number, py: number) {
    const gx = (px - width / 2 - view.x) / view.k;
    const gy = (py - height / 2 - view.y) / view.k;
    const out = k < view.k;
    view = { k, x: px - width / 2 - gx * k, y: py - height / 2 - gy * k };
    // Zooming out shows what the cached picture doesn't hold: paint it.
    requestRender(out);
  }

  /** The zoom being eased toward (wheel notches and buttons add up). */
  let zoomTo: { id: number; k: number; px: number; py: number; last: number } | null = null;

  function zoomBy(factor: number, px = width / 2, py = height / 2) {
    const running = zoomTo !== null && zoomTo.id === glide;
    const from = running ? zoomTo!.k : view.k;
    const k = Math.min(8, Math.max(0.05, from * factor));
    if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
      glide++;
      zoomTo = null;
      zoomAt(k, px, py);
      return;
    }
    if (running) {
      Object.assign(zoomTo!, { k, px, py });
      return;
    }
    // A new zoom stops a fit that is still gliding (and the other way round).
    zoomTo = { id: ++glide, k, px, py, last: performance.now() };
    requestAnimationFrame(zoomStep);
  }

  function zoomStep(now: number) {
    const z = zoomTo;
    if (!z || z.id !== glide) return;
    const dt = Math.min(64, Math.max(0, now - z.last));
    z.last = now;
    const ease = 1 - Math.exp(-dt / ZOOM_TAU);
    // Eased on a log scale, so each notch feels the same at any zoom.
    let k = view.k * Math.pow(z.k / view.k, ease);
    if (Math.abs(Math.log(z.k / k)) < 0.003) k = z.k;
    zoomAt(k, z.px, z.py);
    if (k === z.k) zoomTo = null;
    else requestAnimationFrame(zoomStep);
  }

  // --------------------------------------------------------- interaction
  const toGraph = (px: number, py: number) => [(px - width / 2 - view.x) / view.k, (py - height / 2 - view.y) / view.k];

  function nodeAt(px: number, py: number): N | null {
    const [x, y] = toGraph(px, py);
    let best: N | null = null;
    let bestD = Infinity;
    for (const n of nodes) {
      const d = Math.hypot(n.x! - x, n.y! - y);
      // hit target a little bigger than the dot
      if (d <= n.r + 4 / view.k && d < bestD) {
        best = n;
        bestD = d;
      }
    }
    return best;
  }

  let drag: { node: N | null; startX: number; startY: number; viewX: number; viewY: number; moved: boolean } | null = null;

  function pointerXY(e: PointerEvent | WheelEvent) {
    const rect = canvas!.getBoundingClientRect();
    return [e.clientX - rect.left, e.clientY - rect.top];
  }

  function onpointerdown(e: PointerEvent) {
    if (e.button !== 0 || warming) return;
    const [px, py] = pointerXY(e);
    glide++;
    canvas!.setPointerCapture(e.pointerId);
    const node = nodeAt(px, py);
    drag = { node, startX: px, startY: py, viewX: view.x, viewY: view.y, moved: false };
    if (node) {
      node.fx = node.x;
      node.fy = node.y;
    }
  }

  function onpointermove(e: PointerEvent) {
    const [px, py] = pointerXY(e);
    if (drag) {
      if (!drag.moved && Math.hypot(px - drag.startX, py - drag.startY) > 3) {
        drag.moved = true;
        if (drag.node) sim?.alphaTarget(0.3).restart();
      }
      if (drag.node && drag.moved) {
        const [x, y] = toGraph(px, py);
        drag.node.fx = x;
        drag.node.fy = y;
      } else if (!drag.node) {
        view = { ...view, x: drag.viewX + px - drag.startX, y: drag.viewY + py - drag.startY };
        requestRender(false);
      }
      return;
    }
    if (warming) return;
    const node = nodeAt(px, py);
    if (node !== hovered) {
      hovered = node;
      canvas!.style.cursor = node ? "pointer" : "grab";
      requestRender(false);
    }
    tip = node ? { title: node.title, kind: node.kind, degree: node.degree, x: px, y: py } : null;
  }

  function onpointerup(e: PointerEvent) {
    if (!drag) return;
    const { node, moved } = drag;
    drag = null;
    canvas!.releasePointerCapture(e.pointerId);
    if (!node) return;
    node.fx = null;
    node.fy = null;
    if (moved) {
      sim?.alphaTarget(0);
      // Big graphs: don't spend seconds re-animating the whole layout.
      if (large) {
        sim?.stop();
        requestRender(true);
      }
    } else if (node.kind !== "missing") {
      onopen(node.id);
    }
  }

  function onpointerleave() {
    if (drag) return;
    hovered = null;
    tip = null;
    requestRender(false);
  }

  function onwheel(e: WheelEvent) {
    e.preventDefault();
    const [px, py] = pointerXY(e);
    zoomBy(Math.exp(-e.deltaY * 0.0015), px, py);
  }

  // Canvas colors come from the CSS tokens; re-read them when the theme changes.
  $effect(() => {
    settings.resolved;
    if (!ready) return;
    readColors();
    requestRender(true);
  });

  // ------------------------------------------------------------ lifecycle
  onMount(() => {
    let disposed = false;
    base = document.createElement("canvas");

    import("d3-force").then((mod) => {
      if (disposed) return;
      d3 = mod;
      sim = mod
        .forceSimulation<N, L>()
        .force("link", mod.forceLink<N, L>().id((d) => d.id).distance(50))
        .force("charge", mod.forceManyBody<N>().strength(-90).distanceMax(500))
        .force("x", mod.forceX<N>(0).strength(0.05))
        .force("y", mod.forceY<N>(0).strength(0.05))
        .force("collide", mod.forceCollide<N>((d) => d.r + 2))
        .alphaDecay(0.03)
        .stop()
        .on("tick", () => requestRender(true));
      readColors();
      ready = true;
    });

    return () => {
      disposed = true;
      warming = false;
      clearTimeout(settleTimer);
      sim?.stop();
    };
  });

  $effect(() => {
    version;
    load();
    loadGroups().then(recolor);
  });

  // Re-filter when data or options change. Opening another note only
  // matters in the local view; elsewhere the graph stays as it is.
  $effect(() => {
    raw;
    showOrphans;
    showMissing;
    showAttachments;
    const frame = local ? `${depth}|${currentPath ?? ""}` : "all";
    if (frame !== framedOn) {
      framedOn = frame;
      needsFit = true; // a different neighbourhood is framed again
    }
    if (ready) rebuild();
  });

  // Backing stores follow the element size (crisp on HiDPI).
  $effect(() => {
    if (!canvas || !base || width === 0 || height === 0) return;
    const dpr = window.devicePixelRatio || 1;
    for (const c of [canvas, base]) {
      c.width = Math.round(width * dpr);
      c.height = Math.round(height * dpr);
    }
    if (needsFit && !warming) fit(); // a fit that waited for the stage to get a size
    requestRender(true);
  });

  $effect(() => {
    query;
    currentPath;
    requestRender(true);
  });

  const canLocal = $derived(!!currentPath && currentPath.toLowerCase().endsWith(".md"));
  const KIND_LABEL: Record<GraphNodeKind, string> = { note: "노트", attachment: "첨부 파일", missing: "아직 없는 노트" };
</script>

<section class="graph view-in" aria-label="그래프">
  <header class="toolbar">
    <h1>그래프</h1>
    <input class="search" type="search" bind:value={query} placeholder="노트 이름으로 강조" aria-label="그래프에서 노트 찾기" />
    <label title={canLocal ? "" : "노트를 연 상태에서 쓸 수 있습니다"}>
      <input type="checkbox" bind:checked={local} disabled={!canLocal} /> 현재 노트 주변
    </label>
    {#if local && canLocal}
      <select bind:value={depth} aria-label="거리">
        <option value={1}>1단계</option>
        <option value={2}>2단계</option>
        <option value={3}>3단계</option>
      </select>
    {/if}
    <label><input type="checkbox" bind:checked={showOrphans} disabled={local} /> 고립된 노트</label>
    <label><input type="checkbox" bind:checked={showMissing} /> 없는 노트</label>
    <label><input type="checkbox" bind:checked={showAttachments} /> 첨부 파일</label>
    <button class="groups-btn" class:on={showGroups} aria-expanded={showGroups} onclick={() => (showGroups = !showGroups)}>
      색 그룹{#if groups.length} <span class="n">{groups.length}</span>{/if}
    </button>
    {#if showGroups}
      <div class="groups-panel" role="region" aria-label="색 그룹">
        <p class="groups-help">조건에 맞는 노트를 그 색으로 칠합니다. 위에 있는 그룹이 먼저. <code>path:폴더</code> <code>tag:#태그</code> <code>file:이름</code> <code>-제외</code></p>
        {#each groups as g, i (i)}
          <div class="group-row">
            <input type="color" bind:value={g.color} oninput={groupsChanged} aria-label="그룹 {i + 1} 색" />
            <input class="group-query" bind:value={g.query} oninput={groupsChanged} placeholder="예: path:Daily"
              aria-label="그룹 {i + 1} 조건" spellcheck="false" />
            <button class="mini" onclick={() => moveGroup(i, -1)} disabled={i === 0} aria-label="위로"><Icon name="chevron-up" size={13} /></button>
            <button class="mini" onclick={() => moveGroup(i, 1)} disabled={i === groups.length - 1} aria-label="아래로"><Icon name="chevron-down" size={13} /></button>
            <button class="mini" onclick={() => removeGroup(i)} aria-label="그룹 {i + 1} 삭제"><Icon name="close" size={13} /></button>
          </div>
        {/each}
        <button class="add-group" onclick={addGroup}><Icon name="plus" size={13} /> 새 그룹</button>
        <p class="groups-help">Obsidian과 같은 <code>.obsidian/graph.json</code>에 저장됩니다.</p>
      </div>
    {/if}
  </header>

  {#if errorText}
    <p class="error" role="alert">{errorText}</p>
  {/if}

  <div class="stage" bind:clientWidth={width} bind:clientHeight={height}>
    <canvas bind:this={canvas} style:width="{width}px" style:height="{height}px"
      aria-label="링크 그래프: 노트 {counts.nodes}개, 링크 {counts.edges}개"
      {onpointerdown} {onpointermove} {onpointerup} {onpointerleave} {onwheel}></canvas>

    {#if warming && warmNotice}
      <div class="warming" role="status">
        <span>배치 계산 중… {warmProgress}%</span>
        <span class="bar"><span style:width="{warmProgress}%"></span></span>
      </div>
    {/if}

    {#if tip}
      <div class="tip" style:left="{tip.x + 14}px" style:top="{tip.y + 14}px">
        <strong>{tip.title}</strong>
        <span>{KIND_LABEL[tip.kind]} · 연결 {tip.degree}개</span>
        {#if tip.kind !== "missing"}<span class="hint">클릭해서 열기</span>{/if}
      </div>
    {/if}

    {#if groups.some((g) => g.query.trim())}
      <ul class="legend" aria-label="색 그룹 범례">
        {#each groups.filter((g) => g.query.trim()) as g, i (i)}
          <li><span class="swatch" style:background={g.color}></span>{g.query}</li>
        {/each}
      </ul>
    {/if}

    <div class="corner">
      <span class="stats">노트 {counts.nodes} · 링크 {counts.edges}</span>
      <button onclick={() => zoomBy(1.3)} aria-label="확대" title="확대"><Icon name="plus" /></button>
      <button onclick={() => zoomBy(1 / 1.3)} aria-label="축소" title="축소"><Icon name="minus" /></button>
      <button onclick={fit} aria-label="화면에 맞추기" title="화면에 맞추기"><Icon name="fit" /></button>
    </div>

    {#if ready && raw && counts.nodes === 0}
      <p class="empty">{local ? "이 노트에는 연결된 노트가 없습니다." : "표시할 노트가 없습니다."}</p>
    {/if}
  </div>
</section>

<style>
  .graph {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .groups-btn {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text-muted);
    font: inherit;
    padding: 4px 10px;
    cursor: pointer;
  }
  .groups-btn.on {
    border-color: var(--accent);
    color: var(--text);
  }
  .groups-btn .n {
    color: var(--text-faint);
  }
  /* Floats under the toolbar, over the graph. */
  .groups-panel {
    position: absolute;
    top: calc(100% + 4px);
    right: 16px;
    z-index: 5;
    width: min(420px, calc(100% - 32px));
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 12px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.18);
  }
  .groups-help {
    margin: 0;
    font-size: 12px;
    color: var(--text-faint);
    line-height: 1.6;
  }
  .groups-help code {
    font-family: var(--font-mono);
    background: var(--code-bg);
    padding: 0 3px;
    border-radius: 3px;
  }
  .group-row {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .group-row input[type="color"] {
    width: 28px;
    height: 26px;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 5px;
    background: none;
    cursor: pointer;
  }
  .group-query {
    flex: 1;
    min-width: 0;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-family: var(--font-mono);
    font-size: 12px;
    padding: 4px 8px;
    outline: none;
  }
  .group-query:focus {
    border-color: var(--accent);
  }
  .mini {
    display: inline-flex;
    border: 0;
    background: none;
    color: var(--text-faint);
    padding: 4px;
    border-radius: 4px;
    cursor: pointer;
  }
  .mini:hover:not(:disabled) {
    background: var(--hover);
    color: var(--text);
  }
  .mini:disabled {
    opacity: 0.3;
    cursor: default;
  }
  .add-group {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    border: 0;
    background: none;
    color: var(--accent);
    font: inherit;
    font-size: 13px;
    padding: 4px 2px;
    cursor: pointer;
  }
  .legend {
    position: absolute;
    left: 12px;
    bottom: 12px;
    margin: 0;
    padding: 8px 10px;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
    color: var(--text-muted);
    background: color-mix(in srgb, var(--bg) 85%, transparent);
    border: 1px solid var(--border);
    border-radius: 6px;
    max-width: 40%;
  }
  .legend li {
    display: flex;
    align-items: center;
    gap: 6px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .swatch {
    flex: none;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    box-shadow: 0 0 0 2px var(--bg);
  }
  .toolbar {
    position: relative;
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px 14px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
    font-size: 13px;
    color: var(--text-muted);
  }
  h1 {
    margin: 0 6px 0 0;
    font-size: 18px;
    color: var(--text);
  }
  .search,
  select {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    padding: 5px 8px;
    outline: none;
  }
  .search {
    width: 180px;
  }
  .search:focus {
    border-color: var(--accent);
  }
  label {
    display: flex;
    align-items: center;
    gap: 4px;
    white-space: nowrap;
  }
  input[type="checkbox"] {
    accent-color: var(--accent);
  }
  .stage {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
  canvas {
    display: block;
    cursor: grab;
    touch-action: none;
  }
  canvas:active {
    cursor: grabbing;
  }
  .warming {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    font-size: 13px;
    color: var(--text-muted);
    pointer-events: none;
  }
  .bar {
    width: 180px;
    height: 4px;
    border-radius: 2px;
    background: var(--border);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  .tip {
    position: absolute;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    box-shadow: 0 4px 12px rgb(0 0 0 / 0.15);
    font-size: 12px;
    color: var(--text-muted);
    pointer-events: none;
    white-space: nowrap;
  }
  .tip strong {
    color: var(--text);
  }
  .hint {
    color: var(--text-faint);
  }
  .corner {
    position: absolute;
    right: 12px;
    bottom: 12px;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .stats {
    font-size: 12px;
    color: var(--text-faint);
    margin-right: 6px;
  }
  .corner button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text-muted);
    cursor: pointer;
  }
  .corner button:hover {
    background: var(--hover);
    color: var(--text);
  }
  .empty {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    margin: 0;
    color: var(--text-faint);
    pointer-events: none;
  }
  .error {
    margin: 0;
    padding: 8px 16px;
    background: var(--danger-bg);
    color: var(--danger);
    font-size: 13px;
  }
</style>
