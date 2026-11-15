<script lang="ts">
  import { onMount, untrack } from "svelte";
  import type { FixedExpense } from "./api";
  import Icon from "./Icon.svelte";

  interface Props {
    value: FixedExpense;
    isNew: boolean;
    categories: string[];
    onclose: () => void;
    onsave: (v: FixedExpense) => void;
    /** Present for an item already on the list. */
    ondelete?: () => void;
  }

  let { value, isNew, categories, onclose, onsave, ondelete }: Props = $props();

  const start = untrack(() => value);
  let name = $state(start.name);
  let amount = $state(start.amount ? (start.amount / 100).toLocaleString("en-US", { maximumFractionDigits: 2 }) : "");
  let day = $state(start.day || 1);
  let keyword = $state(start.keyword);
  let category = $state(start.category ?? "");
  let account = $state(start.account ?? "");
  let error = $state<string | null>(null);
  let first: HTMLInputElement | undefined = $state();

  onMount(() => {
    const before = document.activeElement as HTMLElement | null;
    first?.focus();
    return () => {
      if (before?.isConnected) before.focus();
    };
  });

  function submit(e: SubmitEvent) {
    e.preventDefault();
    const cents = Math.round(Number(amount.replace(/[,원₩\s]/g, "")) * 100);
    if (!name.trim()) return void (error = "이름을 적으세요.");
    if (!Number.isFinite(cents) || cents <= 0) return void (error = "금액을 확인하세요. (예: 10,948)");
    if (!Number.isInteger(day) || day < 1 || day > 31) return void (error = "날짜는 1~31일입니다.");
    onsave({
      name: name.trim(),
      amount: cents,
      day,
      keyword: keyword.trim() || name.trim(),
      category: category.trim().replace(/^#/, "") || null,
      account: account.trim() || null,
    });
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
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="fixed-edit-title" tabindex="-1" {onkeydown}>
    <form onsubmit={submit}>
      <header>
        <h2 id="fixed-edit-title">{isNew ? "고정지출 추가" : "고정지출 편집"}</h2>
        <button type="button" class="close" onclick={onclose} aria-label="닫기"><Icon name="close" size={16} /></button>
      </header>
      <div class="body">
        <label class="wide">이름<input bind:this={first} bind:value={name} placeholder="통신요금" required /></label>
        <label>금액<input bind:value={amount} inputmode="decimal" placeholder="130,000" required /></label>
        <label>매달<span class="day"><input type="number" min="1" max="31" bind:value={day} required />일</span></label>
        <label class="wide">찾을 글자 (키워드)
          <input bind:value={keyword} placeholder={name || "기록 내용에 들어가는 글자"} spellcheck="false" />
        </label>
        <p class="wide note">그 달 기록 중 내용에 이 글자가 들어간 결제를 이 고정지출로 봅니다. 대소문자는 가리지 않습니다. 비우면 이름을 씁니다.</p>
        <label>분류
          <input bind:value={category} list="fixed-edit-categories" placeholder="없음" spellcheck="false" />
          <datalist id="fixed-edit-categories">{#each categories as c (c)}<option value={c}></option>{/each}</datalist>
        </label>
        <label>나가는 계좌<input bind:value={account} placeholder="농협" /></label>
        {#if error}<p class="wide error" role="alert">{error}</p>{/if}
      </div>
      <footer>
        {#if ondelete}
          <button type="button" class="danger" onclick={ondelete}><Icon name="trash" size={15} /> 빼기</button>
        {/if}
        <span class="spacer"></span>
        <button type="button" onclick={onclose}>취소</button>
        <button type="submit" class="primary">{isNew ? "추가" : "저장"}</button>
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
    width: min(440px, calc(100vw - 32px));
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: 0 16px 48px rgb(0 0 0 / 0.25);
    outline: none;
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
  input {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 14px;
    padding: 6px 8px;
    outline: none;
    min-width: 0;
  }
  input:focus {
    border-color: var(--accent);
  }
  .day {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text);
    font-size: 14px;
  }
  .day input {
    width: 70px;
  }
  .note {
    margin: -6px 0 0;
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
  footer button:hover {
    background: var(--hover);
  }
  footer .primary {
    border-color: var(--accent);
    background: var(--accent);
    color: #fff;
  }
  footer .danger {
    color: var(--danger);
  }
</style>
