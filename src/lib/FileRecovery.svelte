<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import {
    historyClear, historyDiff, historyFiles, historyRead, historyRestore, historyVersions,
    type DiffLine, type HistoryFile, type HistoryVersion,
  } from "./api";
  import Icon from "./Icon.svelte";

  interface Props {
    /** Note to show first (the one open now), if any. */
    start: string | null;
    /** Save open notes before a note is overwritten. */
    beforewrite: () => Promise<void>;
    onclose: () => void;
    /** `path` was put back to an earlier version (or created again). */
    onrestored: (path: string) => void;
  }

  let { start, beforewrite, onclose, onrestored }: Props = $props();

  let files = $state<HistoryFile[]>([]);
  let filter = $state("");
  let path = $state<string | null>(untrack(() => start));
  let versions = $state<HistoryVersion[]>([]);
  let id = $state<number | null>(null);
  let view = $state<"diff" | "text">("diff");
  let diff = $state<DiffLine[]>([]);
  let text = $state("");
  let loading = $state(false);
  let message = $state<string | null>(null);
  let error = $state<string | null>(null);
  let dialog: HTMLDivElement | undefined = $state();

  const file = $derived(files.find((f) => f.path === path) ?? null);
  /** The note is gone: there is nothing to compare with, only to bring back. */
  const deleted = $derived(!!file && !file.exists);
  const shown = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    return q ? files.filter((f) => f.path.toLowerCase().includes(q)) : files;
  });
  const changes = $derived(diff.filter((l) => l.change !== "same").length);

  onMount(() => {
    const before = document.activeElement as HTMLElement | null;
    dialog?.querySelector<HTMLInputElement>("input.filter")?.focus();
    loadFiles();
    return () => {
      if (before?.isConnected) before.focus();
    };
  });

  async function loadFiles() {
    try {
      files = await historyFiles();
      if (!path && files.length) path = files[0].path;
    } catch (e) {
      error = String(e);
    }
  }

  // Versions of the chosen note; the newest is shown first.
  $effect(() => {
    const p = path;
    id = null;
    versions = [];
    if (!p) return;
    historyVersions(p)
      .then((v) => {
        if (path !== p) return;
        versions = v;
        id = v[0]?.id ?? null;
      })
      .catch((e) => (error = String(e)));
  });

  // The chosen version, as changes or as its whole text.
  let seq = 0;
  $effect(() => {
    const p = path;
    const v = id;
    diff = [];
    text = "";
    if (!p || v === null) return;
    const s = ++seq;
    loading = true;
    Promise.all([historyDiff(p, v), historyRead(p, v)])
      .then(([d, t]) => {
        if (s !== seq) return;
        diff = d;
        text = t;
      })
      .catch((e) => (error = String(e)))
      .finally(() => {
        if (s === seq) loading = false;
      });
  });

  $effect(() => {
    if (deleted) view = "text";
  });

  const name = (p: string) => p.split("/").pop()!.replace(/\.md$/i, "");
  const folder = (p: string) => p.split("/").slice(0, -1).join("/");
  const WEEK = ["일", "월", "화", "수", "목", "금", "토"];

  function when(ms: number): string {
    const d = new Date(ms);
    const hm = `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
    return `${d.getMonth() + 1}월 ${d.getDate()}일 (${WEEK[d.getDay()]}) ${hm}`;
  }

  function ago(ms: number): string {
    const min = Math.floor((Date.now() - ms) / 60_000);
    if (min < 1) return "방금";
    if (min < 60) return `${min}분 전`;
    if (min < 60 * 24) return `${Math.floor(min / 60)}시간 전`;
    return `${Math.floor(min / 60 / 24)}일 전`;
  }

  const size = (bytes: number) => (bytes >= 1024 ? `${(bytes / 1024).toFixed(1)} KB` : `${bytes} B`);

  type Row = DiffLine | { change: "gap"; count: number };
  /** Unchanged runs are folded down to a few lines around each change. */
  const CONTEXT = 3;
  const rows = $derived.by((): Row[] => {
    const out: Row[] = [];
    let i = 0;
    while (i < diff.length) {
      if (diff[i].change !== "same") {
        out.push(diff[i++]);
        continue;
      }
      let j = i;
      while (j < diff.length && diff[j].change === "same") j++;
      const head = i === 0 ? 0 : CONTEXT;
      const tail = j === diff.length ? 0 : CONTEXT;
      if (j - i > head + tail + 1) {
        out.push(...diff.slice(i, i + head), { change: "gap", count: j - i - head - tail }, ...diff.slice(j - tail, j));
      } else {
        out.push(...diff.slice(i, j));
      }
      i = j;
    }
    return out;
  });

  async function restore() {
    if (!path || id === null) return;
    const p = path;
    const v = id;
    const question = deleted
      ? `삭제된 "${name(p)}" 노트를 ${when(v)} 내용으로 되살릴까요?`
      : `"${name(p)}"을(를) ${when(v)} 버전으로 되돌릴까요?\n지금 내용은 이전 버전으로 남겨 둡니다.`;
    const ok = await ask(question, { title: "파일 복구", kind: "warning", okLabel: deleted ? "되살리기" : "되돌리기", cancelLabel: "취소" });
    if (!ok) return;
    try {
      await beforewrite();
      await historyRestore(p, v);
      onrestored(p);
      message = deleted ? `"${name(p)}" 노트를 되살렸습니다.` : `${when(v)} 버전으로 되돌렸습니다.`;
      await loadFiles();
      if (path === p) {
        versions = await historyVersions(p);
        id = versions[0]?.id ?? null;
      }
    } catch (e) {
      error = String(e);
    }
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
      message = "이 버전의 내용을 복사했습니다.";
    } catch (e) {
      error = String(e);
    }
  }

  async function clear() {
    if (!path) return;
    const p = path;
    const ok = await ask(`"${name(p)}"의 이전 버전 ${versions.length}개를 모두 지울까요? 되돌릴 수 없습니다.`, {
      title: "기록 지우기", kind: "warning", okLabel: "지우기", cancelLabel: "취소",
    });
    if (!ok) return;
    try {
      await historyClear(p);
      path = null;
      await loadFiles();
      message = "기록을 지웠습니다.";
    } catch (e) {
      error = String(e);
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
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="recovery-title" tabindex="-1" bind:this={dialog} {onkeydown}>
    <header>
      <h2 id="recovery-title">파일 복구</h2>
      <span class="status">노트를 고치거나 지우기 전 내용을 이 컴퓨터에만 30일 동안 남겨 둡니다 (5분에 한 번까지).</span>
      <button class="close" onclick={onclose} aria-label="닫기"><Icon name="close" size={16} /></button>
    </header>

    <div class="body">
      <aside class="files">
        <input class="filter" placeholder="노트 찾기" bind:value={filter} />
        <ul>
          {#if path && !file}
            <li><button class="file active" disabled><span class="fname">{name(path)}</span><span class="fmeta">아직 이전 버전 없음</span></button></li>
          {/if}
          {#each shown as f (f.path)}
            <li>
              <button class={["file", { active: f.path === path }]} onclick={() => (path = f.path)} title={f.path}>
                <span class="fname">{name(f.path)}{#if !f.exists}<span class="chip">삭제됨</span>{/if}</span>
                <span class="fmeta">{folder(f.path) || "/"} · {f.count}개 · {ago(f.latest)}</span>
              </button>
            </li>
          {:else}
            {#if !path}<li class="empty">{filter ? "일치하는 노트 없음" : "아직 남겨 둔 버전이 없습니다."}</li>{/if}
          {/each}
        </ul>
      </aside>

      <aside class="versions">
        <ul>
          {#each versions as v (v.id)}
            <li>
              <button class={["version", { active: v.id === id }]} onclick={() => (id = v.id)}>
                <span>{when(v.id)}</span>
                <span class="fmeta">{ago(v.id)} · {size(v.size)}</span>
              </button>
            </li>
          {:else}
            <li class="empty">{path ? "이전 버전이 없습니다. 노트를 고치면 5분마다 남겨 둡니다." : ""}</li>
          {/each}
        </ul>
      </aside>

      <section class="preview">
        {#if id !== null}
          <div class="tools">
            <div class="seg" role="tablist">
              <button role="tab" aria-selected={view === "diff"} class:on={view === "diff"} disabled={deleted} onclick={() => (view = "diff")}>
                바뀐 부분{#if !deleted && !loading} ({changes}줄){/if}
              </button>
              <button role="tab" aria-selected={view === "text"} class:on={view === "text"} onclick={() => (view = "text")}>이 버전 전체</button>
            </div>
            <span class="hint">{deleted ? "삭제된 노트입니다." : view === "diff" ? "이 버전 → 지금 노트" : ""}</span>
          </div>
          <div class="content" aria-busy={loading}>
            {#if view === "text"}
              <pre class="text">{text}</pre>
            {:else if !loading && changes === 0}
              <p class="empty">지금 노트와 내용이 같습니다.</p>
            {:else}
              <div class="diff">
                {#each rows as r, i (i)}
                  {#if r.change === "gap"}
                    <div class="gap">⋯ 같은 줄 {r.count}개</div>
                  {:else}
                    <div class={["dl", r.change]}><span class="mark">{r.change === "added" ? "+" : r.change === "removed" ? "−" : ""}</span>{r.text || " "}</div>
                  {/if}
                {/each}
              </div>
            {/if}
          </div>
        {:else}
          <p class="empty pad">왼쪽에서 노트와 버전을 고르세요.</p>
        {/if}
      </section>
    </div>

    <footer>
      {#if message}<span class="msg" role="status">{message}</span>{/if}
      {#if error}<span class="err" role="alert">{error}</span>{/if}
      <span class="spacer"></span>
      {#if path && versions.length}
        <button class="danger" onclick={clear} title="이 노트의 이전 버전을 모두 지웁니다">기록 지우기</button>
      {/if}
      <button onclick={copy} disabled={id === null || loading}>내용 복사</button>
      <button class="primary" onclick={restore} disabled={id === null || loading || (!deleted && changes === 0)}>
        {deleted ? "되살리기" : "이 버전으로 되돌리기"}
      </button>
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
    height: min(720px, calc(100vh - 48px));
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
    margin-left: auto;
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
    grid-template-columns: 240px 200px minmax(0, 1fr);
  }
  aside {
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--border);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 6px;
    overflow: auto;
    flex: 1;
  }
  .filter {
    margin: 10px 10px 2px;
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
  .file,
  .version {
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
  .file:hover,
  .version:hover {
    background: var(--hover);
  }
  .file.active,
  .version.active {
    background: var(--active);
  }
  .file:disabled {
    cursor: default;
  }
  .fname {
    display: flex;
    align-items: center;
    gap: 6px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .fmeta {
    font-size: 11px;
    color: var(--text-faint);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chip {
    flex: none;
    font-size: 10px;
    padding: 0 5px;
    border-radius: 4px;
    background: var(--danger-bg);
    color: var(--danger);
  }
  .empty {
    padding: 8px;
    font-size: 12px;
    line-height: 1.6;
    color: var(--text-faint);
  }
  .pad {
    padding: 20px;
  }
  .preview {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .tools {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 14px;
    border-bottom: 1px solid var(--border);
  }
  .seg {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  .seg button {
    border: 0;
    background: var(--bg);
    color: var(--text-muted);
    font: inherit;
    font-size: 12px;
    padding: 4px 10px;
    cursor: pointer;
  }
  .seg button + button {
    border-left: 1px solid var(--border);
  }
  .seg button.on {
    background: var(--accent-bg);
    color: var(--accent);
  }
  .seg button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .hint {
    font-size: 12px;
    color: var(--text-faint);
  }
  .content {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .text,
  .diff {
    margin: 0;
    padding: 10px 0;
    font-family: var(--font-mono);
    font-size: 12.5px;
    line-height: 1.6;
  }
  .text {
    padding: 10px 16px;
    white-space: pre-wrap;
    word-break: break-word;
    color: var(--text);
  }
  .dl {
    padding: 0 16px 0 28px;
    white-space: pre-wrap;
    word-break: break-word;
    position: relative;
  }
  .mark {
    position: absolute;
    left: 12px;
    color: var(--text-faint);
  }
  .dl.same {
    color: var(--text-muted);
  }
  .dl.added {
    background: rgb(46 160 67 / 0.15);
  }
  .dl.removed {
    background: var(--danger-bg);
  }
  .gap {
    padding: 2px 16px 2px 28px;
    font-family: var(--font-text);
    font-size: 11px;
    color: var(--text-faint);
    background: var(--bg-side);
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
  footer .danger {
    color: var(--danger);
  }
  footer button:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
