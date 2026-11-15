<script lang="ts">
  import { onMount, untrack } from "svelte";
  import {
    importCsvCommit, importCsvPlan, type ImportMapping, type ImportPreview, type ImportResult, type PlannedRow,
  } from "./api";
  import Icon from "./Icon.svelte";

  interface Props {
    /** Absolute path of the picked CSV. */
    file: string;
    preview: ImportPreview;
    /** Category suggestions. */
    categories: string[];
    onclose: () => void;
    ondone: (result: ImportResult) => void;
    beforewrite: () => Promise<void>;
    /** The password a protected Excel file was opened with (kept only while the dialog is open). */
    password?: string | null;
  }

  let { file, preview, categories, onclose, ondone, beforewrite, password = null }: Props = $props();

  const start = untrack(() => preview);
  const fileName = untrack(() => file).split(/[\\/]/).pop() ?? "가져오기.csv";
  const stem = fileName.replace(/\.[^.]+$/, "");

  let header = $state(start.header);
  let m = $state<ImportMapping>({ ...start.mapping });
  let split = $state(start.mapping.amount === null);
  let defaultCategory = $state("");
  let note = $state(`가져온 거래/${stem.replace(/[\\/:*?"<>|#^[\]]/g, "-")}`);
  let planned = $state<PlannedRow[]>([]);
  let planning = $state(false);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let showSkipped = $state(false);
  let dialog: HTMLDivElement | undefined = $state();

  const headerRow = $derived(start.rows[header] ?? []);
  const columns = $derived(headerRow.map((name, i) => ({ i, label: `${i + 1}. ${name || "(이름 없음)"}` })));
  /** A few rows after the header, to recognise the columns by their values. */
  const sample = $derived(start.rows.slice(header + 1, header + 4));

  onMount(() => {
    const before = document.activeElement as HTMLElement | null;
    dialog?.focus();
    return () => {
      if (before?.isConnected) before.focus();
    };
  });

  /** What the server is asked to use: one amount column, or withdrawal + deposit. */
  const effective = $derived<ImportMapping>({
    ...m,
    amount: split ? null : m.amount,
    withdrawal: split ? m.withdrawal : null,
    deposit: split ? m.deposit : null,
    direction: split ? null : m.direction,
  });

  let seq = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const args = [header, JSON.stringify(effective), defaultCategory] as const;
    clearTimeout(timer);
    timer = setTimeout(async () => {
      const s = ++seq;
      planning = true;
      try {
        const result = await importCsvPlan(file, args[0], JSON.parse(args[1]), args[2], password);
        if (s === seq) {
          planned = result;
          error = null;
        }
      } catch (e) {
        if (s === seq) error = String(e);
      } finally {
        if (s === seq) planning = false;
      }
    }, 200);
  });

  const ok = $derived(planned.filter((p) => p.status === "ok"));
  const dupes = $derived(planned.filter((p) => p.status === "duplicate").length);
  const skipped = $derived(planned.filter((p) => p.status === "skipped"));
  const total = (kind: "expense" | "income") => ok.filter((p) => p.kind === kind).reduce((n, p) => n + (p.amount ?? 0), 0);
  const won = (cents: number) => `${(cents / 100).toLocaleString("ko-KR", { maximumFractionDigits: 2 })}원`;
  const shown = $derived(planned.filter((p) => p.status !== "skipped" || showSkipped).slice(0, 300));

  async function commit() {
    busy = true;
    error = null;
    try {
      await beforewrite();
      ondone(await importCsvCommit(file, header, effective, defaultCategory, note, password));
      onclose();
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

  const STATUS: Record<PlannedRow["status"], string> = { ok: "가져옴", duplicate: "이미 있음", skipped: "건너뜀" };
</script>

{#snippet columnSelect(label: string, value: number | null, set: (v: number | null) => void, optional = true)}
  <label>{label}
    <select value={value ?? ""} onchange={(e) => set(e.currentTarget.value === "" ? null : Number(e.currentTarget.value))}>
      {#if optional}<option value="">없음</option>{/if}
      {#each columns as c (c.i)}<option value={c.i}>{c.label}</option>{/each}
    </select>
  </label>
{/snippet}

<div class="backdrop" role="presentation" onpointerdown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="import-title" tabindex="-1" bind:this={dialog} {onkeydown}>
    <header>
      <h2 id="import-title">거래 내역 가져오기</h2>
      <span class="file" title={file}>{fileName} · {start.encoding} · {start.rows.length}줄</span>
      <button class="close" onclick={onclose} aria-label="닫기"><Icon name="close" size={16} /></button>
    </header>

    <div class="body">
      <section class="mapping" aria-label="열 맞추기">
        <label>머리글 줄
          <select bind:value={header}>
            {#each start.rows.slice(0, 30) as r, i (i)}
              <option value={i}>{i + 1}번째 줄: {r.filter(Boolean).slice(0, 4).join(", ").slice(0, 50)}</option>
            {/each}
          </select>
        </label>
        {@render columnSelect("날짜", m.date, (v) => (m.date = v), false)}
        {@render columnSelect("내용", m.memo, (v) => (m.memo = v))}
        {@render columnSelect("분류 (업종)", m.category, (v) => (m.category = v))}
        {@render columnSelect("상태 ('취소' 줄은 건너뜀)", m.status, (v) => (m.status = v))}

        <fieldset class="wide">
          <legend>금액</legend>
          <label class="radio"><input type="radio" bind:group={split} value={true} /> 출금·입금 열이 따로 (통장)</label>
          <label class="radio"><input type="radio" bind:group={split} value={false} /> 금액 열 하나 (카드)</label>
          <div class="amount-cols">
            {#if split}
              {@render columnSelect("출금 (지출)", m.withdrawal, (v) => (m.withdrawal = v))}
              {@render columnSelect("입금 (수입)", m.deposit, (v) => (m.deposit = v))}
            {:else}
              {@render columnSelect("금액", m.amount, (v) => (m.amount = v), false)}
              {@render columnSelect("입출 구분 (입금/출금)", m.direction, (v) => (m.direction = v))}
              {#if m.direction === null}
                <label>부호
                  <select bind:value={m.positive_is_expense}>
                    <option value={true}>양수가 지출 (카드 명세서)</option>
                    <option value={false}>음수가 지출</option>
                  </select>
                </label>
              {/if}
            {/if}
          </div>
        </fieldset>

        <label>분류가 없을 때
          <input bind:value={defaultCategory} list="import-categories" placeholder="없음 (예: 카드)" spellcheck="false" />
          <datalist id="import-categories">{#each categories as c (c)}<option value={c}></option>{/each}</datalist>
        </label>
        <label>기록할 노트
          <input bind:value={note} spellcheck="false" />
        </label>
      </section>

      {#if sample.length}
        <details class="raw">
          <summary>원본 줄 미리보기</summary>
          <div class="table-wrap">
            <table>
              <thead><tr>{#each headerRow as h, i (i)}<th>{i + 1}. {h}</th>{/each}</tr></thead>
              <tbody>
                {#each sample as r, i (i)}<tr>{#each headerRow as _, j (j)}<td>{r[j] ?? ""}</td>{/each}</tr>{/each}
              </tbody>
            </table>
          </div>
        </details>
      {/if}

      <section class="summary" aria-live="polite">
        <strong>{ok.length}건 가져옴</strong>
        <span>지출 {won(total("expense"))} · 수입 {won(total("income"))}</span>
        {#if dupes}<span class="muted">이미 있는 기록 {dupes}건 제외</span>{/if}
        {#if skipped.length}
          <button class="link" onclick={() => (showSkipped = !showSkipped)}>건너뛴 줄 {skipped.length}건 {showSkipped ? "숨기기" : "보기"}</button>
        {/if}
        {#if planning}<span class="muted">계산 중…</span>{/if}
      </section>

      <div class="table-wrap result">
        <table>
          <thead><tr><th>상태</th><th>날짜</th><th class="num">금액</th><th>내용</th><th>분류</th></tr></thead>
          <tbody>
            {#each shown as p (p.row)}
              <tr class={p.status}>
                <td><span class="badge">{STATUS[p.status]}</span></td>
                <td>{p.date ?? ""}</td>
                <td class="num">{#if p.amount !== null}{p.kind === "expense" ? "−" : "+"}{won(p.amount)}{:else}<span class="muted">{p.reason}</span>{/if}</td>
                <td>{p.memo}</td>
                <td>{p.category ? `#${p.category}` : ""}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      {#if error}<p class="error" role="alert">{error}</p>{/if}
    </div>

    <footer>
      <span class="hint">같은 날짜·종류·금액·내용의 기록이 vault에 있으면 건너뜁니다. 다시 가져와도 중복되지 않습니다.</span>
      <button onclick={onclose}>취소</button>
      <button class="primary" onclick={commit} disabled={busy || planning || ok.length === 0}>{ok.length}건 가져오기</button>
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
    width: min(860px, calc(100vw - 32px));
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
  }
  .file {
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
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px 20px;
    overflow: hidden;
  }
  .mapping {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 10px 14px;
  }
  .wide {
    grid-column: 1 / -1;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    font-size: 12px;
    color: var(--text-muted);
  }
  select,
  input:not([type="radio"]) {
    min-width: 0;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    padding: 5px 8px;
    outline: none;
  }
  select:focus,
  input:focus {
    border-color: var(--accent);
  }
  fieldset {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: 6px 16px;
    margin: 0;
    padding: 8px 12px 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  legend {
    font-size: 12px;
    color: var(--text-muted);
    padding: 0 4px;
  }
  .radio {
    flex-direction: row;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    color: var(--text);
  }
  .radio input {
    accent-color: var(--accent);
  }
  .amount-cols {
    flex-basis: 100%;
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px 14px;
  }
  .raw summary {
    font-size: 12px;
    color: var(--text-muted);
    cursor: pointer;
  }
  .table-wrap {
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  .raw .table-wrap {
    margin-top: 6px;
    max-height: 140px;
  }
  .result {
    flex: 1;
    min-height: 0;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
  }
  th,
  td {
    padding: 5px 8px;
    border-bottom: 1px solid var(--border);
    text-align: left;
    white-space: nowrap;
  }
  th {
    position: sticky;
    top: 0;
    background: var(--bg-side);
    font-weight: 600;
    color: var(--text-muted);
  }
  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  tr.duplicate,
  tr.skipped {
    color: var(--text-faint);
  }
  .badge {
    font-size: 11px;
    padding: 1px 6px;
    border-radius: 8px;
    background: var(--hover);
  }
  tr.ok .badge {
    background: var(--accent-bg);
    color: var(--accent);
  }
  .summary {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 6px 16px;
    font-size: 13px;
  }
  .muted {
    color: var(--text-faint);
  }
  .link {
    border: 0;
    background: none;
    color: var(--accent);
    font: inherit;
    padding: 0;
    cursor: pointer;
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
    align-items: center;
    gap: 8px;
    padding: 12px 20px 16px;
    border-top: 1px solid var(--border);
  }
  .hint {
    flex: 1;
    font-size: 12px;
    color: var(--text-faint);
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
