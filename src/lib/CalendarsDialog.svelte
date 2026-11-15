<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { setCalendars, type CalendarSource } from "./api";
  import { GROUP_COLORS } from "./graphGroups";
  import Icon from "./Icon.svelte";

  interface Props {
    calendars: CalendarSource[];
    onclose: () => void;
    onsaved: (calendars: CalendarSource[]) => void;
  }

  let { calendars, onclose, onsaved }: Props = $props();

  let rows = $state<CalendarSource[]>(untrack(() => calendars).map((c) => ({ ...c })));
  let error = $state<string | null>(null);
  let busy = $state(false);
  let dialog: HTMLDivElement | undefined = $state();

  onMount(() => {
    const before = document.activeElement as HTMLElement | null;
    if (rows.length === 0) add();
    dialog?.querySelector<HTMLInputElement>("input.url")?.focus();
    return () => {
      if (before?.isConnected) before.focus();
    };
  });

  function add(url = "") {
    // The first palette color not taken yet, in the validated order.
    const used = new Set(rows.map((r) => r.color.toLowerCase()));
    const color = GROUP_COLORS.find((c) => !used.has(c)) ?? GROUP_COLORS[rows.length % GROUP_COLORS.length];
    rows.push({ id: "", name: "", url, color, enabled: true });
  }

  async function pickFile() {
    const picked = await open({ multiple: false, title: ".ics 파일 고르기", filters: [{ name: "iCalendar", extensions: ["ics"] }] });
    if (typeof picked !== "string") return;
    const empty = rows.find((r) => !r.url.trim());
    if (empty) empty.url = picked;
    else add(picked);
  }

  async function save(e: SubmitEvent) {
    e.preventDefault();
    busy = true;
    error = null;
    try {
      const list = rows.filter((r) => r.url.trim());
      onsaved(await setCalendars(list));
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
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="cal-title" tabindex="-1" bind:this={dialog} {onkeydown}>
    <form onsubmit={save}>
      <header>
        <h2 id="cal-title">외부 캘린더 구독</h2>
        <button type="button" class="close" onclick={onclose} aria-label="닫기"><Icon name="close" size={16} /></button>
      </header>
      <div class="body">
        <p class="note">
          ICS(iCal) 주소를 붙여 넣으면 그 일정을 캘린더에 함께 보여줍니다(읽기 전용, 15분마다 새로 받음).
          Google 캘린더: 설정 › 내 캘린더 › 캘린더 통합 › <b>iCal 형식의 비공개 주소</b>.
          주소는 이 컴퓨터의 앱 설정에만 저장되고 vault에는 들어가지 않습니다.
        </p>
        {#each rows as r, i (i)}
          <div class="row">
            <input type="color" bind:value={r.color} aria-label="캘린더 {i + 1} 색" />
            <div class="fields">
              <input class="name" bind:value={r.name} placeholder="이름 (예: 회사)" aria-label="캘린더 {i + 1} 이름" spellcheck="false" />
              <input class="url" bind:value={r.url} placeholder="https://… 또는 webcal://…" aria-label="캘린더 {i + 1} 주소" spellcheck="false" />
            </div>
            <label class="on" title="보이기"><input type="checkbox" bind:checked={r.enabled} /> 보이기</label>
            <button type="button" class="mini" onclick={() => rows.splice(i, 1)} aria-label="캘린더 {i + 1} 삭제"><Icon name="close" size={14} /></button>
          </div>
        {/each}
        <div class="adds">
          <button type="button" onclick={() => add()}><Icon name="plus" size={14} /> 주소 추가</button>
          <button type="button" onclick={pickFile}><Icon name="folder-open" size={14} /> .ics 파일 고르기</button>
        </div>
        {#if error}<p class="error" role="alert">{error}</p>{/if}
      </div>
      <footer>
        <span class="spacer"></span>
        <button type="button" onclick={onclose}>취소</button>
        <button type="submit" class="primary" disabled={busy}>저장</button>
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
    width: min(620px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    display: flex;
    flex-direction: column;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: 0 16px 48px rgb(0 0 0 / 0.25);
    outline: none;
  }
  form {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  header {
    display: flex;
    align-items: center;
    padding: 14px 14px 10px 20px;
    border-bottom: 1px solid var(--border);
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: 16px;
  }
  .close,
  .mini {
    display: inline-flex;
    border: 0;
    background: none;
    color: var(--text-muted);
    padding: 5px;
    border-radius: 5px;
    cursor: pointer;
  }
  .close:hover,
  .mini:hover {
    background: var(--hover);
    color: var(--text);
  }
  .body {
    padding: 12px 20px;
    overflow: auto;
    min-height: 0;
  }
  .note {
    margin: 0 0 12px;
    font-size: 12px;
    line-height: 1.6;
    color: var(--text-faint);
  }
  .note b {
    color: var(--text-muted);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 0;
    border-bottom: 1px solid var(--border);
  }
  .row input[type="color"] {
    flex: none;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: none;
    cursor: pointer;
  }
  .fields {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .fields input {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    padding: 5px 8px;
    outline: none;
  }
  .fields input:focus {
    border-color: var(--accent);
  }
  .url {
    font-family: var(--font-mono);
    font-size: 12px !important;
  }
  .on {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
    color: var(--text-muted);
    white-space: nowrap;
  }
  .on input {
    accent-color: var(--accent);
  }
  .adds {
    display: flex;
    gap: 8px;
    margin-top: 10px;
  }
  .adds button,
  footer button {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    padding: 6px 12px;
    cursor: pointer;
  }
  .adds button:hover,
  footer button:hover:not(:disabled) {
    background: var(--hover);
  }
  .error {
    margin: 10px 0 0;
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
    border-top: 1px solid var(--border);
  }
  .spacer {
    flex: 1;
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
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
