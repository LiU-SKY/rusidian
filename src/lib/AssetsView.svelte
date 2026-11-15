<script lang="ts">
  import { addBalance, balances as loadBalances, setSavings, type BalanceItem } from "./api";
  import { localDate, parseYmd } from "./dates";
  import Icon from "./Icon.svelte";

  interface Props {
    /** Bumped when vault files change; reloads. */
    version: number;
    onopen: (path: string) => void;
    beforewrite: () => Promise<void>;
    /** Categories / groups that are savings. */
    savings: string[];
    onsavings: (list: string[]) => void;
  }

  let { version, onopen, beforewrite, savings, onsavings }: Props = $props();

  let items = $state<BalanceItem[]>([]);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let open = $state<string | null>(null);
  let seq = 0;

  async function load() {
    const s = ++seq;
    try {
      const list = await loadBalances();
      if (s === seq) {
        items = list;
        loaded = true;
        error = null;
      }
    } catch (e) {
      if (s === seq) error = String(e);
    }
  }

  $effect(() => {
    version;
    load();
  });

  // ------------------------------------------------------------ accounts
  const isSaving = (group: string | null) => !!group && savings.some((s) => group === s || group.startsWith(`${s}/`));
  const OTHER = "기타";

  interface Account { name: string; group: string | null; history: BalanceItem[]; latest: BalanceItem; previous: BalanceItem | null }
  const accounts = $derived.by((): Account[] => {
    const by = new Map<string, BalanceItem[]>();
    for (const b of items) {
      const list = by.get(b.account);
      if (list) list.push(b);
      else by.set(b.account, [b]);
    }
    return [...by].map(([name, history]) => {
      const latest = history[history.length - 1];
      return { name, group: latest.group, history, latest, previous: history.length > 1 ? history[history.length - 2] : null };
    });
  });
  const groups = $derived.by(() => {
    const by = new Map<string, Account[]>();
    for (const a of accounts) {
      const g = a.group ?? OTHER;
      const list = by.get(g);
      if (list) list.push(a);
      else by.set(g, [a]);
    }
    return [...by]
      .map(([name, list]) => ({ name, saving: isSaving(name), list: list.sort((a, b) => b.latest.amount - a.latest.amount), total: list.reduce((n, a) => n + a.latest.amount, 0) }))
      .sort((a, b) => Number(a.saving) - Number(b.saving) || b.total - a.total);
  });
  const total = $derived(accounts.reduce((n, a) => n + a.latest.amount, 0));
  const savingTotal = $derived(accounts.filter((a) => isSaving(a.group)).reduce((n, a) => n + a.latest.amount, 0));
  const lastDate = $derived(items.reduce<string | null>((d, b) => (b.date && (!d || b.date > d) ? b.date : d), null));

  /** Total assets on each day something was recorded (each account at its latest value by then). */
  const history = $derived.by(() => {
    const dates = [...new Set(items.map((b) => b.date).filter((d): d is string => !!d))].sort();
    return dates.slice(-8).map((d) => ({
      date: d,
      total: accounts.reduce((n, a) => {
        const upTo = a.history.filter((b) => b.date && b.date <= d);
        return n + (upTo.length ? upTo[upTo.length - 1].amount : 0);
      }, 0),
    }));
  });
  const historyMax = $derived(Math.max(1, ...history.map((h) => Math.abs(h.total))));

  // ------------------------------------------------------------ formats
  const won = (cents: number) => `${(cents / 100).toLocaleString("ko-KR", { maximumFractionDigits: 2 })}원`;
  const signedWon = (cents: number) => `${cents < 0 ? "−" : "+"}${won(Math.abs(cents))}`;
  const md = (d: string | null) => {
    if (!d) return "날짜 없음";
    const x = parseYmd(d);
    return `${x.getMonth() + 1}/${x.getDate()}`;
  };

  // ------------------------------------------------------------ form
  let account = $state("");
  let amount = $state("");
  let group = $state("");
  let date = $state(localDate());
  /** Where new records go: the note of the latest record, or 자산.md. */
  let note = $state("");
  let busy = $state(false);
  const defaultNote = $derived(items.length ? items[items.length - 1].path : "자산.md");
  const groupNames = $derived([...new Set(["입출금", "페이", "외화", ...savings, ...accounts.map((a) => a.group ?? "").filter(Boolean)])]);

  function pickAccount(name: string) {
    account = name;
    const a = accounts.find((x) => x.name === name);
    if (a && !group) group = a.group ?? "";
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (busy) return;
    busy = true;
    error = null;
    try {
      await beforewrite();
      const target = note.trim() || defaultNote;
      await addBalance(target, amount, account, group, date);
      notice = `${account.trim()} 잔액을 ${target}에 기록했습니다.`;
      account = "";
      amount = "";
      group = "";
      await load();
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }

  // ------------------------------------------------------------ savings setting
  let savingsText = $state("");
  $effect(() => {
    savingsText = savings.join(", ");
  });
  async function saveSavings() {
    try {
      const list = await setSavings(savingsText.split(/[,\s]+/).filter(Boolean));
      onsavings(list);
      notice = `저축으로 볼 분류: ${list.map((c) => `#${c}`).join(" ")}`;
    } catch (e) {
      error = String(e);
    }
  }
</script>

<div class="assets view-in">
  <p class="intro">
    계좌 잔액을 노트에 <code>- 🏦 2,700,000 계좌 이름 #분류 📅 날짜</code>로 기록하면, 계좌마다 가장 최근 기록을 지금 잔액으로 봅니다.
    잔액은 설정이 아니라 노트에 있어서, 동기화할 폴더를 고를 때 함께 정할 수 있습니다.
  </p>

  {#if error}<p class="message error" role="alert">{error}</p>{/if}
  {#if notice}
    <p class="message ok" role="status">
      {notice}
      <button class="dismiss" onclick={() => (notice = null)} aria-label="닫기"><Icon name="close" size={14} /></button>
    </p>
  {/if}

  <div class="tiles">
    <div class="tile">
      <span class="label">총자산</span>
      <span class="value">{won(total)}</span>
      <span class="sub">{accounts.length}개 계좌{lastDate ? ` · ${md(lastDate)} 기준` : ""}</span>
    </div>
    <div class="tile">
      <span class="label">쓸 수 있는 돈</span>
      <span class="value">{won(total - savingTotal)}</span>
      <span class="sub">저축·투자 말고</span>
    </div>
    <div class="tile">
      <span class="label">모으는 돈</span>
      <span class="value">{won(savingTotal)}</span>
      <span class="sub">#{savings.join(" #")}</span>
    </div>
  </div>

  <section class="panel">
    <h2>잔액 기록</h2>
    <form class="form" onsubmit={submit}>
      <label>계좌
        <input bind:value={account} list="asset-accounts" placeholder="NH청년도약계좌" required
          onchange={(e) => pickAccount(e.currentTarget.value)} />
        <datalist id="asset-accounts">{#each accounts as a (a.name)}<option value={a.name}></option>{/each}</datalist>
      </label>
      <label>잔액<input bind:value={amount} inputmode="decimal" placeholder="2,700,000" required /></label>
      <label>분류
        <input bind:value={group} list="asset-groups" placeholder="저축" />
        <datalist id="asset-groups">{#each groupNames as g (g)}<option value={g}></option>{/each}</datalist>
      </label>
      <label>날짜<input type="date" bind:value={date} required /></label>
      <label class="wide">기록할 노트<input bind:value={note} placeholder={defaultNote} spellcheck="false" /></label>
      <button class="submit" type="submit" disabled={busy || !account.trim() || !amount.trim()}>기록</button>
    </form>
  </section>

  {#if loaded && accounts.length === 0}
    <p class="empty">아직 잔액 기록이 없습니다. 위에서 계좌별 잔액을 기록해 보세요.</p>
  {/if}

  {#each groups as g (g.name)}
    <section class="panel">
      <div class="panel-head">
        <h2>{g.name}{#if g.saving} <span class="badge">모으는 돈</span>{/if}</h2>
        <span class="group-total">{won(g.total)}</span>
      </div>
      <ul class="rows">
        {#each g.list as a (a.name)}
          {@const change = a.previous ? a.latest.amount - a.previous.amount : null}
          <li>
            <button class="row" onclick={() => (open = open === a.name ? null : a.name)} aria-expanded={open === a.name}>
              <span class="main">
                <span class="name">{a.name}</span>
                <span class="sub">{md(a.latest.date)} 기준{a.history.length > 1 ? ` · 기록 ${a.history.length}개` : ""}</span>
              </span>
              <span class="right">
                <span class="amount">{won(a.latest.amount)}</span>
                {#if change !== null && change !== 0}
                  <span class={["change", change > 0 ? "up" : "down"]}>{signedWon(change)} ({md(a.previous!.date)}보다)</span>
                {/if}
              </span>
              <Icon name={open === a.name ? "chevron-up" : "chevron-down"} size={14} />
            </button>
            {#if open === a.name}
              <ul class="history">
                {#each [...a.history].reverse() as b (`${b.path}:${b.line}`)}
                  <li>
                    <span class="when">{b.date ?? "날짜 없음"}</span>
                    <span class="amount">{won(b.amount)}</span>
                    <button class="source" onclick={() => onopen(b.path)} title={b.path}><Icon name="file" size={13} /> {b.title}</button>
                  </li>
                {/each}
              </ul>
            {/if}
          </li>
        {/each}
      </ul>
    </section>
  {/each}

  {#if history.length > 1}
    <section class="panel">
      <h2>총자산 변화</h2>
      <ul class="trend" aria-label="기록한 날의 총자산">
        {#each history as h (h.date)}
          <li>
            <span class="when">{md(h.date)}</span>
            <span class="track"><span class="bar" style:width="{(Math.max(0, h.total) / historyMax) * 100}%"></span></span>
            <span class="amount">{won(h.total)}</span>
          </li>
        {/each}
      </ul>
    </section>
  {/if}

  <section class="panel">
    <h2>저축으로 볼 분류</h2>
    <p class="hint">가계부에서 이 분류(하위 분류 포함)의 💸 기록은 지출이 아니라 저축으로 셉니다. 자산의 이 분류는 '모으는 돈'입니다.</p>
    <div class="savings">
      <input bind:value={savingsText} placeholder="저축, 투자" aria-label="저축으로 볼 분류 (쉼표로 구분)" />
      <button onclick={saveSavings}>저장</button>
    </div>
  </section>
</div>

<style>
  .assets {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .intro,
  .hint {
    margin: 0;
    font-size: 13px;
    color: var(--text-muted);
  }
  .hint {
    margin: -4px 0 8px;
    font-size: 12px;
    color: var(--text-faint);
  }
  code {
    font-family: var(--font-mono);
    font-size: 12px;
    background: var(--code-bg);
    padding: 0 3px;
    border-radius: 3px;
  }
  .message {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    padding: 8px 12px;
    border-radius: 7px;
    font-size: 13px;
  }
  .message.ok {
    background: var(--accent-bg);
  }
  .message.error {
    background: var(--danger-bg);
    color: var(--danger);
  }
  .dismiss {
    margin-left: auto;
    display: inline-flex;
    border: 0;
    background: none;
    color: inherit;
    cursor: pointer;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 12px;
  }
  .tile {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 12px 14px;
    border: 1px solid var(--border);
    border-radius: 8px;
    min-width: 0;
  }
  .label {
    font-size: 12px;
    color: var(--text-muted);
  }
  .value {
    font-size: 20px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }
  .sub {
    font-size: 12px;
    color: var(--text-faint);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .panel {
    padding: 14px 16px;
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  .panel-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
  }
  h2 {
    margin: 0 0 10px;
    font-size: 14px;
    font-weight: 600;
  }
  .badge {
    margin-left: 4px;
    padding: 0 6px;
    border-radius: 9px;
    background: var(--accent-bg);
    color: var(--accent);
    font-size: 11px;
    font-weight: 400;
  }
  .group-total {
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .empty {
    margin: 0;
    font-size: 13px;
    color: var(--text-faint);
  }
  .form {
    display: grid;
    grid-template-columns: 2fr 1.4fr 1fr 1.2fr;
    gap: 10px 12px;
    align-items: end;
  }
  .form label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
    color: var(--text-muted);
    min-width: 0;
  }
  .form .wide {
    grid-column: 1 / 4;
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
    color-scheme: inherit;
  }
  input:focus {
    border-color: var(--accent);
  }
  .submit,
  .savings button {
    border: 0;
    border-radius: 6px;
    background: var(--accent);
    color: #fff;
    font: inherit;
    font-size: 14px;
    padding: 7px 16px;
    cursor: pointer;
  }
  .submit:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .rows,
  .history,
  .trend {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .rows > li + li {
    border-top: 1px solid var(--border);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 9px 4px;
    border: 0;
    border-radius: 6px;
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .row:hover {
    background: var(--hover);
  }
  .row > :global(.icon) {
    flex: none;
    color: var(--text-faint);
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .name {
    font-size: 14px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .right {
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 2px;
  }
  .amount {
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .change {
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  .change.up {
    color: var(--series-income, var(--accent));
  }
  .change.down {
    color: var(--danger);
  }
  .history {
    margin: 0 4px 8px 4px;
    padding: 6px 10px;
    border-radius: 6px;
    background: var(--bg-side);
  }
  .history li,
  .trend li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 4px 0;
    font-size: 13px;
  }
  .when {
    flex: none;
    width: 86px;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
  }
  .trend .when {
    width: 44px;
  }
  .history .amount {
    flex: 1;
    font-weight: 400;
  }
  .source {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    border: 0;
    background: none;
    color: var(--text-faint);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .source:hover {
    color: var(--accent);
  }
  .track {
    flex: 1;
    height: 8px;
    border-radius: 4px;
    background: var(--bg-side);
    overflow: hidden;
  }
  .bar {
    display: block;
    height: 100%;
    border-radius: 4px;
    background: var(--accent);
  }
  .savings {
    display: flex;
    gap: 8px;
  }
  .savings input {
    flex: 1;
  }
  @media (max-width: 640px) {
    .tiles {
      grid-template-columns: 1fr;
    }
    .form {
      grid-template-columns: 1fr 1fr;
    }
    .form .wide {
      grid-column: 1 / -1;
    }
  }
</style>
