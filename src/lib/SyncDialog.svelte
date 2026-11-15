<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { syncScan, type SyncConfig, type SyncInfo, type SyncScan, type TreeNode } from "./api";
  import Icon from "./Icon.svelte";

  interface Props {
    info: SyncInfo;
    /** The vault's file tree (folders are offered for sync). */
    tree: TreeNode[];
    /** "Last synced …" or the last error, shown at the top. */
    status: string | null;
    running: boolean;
    onclose: () => void;
    /** Save (null: turn sync off on this device), then sync if asked. */
    onsave: (config: SyncConfig | null, syncNow: boolean) => void;
  }

  let { info, tree, status, running, onclose, onsave }: Props = $props();

  const start = untrack(() => info);
  let remote = $state(start.config?.remote ?? "");
  let branch = $state(start.config?.branch ?? "main");
  let interval = $state(start.config?.interval_minutes ?? 15);
  let includeCalendars = $state(start.include_calendars);
  let selected = $state<string[]>([...start.folders]);
  // Folders chosen earlier stay visible: open every folder above them.
  let open = $state<Record<string, boolean>>(
    Object.fromEntries(
      start.folders.flatMap((f) => f.split("/").slice(0, -1).map((_, i, a) => [a.slice(0, i + 1).join("/"), true])),
    ),
  );
  let scan = $state<SyncScan | null>(null);
  let scanning = $state(false);
  let showFiles = $state(false);
  let dialog: HTMLDivElement | undefined = $state();

  onMount(() => {
    const before = document.activeElement as HTMLElement | null;
    dialog?.querySelector<HTMLInputElement>("input.remote")?.focus();
    return () => {
      if (before?.isConnected) before.focus();
    };
  });

  const covered = (path: string) => selected.some((s) => path === s || path.startsWith(`${s}/`));
  const coveredByParent = (path: string) => selected.some((s) => path.startsWith(`${s}/`));
  /** Only some folders inside are chosen. */
  const partial = (path: string) => !covered(path) && selected.some((s) => s.startsWith(`${path}/`));

  function toggle(path: string, on: boolean) {
    // A chosen folder includes everything below it.
    const rest = selected.filter((s) => s !== path && !s.startsWith(`${path}/`));
    selected = on ? [...rest, path].sort() : rest;
  }

  // What would be uploaded, recomputed as folders change.
  let seq = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const folders = [...selected];
    clearTimeout(timer);
    timer = setTimeout(async () => {
      const s = ++seq;
      scanning = true;
      try {
        const result = await syncScan(folders);
        if (s === seq) scan = result;
      } finally {
        if (s === seq) scanning = false;
      }
    }, 250);
  });

  const size = (bytes: number) =>
    bytes >= 1024 ** 3 ? `${(bytes / 1024 ** 3).toFixed(1)} GB`
    : bytes >= 1024 ** 2 ? `${(bytes / 1024 ** 2).toFixed(1)} MB`
    : bytes >= 1024 ? `${Math.round(bytes / 1024)} KB` : `${bytes} B`;
  const folderSize = (path: string) => scan?.per_folder.find(([p]) => p === path)?.[1];
  const BIG_TOTAL = 500 * 1024 ** 2;

  function config(): SyncConfig {
    return { remote: remote.trim(), branch: branch.trim() || "main", folders: selected, include_calendars: includeCalendars, interval_minutes: interval };
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
    e.stopPropagation();
  }

  const folders = (nodes: TreeNode[]) => nodes.filter((n) => n.is_dir);
</script>

{#snippet branchList(nodes: TreeNode[], depth: number)}
  {#each folders(nodes) as n (n.path)}
    {@const sub = folders(n.children)}
    <li>
      <div class="frow" style:padding-left="{depth * 18}px">
        {#if sub.length}
          <button class="twisty" onclick={() => (open[n.path] = !open[n.path])} aria-label={open[n.path] ? "접기" : "펼치기"}>
            <Icon name={open[n.path] ? "chevron-down" : "chevron-right"} size={13} />
          </button>
        {:else}
          <span class="twisty"></span>
        {/if}
        <label>
          <input type="checkbox" checked={covered(n.path)} indeterminate={partial(n.path)} disabled={coveredByParent(n.path)}
            onchange={(e) => toggle(n.path, e.currentTarget.checked)} />
          {n.name}
        </label>
        {#if selected.includes(n.path) && folderSize(n.path) !== undefined}<span class="fsize">{size(folderSize(n.path)!)}</span>{/if}
      </div>
      {#if sub.length && open[n.path]}<ul>{@render branchList(n.children, depth + 1)}</ul>{/if}
    </li>
  {/each}
{/snippet}

<div class="backdrop" role="presentation" onpointerdown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="sync-title" tabindex="-1" bind:this={dialog} {onkeydown}>
    <header>
      <h2 id="sync-title">Git 동기화</h2>
      {#if status}<span class="status">{status}</span>{/if}
      <button class="close" onclick={onclose} aria-label="닫기"><Icon name="close" size={16} /></button>
    </header>

    <div class="body">
      {#if !info.git}
        <p class="warn">
          Git이 설치되어 있지 않습니다. <button class="link" onclick={() => openUrl("https://git-scm.com/download/win")}>Git for Windows</button>를
          설치한 뒤 다시 열어 주세요.
        </p>
      {/if}

      <section>
        <label class="field">원격 저장소 주소
          <input class="remote" bind:value={remote} placeholder="https://github.com/아이디/저장소.git" spellcheck="false" />
        </label>
        <p class="help">GitHub 등에서 <b>비공개(private)</b> 저장소를 만들어 주소를 붙여 넣으세요. 로그인 창은 처음 동기화할 때 Git이 띄웁니다.
          저장소 기록은 vault 밖(앱 데이터 폴더)에 두어 vault에는 아무것도 생기지 않습니다.</p>
        <div class="row">
          <label class="field">브랜치<input bind:value={branch} spellcheck="false" /></label>
          <label class="field">자동 동기화
            <select bind:value={interval}>
              <option value={0}>앱을 열 때와 직접 할 때만</option>
              <option value={5}>5분마다</option>
              <option value={15}>15분마다</option>
              <option value={30}>30분마다</option>
              <option value={60}>1시간마다</option>
            </select>
          </label>
        </div>
      </section>

      <section>
        <h3>동기화할 것</h3>
        <label class="item"><input type="checkbox" checked disabled /> <span><b>설정</b> — <code>.obsidian</code> 폴더(플러그인·테마·CSS 스니펫·단축키·Obsidian 설정)와 Rusidian 설정 <span class="muted">(항상)</span></span></label>
        <label class="item"><input type="checkbox" bind:checked={includeCalendars} /> <span>캘린더 구독 주소 <span class="muted">— 비공개 주소의 토큰이 저장소에 올라갑니다. 저장소가 비공개일 때만 켜세요.</span></span></label>
        <p class="help">노트 폴더는 고른 것만 올라갑니다. 고르지 않은 폴더와 나중에 새로 만드는 폴더는 이 컴퓨터에만 남습니다. 폴더 선택은 이 vault를 쓰는 모든 기기에 함께 적용됩니다.</p>
        {#if folders(tree).length}
          <ul class="tree">{@render branchList(tree, 0)}</ul>
        {:else}
          <p class="muted">vault에 폴더가 없습니다.</p>
        {/if}
      </section>

      <section class="summary" aria-live="polite">
        {#if scan}
          <div class={["total", { big: scan.bytes > BIG_TOTAL }]}>
            올라갈 파일 <b>{scan.count.toLocaleString()}개</b> · {size(scan.bytes)}
            {#if scanning}<span class="muted">계산 중…</span>{/if}
            <button class="link" onclick={() => (showFiles = !showFiles)}>{showFiles ? "목록 숨기기" : "목록 보기"}</button>
          </div>
          {#if scan.bytes > BIG_TOTAL}<p class="warn">500MB가 넘습니다. Git 저장소가 커지면 느려지니 큰 폴더는 빼는 것이 좋습니다.</p>{/if}
          {#if scan.large.length}
            <p class="help">50MB가 넘는 파일 {scan.large.length}개는 빼고 올립니다: {scan.large.slice(0, 5).map(([p, b]) => `${p} (${size(b)})`).join(", ")}{scan.large.length > 5 ? " …" : ""}</p>
          {/if}
          {#if showFiles}
            <ul class="files">
              {#each scan.files as f (f)}<li>{f}</li>{/each}
              {#if scan.count > scan.files.length}<li class="muted">… 외 {scan.count - scan.files.length}개</li>{/if}
            </ul>
          {/if}
        {/if}
      </section>
    </div>

    <footer>
      {#if info.config}
        <button class="danger" onclick={() => onsave(null, false)} title="이 기기에서 동기화를 끕니다 (파일과 원격 저장소는 그대로)">동기화 끄기</button>
      {/if}
      <span class="spacer"></span>
      <button onclick={onclose}>취소</button>
      <button onclick={() => onsave(config(), false)} disabled={!remote.trim()}>저장</button>
      <button class="primary" onclick={() => onsave(config(), true)} disabled={!remote.trim() || !info.git || running}>
        {running ? "동기화 중…" : "저장하고 지금 동기화"}
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
    width: min(680px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
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
  .body {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 6px 20px 14px;
  }
  section {
    padding: 10px 0;
    border-bottom: 1px solid var(--border);
  }
  section:last-child {
    border-bottom: 0;
  }
  h3 {
    margin: 0 0 8px;
    font-size: 13px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    flex: 1;
    font-size: 12px;
    color: var(--text-muted);
  }
  .row {
    display: flex;
    gap: 14px;
    margin-top: 8px;
  }
  input:not([type="checkbox"]),
  select {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    padding: 6px 8px;
    outline: none;
  }
  .remote {
    font-family: var(--font-mono);
  }
  input:focus,
  select:focus {
    border-color: var(--accent);
  }
  input[type="checkbox"] {
    accent-color: var(--accent);
  }
  .help {
    margin: 6px 0 0;
    font-size: 12px;
    line-height: 1.6;
    color: var(--text-faint);
  }
  .help b {
    color: var(--text-muted);
  }
  .item {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    font-size: 13px;
    padding: 3px 0;
  }
  .item input {
    margin-top: 3px;
  }
  code {
    font-family: var(--font-mono);
    background: var(--code-bg);
    padding: 0 3px;
    border-radius: 3px;
  }
  .muted {
    color: var(--text-faint);
  }
  .tree,
  .tree ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .tree {
    margin-top: 8px;
    max-height: 220px;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 6px;
  }
  .frow {
    display: flex;
    align-items: center;
    gap: 2px;
    font-size: 13px;
  }
  .frow label {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 0;
  }
  .twisty {
    flex: none;
    width: 20px;
    height: 20px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 0;
    background: none;
    color: var(--text-faint);
    padding: 0;
    cursor: pointer;
  }
  .fsize {
    margin-left: auto;
    font-size: 11px;
    color: var(--text-faint);
    font-variant-numeric: tabular-nums;
  }
  .summary .total {
    display: flex;
    align-items: baseline;
    gap: 10px;
    font-size: 13px;
  }
  .summary .total.big b {
    color: var(--danger);
  }
  .files {
    list-style: none;
    margin: 8px 0 0;
    padding: 6px 8px;
    max-height: 160px;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: 6px;
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-muted);
  }
  .warn {
    margin: 8px 0 0;
    padding: 8px 10px;
    border-radius: 6px;
    background: var(--danger-bg);
    color: var(--danger);
    font-size: 12px;
  }
  .link {
    border: 0;
    background: none;
    color: var(--accent);
    font: inherit;
    padding: 0;
    cursor: pointer;
  }
  footer {
    display: flex;
    gap: 8px;
    padding: 12px 20px 16px;
    border-top: 1px solid var(--border);
  }
  .spacer {
    flex: 1;
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
