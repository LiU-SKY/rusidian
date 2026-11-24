<script lang="ts" module>
  import type { IconName } from "./icons";

  export interface PaletteItem {
    id: string;
    label: string;
    /** Indices of `label` characters to highlight (fuzzy match). */
    marks?: number[];
    /** Second line, e.g. the folder. */
    detail?: string;
    /** Right-aligned hint, e.g. a shortcut. */
    hint?: string;
    icon?: IconName;
    /** Shown dimmed; picking it still calls onpick. */
    disabled?: boolean;
    /** Offer a × button (see the palette's `onremove`). */
    removable?: boolean;
  }
</script>

<script lang="ts">
  import { onMount, untrack } from "svelte";
  import Icon from "./Icon.svelte";
  import { MOBILE } from "./platform";

  interface Props {
    placeholder: string;
    /** Items for the current query, best first. */
    items: (query: string) => PaletteItem[];
    /** Modifier keys held while picking (Shift+Enter, Ctrl+Enter, or with a click). */
    onpick: (item: PaletteItem, query: string, keys: { shift: boolean; mod: boolean }) => void;
    onclose: () => void;
    /** Key hints shown under the list. */
    footer?: string;
    empty?: string;
    /** Items with `removable` get a × button that calls this (e.g. drop from a list). */
    onremove?: (item: PaletteItem) => void;
    /**
     * Called with the query as it changes (and once on opening), for lists
     * filled asynchronously: fetch, put the results in state that `items`
     * reads, and the list follows.
     */
    onquery?: (query: string) => void;
  }

  let { placeholder, items, onpick, onclose, footer, empty = "결과 없음", onremove, onquery }: Props = $props();

  let query = $state("");
  let active = $state(0);
  let input: HTMLInputElement | undefined = $state();
  let listEl: HTMLUListElement | undefined = $state();
  let list = $derived(items(query));

  onMount(() => {
    const before = document.activeElement as HTMLElement | null;
    input?.focus();
    // Give focus back (usually the editor) when the palette closes.
    return () => {
      if (before?.isConnected) before.focus();
    };
  });

  $effect(() => {
    active;
    listEl?.querySelector(".active")?.scrollIntoView({ block: "nearest" });
  });

  $effect(() => {
    const q = query;
    untrack(() => onquery?.(q));
  });

  // Async results can arrive shorter than the list the cursor was on.
  $effect(() => {
    if (active >= list.length) untrack(() => (active = Math.max(0, list.length - 1)));
  });

  function pick(i: number, e: { shiftKey: boolean; ctrlKey: boolean; metaKey: boolean }) {
    const item = list[i];
    if (!item) return;
    onclose();
    onpick(item, query.trim(), { shift: e.shiftKey, mod: e.ctrlKey || e.metaKey });
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.isComposing) return;
    if (e.key === "ArrowDown" || (e.key === "n" && e.ctrlKey)) {
      e.preventDefault();
      if (list.length) active = (active + 1) % list.length;
    } else if (e.key === "ArrowUp" || (e.key === "p" && e.ctrlKey)) {
      e.preventDefault();
      if (list.length) active = (active - 1 + list.length) % list.length;
    } else if (e.key === "Enter") {
      e.preventDefault();
      pick(active, e);
    } else if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
    // Keep app shortcuts (Ctrl+O, Ctrl+P, …) from firing underneath.
    e.stopPropagation();
  }

  /** Label split into highlighted / plain runs. */
  function runs(label: string, marks: number[] = []): { text: string; hit: boolean }[] {
    const set = new Set(marks);
    const out: { text: string; hit: boolean }[] = [];
    for (let i = 0; i < label.length; i++) {
      const hit = set.has(i);
      const last = out[out.length - 1];
      if (last && last.hit === hit) last.text += label[i];
      else out.push({ text: label[i], hit });
    }
    return out;
  }
</script>

<div class="backdrop" role="presentation" onpointerdown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="palette" role="dialog" aria-modal="true" aria-label={placeholder}>
    <input bind:this={input} bind:value={query} {placeholder} {onkeydown} oninput={() => (active = 0)}
      spellcheck="false" autocomplete="off" role="combobox" aria-expanded="true" aria-controls="palette-list"
      aria-activedescendant={list[active] ? `palette-${active}` : undefined} />
    <ul id="palette-list" role="listbox" bind:this={listEl}>
      {#each list as item, i (item.id)}
        <li id="palette-{i}" role="option" aria-selected={i === active} class:active={i === active}
          class:disabled={item.disabled} onpointermove={() => (active = i)}>
          <button tabindex="-1" onclick={(e) => pick(i, e)}>
            {#if item.icon}<Icon name={item.icon} size={16} />{/if}
            <span class="text">
              <span class="label">{#each runs(item.label, item.marks) as r}{#if r.hit}<mark>{r.text}</mark>{:else}{r.text}{/if}{/each}</span>
              {#if item.detail}<span class="detail">{item.detail}</span>{/if}
            </span>
            {#if item.hint}<span class="hint">{item.hint}</span>{/if}
          </button>
          {#if item.removable && onremove}
            <button class="remove" tabindex="-1" onclick={() => { onremove(item); input?.focus(); }}
              title="목록에서 빼기" aria-label="{item.label} 목록에서 빼기"><Icon name="close" size={14} /></button>
          {/if}
        </li>
      {:else}
        <li class="none">{empty}</li>
      {/each}
    </ul>
    <!-- key hints: nothing to press on a phone -->
    {#if footer && !MOBILE}<div class="footer">{footer}</div>{/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: rgb(0 0 0 / 0.25);
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: 12vh;
  }
  .palette {
    width: min(620px, calc(100vw - 32px));
    max-height: 70vh;
    display: flex;
    flex-direction: column;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: 0 16px 48px rgb(0 0 0 / 0.25);
    overflow: hidden;
  }
  input {
    border: 0;
    border-bottom: 1px solid var(--border);
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 15px;
    padding: 14px 16px;
    outline: none;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 6px;
    overflow: auto;
    min-height: 0;
  }
  li button {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    border: 0;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 14px;
    text-align: left;
    padding: 7px 10px;
    border-radius: 6px;
    cursor: pointer;
  }
  li {
    position: relative;
  }
  li.active > button:first-child {
    background: var(--active);
  }
  li.disabled .text {
    opacity: 0.5;
  }
  .remove {
    position: absolute;
    right: 8px;
    top: 50%;
    transform: translateY(-50%);
    width: auto !important;
    padding: 4px !important;
    border-radius: 4px;
    opacity: 0;
  }
  li:has(.remove) > button:first-child {
    padding-right: 38px;
  }
  li:hover .remove,
  li.active .remove {
    opacity: 1;
  }
  .remove:hover {
    background: var(--hover) !important;
  }
  li button :global(.icon) {
    flex: none;
    color: var(--text-muted);
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .label,
  .detail {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .detail {
    font-size: 12px;
    color: var(--text-faint);
  }
  mark {
    background: none;
    color: var(--accent);
    font-weight: 600;
  }
  .hint {
    flex: none;
    font-size: 12px;
    color: var(--text-faint);
  }
  .none {
    padding: 10px;
    font-size: 13px;
    color: var(--text-faint);
  }
  .footer {
    border-top: 1px solid var(--border);
    padding: 7px 14px;
    font-size: 12px;
    color: var(--text-faint);
  }
</style>
