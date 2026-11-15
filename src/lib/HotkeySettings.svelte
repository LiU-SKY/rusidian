<script lang="ts" module>
  import type { Hotkey } from "./api";
  import type { IconName } from "./icons";

  export interface HotkeyCommand {
    /** Its id in hotkeys.json. */
    id: string;
    label: string;
    icon: IconName;
    defaults: Hotkey[];
  }
</script>

<script lang="ts">
  import { fromEvent, hotkeys, label, same, usable } from "./hotkeys.svelte";
  import Icon from "./Icon.svelte";

  interface Props {
    commands: HotkeyCommand[];
    /** Keys are saved in the vault: without one they can't change. */
    hasVault: boolean;
  }

  let { commands, hasVault }: Props = $props();

  let filter = $state("");
  /** The command waiting for a key press. */
  let recording = $state<string | null>(null);
  let notice = $state<string | null>(null);

  const keysOf = (c: HotkeyCommand) => hotkeys.keysFor(c.id, c.defaults);
  const changed = (c: HotkeyCommand) => c.id in hotkeys.custom;

  /** Key label → commands using it, to point out clashes. */
  const users = $derived.by(() => {
    const map = new Map<string, string[]>();
    for (const c of commands) {
      for (const k of keysOf(c)) map.set(label(k), [...(map.get(label(k)) ?? []), c.label]);
    }
    return map;
  });

  const shown = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    if (!q) return commands;
    return commands.filter(
      (c) => c.label.toLowerCase().includes(q) || keysOf(c).some((k) => label(k).toLowerCase().includes(q)),
    );
  });

  async function save(c: HotkeyCommand, keys: Hotkey[] | null) {
    try {
      await hotkeys.set(c.id, keys);
    } catch (e) {
      notice = String(e);
    }
  }

  // While recording, the next key press is the new hotkey (Esc cancels).
  $effect(() => {
    const id = recording;
    if (!id) return;
    const onkey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopImmediatePropagation();
      if (e.key === "Escape") {
        recording = null;
        return;
      }
      const h = fromEvent(e);
      if (!h) return; // a modifier on its own: wait for the key
      const c = commands.find((x) => x.id === id);
      recording = null;
      if (!c) return;
      if (!usable(h)) {
        notice = "Ctrl·Alt 같은 키와 함께 누르세요. (F1~F12는 그대로 쓸 수 있습니다)";
        return;
      }
      const current = keysOf(c);
      if (current.some((k) => same(k, h))) return;
      const others = (users.get(label(h)) ?? []).filter((l) => l !== c.label);
      notice = others.length ? `${label(h)}는 "${others.join('", "')}"에도 쓰입니다. 목록에서 위에 있는 명령이 실행됩니다.` : null;
      save(c, [...current, h]);
    };
    window.addEventListener("keydown", onkey, true);
    return () => window.removeEventListener("keydown", onkey, true);
  });
</script>

<div class="hotkeys">
  <div class="top">
    <input class="filter" placeholder="명령이나 키로 찾기" bind:value={filter} />
    <span class="where">Obsidian과 같은 hotkeys.json에 저장</span>
  </div>
  {#if !hasVault}
    <p class="notice">vault를 열면 단축키를 바꿀 수 있습니다.</p>
  {:else if notice}
    <p class="notice" role="status">{notice}</p>
  {/if}
  <ul>
    {#each shown as c (c.id)}
      {@const keys = keysOf(c)}
      <li>
        <Icon name={c.icon} size={15} />
        <span class="label">{c.label}{#if changed(c)}<span class="dot" title="바꾼 단축키"></span>{/if}</span>
        <span class="keys">
          {#each keys as k (label(k))}
            {@const clash = (users.get(label(k))?.length ?? 0) > 1}
            <span class={["key", { clash }]} title={clash ? `같은 키: ${users.get(label(k))!.join(", ")}` : undefined}>
              <kbd>{label(k)}</kbd>
              <button class="x" disabled={!hasVault} onclick={() => save(c, keys.filter((x) => !same(x, k)))}
                aria-label="{label(k)} 지우기"><Icon name="close" size={11} /></button>
            </span>
          {/each}
          {#if recording === c.id}
            <span class="key recording">키를 누르세요… (Esc 취소)</span>
          {:else if !keys.length}
            <span class="none">없음</span>
          {/if}
        </span>
        {#if changed(c)}
          <button class="tool" disabled={!hasVault} onclick={() => save(c, null)} title="기본값으로 ({c.defaults.map(label).join(", ") || "없음"})"
            aria-label="{c.label} 기본값으로"><Icon name="refresh" size={14} /></button>
        {/if}
        <button class="tool" disabled={!hasVault} onclick={() => { notice = null; recording = recording === c.id ? null : c.id; }}
          title="단축키 추가" aria-label="{c.label} 단축키 추가"><Icon name="plus" size={14} /></button>
      </li>
    {:else}
      <li class="empty">일치하는 명령 없음</li>
    {/each}
  </ul>
</div>

<style>
  .hotkeys {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
  }
  .top {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 20px 8px;
  }
  .filter {
    flex: 1;
    min-width: 0;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    padding: 6px 8px;
    outline: none;
  }
  .filter:focus {
    border-color: var(--accent);
  }
  .where {
    font-size: 11px;
    color: var(--text-faint);
    white-space: nowrap;
  }
  .notice {
    margin: 0 20px 6px;
    padding: 6px 10px;
    border-radius: 6px;
    background: var(--notice-bg);
    color: var(--notice);
    font-size: 12px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0 12px 12px;
    overflow: auto;
    min-height: 0;
    flex: 1;
  }
  li {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 34px;
    padding: 3px 8px;
    border-radius: 6px;
    color: var(--text-muted);
  }
  li:hover {
    background: var(--hover);
  }
  .label {
    flex: 1;
    min-width: 0;
    font-size: 13px;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dot {
    display: inline-block;
    width: 6px;
    height: 6px;
    margin-left: 6px;
    border-radius: 50%;
    background: var(--accent);
    vertical-align: middle;
  }
  .keys {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 4px;
  }
  .key {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 1px 2px 1px 6px;
    border: 1px solid var(--border);
    border-radius: 5px;
    background: var(--bg);
    font-size: 12px;
  }
  .key.clash {
    border-color: var(--danger);
    color: var(--danger);
  }
  .key.recording {
    padding: 1px 8px;
    border-color: var(--accent);
    color: var(--accent);
  }
  kbd {
    font-family: inherit;
    color: inherit;
  }
  .none {
    font-size: 12px;
    color: var(--text-faint);
  }
  .x,
  .tool {
    display: inline-flex;
    border: 0;
    background: none;
    color: var(--text-faint);
    padding: 3px;
    border-radius: 4px;
    cursor: pointer;
  }
  .x:hover,
  .tool:hover {
    color: var(--text);
    background: var(--active);
  }
  .x:disabled,
  .tool:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .empty {
    font-size: 12px;
    color: var(--text-faint);
  }
</style>
