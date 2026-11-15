<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import {
    conflictBlocks, displayName, listConflicts, renamePath, trashPath, writeNote,
    type ConflictCopy, type ConflictView,
  } from "./api";
  import Icon from "./Icon.svelte";

  interface Props {
    /** Conflict copy to show first. */
    start?: string | null;
    /** Save open notes before a file is overwritten. */
    beforewrite: () => Promise<void>;
    onclose: () => void;
    /** Files were written, moved or trashed. */
    onchanged: (paths: string[]) => void;
  }

  let { start = null, beforewrite, onclose, onchanged }: Props = $props();

  type Pick = "original" | "copy" | "both";

  let list = $state<ConflictCopy[]>([]);
  let selected = $state<string | null>(untrack(() => start));
  let view = $state<ConflictView | null>(null);
  /** What to keep at each differing block (by block index). */
  let picks = $state<Record<number, Pick>>({});
  /** The merged text, once edited by hand. */
  let edited = $state<string | null>(null);
  let showResult = $state(false);
  let busy = $state(false);
  let message = $state<string | null>(null);
  let error = $state<string | null>(null);
  let dialog: HTMLDivElement | undefined = $state();

  onMount(() => {
    const before = document.activeElement as HTMLElement | null;
    dialog?.focus();
    loadList();
    return () => {
      if (before?.isConnected) before.focus();
    };
  });

  async function loadList() {
    try {
      list = await listConflicts();
      if (!selected || !list.some((c) => c.copy === selected)) selected = list[0]?.copy ?? null;
    } catch (e) {
      error = String(e);
    }
  }

  let seq = 0;
  $effect(() => {
    const copy = selected;
    view = null;
    edited = null;
    showResult = false;
    if (!copy) return;
    const s = ++seq;
    conflictBlocks(copy)
      .then((v) => {
        if (s !== seq) return;
        view = v;
        // Start from the file as it is (or the copy, when the file is gone).
        const first: Pick = v.original_exists ? "original" : "copy";
        const next: Record<number, Pick> = {};
        v.blocks.forEach((b, i) => {
          if (b.kind === "differ") next[i] = first;
        });
        picks = next;
      })
      .catch((e) => (error = String(e)));
  });

  const name = (p: string) => displayName(p.split("/").pop()!);
  const differ = $derived(view?.blocks.filter((b) => b.kind === "differ").length ?? 0);
  /** Who made each side. */
  const originalLabel = $derived(view?.this_device ? "다른 기기 (원래 파일)" : "이 기기 (원래 파일)");
  const copyLabel = $derived(view?.this_device ? "이 기기 (사본)" : "다른 기기 (사본)");

  const merged = $derived.by(() => {
    if (!view) return "";
    const lines = view.blocks.flatMap((b, i) => {
      if (b.kind === "same") return b.lines;
      const p = picks[i] ?? "original";
      return p === "original" ? b.original : p === "copy" ? b.copy : [...b.original, ...b.copy];
    });
    const eol = view.crlf ? "\r\n" : "\n";
    return lines.join(eol) + (view.trailing_newline && lines.length ? eol : "");
  });

  function pickAll(p: Pick) {
    if (!view) return;
    const next: Record<number, Pick> = {};
    for (const k of Object.keys(picks)) next[Number(k)] = p;
    picks = next;
    edited = null;
  }

  /** Runs of shared lines are folded down to a few around each difference. */
  const CONTEXT = 2;
  function folded(lines: string[], first: boolean, last: boolean): (string | { gap: number })[] {
    const head = first ? 0 : CONTEXT;
    const tail = last ? 0 : CONTEXT;
    if (lines.length <= head + tail + 1) return lines;
    return [...lines.slice(0, head), { gap: lines.length - head - tail }, ...lines.slice(lines.length - tail)];
  }

  async function finish(text: string, paths: string[]) {
    message = text;
    onchanged(paths);
    await loadList();
  }

  /** Write the merged text into the file and drop the copy. */
  async function save() {
    if (!view) return;
    const v = view;
    busy = true;
    try {
      await beforewrite();
      await writeNote(v.original, edited ?? merged);
      await trashPath(v.copy);
      await finish(`"${name(v.original)}"에 합쳐 저장하고 사본을 휴지통으로 옮겼습니다.`, [v.original, v.copy]);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  /** Keep the file as it is. */
  async function dropCopy() {
    if (!view) return;
    const v = view;
    const ok = await ask(`"${name(v.copy)}" 사본을 휴지통으로 옮길까요? "${name(v.original)}"은(는) 그대로 둡니다.`, {
      title: "사본 버리기", kind: "warning", okLabel: "휴지통으로", cancelLabel: "취소",
    });
    if (!ok) return;
    busy = true;
    try {
      await trashPath(v.copy);
      await finish("사본을 휴지통으로 옮겼습니다.", [v.copy]);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  /** The copy becomes the file (for files that can't be compared, or a file that is gone). */
  async function useCopy() {
    if (!view) return;
    const v = view;
    const ok = await ask(
      v.original_exists
        ? `"${name(v.original)}"을(를) 휴지통으로 옮기고 사본을 그 이름으로 바꿀까요?`
        : `사본을 "${name(v.original)}" 이름으로 바꿀까요?`,
      { title: "사본으로 바꾸기", kind: "warning", okLabel: "바꾸기", cancelLabel: "취소" },
    );
    if (!ok) return;
    busy = true;
    try {
      await beforewrite();
      if (v.original_exists) await trashPath(v.original);
      await renamePath(v.copy, v.original.split("/").pop()!);
      await finish(`사본을 "${name(v.original)}"(으)로 바꿨습니다.`, [v.original, v.copy]);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
    e.stopPropagation();
  }
</script>

<div class="backdrop" role="presentation" onpointerdown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="conflict-title" tabindex="-1" bind:this={dialog} {onkeydown}>
    <header>
      <h2 id="conflict-title">동기화 충돌 합치기</h2>
      <span class="status">두 기기에서 같은 파일을 고쳐 생긴 사본을 원래 파일과 비교해 하나로 합칩니다.</span>
      <button class="close" onclick={onclose} aria-label="닫기"><Icon name="close" size={16} /></button>
    </header>

    <div class="body">
      <aside>
        <ul>
          {#each list as c (c.copy)}
            <li>
              <button class={["item", { active: c.copy === selected }]} onclick={() => (selected = c.copy)} title={c.copy}>
                <span class="iname">{name(c.original)}</span>
                <span class="imeta">{c.stamp} · {c.this_device ? "이 기기 사본" : "다른 기기 사본"}</span>
              </button>
            </li>
          {:else}
            <li class="empty">충돌 사본이 없습니다.</li>
          {/each}
        </ul>
      </aside>

      <section class="main">
        {#if view}
          <div class="tools">
            <span class="paths" title="{view.original} ↔ {view.copy}">
              <b>{view.original}</b>
              {#if !view.original_exists}<span class="chip">원래 파일 없음</span>{/if}
            </span>
            {#if view.text && differ > 0}
              <span class="count">다른 곳 {differ}군데</span>
              <button class="small" onclick={() => pickAll("original")}>모두 원래 파일</button>
              <button class="small" onclick={() => pickAll("copy")}>모두 사본</button>
              <button class={["small", { on: showResult }]} onclick={() => (showResult = !showResult)}>결과 보기·고치기</button>
            {/if}
          </div>

          <div class="scroll">
            {#if !view.text}
              <p class="note">글자 파일이 아니어서 줄 단위로 비교할 수 없습니다. 원래 파일을 둘지, 사본으로 바꿀지 고르세요.</p>
            {:else if differ === 0}
              <p class="note">두 파일의 내용이 같습니다. 사본을 버려도 잃는 것이 없습니다.</p>
            {:else if showResult}
              <textarea class="result" value={edited ?? merged} oninput={(e) => (edited = e.currentTarget.value)}
                aria-label="합친 결과"></textarea>
              {#if edited !== null}
                <p class="hint">직접 고친 내용으로 저장합니다. <button class="link" onclick={() => (edited = null)}>고친 것 되돌리기</button></p>
              {/if}
            {:else}
              {#each view.blocks as b, i (i)}
                {#if b.kind === "same"}
                  <div class="same">
                    {#each folded(b.lines, i === 0, i === view.blocks.length - 1) as l, j (j)}
                      {#if typeof l === "string"}<div class="line">{l || " "}</div>{:else}<div class="gap">⋯ 같은 줄 {l.gap}개</div>{/if}
                    {/each}
                  </div>
                {:else}
                  {@const p = picks[i] ?? "original"}
                  <div class="differ">
                    <div class="pick" role="radiogroup" aria-label="이 부분에 남길 내용">
                      {#each [["original", "원래 파일"], ["copy", "사본"], ["both", "둘 다"]] as [value, label] (value)}
                        <button role="radio" aria-checked={p === value} class:on={p === value}
                          onclick={() => { picks[i] = value as Pick; edited = null; }}>{label}</button>
                      {/each}
                    </div>
                    <div class="sides">
                      <div class={["side", "original", { chosen: p !== "copy" }]}>
                        <div class="label">{originalLabel}</div>
                        {#each b.original as l, j (j)}<div class="line">{l || " "}</div>{:else}<div class="line none">(없음)</div>{/each}
                      </div>
                      <div class={["side", "copy", { chosen: p !== "original" }]}>
                        <div class="label">{copyLabel}</div>
                        {#each b.copy as l, j (j)}<div class="line">{l || " "}</div>{:else}<div class="line none">(없음)</div>{/each}
                      </div>
                    </div>
                  </div>
                {/if}
              {/each}
            {/if}
          </div>
        {:else if selected}
          <p class="note pad">불러오는 중…</p>
        {:else}
          <p class="note pad">합칠 충돌 사본이 없습니다.</p>
        {/if}
      </section>
    </div>

    <footer>
      {#if message}<span class="msg" role="status">{message}</span>{/if}
      {#if error}<span class="err" role="alert">{error}</span>{/if}
      <span class="spacer"></span>
      {#if view}
        {#if view.original_exists}
          <button onclick={dropCopy} disabled={busy} title="원래 파일은 그대로 두고 사본만 휴지통으로">사본 버리기</button>
        {/if}
        {#if !view.text || !view.original_exists}
          <button class="primary" onclick={useCopy} disabled={busy}>사본으로 바꾸기</button>
        {:else}
          <button class="primary" onclick={save} disabled={busy}>합쳐서 저장</button>
        {/if}
      {/if}
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: rgb(0 0 0 / 0.25);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .dialog {
    width: min(1100px, calc(100vw - 32px));
    height: min(760px, calc(100vh - 48px));
    display: flex;
    flex-direction: column;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: 0 16px 48px rgb(0 0 0 / 0.25);
    outline: none;
  }
  header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px 14px 10px 20px;
    border-bottom: 1px solid var(--border);
  }
  h2 {
    margin: 0;
    font-size: 16px;
    white-space: nowrap;
  }
  .status {
    flex: 1;
    min-width: 0;
    font-size: 12px;
    color: var(--text-faint);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .close {
    display: inline-flex;
    border: 0;
    background: none;
    color: var(--text-muted);
    padding: 5px;
    border-radius: 5px;
    cursor: pointer;
  }
  .close:hover {
    background: var(--hover);
  }
  .body {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 240px minmax(0, 1fr);
  }
  aside {
    min-height: 0;
    overflow: auto;
    border-right: 1px solid var(--border);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 6px;
  }
  .item {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 2px;
    border: 0;
    border-radius: 6px;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 13px;
    text-align: left;
    padding: 6px 8px;
    cursor: pointer;
  }
  .item:hover {
    background: var(--hover);
  }
  .item.active {
    background: var(--active);
  }
  .iname,
  .imeta {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .imeta {
    font-size: 11px;
    color: var(--text-faint);
  }
  .empty,
  .note {
    font-size: 12px;
    line-height: 1.6;
    color: var(--text-faint);
    padding: 8px;
    margin: 0;
  }
  .note {
    font-size: 13px;
    color: var(--text-muted);
  }
  .pad {
    padding: 20px;
  }
  .main {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .tools {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    padding: 10px 14px;
    border-bottom: 1px solid var(--border);
    font-size: 13px;
  }
  .paths {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    font-size: 12px;
    color: var(--text-muted);
  }
  .chip {
    margin-left: 6px;
    font-size: 10px;
    padding: 0 5px;
    border-radius: 4px;
    background: var(--danger-bg);
    color: var(--danger);
  }
  .small {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text-muted);
    font: inherit;
    font-size: 12px;
    padding: 3px 8px;
    cursor: pointer;
  }
  .small:hover {
    color: var(--text);
  }
  .small.on {
    border-color: var(--accent);
    color: var(--accent);
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 10px 14px 16px;
    font-family: var(--font-mono);
    font-size: 12.5px;
    line-height: 1.6;
  }
  .line {
    white-space: pre-wrap;
    word-break: break-word;
  }
  .same {
    padding: 2px 8px;
    color: var(--text-muted);
  }
  .gap {
    font-family: var(--font-text);
    font-size: 11px;
    color: var(--text-faint);
  }
  .differ {
    margin: 6px 0;
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
  }
  .pick {
    display: flex;
    gap: 2px;
    padding: 4px;
    background: var(--bg-side);
    border-bottom: 1px solid var(--border);
    font-family: var(--font-text);
  }
  .pick button {
    border: 0;
    border-radius: 5px;
    background: none;
    color: var(--text-muted);
    font: inherit;
    font-size: 12px;
    padding: 3px 10px;
    cursor: pointer;
  }
  .pick button.on {
    background: var(--bg);
    color: var(--text);
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.15);
  }
  .sides {
    display: grid;
    grid-template-columns: 1fr 1fr;
  }
  .side {
    padding: 6px 10px;
    opacity: 0.45;
    transition: opacity 0.15s;
  }
  .side.chosen {
    opacity: 1;
  }
  .side.original {
    border-right: 1px solid var(--border);
    background: var(--danger-bg);
  }
  .side.copy {
    background: rgb(46 160 67 / 0.14);
  }
  .label {
    font-family: var(--font-text);
    font-size: 11px;
    color: var(--text-faint);
    margin-bottom: 2px;
  }
  .none {
    color: var(--text-faint);
    font-style: italic;
  }
  .result {
    width: 100%;
    height: calc(100% - 4px);
    min-height: 300px;
    box-sizing: border-box;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    padding: 10px;
    resize: none;
    outline: none;
  }
  .result:focus {
    border-color: var(--accent);
  }
  .hint {
    margin: 6px 0 0;
    font-family: var(--font-text);
    font-size: 12px;
    color: var(--text-muted);
  }
  .link {
    border: 0;
    background: none;
    padding: 0;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
  }
  footer {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 20px 16px;
    border-top: 1px solid var(--border);
  }
  .spacer {
    flex: 1;
  }
  .msg,
  .err {
    font-size: 12px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .msg {
    color: var(--text-muted);
  }
  .err {
    color: var(--danger);
  }
  footer button {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    padding: 6px 14px;
    cursor: pointer;
    white-space: nowrap;
  }
  footer .primary {
    border-color: var(--accent);
    background: var(--accent);
    color: #fff;
  }
  footer button:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
