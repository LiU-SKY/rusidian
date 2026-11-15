<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { deleteFinance, updateFinance, type EntryKind, type FinanceItem } from "./api";
  import Icon from "./Icon.svelte";
  import { slider } from "./slider";

  interface Props {
    entry: FinanceItem;
    /** Category suggestions. */
    categories: string[];
    onclose: () => void;
    /** After a successful save or delete. */
    ondone: (message: string) => void;
    beforewrite: () => Promise<void>;
    onopen: (path: string) => void;
  }

  let { entry, categories, onclose, ondone, beforewrite, onopen }: Props = $props();

  // The dialog edits a copy taken when it opens.
  const initial = untrack(() => entry);
  const amountText = (cents: number) =>
    (cents / 100).toLocaleString("en-US", { maximumFractionDigits: 2, minimumFractionDigits: cents % 100 ? 2 : 0 });
  let kind = $state<EntryKind>(initial.kind);
  let amount = $state(amountText(initial.amount));
  let memo = $state(initial.memo);
  let category = $state(initial.category ?? "");
  let date = $state(initial.date ?? "");
  let error = $state<string | null>(null);
  let busy = $state(false);
  let first: HTMLInputElement | undefined = $state();

  onMount(() => {
    const before = document.activeElement as HTMLElement | null;
    first?.focus();
    first?.select();
    return () => {
      if (before?.isConnected) before.focus();
    };
  });

  /** A date taken from the note stays implicit unless it was changed. */
  const dateToWrite = () => (initial.explicit_date || date !== (initial.date ?? "") ? date || null : null);

  async function save(e: SubmitEvent) {
    e.preventDefault();
    if (busy) return;
    busy = true;
    error = null;
    try {
      await beforewrite();
      await updateFinance(initial, { kind, amount, memo, category, date: dateToWrite() });
      ondone("기록을 고쳤습니다.");
      onclose();
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }

  async function remove() {
    const ok = await ask(`"${initial.memo || "이 기록"}"을(를) 노트에서 지울까요?`, {
      title: "기록 삭제", kind: "warning", okLabel: "삭제", cancelLabel: "취소",
    });
    if (!ok) return;
    busy = true;
    try {
      await beforewrite();
      await deleteFinance(initial);
      ondone("기록을 지웠습니다.");
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
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="fin-edit-title" tabindex="-1" {onkeydown}>
    <form onsubmit={save}>
      <header>
        <h2 id="fin-edit-title">기록 편집</h2>
        <button type="button" class="link" onclick={() => { onclose(); onopen(initial.path); }} title={initial.path}>{initial.title} 열기</button>
        <button type="button" class="close" onclick={onclose} aria-label="닫기"><Icon name="close" size={16} /></button>
      </header>

      <div class="body">
        <div class="seg wide" role="radiogroup" aria-label="종류" use:slider={{ selector: ".on", color: "var(--bg)", shadow: "0 1px 2px rgb(0 0 0 / 0.15)" }}>
          <button type="button" role="radio" aria-checked={kind === "expense"} class:on={kind === "expense"}
            onclick={() => (kind = "expense")}><Icon name="arrow-up-right" size={15} /> 지출</button>
          <button type="button" role="radio" aria-checked={kind === "income"} class:on={kind === "income"}
            onclick={() => (kind = "income")}><Icon name="arrow-down-left" size={15} /> 수입</button>
        </div>
        <label>금액<input bind:this={first} bind:value={amount} inputmode="decimal" required /></label>
        <label>날짜<input type="date" bind:value={date} /></label>
        <label class="wide">내용<input bind:value={memo} spellcheck="false" /></label>
        <label class="wide">분류
          <input bind:value={category} list="fin-edit-categories" placeholder="없음" spellcheck="false" />
          <datalist id="fin-edit-categories">{#each categories as c (c)}<option value={c}></option>{/each}</datalist>
        </label>
        {#if !initial.explicit_date}
          <p class="wide note">날짜는 노트({initial.title})에서 정해집니다. 바꾸면 줄에 <code>📅 날짜</code>로 적습니다.</p>
        {/if}
        {#if error}<p class="wide error" role="alert">{error}</p>{/if}
      </div>

      <footer>
        <button type="button" class="danger" onclick={remove} disabled={busy}><Icon name="trash" size={15} /> 삭제</button>
        <span class="spacer"></span>
        <button type="button" onclick={onclose}>취소</button>
        <button type="submit" class="primary" disabled={busy || !amount.trim()}>저장</button>
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
    width: min(480px, calc(100vw - 32px));
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: 0 16px 48px rgb(0 0 0 / 0.25);
    outline: none;
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
    color-scheme: inherit;
  }
  input:focus {
    border-color: var(--accent);
  }
  .seg {
    display: flex;
    padding: 2px;
    background: var(--bg-side);
    border: 1px solid var(--border);
    border-radius: 7px;
    justify-self: start;
  }
  .seg button {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: 0;
    background: none;
    color: var(--text-muted);
    font: inherit;
    font-size: 13px;
    padding: 4px 14px;
    border-radius: 5px;
    cursor: pointer;
  }
  .seg button {
    transition: color 0.2s;
  }
  .seg button.on {
    color: var(--text);
  }
  .note {
    margin: 0;
    font-size: 12px;
    color: var(--text-faint);
  }
  code {
    font-family: var(--font-mono);
    background: var(--code-bg);
    padding: 0 3px;
    border-radius: 3px;
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
