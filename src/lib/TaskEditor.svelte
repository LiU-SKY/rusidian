<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { deleteTask, updateTask, type Priority, type TaskItem } from "./api";
  import Icon from "./Icon.svelte";

  interface Props {
    task: TaskItem;
    onclose: () => void;
    /** After a successful save or delete (reload the list). */
    ondone: (message: string) => void;
    /** Flush unsaved editor changes before writing. */
    beforewrite: () => Promise<void>;
    onopen: (path: string) => void;
  }

  let { task, onclose, ondone, beforewrite, onopen }: Props = $props();

  // The dialog edits a copy taken when it opens; nothing is written until 저장.
  const initial = untrack(() => task);
  let text = $state(initial.text);
  let priority = $state<Priority>(initial.priority);
  let due = $state(initial.due ?? "");
  let scheduled = $state(initial.scheduled ?? "");
  let start = $state(initial.start ?? "");
  let recurrence = $state(initial.recurrence ?? "");
  let time = $state(initial.time ?? "");
  let endTime = $state(initial.end_time ?? "");
  let reminder = $state(initial.reminder?.replace(" ", "T") ?? "");
  let error = $state<string | null>(null);
  let busy = $state(false);
  let first: HTMLInputElement | undefined = $state();

  const PRIORITIES: { value: Priority; label: string }[] = [
    { value: "highest", label: "최우선 🔺" },
    { value: "high", label: "높음 ⏫" },
    { value: "medium", label: "보통 🔼" },
    { value: "normal", label: "없음" },
    { value: "low", label: "낮음 🔽" },
    { value: "lowest", label: "최하 ⏬" },
  ];
  const RULES = [
    "every day", "every weekday", "every week", "every week on Monday", "every 2 weeks", "every month",
    "every month on the 1st", "every month on the last", "every month on the 2nd Tuesday", "every year",
    "every 3 days when done",
  ];

  onMount(() => {
    const before = document.activeElement as HTMLElement | null;
    first?.focus();
    return () => {
      if (before?.isConnected) before.focus();
    };
  });

  const orNull = (s: string) => (s.trim() ? s.trim() : null);

  async function save(e: SubmitEvent) {
    e.preventDefault();
    if (busy) return;
    busy = true;
    error = null;
    try {
      await beforewrite();
      await updateTask(task, {
        text,
        priority,
        due: orNull(due),
        scheduled: orNull(scheduled),
        start: orNull(start),
        recurrence: orNull(recurrence),
        time: orNull(time),
        end_time: orNull(time) ? orNull(endTime) : null,
        reminder: orNull(reminder)?.replace("T", " ") ?? null,
      });
      ondone("할일을 고쳤습니다.");
      onclose();
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }

  async function remove() {
    const ok = await ask(`"${task.text}" 할일을 노트에서 지울까요?`, {
      title: "할일 삭제", kind: "warning", okLabel: "삭제", cancelLabel: "취소",
    });
    if (!ok) return;
    busy = true;
    try {
      await beforewrite();
      await deleteTask(task);
      ondone("할일을 지웠습니다.");
      onclose();
    } catch (err) {
      error = String(err);
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
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="task-edit-title" tabindex="-1" {onkeydown}>
  <form onsubmit={save}>
    <header>
      <h2 id="task-edit-title">할일 편집</h2>
      <button type="button" class="link" onclick={() => { onclose(); onopen(task.path); }} title={task.path}>{task.title} 열기</button>
      <button type="button" class="close" onclick={onclose} aria-label="닫기"><Icon name="close" size={16} /></button>
    </header>

    <div class="body">
      <label class="wide">내용
        <input bind:this={first} bind:value={text} required spellcheck="false" />
      </label>

      <label>우선순위
        <select bind:value={priority}>
          {#each PRIORITIES as p (p.value)}<option value={p.value}>{p.label}</option>{/each}
        </select>
      </label>
      <label>반복
        <input bind:value={recurrence} list="task-rules" placeholder="없음" spellcheck="false" />
        <datalist id="task-rules">{#each RULES as r (r)}<option value={r}></option>{/each}</datalist>
      </label>

      <label>마감 📅<input type="date" bind:value={due} /></label>
      <label>예정 ⏳<input type="date" bind:value={scheduled} /></label>
      <label>시작 🛫<input type="date" bind:value={start} /></label>
      <label>알림 ⏰<input type="datetime-local" bind:value={reminder} /></label>

      <label>시작 시간<input type="time" bind:value={time} /></label>
      <label>끝 시간<input type="time" bind:value={endTime} disabled={!time} /></label>

      {#if task.created || task.done || task.cancelled}
        <p class="wide kept">
          {#if task.created}만든 날 {task.created}{/if}
          {#if task.done} · 완료 {task.done}{/if}
          {#if task.cancelled} · 취소 {task.cancelled}{/if}
          (그대로 둡니다)
        </p>
      {/if}
      {#if error}<p class="wide error" role="alert">{error}</p>{/if}
    </div>

    <footer>
      <button type="button" class="danger" onclick={remove} disabled={busy}><Icon name="trash" size={15} /> 삭제</button>
      <span class="spacer"></span>
      <button type="button" onclick={onclose}>취소</button>
      <button type="submit" class="primary" disabled={busy || !text.trim()}>저장</button>
    </footer>
  </form>
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
    width: min(560px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    overflow: auto;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: 0 16px 48px rgb(0 0 0 / 0.25);
  }
  header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 14px 14px 10px 20px;
    border-bottom: 1px solid var(--border);
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: 16px;
  }
  .link {
    border: 0;
    background: none;
    color: var(--text-muted);
    font: inherit;
    font-size: 12px;
    text-decoration: underline;
    cursor: pointer;
    max-width: 200px;
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
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px 16px;
    padding: 16px 20px;
  }
  .wide {
    grid-column: 1 / -1;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
    color: var(--text-muted);
  }
  input,
  select {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 14px;
    padding: 6px 8px;
    outline: none;
    color-scheme: inherit;
  }
  input:focus,
  select:focus {
    border-color: var(--accent);
  }
  input:disabled {
    opacity: 0.5;
  }
  .kept {
    margin: 0;
    font-size: 12px;
    color: var(--text-faint);
  }
  .error {
    margin: 0;
    padding: 8px 10px;
    border-radius: 6px;
    background: var(--danger-bg);
    color: var(--danger);
    font-size: 13px;
  }
  footer {
    display: flex;
    gap: 8px;
    padding: 12px 20px 16px;
  }
  .spacer {
    flex: 1;
  }
  footer button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    padding: 6px 14px;
    cursor: pointer;
  }
  footer button:hover:not(:disabled) {
    background: var(--hover);
  }
  footer .primary {
    border-color: var(--accent);
    background: var(--accent);
    color: #fff;
  }
  footer .primary:hover:not(:disabled) {
    background: var(--accent);
    filter: brightness(1.08);
  }
  footer .danger {
    color: var(--danger);
  }
  footer button:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
