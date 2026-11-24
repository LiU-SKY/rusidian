<script lang="ts">
  import {
    addTask, calendarRange, externalEvents, listCalendars, openDailyNote, type CalendarData, type CalendarSource,
    type TaskItem,
  } from "./api";
  import CalendarsDialog from "./CalendarsDialog.svelte";
  import { WEEKDAYS, addDays, localDate, localTime, longLabel, parseYmd, ymd } from "./dates";
  import { onDay, timeLabel, toLocal, type LocalEvent } from "./externalEvents";
  import Icon from "./Icon.svelte";
  import { slider } from "./slider";
  import type { IconName } from "./icons";
  import TaskEditor from "./TaskEditor.svelte";
  import TaskRow from "./TaskRow.svelte";
  import { createTaskToggle, taskKey } from "./taskToggle.svelte";
  import { tick, untrack } from "svelte";

  interface Props {
    /** Bumped when vault files change; reloads the month. */
    version: number;
    onopen: (path: string) => void;
    /** Called before any file write so unsaved editor changes are flushed first. */
    beforewrite: () => Promise<void>;
  }

  let { version, onopen, beforewrite }: Props = $props();

  const MAX_CHIPS = 3;

  const today = $derived.by(() => {
    version; // re-evaluate on refresh so "today" rolls over at midnight
    return localDate();
  });
  let selected = $state(localDate());
  /** Shown month as "YYYY-MM". */
  let month = $state(localDate().slice(0, 7));
  let data = $state<CalendarData | null>(null);
  let message = $state<{ kind: "ok" | "error"; text: string } | null>(null);
  let newText = $state("");
  let editing = $state<TaskItem | null>(null);
  let addInput: HTMLInputElement | undefined = $state();
  let seq = 0;

  type Mode = "month" | "week" | "day";
  const MODES: { id: Mode; label: string }[] = [
    { id: "month", label: "월" },
    { id: "week", label: "주" },
    { id: "day", label: "일" },
  ];
  let mode = $state<Mode>("month");

  // 6 weeks starting on the Sunday on/before the 1st.
  const gridStart = $derived.by(() => {
    const first = parseYmd(`${month}-01`);
    first.setDate(1 - first.getDay());
    return ymd(first);
  });
  const gridDays = $derived(Array.from({ length: 42 }, (_, i) => addDays(gridStart, i)));
  /** Sunday of the selected day's week. */
  const weekStart = $derived(addDays(selected, -parseYmd(selected).getDay()));
  /** Columns of the time grid. */
  const columns = $derived(
    mode === "week" ? Array.from({ length: 7 }, (_, i) => addDays(weekStart, i)) : [selected],
  );
  const range = $derived(
    mode === "month"
      ? { start: gridStart, end: addDays(gridStart, 41) }
      : { start: columns[0], end: columns[columns.length - 1] },
  );

  async function load() {
    const s = ++seq;
    const { start, end } = range;
    try {
      const result = await calendarRange(start, end);
      if (s === seq) data = result;
    } catch (e) {
      if (s === seq) message = { kind: "error", text: String(e) };
    }
  }

  $effect(() => {
    version;
    range;
    load();
  });

  // ---------------------------------------------------- external calendars
  const EXTERNAL_REFRESH_MS = 15 * 60_000;
  let sources = $state<CalendarSource[]>([]);
  let external = $state<LocalEvent[]>([]);
  let extErrors = $state<{ name: string; message: string }[]>([]);
  let extLoading = $state(false);
  let showCalendars = $state(false);
  /** External event whose details are open. */
  let viewing = $state<LocalEvent | null>(null);
  let extSeq = 0;

  async function loadExternal(force = false) {
    const s = ++extSeq;
    const { start, end } = range;
    try {
      const list = await listCalendars();
      if (s !== extSeq) return;
      sources = list;
      if (!list.some((c) => c.enabled)) {
        external = [];
        extErrors = [];
        return;
      }
      extLoading = true;
      const res = await externalEvents(start, end, force);
      if (s !== extSeq) return;
      const byId = new Map(list.map((c) => [c.id, c]));
      external = res.events.map((e, i) => toLocal(e, byId.get(e.calendar), i));
      extErrors = res.errors;
    } catch (e) {
      if (s === extSeq) extErrors = [{ name: "외부 캘린더", message: String(e) }];
    } finally {
      if (s === extSeq) extLoading = false;
    }
  }

  // Vault changes don't touch external calendars: only the range does.
  $effect(() => {
    range;
    untrack(() => loadExternal());
  });
  $effect(() => {
    const id = setInterval(() => loadExternal(), EXTERNAL_REFRESH_MS);
    return () => clearInterval(id);
  });

  const enabledSources = $derived(sources.filter((c) => c.enabled));
  const byEventTime = (a: LocalEvent, b: LocalEvent) =>
    Number(b.allDay) - Number(a.allDay) || (a.startTime ?? "").localeCompare(b.startTime ?? "");
  const eventsOn = (date: string) => external.filter((e) => onDay(e, date)).sort(byEventTime);

  const toggler = createTaskToggle({
    beforewrite: () => beforewrite(),
    reload: load,
    onerror: (text) => (message = { kind: "error", text }),
  });

  interface DayTasks { due: TaskItem[]; scheduled: TaskItem[]; start: TaskItem[]; done: TaskItem[] }

  const notes = $derived(new Map((data?.days ?? []).map((d) => [d.date, d.note])));
  const byDate = $derived.by(() => {
    const map = new Map<string, DayTasks>();
    const at = (date: string) => {
      let e = map.get(date);
      if (!e) map.set(date, (e = { due: [], scheduled: [], start: [], done: [] }));
      return e;
    };
    for (const t of data?.tasks ?? []) {
      if (t.due) at(t.due).due.push(t);
      if (t.scheduled) at(t.scheduled).scheduled.push(t);
      if (t.start) at(t.start).start.push(t);
      if (t.done) at(t.done).done.push(t);
    }
    return map;
  });

  const isOpen = (t: TaskItem) => t.kind === "todo" || t.kind === "in_progress";
  /** Timed tasks without a date of their own, by their daily note's day. */
  const dailyByDate = $derived.by(() => {
    const map = new Map<string, TaskItem[]>();
    for (const t of data?.daily_tasks ?? []) map.set(t.date, [...(map.get(t.date) ?? []), t]);
    return map;
  });
  const byTime = (a: TaskItem, b: TaskItem) => (a.time ?? "").localeCompare(b.time ?? "");

  /** Open tasks of a day: due that day, scheduled without a due date, or timed in its daily note. */
  function cellTasks(date: string): TaskItem[] {
    const e = byDate.get(date);
    const own = e ? [...e.due, ...e.scheduled.filter((t) => !t.due)] : [];
    return [...own, ...(dailyByDate.get(date) ?? [])].filter(isOpen).sort(byTime);
  }

  /** Tasks with a time on `date` (any status), for the time grid. */
  function timedTasks(date: string): TaskItem[] {
    const e = byDate.get(date);
    const own = e ? [...e.due, ...e.scheduled.filter((t) => !t.due), ...e.start.filter((t) => !t.due && !t.scheduled)] : [];
    return [...own, ...(dailyByDate.get(date) ?? [])].filter((t) => t.time);
  }

  // ------------------------------------------------------------ time grid
  const HOUR_PX = 48;
  const DEFAULT_MINUTES = 30;
  const minutes = (hhmm: string) => {
    const [h, m] = hhmm.split(":").map(Number);
    return h * 60 + m;
  };

  /** A block in the time grid: a timed task or an external event. */
  type GridItem = { kind: "task"; t: TaskItem; key: string } | { kind: "event"; e: LocalEvent; key: string };
  interface Placed { item: GridItem; start: number; end: number; lane: number; lanes: number }

  /** Timed tasks and timed external events of `date`, with minutes. */
  function gridItems(date: string): Omit<Placed, "lane" | "lanes">[] {
    const tasks = timedTasks(date).map((t) => {
      const start = minutes(t.time!);
      const end = t.end_time && minutes(t.end_time) > start ? minutes(t.end_time) : start + DEFAULT_MINUTES;
      return { item: { kind: "task" as const, t, key: taskKey(t) }, start, end };
    });
    const events = eventsOn(date)
      .filter((e) => !e.allDay && e.startTime)
      .map((e) => {
        const start = minutes(e.startTime!);
        const endMin = e.endTime ? minutes(e.endTime) : start + DEFAULT_MINUTES;
        // Ending at midnight means the end of this day.
        const end = endMin > start ? endMin : e.endTime === "00:00" ? 24 * 60 : start + DEFAULT_MINUTES;
        return { item: { kind: "event" as const, e, key: e.key }, start, end };
      });
    return [...tasks, ...events];
  }

  /** Side-by-side lanes for overlapping blocks (per cluster of overlaps). */
  function layout(blocks: Omit<Placed, "lane" | "lanes">[]): Placed[] {
    const items: Placed[] = blocks
      .map((b) => ({ ...b, lane: 0, lanes: 1 }))
      .sort((a, b) => a.start - b.start || b.end - a.end);
    let cluster: Placed[] = [];
    let laneEnds: number[] = [];
    const flush = () => {
      for (const p of cluster) p.lanes = laneEnds.length;
      cluster = [];
      laneEnds = [];
    };
    for (const it of items) {
      if (cluster.length && it.start >= Math.max(...laneEnds)) flush();
      let lane = laneEnds.findIndex((end) => end <= it.start);
      if (lane < 0) lane = laneEnds.push(0) - 1;
      laneEnds[lane] = it.end;
      it.lane = lane;
      cluster.push(it);
    }
    flush();
    return items;
  }

  let gridScroller: HTMLDivElement | undefined = $state();
  let nowMinutes = $state(0);
  $effect(() => {
    const tickNow = () => {
      const d = new Date();
      nowMinutes = d.getHours() * 60 + d.getMinutes();
    };
    tickNow();
    const id = setInterval(tickNow, 60_000);
    return () => clearInterval(id);
  });
  // Start the time grid at 7:00 when it appears.
  $effect(() => {
    if (mode !== "month" && gridScroller) gridScroller.scrollTop = 7 * HOUR_PX;
  });

  /** Empty slot clicked: pick that day and start a task at that hour. */
  async function addAt(date: string, hour: number) {
    select(date);
    newText = `${String(hour).padStart(2, "0")}:00 `;
    await tick();
    addInput?.focus();
    addInput?.setSelectionRange(newText.length, newText.length);
  }

  const monthLabel = $derived.by(() => {
    const [y, m] = month.split("-").map(Number);
    return `${y}년 ${m}월`;
  });
  const title = $derived.by(() => {
    if (mode === "month") return monthLabel;
    if (mode === "day") return longLabel(selected);
    const [a, b] = [parseYmd(columns[0]), parseYmd(columns[6])];
    return a.getMonth() === b.getMonth()
      ? `${a.getMonth() + 1}월 ${a.getDate()}일 – ${b.getDate()}일`
      : `${a.getMonth() + 1}월 ${a.getDate()}일 – ${b.getMonth() + 1}월 ${b.getDate()}일`;
  });

  function shiftMonth(delta: number) {
    const d = parseYmd(`${month}-01`);
    d.setMonth(d.getMonth() + delta);
    month = ymd(d).slice(0, 7);
  }

  function shift(delta: number) {
    if (mode === "month") shiftMonth(delta);
    else select(addDays(selected, delta * (mode === "week" ? 7 : 1)));
  }

  function select(date: string) {
    selected = date;
    if (date.slice(0, 7) !== month) month = date.slice(0, 7);
    message = null;
  }

  function goToday() {
    select(today);
  }

  async function openNote(date: string) {
    try {
      onopen(await openDailyNote(date, localTime()));
    } catch (e) {
      message = { kind: "error", text: String(e) };
    }
  }

  async function add(e: SubmitEvent) {
    e.preventDefault();
    if (!newText.trim()) return;
    try {
      await beforewrite();
      const path = await addTask(newText, selected, localDate(), localTime());
      message = { kind: "ok", text: `"${path}"에 추가했습니다. (마감 ${selected})` };
      newText = "";
      await load();
    } catch (err) {
      message = { kind: "error", text: String(err) };
    }
  }

  const day = $derived(byDate.get(selected) ?? { due: [], scheduled: [], start: [], done: [] });
  const sections = $derived(([
    { id: "planned", icon: "clock", label: "데일리 노트 시간 계획", items: [...(dailyByDate.get(selected) ?? [])].sort(byTime) },
    { id: "due", icon: "calendar", label: "마감", items: day.due },
    { id: "scheduled", icon: "hourglass", label: "예정", items: day.scheduled },
    { id: "start", icon: "flag", label: "시작", items: day.start },
    { id: "done", icon: "check-circle", label: "완료한 일", items: day.done },
  ] satisfies { id: string; icon: IconName; label: string; items: TaskItem[] }[]).filter((s) => s.items.length > 0));
  const selectedNote = $derived(notes.get(selected) ?? null);
</script>

<section class="calendar view-in" aria-label="캘린더">
  <header class="head">
    <h1>{title}</h1>
    <div class="modes" role="radiogroup" aria-label="보기" use:slider={{ selector: ".on", color: "var(--bg)", shadow: "0 1px 2px rgb(0 0 0 / 0.15)" }}>
      {#each MODES as m (m.id)}
        <button role="radio" aria-checked={mode === m.id} class:on={mode === m.id} onclick={() => (mode = m.id)}>{m.label}</button>
      {/each}
    </div>
    <div class="nav">
      <button onclick={() => shift(-1)} aria-label="이전"><Icon name="chevron-left" /></button>
      <button class="today-btn" onclick={goToday}>오늘</button>
      <button onclick={() => shift(1)} aria-label="다음"><Icon name="chevron-right" /></button>
      <button onclick={() => (showCalendars = true)} title="Google 캘린더 등 ICS 주소 구독">구독</button>
      {#if enabledSources.length}
        <button onclick={() => loadExternal(true)} disabled={extLoading} aria-label="외부 캘린더 새로 받기"
          title="외부 캘린더 새로 받기"><Icon name="refresh" /></button>
      {/if}
    </div>
  </header>

  {#if enabledSources.length}
    <ul class="sources" aria-label="구독한 캘린더">
      {#each enabledSources as c (c.id)}
        <li><span class="dot" style:background={c.color}></span>{c.name}</li>
      {/each}
      {#if extLoading}<li class="loading">받는 중…</li>{/if}
    </ul>
  {/if}
  {#each extErrors as err (err.name)}
    <p class="message error">"{err.name}" 캘린더: {err.message}</p>
  {/each}

  {#if data?.daily_error}
    <p class="message error">{data.daily_error} 데일리 노트 표시가 꺼졌습니다.</p>
  {/if}

  <div class="scroll">
    {#if mode !== "month"}
      <div class="timegrid" style:--cols={columns.length}>
        <div class="tg-head">
          <span class="tg-gutter"></span>
          {#each columns as date (date)}
            {@const d = parseYmd(date)}
            <button class={["tg-day", { today: date === today, selected: date === selected, sun: d.getDay() === 0, sat: d.getDay() === 6 }]}
              onclick={() => select(date)} ondblclick={() => openNote(date)} title="더블클릭으로 데일리 노트 열기">
              <span class="tg-wd">{WEEKDAYS[d.getDay()]}</span>
              <span class="tg-num">{d.getDate()}</span>
            </button>
          {/each}
        </div>
        <div class="tg-allday">
          <span class="tg-gutter">종일</span>
          {#each columns as date (date)}
            <div class="tg-allday-cell">
              {#each eventsOn(date).filter((e) => e.allDay) as e (e.key)}
                <button class="chip ext" style:--cal={e.color} onclick={() => (viewing = e)}
                  title="{timeLabel(e)} {e.title} ({e.calendarName})">{e.title}</button>
              {/each}
              {#each cellTasks(date).filter((t) => !t.time) as t (taskKey(t))}
                <button class={["chip", { overdue: date < today }]} onclick={() => (editing = t)} title={t.text}>{t.text}</button>
              {/each}
            </div>
          {/each}
        </div>
        <div class="tg-scroll" bind:this={gridScroller}>
          <div class="tg-body" style:height="{24 * HOUR_PX}px">
            <div class="tg-gutter tg-hours">
              {#each Array.from({ length: 24 }, (_, h) => h) as h (h)}
                <span style:top="{h * HOUR_PX}px">{h === 0 ? "" : `${h}:00`}</span>
              {/each}
            </div>
            {#each columns as date (date)}
              <div class="tg-col">
                {#each Array.from({ length: 24 }, (_, h) => h) as h (h)}
                  <button class="tg-slot" style:top="{h * HOUR_PX}px" style:height="{HOUR_PX}px"
                    onclick={() => addAt(date, h)} aria-label="{longLabel(date)} {h}시에 할일 추가" tabindex="-1"></button>
                {/each}
                {#each layout(gridItems(date)) as p (p.item.key)}
                  {#if p.item.kind === "task"}
                    {@const t = p.item.t}
                    <div class={["block", t.kind]}
                      style:top="{(p.start / 60) * HOUR_PX}px"
                      style:height="{Math.max(((p.end - p.start) / 60) * HOUR_PX, 20)}px"
                      style:left="calc({(p.lane / p.lanes) * 100}% + 2px)"
                      style:width="calc({100 / p.lanes}% - 4px)">
                      <input type="checkbox" checked={t.kind === "done"} disabled={toggler.isBusy(t)}
                        onchange={(e) => { e.currentTarget.checked = t.kind === "done"; toggler.toggle(t); }}
                        aria-label={isOpen(t) ? "완료" : "다시 열기"} />
                      <button class="block-body" onclick={() => (editing = t)} title="{t.time}{t.end_time ? `–${t.end_time}` : ""} {t.text}">
                        <span class="block-time">{t.time}{t.end_time ? `–${t.end_time}` : ""}</span>
                        <span class="block-text">{t.text}</span>
                      </button>
                    </div>
                  {:else}
                    {@const ev = p.item.e}
                    <button class="block event" style:--cal={ev.color}
                      style:top="{(p.start / 60) * HOUR_PX}px"
                      style:height="{Math.max(((p.end - p.start) / 60) * HOUR_PX, 20)}px"
                      style:left="calc({(p.lane / p.lanes) * 100}% + 2px)"
                      style:width="calc({100 / p.lanes}% - 4px)"
                      onclick={() => (viewing = ev)} title="{timeLabel(ev)} {ev.title} ({ev.calendarName})">
                      <span class="block-body">
                        <span class="block-time">{timeLabel(ev)}</span>
                        <span class="block-text">{ev.title}</span>
                      </span>
                    </button>
                  {/if}
                {/each}
                {#if date === today}
                  <div class="now" style:top="{(nowMinutes / 60) * HOUR_PX}px" aria-hidden="true"></div>
                {/if}
              </div>
            {/each}
          </div>
        </div>
      </div>
    {:else}
    <div class="grid" role="grid" aria-label={monthLabel}>
      <div class="row weekdays" role="row">
        {#each WEEKDAYS as w, i (w)}
          <span class={["wd", { sun: i === 0, sat: i === 6 }]} role="columnheader">{w}</span>
        {/each}
      </div>
      {#each [0, 1, 2, 3, 4, 5] as week (week)}
        <div class="row" role="row">
          {#each gridDays.slice(week * 7, week * 7 + 7) as date, i (date)}
            {@const tasks = cellTasks(date)}
            {@const evs = eventsOn(date)}
            {@const shownEvs = evs.slice(0, MAX_CHIPS)}
            {@const shownTasks = tasks.slice(0, MAX_CHIPS - shownEvs.length)}
            {@const hidden = evs.length + tasks.length - shownEvs.length - shownTasks.length}
            {@const doneCount = byDate.get(date)?.done.length ?? 0}
            <button role="gridcell" aria-selected={date === selected}
              class={["cell", {
                other: date.slice(0, 7) !== month,
                today: date === today,
                selected: date === selected,
                sun: i === 0,
                sat: i === 6,
              }]}
              onclick={() => select(date)} ondblclick={() => openNote(date)}
              title={notes.get(date) ? `데일리 노트: ${notes.get(date)} (더블클릭으로 열기)` : "더블클릭으로 데일리 노트 만들기"}>
              <span class="cell-head">
                <span class="num">{parseYmd(date).getDate()}</span>
                {#if notes.get(date)}<span class="note-dot" aria-label="데일리 노트 있음"></span>{/if}
                {#if doneCount}<span class="done-count" title="완료 {doneCount}"><Icon name="check" size={11} />{doneCount}</span>{/if}
              </span>
              {#each shownEvs as e (e.key)}
                <span class="chip ext" style:--cal={e.color}>{#if e.startTime && !e.allDay}<b>{e.startTime}</b> {/if}{e.title}</span>
              {/each}
              {#each shownTasks as t (taskKey(t))}
                <span class={["chip", { overdue: date < today }]}>{#if t.time}<b>{t.time}</b> {/if}{t.text}</span>
              {/each}
              {#if hidden > 0}
                <span class="more">+{hidden}</span>
              {/if}
            </button>
          {/each}
        </div>
      {/each}
    </div>
    {/if}

    <section class="day" aria-label="선택한 날짜">
      <header class="day-head">
        <h2>{longLabel(selected)}{#if selected === today}<span class="badge">오늘</span>{/if}</h2>
        <button class="note-btn" onclick={() => openNote(selected)} disabled={!!data?.daily_error}>
          <Icon name={selectedNote ? "note-pen" : "plus"} size={15} />
          {selectedNote ? "데일리 노트 열기" : "데일리 노트 만들기"}
        </button>
      </header>

      {#if message}
        <p class={["message", message.kind]} role={message.kind === "error" ? "alert" : "status"}>
          {message.text}
          <button class="dismiss" onclick={() => (message = null)} aria-label="닫기"><Icon name="close" size={14} /></button>
        </p>
      {/if}

      <form class="add" onsubmit={add}>
        <input bind:this={addInput} bind:value={newText} placeholder="{longLabel(selected)} 마감 할일 추가… (14:00 회의 처럼 시간도)" aria-label="새 할일" />
        <button type="submit" disabled={!newText.trim()}>추가</button>
      </form>

      {#if eventsOn(selected).length}
        {@const evs = eventsOn(selected)}
        <h3><Icon name="calendar" size={14} /> 외부 일정 <span class="n">{evs.length}</span></h3>
        <ul>
          {#each evs as e (e.key)}
            <li>
              <button class="ext-row" onclick={() => (viewing = e)}>
                <span class="dot" style:background={e.color}></span>
                <span class="ext-time">{timeLabel(e)}</span>
                <span class="ext-title">{e.title}</span>
                {#if e.location}<span class="ext-loc">{e.location}</span>{/if}
                <span class="ext-cal">{e.calendarName}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}

      {#if sections.length === 0}
        <p class="empty">이 날짜의 할일이 없습니다.</p>
      {/if}
      {#each sections as s (s.id)}
        <h3><Icon name={s.icon} size={14} /> {s.label} <span class="n">{s.items.length}</span></h3>
        <ul>
          {#each s.items as t (taskKey(t))}
            <TaskRow task={t} {today} showDate={false} busy={toggler.isBusy(t)} ontoggle={toggler.toggle} {onopen}
              onedit={(x) => (editing = x)} />
          {/each}
        </ul>
      {/each}
    </section>
  </div>
</section>

{#if showCalendars}
  <CalendarsDialog calendars={sources} onclose={() => (showCalendars = false)}
    onsaved={(list) => { sources = list; loadExternal(true); }} />
{/if}

{#if viewing}
  {@const ev = viewing}
  <div class="backdrop" role="presentation" onpointerdown={(e) => e.target === e.currentTarget && (viewing = null)}>
    <div class="event-card" role="dialog" aria-modal="true" aria-labelledby="event-title" tabindex="-1"
      onkeydown={(e) => { if (e.key === "Escape") { e.stopPropagation(); viewing = null; } }}>
      <header>
        <span class="dot big" style:background={ev.color}></span>
        <h2 id="event-title">{ev.title}</h2>
        <!-- svelte-ignore a11y_autofocus -->
        <button class="dismiss" onclick={() => (viewing = null)} aria-label="닫기" autofocus><Icon name="close" size={16} /></button>
      </header>
      <dl>
        <dt>시간</dt><dd>{longLabel(ev.startDate)}{ev.endDate !== ev.startDate ? ` – ${longLabel(ev.endDate)}` : ""} · {timeLabel(ev)}</dd>
        <dt>캘린더</dt><dd>{ev.calendarName} <span class="muted">(읽기 전용)</span></dd>
        {#if ev.location}<dt>장소</dt><dd>{ev.location}</dd>{/if}
        {#if ev.description}<dt>메모</dt><dd class="desc">{ev.description}</dd>{/if}
      </dl>
    </div>
  </div>
{/if}

{#if editing}
  <TaskEditor task={editing} onclose={() => (editing = null)} {beforewrite} {onopen}
    ondone={(text) => { message = { kind: "ok", text }; load(); }} />
{/if}

<style>
  .calendar {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 20px max(16px, calc((100% - 980px) / 2)) 0;
  }
  /* On a narrow screen (a phone) the controls wrap under the title rather
     than squeezing it and their labels to a letter per line. */
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 12px;
  }
  .head :is(h1, button) {
    white-space: nowrap;
  }
  h1 {
    flex: 1;
    margin: 0;
    font-size: 22px;
  }
  .modes {
    display: flex;
    padding: 2px;
    background: var(--bg-side);
    border: 1px solid var(--border);
    border-radius: 7px;
  }
  .modes button {
    border: 0;
    background: none;
    color: var(--text-muted);
    font: inherit;
    font-size: 13px;
    padding: 3px 12px;
    border-radius: 5px;
    cursor: pointer;
  }
  .modes button {
    transition: color 0.2s;
  }
  .modes button.on {
    color: var(--text);
  }
  /* ---------------------------------------------------- external calendars
     (read-only events from ICS subscriptions) */
  .nav button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .sources {
    list-style: none;
    margin: 8px 0 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
    font-size: 12px;
    color: var(--text-muted);
  }
  .sources li {
    display: flex;
    align-items: center;
    gap: 5px;
  }
  .sources .loading {
    color: var(--text-faint);
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
  .dot.big {
    width: 12px;
    height: 12px;
  }
  /* External events: the calendar's color marks identity; text stays in ink colors. */
  .chip.ext {
    background: color-mix(in srgb, var(--cal) 16%, var(--bg));
    border-left: 3px solid var(--cal);
    border-radius: 3px;
  }
  .block.event {
    border: 0;
    border-left: 3px solid var(--cal);
    background: color-mix(in srgb, var(--cal) 18%, var(--bg));
    font: inherit;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }
  .ext-row {
    display: flex;
    align-items: baseline;
    gap: 10px;
    width: 100%;
    border: 0;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 14px;
    text-align: left;
    padding: 7px 8px;
    border-radius: 6px;
    cursor: pointer;
  }
  .ext-row:hover {
    background: var(--hover);
  }
  .ext-row .dot {
    align-self: center;
  }
  .ext-time {
    flex: none;
    min-width: 92px;
    font-size: 12px;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
  }
  .ext-title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ext-loc,
  .ext-cal {
    font-size: 12px;
    color: var(--text-faint);
    white-space: nowrap;
  }
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: rgb(0 0 0 / 0.25);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .event-card {
    width: min(440px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    overflow: auto;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: 0 16px 48px rgb(0 0 0 / 0.25);
    padding: 14px 16px 18px 20px;
    outline: none;
  }
  .event-card header {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .event-card h2 {
    flex: 1;
    margin: 0;
    font-size: 17px;
  }
  .event-card dl {
    display: grid;
    grid-template-columns: 52px 1fr;
    gap: 8px 12px;
    margin: 14px 0 0;
    font-size: 13px;
  }
  .event-card dt {
    color: var(--text-faint);
  }
  .event-card dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .event-card .desc {
    white-space: pre-wrap;
    color: var(--text-muted);
  }
  .muted {
    color: var(--text-faint);
  }
  /* ------------------------------------------------------- week / day grid */
  .timegrid {
    --gutter: 52px;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
  }
  .tg-head,
  .tg-allday,
  .tg-body {
    display: grid;
    grid-template-columns: var(--gutter) repeat(var(--cols), minmax(0, 1fr));
  }
  .tg-head {
    background: var(--bg-side);
    border-bottom: 1px solid var(--border);
  }
  .tg-gutter {
    font-size: 11px;
    color: var(--text-faint);
    text-align: right;
    padding-right: 6px;
  }
  .tg-allday .tg-gutter {
    padding-top: 6px;
  }
  .tg-day {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1px;
    border: 0;
    border-left: 1px solid var(--border);
    background: none;
    color: var(--day-color, var(--text));
    font: inherit;
    padding: 6px 0;
    cursor: pointer;
  }
  .tg-day:hover {
    background: var(--hover);
  }
  .tg-wd {
    font-size: 11px;
    color: var(--day-color, var(--text-muted));
  }
  .tg-num {
    font-size: 16px;
    min-width: 28px;
    height: 28px;
    line-height: 28px;
    text-align: center;
    border-radius: 14px;
  }
  .tg-day.today .tg-num {
    background: var(--accent);
    color: #fff;
    font-weight: 600;
  }
  .tg-day.selected {
    box-shadow: inset 0 -2px 0 var(--accent);
  }
  .tg-allday {
    border-bottom: 1px solid var(--border);
    min-height: 30px;
  }
  .tg-allday-cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 3px;
    border-left: 1px solid var(--border);
    min-width: 0;
  }
  .tg-allday-cell .chip {
    border: 0;
    font: inherit;
    font-size: 11px;
    text-align: left;
    cursor: pointer;
  }
  .tg-scroll {
    height: min(60vh, 560px);
    overflow: auto;
  }
  .tg-body {
    position: relative;
  }
  .tg-hours {
    position: relative;
  }
  .tg-hours span {
    position: absolute;
    right: 6px;
    transform: translateY(-50%);
  }
  .tg-col {
    position: relative;
    border-left: 1px solid var(--border);
  }
  .tg-slot {
    position: absolute;
    left: 0;
    right: 0;
    border: 0;
    border-top: 1px solid var(--border);
    background: none;
    padding: 0;
    cursor: pointer;
  }
  .tg-slot:first-child {
    border-top: 0;
  }
  .tg-slot:hover {
    background: var(--hover);
  }
  .block {
    position: absolute;
    box-sizing: border-box;
    display: flex;
    align-items: flex-start;
    gap: 4px;
    padding: 2px 4px;
    border-radius: 5px;
    border-left: 3px solid var(--accent);
    background: color-mix(in srgb, var(--accent) 18%, var(--bg));
    overflow: hidden;
    font-size: 12px;
    z-index: 1;
  }
  .block.done,
  .block.cancelled {
    opacity: 0.55;
  }
  .block.done .block-text {
    text-decoration: line-through;
  }
  .block input {
    margin: 2px 0 0;
    accent-color: var(--accent);
    flex: none;
  }
  /* Time and text flow as one paragraph, so a 30-minute block still shows the text. */
  .block-body {
    flex: 1;
    min-width: 0;
    align-self: stretch;
    overflow: hidden;
    border: 0;
    background: none;
    color: var(--text);
    font: inherit;
    line-height: 1.35;
    text-align: left;
    padding: 0;
    cursor: pointer;
    overflow-wrap: anywhere;
  }
  .block-time {
    font-size: 10px;
    color: var(--text-muted);
    margin-right: 4px;
  }
  .block input {
    width: 12px;
    height: 12px;
  }
  .now {
    position: absolute;
    left: 0;
    right: 0;
    height: 2px;
    background: var(--danger);
    z-index: 2;
    pointer-events: none;
  }
  .now::before {
    content: "";
    position: absolute;
    left: -4px;
    top: -3px;
    width: 8px;
    height: 8px;
    border-radius: 4px;
    background: var(--danger);
  }
  .nav {
    display: flex;
    gap: 4px;
  }
  .nav button {
    display: inline-flex;
    align-items: center;
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 14px;
    padding: 4px 12px;
    border-radius: 6px;
    cursor: pointer;
  }
  .nav button:hover {
    background: var(--hover);
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
    margin-top: 12px;
    padding-bottom: 40px;
  }
  .grid {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
  }
  .row {
    display: grid;
    grid-template-columns: repeat(7, minmax(0, 1fr));
  }
  .row + .row {
    border-top: 1px solid var(--border);
  }
  .wd {
    text-align: center;
    font-size: 12px;
    color: var(--text-muted);
    padding: 6px 0;
    background: var(--bg-side);
  }
  .sun {
    --day-color: var(--sunday);
  }
  .sat {
    --day-color: var(--saturday);
  }
  .wd.sun,
  .wd.sat {
    color: var(--day-color);
  }
  .cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-height: 88px;
    min-width: 0;
    padding: 4px 5px;
    border: 0;
    border-left: 1px solid var(--border);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .cell:first-child {
    border-left: 0;
  }
  .cell:hover {
    background: var(--hover);
  }
  .cell.other {
    background: var(--bg-side);
  }
  .cell.other .num {
    opacity: 0.45;
  }
  .cell.selected {
    box-shadow: inset 0 0 0 2px var(--accent);
  }
  .cell-head {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 13px;
  }
  .num {
    color: var(--day-color, var(--text));
    min-width: 22px;
    height: 22px;
    line-height: 22px;
    text-align: center;
    border-radius: 11px;
  }
  .cell.today .num {
    background: var(--accent);
    color: #fff;
    font-weight: 600;
  }
  .note-dot {
    width: 6px;
    height: 6px;
    border-radius: 3px;
    background: var(--accent);
  }
  .done-count {
    display: inline-flex;
    align-items: center;
    gap: 1px;
    margin-left: auto;
    font-size: 11px;
    color: var(--text-faint);
  }
  .chip {
    font-size: 11px;
    line-height: 1.5;
    padding: 0 5px;
    border-radius: 4px;
    background: var(--accent-bg);
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chip.overdue {
    background: var(--danger-bg);
    color: var(--danger);
  }
  .more {
    font-size: 11px;
    color: var(--text-faint);
    padding-left: 4px;
  }
  .day {
    max-width: 800px;
    margin: 20px auto 0;
  }
  .day-head {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: 17px;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .badge {
    font-size: 11px;
    font-weight: 500;
    color: var(--accent);
    background: var(--accent-bg);
    border-radius: 8px;
    padding: 1px 7px;
  }
  .note-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    padding: 6px 12px;
    border-radius: 6px;
    cursor: pointer;
  }
  .note-btn:hover:not(:disabled) {
    border-color: var(--accent);
    color: var(--accent);
  }
  .note-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .add {
    display: flex;
    gap: 8px;
    margin-top: 12px;
  }
  .add input {
    flex: 1;
    min-width: 0;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 14px;
    padding: 7px 10px;
    outline: none;
  }
  .add input:focus {
    border-color: var(--accent);
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
  h3 {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-muted);
    margin: 18px 0 4px;
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
  .empty {
    color: var(--text-faint);
    font-size: 13px;
    margin: 16px 0 0;
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
</style>
