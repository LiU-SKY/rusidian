<script lang="ts">
  import { onMount } from "svelte";
  import { addSettlement, type FinanceItem } from "./api";
  import { WEEKDAYS, localDate, parseYmd } from "./dates";
  import { splitMemo, type TxGroup } from "./txGroups";
  import Icon from "./Icon.svelte";

  interface Props {
    group: TxGroup;
    /** The payment this record settles, when it is a settlement shown on its own. */
    parent: FinanceItem | null;
    onclose: () => void;
    onedit: (e: FinanceItem) => void;
    onopen: (path: string) => void;
    /** Show another record's detail (the parent of a settlement). */
    onshow: (e: FinanceItem) => void;
    /** After a write; the view reloads. */
    ondone: (message: string) => void;
    beforewrite: () => Promise<void>;
  }

  let { group, parent, onclose, onedit, onopen, onshow, ondone, beforewrite }: Props = $props();

  const item = $derived(group.item);
  const memo = $derived(splitMemo(item.memo));
  const isExpense = $derived(item.kind === "expense");
  const back = $derived(-group.children.reduce((n, c) => n + c.amount, 0));

  const won = (cents: number) => `${(cents / 100).toLocaleString("ko-KR", { maximumFractionDigits: 2 })}원`;
  /** Money in (+) or out (−) for this record's kind: money back on a payment is "+". */
  const signed = (e: FinanceItem) => `${(e.kind === "expense") !== (e.amount < 0) ? "−" : "+"}${won(Math.abs(e.amount))}`;
  const longDate = (d: string | null) => {
    if (!d) return "날짜 없음";
    const x = parseYmd(d);
    return `${x.getFullYear()}년 ${x.getMonth() + 1}월 ${x.getDate()}일 (${WEEKDAYS[x.getDay()]})`;
  };
  const shortDate = (d: string | null) => {
    if (!d) return "";
    const x = parseYmd(d);
    return `${x.getMonth() + 1}/${x.getDate()}`;
  };

  // ------------------------------------------------------------ add a settlement
  let adding = $state(false);
  let amount = $state("");
  let who = $state("");
  let date = $state(localDate());
  let busy = $state(false);
  let error = $state<string | null>(null);
  let amountInput: HTMLInputElement | undefined = $state();

  async function addOne(e: SubmitEvent) {
    e.preventDefault();
    if (busy) return;
    busy = true;
    error = null;
    try {
      await beforewrite();
      const text = who.trim() ? `${who.trim()} 정산` : "정산";
      await addSettlement(item, amount, text, date || null);
      ondone(`정산 ${amount.trim()}원을 추가했습니다.`);
      amount = "";
      who = "";
      adding = false;
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }

  $effect(() => {
    if (adding) amountInput?.focus();
  });

  let dialog: HTMLDivElement | undefined = $state();
  onMount(() => {
    const before = document.activeElement as HTMLElement | null;
    dialog?.focus();
    return () => {
      if (before?.isConnected) before.focus();
    };
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      if (adding) adding = false;
      else onclose();
    }
    e.stopPropagation();
  }
</script>

<div class="backdrop" role="presentation" onpointerdown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="tx-title" tabindex="-1" bind:this={dialog} {onkeydown}>
    <header>
      <span class={["kind", item.kind]} aria-hidden="true"><Icon name={isExpense ? "arrow-up-right" : "arrow-down-left"} size={16} /></span>
      <div class="heading">
        <h2 id="tx-title">{memo.title || (isExpense ? "지출" : "수입")}</h2>
        <p class="meta">
          {longDate(item.date)}
          {#if item.category}<span class="chip">#{item.category}</span>{/if}
          {#if memo.account}<span class="chip account">{memo.account}</span>{/if}
        </p>
      </div>
      <button class="close" onclick={onclose} aria-label="닫기"><Icon name="close" size={16} /></button>
    </header>

    <div class="body">
      {#if memo.detail}<p class="detail">{memo.detail}</p>{/if}

      {#if group.orphan && parent}
        <button class="parent-link" onclick={() => onshow(parent)}>
          <Icon name="link" size={14} />
          {shortDate(parent.date)} {splitMemo(parent.memo).title} {won(parent.amount)}의 정산입니다
          <Icon name="chevron-right" size={14} />
        </button>
      {/if}

      <dl class="sums">
        <div class="row main">
          <dt>{isExpense ? "결제" : "입금"}</dt>
          <dd>{won(item.amount)}</dd>
          <button class="edit" onclick={() => onedit(item)} title="편집" aria-label="이 기록 편집"><Icon name="pencil" size={14} /></button>
        </div>

        {#if group.children.length > 0}
          <div class="group-label">{isExpense ? "정산·환불" : "차감"} {group.children.length}건</div>
          {#each group.children as c (c.line)}
            {@const m = splitMemo(c.memo)}
            <div class="row child">
              <dt>
                <span class="when">{shortDate(c.date)}</span>
                <span class="who">{m.title || "정산"}</span>
                {#if m.account && m.account !== memo.account}<span class="chip account small">{m.account}</span>{/if}
              </dt>
              <dd class={{ back: (c.kind === "expense") === c.amount < 0 }}>{signed(c)}</dd>
              <button class="edit" onclick={() => onedit(c)} title="편집" aria-label="정산 기록 편집"><Icon name="pencil" size={14} /></button>
            </div>
          {/each}
          <div class="row subtotal">
            <dt>{isExpense ? "돌려받은 돈" : "뺀 돈"}</dt>
            <dd>{back >= 0 ? (isExpense ? "+" : "−") : (isExpense ? "−" : "+")}{won(Math.abs(back))}</dd>
            <span></span>
          </div>
          <div class="row total">
            <dt>{isExpense ? "내 몫" : "실제로 남은 돈"}</dt>
            <dd>{won(group.net)}</dd>
            <span></span>
          </div>
          {#if isExpense && group.net === 0}<p class="note">전액 돌려받았습니다.</p>{/if}
        {/if}
      </dl>

      {#if item.parent_line === null}
        {#if adding}
          <form class="add" onsubmit={addOne}>
            <label>{isExpense ? "돌려받은 금액" : "뺄 금액"}
              <input bind:this={amountInput} bind:value={amount} inputmode="decimal" placeholder="10,000" required />
            </label>
            <label>{isExpense ? "누구에게서" : "내용"}
              <input bind:value={who} placeholder={isExpense ? "이름 (선택)" : "연구실 비용 등"} />
            </label>
            <label>날짜<input type="date" bind:value={date} /></label>
            <div class="add-actions">
              <button type="button" onclick={() => (adding = false)}>취소</button>
              <button type="submit" class="primary" disabled={busy || !amount.trim()}>추가</button>
            </div>
            <p class="hint">이 기록 아래에 들여쓴 줄로 적습니다. 분류는 이 기록을 따릅니다.</p>
          </form>
        {:else}
          <button class="add-open" onclick={() => (adding = true)}>
            <Icon name="plus" size={14} /> {isExpense ? "정산·환불 추가" : "차감 추가"}
          </button>
        {/if}
      {/if}
      {#if error}<p class="error" role="alert">{error}</p>{/if}
    </div>

    <footer>
      <button class="link" onclick={() => { onclose(); onopen(item.path); }} title={item.path}>
        <Icon name="file" size={14} /> {item.title} 노트에서 보기
      </button>
      <span class="spacer"></span>
      <button onclick={onclose}>닫기</button>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 55;
    background: rgb(0 0 0 / 0.25);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .dialog {
    width: min(460px, calc(100vw - 32px));
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
    align-items: flex-start;
    gap: 12px;
    padding: 16px 14px 12px 18px;
    border-bottom: 1px solid var(--border);
  }
  .kind {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border-radius: 50%;
    background: var(--bg-side);
    color: var(--text-muted);
  }
  .heading {
    flex: 1;
    min-width: 0;
  }
  h2 {
    margin: 0;
    font-size: 16px;
    line-height: 1.35;
    overflow-wrap: anywhere;
  }
  .meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin: 4px 0 0;
    font-size: 12px;
    color: var(--text-muted);
  }
  .chip {
    padding: 0 6px;
    border-radius: 4px;
    background: var(--bg-side);
    border: 1px solid var(--border);
    font-size: 11px;
    line-height: 18px;
    color: var(--text-muted);
  }
  .chip.small {
    font-size: 10px;
    line-height: 16px;
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
    padding: 14px 18px 4px;
    overflow-y: auto;
  }
  .detail {
    margin: 0 0 12px;
    font-size: 13px;
    color: var(--text-muted);
  }
  .parent-link {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    margin: 0 0 12px;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: var(--bg-side);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }
  .parent-link:hover {
    background: var(--hover);
  }
  .sums {
    margin: 0;
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
  }
  .row {
    display: grid;
    grid-template-columns: 1fr auto 28px;
    align-items: center;
    gap: 8px;
    padding: 8px 6px 8px 12px;
    font-size: 14px;
  }
  .row + .row,
  .group-label {
    border-top: 1px solid var(--border);
  }
  dt {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    color: var(--text);
  }
  dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .main dd {
    font-weight: 600;
  }
  .group-label {
    padding: 6px 12px 2px;
    font-size: 11px;
    color: var(--text-faint);
  }
  .child {
    border-top: 0 !important;
    padding-top: 5px;
    padding-bottom: 5px;
    font-size: 13px;
  }
  .when {
    flex: none;
    width: 38px;
    color: var(--text-faint);
    font-variant-numeric: tabular-nums;
  }
  .who {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .child dd.back {
    color: var(--accent);
  }
  .subtotal {
    font-size: 13px;
    color: var(--text-muted);
  }
  .subtotal dt {
    color: var(--text-muted);
  }
  .total {
    background: var(--bg-side);
  }
  .total dt,
  .total dd {
    font-weight: 700;
  }
  .edit {
    display: inline-flex;
    justify-content: center;
    border: 0;
    background: none;
    color: var(--text-faint);
    padding: 4px;
    border-radius: 5px;
    cursor: pointer;
  }
  .edit:hover {
    background: var(--hover);
    color: var(--text);
  }
  .note {
    margin: 0;
    padding: 0 12px 8px;
    background: var(--bg-side);
    font-size: 12px;
    color: var(--text-muted);
  }
  .add-open {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin: 12px 0 8px;
    border: 1px dashed var(--border);
    border-radius: 7px;
    background: none;
    color: var(--text-muted);
    font: inherit;
    font-size: 13px;
    padding: 6px 12px;
    cursor: pointer;
  }
  .add-open:hover {
    color: var(--text);
    background: var(--hover);
  }
  .add {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr;
    gap: 10px;
    margin: 12px 0 8px;
    padding: 12px;
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  .add label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
    color: var(--text-muted);
    min-width: 0;
  }
  .add input {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    padding: 5px 7px;
    outline: none;
    min-width: 0;
    color-scheme: inherit;
  }
  .add input:focus {
    border-color: var(--accent);
  }
  .add-actions {
    grid-column: 1 / -1;
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .hint {
    grid-column: 1 / -1;
    margin: 0;
    font-size: 11px;
    color: var(--text-faint);
  }
  .error {
    margin: 8px 0;
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
    padding: 10px 18px 14px;
  }
  .spacer {
    flex: 1;
  }
  footer button,
  .add-actions button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    padding: 5px 12px;
    cursor: pointer;
  }
  footer button:hover,
  .add-actions button:hover:not(:disabled) {
    background: var(--hover);
  }
  footer .link {
    border: 0;
    padding: 5px 4px;
    color: var(--text-muted);
  }
  .add-actions .primary {
    border-color: var(--accent);
    background: var(--accent);
    color: #fff;
  }
  .add-actions button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  @media (max-width: 480px) {
    .add {
      grid-template-columns: 1fr 1fr;
    }
  }
</style>
