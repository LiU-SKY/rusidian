<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { setBudgets, type Budgets } from "./api";
  import Icon from "./Icon.svelte";

  interface Props {
    budgets: Budgets;
    /** Categories used in the vault, most used first. */
    categories: string[];
    onclose: () => void;
    onsaved: (budgets: Budgets) => void;
  }

  let { budgets, categories, onclose, onsaved }: Props = $props();

  const won = (cents: number | null | undefined) => (cents ? String(cents / 100) : "");
  /** "300,000원" → 30_000_000; blank → null. */
  function cents(text: string): number | null | "bad" {
    const t = text.replace(/[,\s원₩]/g, "");
    if (!t) return null;
    const n = Number(t);
    return Number.isFinite(n) && n >= 0 ? Math.round(n * 100) : "bad";
  }

  const start = untrack(() => budgets);
  let total = $state(won(start.total));
  let rows = $state<{ name: string; amount: string }[]>(
    [...new Set([...Object.keys(start.categories), ...untrack(() => categories)])].map((name) => ({
      name,
      amount: won(start.categories[name]),
    })),
  );
  let newName = $state("");
  let error = $state<string | null>(null);
  let busy = $state(false);
  let first: HTMLInputElement | undefined = $state();

  onMount(() => {
    const before = document.activeElement as HTMLElement | null;
    first?.focus();
    return () => {
      if (before?.isConnected) before.focus();
    };
  });

  function addRow() {
    const name = newName.trim().replace(/^#/, "");
    if (!name) return;
    if (!rows.some((r) => r.name === name)) rows.push({ name, amount: "" });
    newName = "";
  }

  async function save(e: SubmitEvent) {
    e.preventDefault();
    const t = cents(total);
    const out: Budgets = { total: null, categories: {} };
    if (t === "bad") return void (error = `월 전체 예산 "${total}"을(를) 숫자로 적어주세요.`);
    out.total = t;
    for (const r of rows) {
      const c = cents(r.amount);
      if (c === "bad") return void (error = `"${r.name}" 예산 "${r.amount}"을(를) 숫자로 적어주세요.`);
      if (c) out.categories[r.name] = c;
    }
    busy = true;
    try {
      onsaved(await setBudgets(out));
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
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="budget-title" tabindex="-1" {onkeydown}>
    <form onsubmit={save}>
      <header>
        <h2 id="budget-title">월 예산</h2>
        <button type="button" class="close" onclick={onclose} aria-label="닫기"><Icon name="close" size={16} /></button>
      </header>
      <div class="body">
        <p class="note">매달 같은 금액이 적용됩니다. 비워 두면 예산 없음. vault의 <code>.obsidian/rusidian.json</code>에 저장됩니다.</p>
        <label class="row total">
          <span>월 전체 지출</span>
          <span class="field"><input bind:this={first} bind:value={total} inputmode="decimal" placeholder="없음" />원</span>
        </label>
        <div class="list">
          {#each rows as r (r.name)}
            <label class="row">
              <span class="name">#{r.name}</span>
              <span class="field"><input bind:value={r.amount} inputmode="decimal" placeholder="없음" aria-label="{r.name} 예산" />원</span>
            </label>
          {/each}
        </div>
        <div class="add">
          <input bind:value={newName} placeholder="다른 분류 추가" aria-label="분류 이름"
            onkeydown={(e) => { if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); addRow(); } }} />
          <button type="button" onclick={addRow} disabled={!newName.trim()}>추가</button>
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
    width: min(440px, calc(100vw - 32px));
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
  .close {
    display: inline-flex;
    border: 0;
    background: none;
    color: var(--text-muted);
    padding: 5px;
    border-radius: 5px;
    cursor: pointer;
  }
  .body {
    padding: 12px 20px;
    overflow: auto;
    min-height: 0;
  }
  .note {
    margin: 0 0 10px;
    font-size: 12px;
    color: var(--text-faint);
  }
  code {
    font-family: var(--font-mono);
    background: var(--code-bg);
    padding: 0 3px;
    border-radius: 3px;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 5px 0;
    font-size: 14px;
  }
  .row.total {
    font-weight: 600;
    border-bottom: 1px solid var(--border);
    padding-bottom: 10px;
    margin-bottom: 4px;
  }
  .name {
    color: var(--text-muted);
  }
  .field {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--text-faint);
    font-size: 13px;
    font-weight: 400;
  }
  input {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 14px;
    padding: 5px 8px;
    outline: none;
  }
  .field input {
    width: 130px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  input:focus {
    border-color: var(--accent);
  }
  .add {
    display: flex;
    gap: 8px;
    margin-top: 10px;
  }
  .add input {
    flex: 1;
  }
  .add button,
  footer button {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    padding: 6px 14px;
    cursor: pointer;
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
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
