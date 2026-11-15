<script lang="ts">
  import { displayName, fileTimes, type FileTimes } from "./api";

  interface Props {
    path: string;
    /** Changes after each save, so the times are read again. */
    saved: unknown;
    /** Rename the note (`name` without ".md"). */
    onrename: (name: string) => Promise<void>;
    /** Enter in the title: go on to the text. */
    ondone: () => void;
  }

  let { path, saved, onrename, ondone }: Props = $props();

  const name = $derived(displayName(path.split("/").pop()!));
  let draft = $state("");
  let input: HTMLInputElement | undefined = $state();
  let times = $state<FileTimes | null>(null);

  // A new note (or a rename done elsewhere) shows its own name.
  $effect(() => {
    draft = name;
  });

  // Read the times when the note opens and again after each save.
  let seq = 0;
  $effect(() => {
    const p = path;
    saved;
    const s = ++seq;
    fileTimes(p)
      .then((t) => s === seq && (times = t))
      .catch(() => s === seq && (times = null));
  });

  /** A rename is under way (Enter and the blur after it ask only once). */
  let busy = false;

  async function commit() {
    const next = draft.trim();
    if (busy) return;
    if (!next || next === name) {
      draft = name;
      return;
    }
    busy = true;
    try {
      await onrename(next);
    } finally {
      busy = false;
    }
    // Failed (bad name, already taken): show the real name again.
    draft = name;
  }

  async function onkeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.isComposing) {
      e.preventDefault();
      await commit();
      input?.blur();
      ondone();
    } else if (e.key === "Escape") {
      e.preventDefault();
      draft = name;
      input?.blur();
    }
  }

  const WEEK = ["일", "월", "화", "수", "목", "금", "토"];
  const pad = (n: number) => String(n).padStart(2, "0");

  /** "오늘 14:02", "어제 09:10", "11월 3일 (월) 09:10", or with the year when it isn't this one. */
  function when(ms: number): string {
    const d = new Date(ms);
    const now = new Date();
    const hm = `${pad(d.getHours())}:${pad(d.getMinutes())}`;
    const day = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
    const diff = Math.round((day(now) - day(d)) / 86_400_000);
    if (diff === 0) return `오늘 ${hm}`;
    if (diff === 1) return `어제 ${hm}`;
    const md = `${d.getMonth() + 1}월 ${d.getDate()}일 (${WEEK[d.getDay()]})`;
    return d.getFullYear() === now.getFullYear() ? `${md} ${hm}` : `${d.getFullYear()}년 ${md} ${hm}`;
  }

  const full = (ms: number) => {
    const d = new Date(ms);
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
  };
</script>

<header class="note-head">
  <input class="title" bind:this={input} bind:value={draft} onblur={commit} {onkeydown}
    spellcheck="false" aria-label="노트 이름" title="노트 이름 (바꾸면 파일 이름과 이 노트를 가리키는 링크가 함께 바뀝니다)" />
  {#if times && (times.created || times.modified)}
    <div class="meta">
      {#if times.created}<span title={full(times.created)}>만든 날 {when(times.created)}</span>{/if}
      {#if times.modified}<span title={full(times.modified)}>고친 날 {when(times.modified)}</span>{/if}
    </div>
  {/if}
</header>

<style>
  .note-head {
    flex-shrink: 0;
    padding: 18px max(24px, calc((100% - var(--line-width, 800px)) / 2 + 24px)) 6px;
  }
  .title {
    display: block;
    width: 100%;
    box-sizing: border-box;
    margin: 0;
    padding: 0;
    border: 0;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: calc(var(--editor-font-size, 16px) * 1.75);
    font-weight: 700;
    line-height: 1.25;
    outline: none;
    user-select: text;
  }
  .title:focus {
    box-shadow: 0 1px 0 var(--accent);
  }
  .meta {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
    margin-top: 6px;
    font-size: 12px;
    color: var(--text-faint);
    font-variant-numeric: tabular-nums;
  }
</style>
