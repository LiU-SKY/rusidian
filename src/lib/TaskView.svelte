<script lang="ts">
  import { addTask, listTasks, type Priority, type TaskItem } from "./api";
  import { localDate, localTime } from "./dates";
  import Icon from "./Icon.svelte";
  import TaskEditor from "./TaskEditor.svelte";
  import TaskRow from "./TaskRow.svelte";
  import { createTaskToggle, taskKey } from "./taskToggle.svelte";

  interface Props {
    /** Bumped when vault files change; reloads the list. */
    version: number;
    onopen: (path: string) => void;
    /** Called before any file write so unsaved editor changes are flushed first. */
    beforewrite: () => Promise<void>;
  }

  let { version, onopen, beforewrite }: Props = $props();

  let tasks = $state<TaskItem[]>([]);
  let loaded = $state(false);
  let showDone = $state(false);
  let filter = $state("");
  let newText = $state("");
  let newDue = $state("");
  let message = $state<{ kind: "ok" | "error"; text: string } | null>(null);
  let editing = $state<TaskItem | null>(null);
  let seq = 0;

  const PRIORITY_RANK: Record<Priority, number> = {
    highest: 0, high: 1, medium: 2, normal: 3, low: 4, lowest: 5,
  };

  async function load() {
    const s = ++seq;
    try {
      const result = await listTasks(showDone);
      if (s === seq) {
        tasks = result;
        loaded = true;
      }
    } catch (e) {
      if (s === seq) message = { kind: "error", text: String(e) };
    }
  }

  $effect(() => {
    version;
    showDone;
    load();
  });

  const toggler = createTaskToggle({
    beforewrite: () => beforewrite(),
    reload: load,
    onerror: (text) => (message = { kind: "error", text }),
  });

  async function add(e: SubmitEvent) {
    e.preventDefault();
    if (!newText.trim()) return;
    try {
      await beforewrite();
      const path = await addTask(newText, newDue || null, localDate(), localTime());
      message = { kind: "ok", text: `"${path}"에 추가했습니다.` };
      newText = "";
      newDue = "";
      await load();
    } catch (err) {
      message = { kind: "error", text: String(err) };
    }
  }

  // ----------------------------------------------------------- grouping
  const today = $derived.by(() => {
    version; // re-evaluate on refresh so the date rolls over at midnight
    return localDate();
  });
  const inAWeek = $derived.by(() => {
    version;
    return localDate(7);
  });

  const isOpen = (t: TaskItem) => t.kind === "todo" || t.kind === "in_progress";
  const when = (t: TaskItem) => t.due ?? t.scheduled ?? null;

  function matches(t: TaskItem, q: string): boolean {
    if (!q) return true;
    const hay = `${t.text} ${t.title} ${t.path}`.toLowerCase();
    return q.toLowerCase().split(/\s+/).every((w) => hay.includes(w));
  }

  function byDateThenPriority(a: TaskItem, b: TaskItem): number {
    const wa = when(a) ?? "9999";
    const wb = when(b) ?? "9999";
    return wa.localeCompare(wb)
      || (a.time ?? "99").localeCompare(b.time ?? "99")
      || PRIORITY_RANK[a.priority] - PRIORITY_RANK[b.priority]
      || a.path.localeCompare(b.path)
      || a.line - b.line;
  }

  interface Group { id: string; label: string; items: TaskItem[] }

  const groups = $derived.by((): Group[] => {
    const q = filter.trim();
    const visible = tasks.filter((t) => matches(t, q));
    const open = visible.filter(isOpen).sort(byDateThenPriority);
    const g: Group[] = [
      { id: "overdue", label: "지난 기한", items: open.filter((t) => when(t) !== null && when(t)! < today) },
      { id: "today", label: "오늘", items: open.filter((t) => when(t) === today) },
      { id: "week", label: "7일 이내", items: open.filter((t) => when(t) !== null && when(t)! > today && when(t)! <= inAWeek) },
      { id: "later", label: "나중", items: open.filter((t) => when(t) !== null && when(t)! > inAWeek) },
      { id: "nodate", label: "날짜 없음", items: open.filter((t) => when(t) === null) },
    ];
    if (showDone) {
      const closed = visible
        .filter((t) => !isOpen(t))
        .sort((a, b) => (b.done ?? b.cancelled ?? "").localeCompare(a.done ?? a.cancelled ?? ""));
      g.push({ id: "done", label: "완료 · 취소", items: closed });
    }
    return g.filter((x) => x.items.length > 0);
  });

  const openCount = $derived(tasks.filter(isOpen).length);
  const overdueCount = $derived(groups.find((g) => g.id === "overdue")?.items.length ?? 0);
</script>

<section class="tasks view-in" aria-label="할일">
  <header class="head">
    <h1>할일</h1>
    {#if loaded}
      <span class="summary">
        미완료 {openCount}{#if overdueCount}<span class="overdue-count">{` · 지난 기한 ${overdueCount}`}</span>{/if}
      </span>
    {/if}
  </header>

  <form class="add" onsubmit={add}>
    <input class="add-text" bind:value={newText} placeholder="오늘 데일리 노트에 할일 추가…" aria-label="새 할일" />
    <input class="add-due" type="date" bind:value={newDue} aria-label="마감일" title="마감일 (선택)" />
    <button type="submit" disabled={!newText.trim()}>추가</button>
  </form>

  <div class="toolbar">
    <input class="filter" type="search" bind:value={filter} placeholder="필터 (텍스트, #태그, 노트 이름)" aria-label="할일 필터" />
    <label class="toggle-done"><input type="checkbox" bind:checked={showDone} /> 완료 표시</label>
  </div>

  {#if message}
    <p class={["message", message.kind]} role={message.kind === "error" ? "alert" : "status"}>
      {message.text}
      <button class="dismiss" onclick={() => (message = null)} aria-label="닫기"><Icon name="close" size={14} /></button>
    </p>
  {/if}

  <div class="list">
    {#if loaded && groups.length === 0}
      <p class="empty">
        {#if !filter}<Icon name="check-circle" size={28} />{/if}
        <span>{filter ? "조건에 맞는 할일이 없습니다." : "남은 할일이 없습니다."}</span>
      </p>
    {/if}

    {#each groups as group (group.id)}
      <h2 class={["group", group.id]}>{group.label} <span class="n">{group.items.length}</span></h2>
      <ul>
        {#each group.items as t (taskKey(t))}
          <TaskRow task={t} {today} busy={toggler.isBusy(t)} ontoggle={toggler.toggle} {onopen} onedit={(x) => (editing = x)} />
        {/each}
      </ul>
    {/each}
  </div>
</section>

{#if editing}
  <TaskEditor task={editing} onclose={() => (editing = null)} {beforewrite} {onopen}
    ondone={(text) => { message = { kind: "ok", text }; load(); }} />
{/if}

<style>
  .tasks {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 20px max(24px, calc((100% - 800px) / 2)) 0;
  }
  .head {
    display: flex;
    align-items: baseline;
    gap: 12px;
  }
  h1 {
    margin: 0;
    font-size: 22px;
  }
  .summary {
    color: var(--text-muted);
    font-size: 13px;
  }
  .overdue-count {
    color: var(--danger);
  }
  .add,
  .toolbar {
    display: flex;
    gap: 8px;
    margin-top: 14px;
  }
  .toolbar {
    margin-top: 8px;
    align-items: center;
  }
  input:not([type="checkbox"]) {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 14px;
    padding: 7px 10px;
    outline: none;
    min-width: 0;
  }
  input:focus {
    border-color: var(--accent);
  }
  .add-text,
  .filter {
    flex: 1;
  }
  .add-due {
    color-scheme: light dark;
  }
  .add button {
    border: 0;
    border-radius: 6px;
    background: var(--accent);
    color: #fff;
    font: inherit;
    padding: 0 16px;
    cursor: pointer;
  }
  .add button:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .toggle-done {
    font-size: 13px;
    color: var(--text-muted);
    display: flex;
    align-items: center;
    gap: 4px;
    white-space: nowrap;
  }
  .message {
    display: flex;
    gap: 8px;
    align-items: center;
    margin: 10px 0 0;
    padding: 8px 12px;
    border-radius: 6px;
    font-size: 13px;
  }
  .message.ok {
    background: var(--notice-bg);
    color: var(--notice);
  }
  .message.error {
    background: var(--danger-bg);
    color: var(--danger);
  }
  .dismiss {
    display: inline-flex;
    margin-left: auto;
    border: 0;
    background: none;
    color: inherit;
    cursor: pointer;
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow: auto;
    margin-top: 12px;
    padding-bottom: 40px;
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    color: var(--text-faint);
    margin-top: 48px;
  }
  .group {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-muted);
    margin: 18px 0 6px;
  }
  .group.overdue {
    color: var(--danger);
  }
  .group.today {
    color: var(--accent);
  }
  .n {
    font-weight: 400;
    color: var(--text-faint);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
</style>
