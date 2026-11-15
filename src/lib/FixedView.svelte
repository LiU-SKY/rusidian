<script lang="ts">
  import {
    fixedExpenses, ignoreFixed, setFixedExpenses, setFixedRemind,
    type FixedExpense, type FixedOverview, type FixedStatus, type FixedSuggestion,
  } from "./api";
  import { settings } from "./settings.svelte";
  import { localDate, parseYmd } from "./dates";
  import FixedEditor from "./FixedEditor.svelte";
  import Icon from "./Icon.svelte";

  interface Props {
    /** "YYYY-MM" shown by the finance view. */
    month: string;
    /** Bumped when vault files change; reloads. */
    version: number;
    categories: string[];
    onopen: (path: string) => void;
  }

  let { month, version, categories, onopen }: Props = $props();

  let overview = $state<FixedOverview | null>(null);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  /** Open editor: an existing item (index) or a new one (index null). */
  let editing = $state<{ index: number | null; value: FixedExpense } | null>(null);
  let seq = 0;

  async function load() {
    const s = ++seq;
    try {
      const o = await fixedExpenses(month);
      if (s === seq) {
        overview = o;
        error = null;
      }
    } catch (e) {
      if (s === seq) error = String(e);
    }
  }

  $effect(() => {
    version;
    month;
    load();
  });

  // Read everything from `editing` before closing it: the dialog's props go with it.
  function saveItem(v: FixedExpense) {
    if (!editing || !overview) return;
    const { index } = editing;
    const list = overview.list;
    editing = null;
    if (index === null) saveList([...list, v], `'${v.name}'을(를) 고정지출에 추가했습니다.`);
    else saveList(list.map((x, i) => (i === index ? v : x)), "고정지출을 고쳤습니다.");
  }

  function deleteItem() {
    if (!editing || editing.index === null || !overview) return;
    const index = editing.index;
    const list = overview.list;
    editing = null;
    saveList(list.filter((_, i) => i !== index), `'${list[index].name}'을(를) 고정지출에서 뺐습니다.`);
  }

  async function saveList(list: FixedExpense[], text: string) {
    try {
      await setFixedExpenses(list);
      notice = text;
      await load();
    } catch (e) {
      error = String(e);
    }
  }

  const REMIND_CHOICES: { value: string; label: string }[] = [
    { value: "", label: "알림 끔" },
    { value: "0", label: "결제일에 알림" },
    { value: "1", label: "하루 전 알림" },
    { value: "3", label: "3일 전 알림" },
    { value: "7", label: "일주일 전 알림" },
  ];

  async function setRemind(value: string) {
    try {
      await setFixedRemind(value === "" ? null : Number(value));
      notice = value === "" ? "고정지출 알림을 껐습니다."
        : `결제일${value === "0" ? "" : ` ${value}일 전`}과 결제일이 지나도 기록이 없을 때 알려 드립니다.`;
      await load();
    } catch (e) {
      error = String(e);
    }
  }

  async function hide(sg: FixedSuggestion) {
    try {
      await ignoreFixed(sg.keyword);
      notice = `'${sg.name}'을(를) 후보에서 숨겼습니다.`;
      await load();
    } catch (e) {
      error = String(e);
    }
  }

  async function unhide() {
    try {
      await ignoreFixed(null);
      notice = "숨긴 후보를 다시 보여 줍니다.";
      await load();
    } catch (e) {
      error = String(e);
    }
  }

  // ------------------------------------------------------------ rows
  const won = (cents: number) => `${(cents / 100).toLocaleString("ko-KR", { maximumFractionDigits: 2 })}원`;
  const today = localDate();
  const thisMonth = today.slice(0, 7);

  interface Row { index: number; item: FixedExpense; status: FixedStatus | undefined }
  const rows = $derived.by((): Row[] => {
    if (!overview) return [];
    const byIndex = new Map(overview.status.map((s) => [s.index, s]));
    return overview.list.map((item, index) => ({ index, item, status: byIndex.get(index) }));
  });
  const total = $derived(rows.reduce((n, r) => n + r.item.amount, 0));
  const paidSum = $derived(rows.reduce((n, r) => n + (r.status?.paid?.amount ?? 0), 0));
  const unpaid = $derived(rows.filter((r) => !r.status?.paid));
  const leftSum = $derived(unpaid.reduce((n, r) => n + r.item.amount, 0));

  function rowState(r: Row): { label: string; tone: "paid" | "soon" | "late" | "plan" } {
    const s = r.status;
    if (s?.paid) {
      const d = parseYmd(s.paid.date);
      return { label: `${d.getMonth() + 1}/${d.getDate()} 냄`, tone: "paid" };
    }
    if (!s || month > thisMonth) return { label: "예정", tone: "plan" };
    if (month < thisMonth || s.due < today) return { label: "기록 없음", tone: "late" };
    const days = Math.round((parseYmd(s.due).getTime() - parseYmd(today).getTime()) / 86_400_000);
    return { label: days === 0 ? "오늘" : `D-${days}`, tone: "soon" };
  }

  const empty = (): FixedExpense => ({ name: "", amount: 0, day: 1, keyword: "", category: null, account: null });
</script>

<div class="fixed view-in">
  <p class="intro">
    매달 나가는 돈을 등록해 두면, 그 달 기록에서 <strong>키워드가 들어간 결제</strong>를 찾아 냈는지 보여줍니다.
    목록은 vault의 <code>.obsidian/rusidian.json</code>에 저장됩니다.
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
      <span class="label">매달 고정지출</span>
      <span class="value">{won(total)}</span>
      <span class="sub">{rows.length}건</span>
    </div>
    <div class="tile">
      <span class="label">이번 달 낸 돈</span>
      <span class="value">{won(paidSum)}</span>
      <span class="sub">{rows.length - unpaid.length}건</span>
    </div>
    <div class="tile">
      <span class="label">남은 돈</span>
      <span class="value">{won(leftSum)}</span>
      <span class="sub">{unpaid.length}건</span>
    </div>
  </div>

  <section class="panel">
    <div class="panel-head">
      <h2>고정지출 <span class="n">{rows.length}</span></h2>
      {#if overview}
        <select class="remind" aria-label="고정지출 알림" value={overview.remind_days === null ? "" : String(overview.remind_days)}
          onchange={(e) => setRemind(e.currentTarget.value)}
          title="결제일이 다가오거나 지났는데 기록이 없으면 Windows 알림으로 알려 줍니다 (앱이 열려 있을 때)">
          {#each REMIND_CHOICES as c (c.value)}<option value={c.value}>{c.label}</option>{/each}
          {#if overview.remind_days !== null && !REMIND_CHOICES.some((c) => c.value === String(overview!.remind_days))}
            <option value={String(overview.remind_days)}>{overview.remind_days}일 전 알림</option>
          {/if}
        </select>
      {/if}
      <button class="add-btn" onclick={() => (editing = { index: null, value: empty() })}><Icon name="plus" size={14} /> 추가</button>
    </div>
    {#if overview?.remind_days != null && !settings.notifications}
      <p class="hint warn">설정(Ctrl+,)에서 <strong>알림</strong>을 켜야 알림이 옵니다.</p>
    {/if}
    {#if overview && rows.length === 0}
      <p class="empty">아직 등록한 고정지출이 없습니다. 아래 후보에서 추가하거나 직접 추가하세요.</p>
    {/if}
    <ul class="rows">
      {#each rows as r (r.index)}
        {@const st = rowState(r)}
        <li>
          <button class="row" onclick={() => (editing = { index: r.index, value: { ...r.item } })}>
            <span class="day"><strong>{r.item.day}</strong>일</span>
            <span class="main">
              <span class="name">{r.item.name}</span>
              <span class="sub">
                {#if r.item.category}<span>#{r.item.category}</span>{/if}
                {#if r.item.account}<span>{r.item.account}</span>{/if}
                <span class="kw" title="이 글자가 들어간 기록을 결제로 봅니다">'{r.item.keyword}'</span>
              </span>
            </span>
            <span class="right">
              <span class="amount">{won(r.status?.paid?.amount ?? r.item.amount)}</span>
              <span class={["state", st.tone]}>
                {#if st.tone === "paid"}<Icon name="check" size={12} />{/if}{st.label}
              </span>
            </span>
          </button>
          {#if r.status?.paid}
            <button class="source" onclick={() => onopen(r.status!.paid!.path)} title="결제 기록이 있는 노트 열기">
              <Icon name="file" size={13} />
            </button>
          {/if}
        </li>
      {/each}
    </ul>
  </section>

  {#if overview && (overview.suggestions.length > 0 || overview.ignored > 0)}
    <section class="panel">
      <h2>기록에서 찾은 후보 <span class="n">{overview.suggestions.length}</span></h2>
      <p class="hint">
        두 달 이상, 매달 비슷한 날에 비슷한 금액으로 나간 결제입니다.
        {#if overview.ignored > 0}<button class="link" onclick={unhide}>숨긴 후보 {overview.ignored}개 다시 보기</button>{/if}
      </p>
      <ul class="rows">
        {#each overview.suggestions as sg (sg.keyword)}
          <li class="suggest">
            <span class="day"><strong>{sg.day}</strong>일</span>
            <span class="main">
              <span class="name">{sg.name}</span>
              <span class="sub">
                {#if sg.category}<span>#{sg.category}</span>{/if}
                {#if sg.account}<span>{sg.account}</span>{/if}
                <span>{sg.months}개월</span>
              </span>
            </span>
            <span class="amount">{won(sg.amount)}</span>
            <button class="add-btn" onclick={() => (editing = { index: null, value: {
              name: sg.name, amount: sg.amount, day: sg.day, keyword: sg.keyword, category: sg.category, account: sg.account,
            } })}>추가</button>
            <button class="hide-btn" onclick={() => hide(sg)} title="고정지출이 아니면 숨깁니다 (vault 설정에 기억)"
              aria-label="{sg.name} 후보 숨기기"><Icon name="close" size={13} /></button>
          </li>
        {/each}
      </ul>
    </section>
  {/if}
</div>

{#if editing && overview}
  <FixedEditor value={editing.value} isNew={editing.index === null} {categories}
    onclose={() => (editing = null)} onsave={saveItem}
    ondelete={editing.index === null ? undefined : deleteItem} />
{/if}

<style>
  .fixed {
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
    margin: -6px 0 6px;
    font-size: 12px;
    color: var(--text-faint);
  }
  .hint.warn {
    margin: 0 0 8px;
    color: var(--danger);
  }
  .link {
    margin-left: 6px;
    border: 0;
    background: none;
    padding: 0;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
  }
  .remind {
    margin-left: auto;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text-muted);
    font: inherit;
    font-size: 12px;
    padding: 3px 6px;
  }
  .hide-btn {
    display: inline-flex;
    border: 0;
    background: none;
    color: var(--text-faint);
    padding: 4px;
    border-radius: 5px;
    cursor: pointer;
  }
  .hide-btn:hover {
    background: var(--hover);
    color: var(--text);
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
    color: var(--text);
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
  }
  .label,
  .tile .sub {
    font-size: 12px;
    color: var(--text-muted);
  }
  .value {
    font-size: 20px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }
  .panel {
    padding: 14px 16px;
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  .panel-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  h2 {
    margin: 0 0 10px;
    font-size: 14px;
    font-weight: 600;
  }
  .panel-head h2 {
    margin: 0;
  }
  .n {
    font-weight: 400;
    color: var(--text-faint);
  }
  .empty {
    margin: 10px 0 0;
    font-size: 13px;
    color: var(--text-faint);
  }
  .rows {
    list-style: none;
    margin: 6px 0 0;
    padding: 0;
  }
  .rows li {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .rows li + li {
    border-top: 1px solid var(--border);
  }
  .row,
  .suggest {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
    padding: 9px 6px;
  }
  .row {
    border: 0;
    border-radius: 7px;
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .row:hover {
    background: var(--hover);
  }
  .day {
    flex: none;
    width: 44px;
    text-align: center;
    font-size: 11px;
    color: var(--text-muted);
  }
  .day strong {
    font-size: 16px;
    color: var(--text);
    font-variant-numeric: tabular-nums;
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
  .main .sub {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 8px;
    font-size: 12px;
    color: var(--text-faint);
  }
  .kw {
    font-family: var(--font-mono);
    font-size: 11px;
  }
  .right {
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 3px;
  }
  .amount {
    font-size: 14px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .state {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 0 7px;
    border-radius: 9px;
    font-size: 11px;
    line-height: 18px;
    white-space: nowrap;
  }
  .state.paid {
    background: var(--accent-bg);
    color: var(--accent);
  }
  .state.soon {
    background: var(--bg-side);
    color: var(--text);
    border: 1px solid var(--border);
  }
  .state.late {
    background: var(--danger-bg);
    color: var(--danger);
  }
  .state.plan {
    background: var(--bg-side);
    color: var(--text-muted);
  }
  .source {
    flex: none;
    display: inline-flex;
    border: 0;
    background: none;
    color: var(--text-faint);
    padding: 6px;
    border-radius: 5px;
    cursor: pointer;
  }
  .source:hover {
    background: var(--hover);
    color: var(--text);
  }
  .add-btn {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 12px;
    padding: 4px 10px;
    cursor: pointer;
  }
  .add-btn:hover {
    background: var(--hover);
  }
  @media (max-width: 560px) {
    .tiles {
      grid-template-columns: 1fr;
    }
  }
</style>
