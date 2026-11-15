<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow, type Window } from "@tauri-apps/api/window";
  import Icon from "./Icon.svelte";

  // The window has no native frame. These three buttons sit in the top-right
  // corner, and every element marked `data-drag` (the top row of each pane)
  // moves the window: press and drag to move, double-click to maximize.

  let win: Window | null = null;
  let maximized = $state(false);

  /** Presses on these never start a drag, even inside a top row. */
  const INTERACTIVE = "button, a, input, select, textarea, label, [contenteditable], [role='tab']";

  function onmousedown(e: MouseEvent) {
    if (!win || e.button !== 0) return;
    const target = e.target as Element | null;
    if (!target?.closest("[data-drag]") || target.closest(INTERACTIVE)) return;
    e.preventDefault();
    if (e.detail === 2) win.toggleMaximize().catch(() => {});
    else win.startDragging().catch(() => {});
  }

  onMount(() => {
    try {
      win = getCurrentWindow();
    } catch {
      return; // plain browser (no Tauri): buttons render, do nothing
    }
    const w = win;
    let unlisten: (() => void) | undefined;
    let destroyed = false;
    const sync = () => {
      w.isMaximized().then((m) => (maximized = m)).catch(() => {});
    };
    sync();
    w.onResized(sync)
      .then((u) => (destroyed ? u() : (unlisten = u)))
      .catch(() => {});
    return () => {
      destroyed = true;
      unlisten?.();
    };
  });
</script>

<svelte:window {onmousedown} />

<div class="controls">
  <button onclick={() => win?.minimize()} title="최소화" aria-label="최소화">
    <Icon name="win-minimize" size={16} />
  </button>
  <button onclick={() => win?.toggleMaximize()} title={maximized ? "이전 크기로" : "최대화"}
    aria-label={maximized ? "이전 크기로" : "최대화"}>
    <Icon name={maximized ? "win-restore" : "win-maximize"} size={16} />
  </button>
  <button class="close" onclick={() => win?.close()} title="닫기" aria-label="닫기">
    <Icon name="win-close" size={16} />
  </button>
</div>

<style>
  .controls {
    position: absolute;
    top: 0;
    right: 0;
    z-index: 20;
    display: flex;
    height: var(--top-h);
  }
  button {
    width: var(--control-w);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 0;
    background: none;
    color: var(--text-muted);
    cursor: default;
  }
  button:hover {
    background: var(--hover);
    color: var(--text);
  }
  /* The one fixed color: the platform's red close hover. */
  .close:hover {
    background: #c42b1c;
    color: #fff;
  }
</style>
