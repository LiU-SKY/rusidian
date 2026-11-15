<script lang="ts">
  import { untrack } from "svelte";
  import { searchVault, type SearchHit } from "./api";

  interface Props {
    /** `newTab`: Ctrl+click or middle click. */
    onopen: (path: string, newTab?: boolean) => void;
    /** Bumped when vault files change; re-runs the current search. */
    version: number;
    selected: string | null;
  }

  let { onopen, version, selected }: Props = $props();

  let query = $state("");
  let hits = $state<SearchHit[]>([]);
  let searched = $state(false);
  let input: HTMLInputElement | undefined = $state();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let seq = 0;

  export function focus() {
    input?.focus();
    input?.select();
  }

  /** Run a search from elsewhere (e.g. a clicked tag). */
  export function setQuery(q: string) {
    clearTimeout(timer);
    query = q;
    run(q);
    input?.focus();
  }

  async function run(q: string) {
    const s = ++seq;
    if (!q.trim()) {
      hits = [];
      searched = false;
      return;
    }
    try {
      const result = await searchVault(q);
      if (s === seq) {
        hits = result;
        searched = true;
      }
    } catch {
      if (s === seq) hits = [];
    }
  }

  function onInput() {
    clearTimeout(timer);
    timer = setTimeout(() => run(query), 150);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && hits.length) {
      clearTimeout(timer);
      onopen(hits[0].path);
    } else if (e.key === "Escape") {
      query = "";
      run("");
    }
  }

  $effect(() => {
    version;
    untrack(() => {
      if (query.trim()) run(query);
    });
  });

  function folderOf(path: string): string {
    const i = path.lastIndexOf("/");
    return i < 0 ? "" : path.slice(0, i);
  }
</script>

<div class="search">
  <input bind:this={input} bind:value={query} oninput={onInput} onkeydown={onKeydown}
    type="search" placeholder="검색 (여러 단어, &quot;구문&quot;)" aria-label="vault 검색" spellcheck="false" />

  {#if !query.trim()}
    <dl class="help">
      <dt>tag:#일</dt><dd>태그 (하위 태그 포함)</dd>
      <dt>path:폴더</dt><dd>경로에 포함</dd>
      <dt>file:이름</dt><dd>파일 이름에 포함</dd>
      <dt>-단어</dt><dd>이 단어가 없는 것</dd>
    </dl>
  {/if}

  {#if searched}
    <p class="summary">{hits.length ? `${hits.length}개 파일` : "결과 없음"}</p>
  {/if}

  <ul class="results">
    {#each hits as hit (hit.path)}
      <li>
        <button class="hit" class:active={hit.path === selected} onclick={(e) => onopen(hit.path, e.ctrlKey || e.metaKey)}
          onauxclick={(e) => e.button === 1 && onopen(hit.path, true)} title={hit.path}>
          <span class="hit-head">
            <span class="hit-title">{hit.title}</span>
            {#if hit.count}<span class="count">{hit.count}</span>{/if}
          </span>
          {#if folderOf(hit.path)}<span class="folder">{folderOf(hit.path)}</span>{/if}
          {#each hit.snippets as s (s.line)}
            <span class="snippet">{s.before}<mark>{s.hit}</mark>{s.after}</span>
          {/each}
        </button>
      </li>
    {/each}
  </ul>
</div>

<style>
  .search {
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: 100%;
  }
  input {
    margin: 2px 2px 6px;
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    outline: none;
  }
  input:focus {
    border-color: var(--accent);
  }
  .help {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 10px;
    margin: 4px 6px;
    font-size: 12px;
    color: var(--text-faint);
  }
  .help dt {
    font-family: var(--font-mono);
    color: var(--text-muted);
  }
  .help dd {
    margin: 0;
  }
  .summary {
    margin: 0 4px 4px;
    font-size: 12px;
    color: var(--text-faint);
  }
  .results {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow: auto;
    min-height: 0;
  }
  .hit {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 100%;
    border: 0;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 13px;
    text-align: left;
    padding: 6px 8px;
    border-radius: 4px;
    cursor: pointer;
  }
  .hit:hover {
    background: var(--hover);
  }
  .hit.active {
    background: var(--active);
  }
  .hit-head {
    display: flex;
    gap: 6px;
    align-items: baseline;
  }
  .hit-title {
    flex: 1;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    font-size: 11px;
    color: var(--text-faint);
  }
  .folder {
    font-size: 11px;
    color: var(--text-faint);
  }
  .snippet {
    font-size: 12px;
    color: var(--text-muted);
    overflow-wrap: anywhere;
  }
  mark {
    background: var(--accent-bg);
    color: var(--accent);
    border-radius: 2px;
  }
</style>
