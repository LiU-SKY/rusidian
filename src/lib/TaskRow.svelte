<script lang="ts">
  import type { Priority, TaskItem } from "./api";
  import { dayLabel } from "./dates";
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";

  interface Props {
    task: TaskItem;
    today: string;
    busy?: boolean;
    /** Show the due/scheduled date chip (off when the context already implies the day). */
    showDate?: boolean;
    ontoggle: (t: TaskItem) => void;
    onopen: (path: string) => void;
    /** Open the edit dialog (button shown on hover). */
    onedit?: (t: TaskItem) => void;
  }

  let { task: t, today, busy = false, showDate = true, ontoggle, onopen, onedit }: Props = $props();

  // Tasks plugin priorities (🔺⏫🔼🔽⏬) drawn with our own icons.
  const PRIORITY: Record<Priority, { icon: IconName; label: string } | null> = {
    highest: { icon: "arrow-up-to-line", label: "최우선" },
    high: { icon: "chevrons-up", label: "높음" },
    medium: { icon: "chevron-up", label: "보통" },
    normal: null,
    low: { icon: "chevron-down", label: "낮음" },
    lowest: { icon: "chevrons-down", label: "최하" },
  };

  const open = $derived(t.kind === "todo" || t.kind === "in_progress");
  const date = $derived(t.due ?? t.scheduled ?? null);
</script>

<li class={["task", t.kind]}>
  <input type="checkbox" checked={t.kind === "done"} disabled={busy}
    onchange={(e) => { (e.currentTarget as HTMLInputElement).checked = t.kind === "done"; ontoggle(t); }}
    aria-label={open ? "완료" : "다시 열기"} />
  <span class="body">
    <span class="text">
      {#if PRIORITY[t.priority]}
        <span class={["prio", t.priority]} title="우선순위: {PRIORITY[t.priority]!.label}">
          <Icon name={PRIORITY[t.priority]!.icon} size={14} label="우선순위 {PRIORITY[t.priority]!.label}" />
        </span>
      {/if}
      {#if t.kind === "in_progress"}<span class="badge">진행 중</span>{/if}
      {t.text}
    </span>
    <span class="meta">
      {#if t.time}<span class="date time" title="시간"><Icon name="clock" size={13} label="시간" /> {t.time}{t.end_time ? `–${t.end_time}` : ""}</span>{/if}
      {#if t.reminder && open}<span class="date" title="알림 {t.reminder}"><Icon name="bell" size={13} label="알림" /> {t.reminder.slice(5).replace("-", "/")}</span>{/if}
      {#if showDate && date && open}
        <span class={["date", { overdue: date < today, today: date === today }]}
          title={t.due ? `마감 ${t.due}` : `예정 ${t.scheduled}`}>
          <Icon name={t.due ? "calendar" : "hourglass"} size={13} label={t.due ? "마감" : "예정"} /> {dayLabel(date, today)}
        </span>
      {/if}
      {#if t.done}<span class="date"><Icon name="check-circle" size={13} label="완료" /> {t.done}</span>{/if}
      {#if t.recurrence}<span class="recur" title={t.recurrence}><Icon name="repeat" size={13} label="반복" /> {t.recurrence}</span>{/if}
      <button class="source" onclick={() => onopen(t.path)} title={t.path}>{t.title}</button>
    </span>
  </span>
  {#if onedit}
    <button class="edit" onclick={() => onedit(t)} title="편집" aria-label="할일 편집"><Icon name="pencil" size={14} /></button>
  {/if}
</li>

<style>
  .task {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 7px 8px;
    border-radius: 6px;
    list-style: none;
  }
  .task:hover {
    background: var(--hover);
  }
  input {
    margin-top: 3px;
    accent-color: var(--accent);
    cursor: pointer;
  }
  .body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .text {
    overflow-wrap: anywhere;
  }
  .task.done .text,
  .task.cancelled .text {
    color: var(--text-faint);
    text-decoration: line-through;
  }
  .prio {
    display: inline-flex;
    vertical-align: -0.1em;
    margin-right: 3px;
    color: var(--text-faint);
  }
  .prio.highest,
  .prio.high {
    color: var(--danger);
  }
  .prio.medium {
    color: var(--accent);
  }
  .date,
  .recur {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .badge {
    font-size: 11px;
    color: var(--accent);
    background: var(--accent-bg);
    border-radius: 8px;
    padding: 0 6px;
    margin-right: 4px;
  }
  .meta {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    font-size: 12px;
    color: var(--text-faint);
  }
  .date.overdue {
    color: var(--danger);
  }
  .date.today {
    color: var(--accent);
  }
  .source {
    border: 0;
    background: none;
    padding: 0;
    font: inherit;
    color: var(--text-muted);
    text-decoration: underline;
    text-decoration-color: var(--border);
    cursor: pointer;
    margin-left: auto;
  }
  .source:hover {
    color: var(--accent);
  }
  .edit {
    display: inline-flex;
    border: 0;
    background: none;
    color: var(--text-faint);
    padding: 3px;
    border-radius: 4px;
    cursor: pointer;
    opacity: 0;
  }
  .task:hover .edit,
  .edit:focus-visible {
    opacity: 1;
  }
  .edit:hover {
    color: var(--text);
    background: var(--active);
  }
  .time {
    color: var(--accent);
  }
</style>
