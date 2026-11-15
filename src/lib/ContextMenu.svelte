<script lang="ts" module>
  import type { IconName } from "./icons";

  export interface MenuItem {
    label: string;
    icon: IconName;
    action: () => void;
    danger?: boolean;
    /** Draw a divider above this item. */
    separated?: boolean;
    shortcut?: string;
  }
</script>

<script lang="ts">
  import { onMount, tick } from "svelte";
  import Icon from "./Icon.svelte";

  interface Props {
    x: number;
    y: number;
    items: MenuItem[];
    onclose: () => void;
  }

  let { x, y, items, onclose }: Props = $props();
  let menu: HTMLDivElement | undefined = $state();
  let left = $state(0);
  let top = $state(0);

  onMount(async () => {
    left = x;
    top = y;
    await tick();
    if (!menu) return;
    // Keep the menu inside the window.
    const r = menu.getBoundingClientRect();
    left = Math.max(4, Math.min(x, innerWidth - r.width - 4));
    top = Math.max(4, Math.min(y, innerHeight - r.height - 4));
    menu.querySelector("button")?.focus();
  });

  function run(item: MenuItem) {
    onclose();
    item.action();
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const buttons = [...(menu?.querySelectorAll("button") ?? [])];
      const i = buttons.indexOf(document.activeElement as HTMLButtonElement);
      const next = e.key === "ArrowDown" ? (i + 1) % buttons.length : (i - 1 + buttons.length) % buttons.length;
      buttons[next]?.focus();
    }
  }

  function onpointerdown(e: PointerEvent) {
    if (menu && !menu.contains(e.target as Node)) onclose();
  }
</script>

<svelte:window {onkeydown} {onpointerdown} onblur={onclose} onresize={onclose} />

<div class="menu" role="menu" bind:this={menu} style:left="{left}px" style:top="{top}px">
  {#each items as item (item.label)}
    {#if item.separated}<div class="sep" role="separator"></div>{/if}
    <button role="menuitem" class:danger={item.danger} onclick={() => run(item)}>
      <Icon name={item.icon} size={15} />
      <span class="label">{item.label}</span>
      {#if item.shortcut}<span class="key">{item.shortcut}</span>{/if}
    </button>
  {/each}
</div>

<style>
  .menu {
    position: fixed;
    z-index: 50;
    min-width: 190px;
    padding: 4px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.18);
    font-size: 13px;
  }
  button {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    border: 0;
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    padding: 6px 10px;
    border-radius: 5px;
    cursor: pointer;
  }
  button :global(.icon) {
    color: var(--text-muted);
  }
  button:hover,
  button:focus-visible {
    background: var(--hover);
    outline: none;
  }
  .danger,
  .danger :global(.icon) {
    color: var(--danger);
  }
  .danger:hover,
  .danger:focus-visible {
    background: var(--danger-bg);
  }
  .label {
    flex: 1;
  }
  .key {
    font-size: 11px;
    color: var(--text-faint);
  }
  .sep {
    height: 1px;
    margin: 4px 6px;
    background: var(--border);
  }
</style>
