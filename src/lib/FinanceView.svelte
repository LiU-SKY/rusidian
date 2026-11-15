<script lang="ts">
  import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
  import {
    addFinance, exportFinanceCsv, financeRange, getBudgets, getSavings, importCsvPreview, type Budgets, type EntryKind,
    type FinanceData, type FinanceItem, type ImportPreview, type ImportResult,
  } from "./api";
  import ImportDialog from "./ImportDialog.svelte";
  import PhoneDialog from "./PhoneDialog.svelte";
  import BudgetDialog from "./BudgetDialog.svelte";
  import { WEEKDAYS, addDays, localDate, localTime, parseYmd, ymd } from "./dates";
  import FinanceEditor from "./FinanceEditor.svelte";
  import FixedView from "./FixedView.svelte";
  import AssetsView from "./AssetsView.svelte";
  import TxDetail from "./TxDetail.svelte";
  import { childrenByParent, groupRecords, parentOf, recordKey, splitMemo, type TxGroup } from "./txGroups";
  import Icon from "./Icon.svelte";
  import { slider } from "./slider";

  interface Props {
    /** Bumped when vault files change; reloads. */
    version: number;
    onopen: (path: string) => void;
    /** Called before any file write so unsaved editor changes are flushed first. */
    beforewrite: () => Promise<void>;
  }

  let { version, onopen, beforewrite }: Props = $props();

  const TREND_MONTHS = 6;
  const UNCATEGORIZED = "미분류";

  let month = $state(localDate().slice(0, 7)); // "YYYY-MM"
  let data = $state<FinanceData | null>(null);
  let selectedCategory = $state<string | null>(null);
  let message = $state<{ kind: "ok" | "error"; text: string } | null>(null);
  let budgets = $state<Budgets>({ total: null, categories: {} });
  let editing = $state<FinanceItem | null>(null);
  let showBudgets = $state(false);
  let seq = 0;
  /** Which page of the finance view: the book or the fixed-expense list. */
  let tab = $state<"book" | "fixed" | "assets">("book");
  /** Categories that are savings, not spending (from `.obsidian/rusidian.json`). */
  let savings = $state<string[]>(["저축", "투자"]);
  /** Payments show with their settlements folded in (false: every record on its own row). */
  const COMBINE_KEY = "rusidian.finance.combine";
  let combine = $state(readCombine());
  function readCombine(): boolean {
    try {
      return localStorage.getItem(COMBINE_KEY) !== "0";
    } catch {
      return true;
    }
  }
  function setCombine(on: boolean) {
    combine = on;
    try {
      localStorage.setItem(COMBINE_KEY, on ? "1" : "0");
    } catch {
      // Remembered for this session only.
    }
  }
  /** The record whose detail is open. */
  let detailKey = $state<string | null>(null);

  // quick-add form
  let kind = $state<EntryKind>("expense");
  let amount = $state("");
  let memo = $state("");
  let category = $state("");
  let date = $state(localDate());

  // ------------------------------------------------------------- months
  function shiftMonth(m: string, delta: number): string {
    const d = parseYmd(`${m}-01`);
    d.setMonth(d.getMonth() + delta);
    return ymd(d).slice(0, 7);
  }
  function lastDay(m: string): string {
    return addDays(`${shiftMonth(m, 1)}-01`, -1);
  }
  const monthLabel = (m: string, withYear = true) => {
    const [y, mo] = m.split("-").map(Number);
    return withYear ? `${y}년 ${mo}월` : `${mo}월`;
  };
  const trendMonths = $derived(
    Array.from({ length: TREND_MONTHS }, (_, i) => shiftMonth(month, i - (TREND_MONTHS - 1))),
  );

  async function load() {
    const s = ++seq;
    try {
      const [result, b, sv] = await Promise.all([
        financeRange(`${trendMonths[0]}-01`, lastDay(month)), getBudgets(), getSavings().catch(() => savings),
      ]);
      if (s === seq) {
        data = result;
        budgets = b;
        savings = sv;
      }
    } catch (e) {
      if (s === seq) message = { kind: "error", text: String(e) };
    }
  }

  $effect(() => {
    version;
    month;
    load();
  });

  // --------------------------------------------------------- aggregates
  /** `저축` covers `저축/주택청약`. */
  const isSaving = (category: string | null) =>
    !!category && savings.some((s) => category === s || category.startsWith(`${s}/`));
  /** Money out to savings is not spending: "expense" here leaves it out, "saving" is only it. */
  const sum = (items: FinanceItem[], k: EntryKind | "saving") =>
    items.reduce((n, e) => {
      if (k === "income") return e.kind === "income" ? n + e.amount : n;
      if (e.kind !== "expense") return n;
      return isSaving(e.category) === (k === "saving") ? n + e.amount : n;
    }, 0);
  const inMonth = (m: string) => (data?.entries ?? []).filter((e) => e.date?.startsWith(m));

  const monthItems = $derived(inMonth(month));
  const income = $derived(sum(monthItems, "income"));
  const expense = $derived(sum(monthItems, "expense"));
  const saved = $derived(sum(monthItems, "saving"));
  const prevExpense = $derived(sum(inMonth(shiftMonth(month, -1)), "expense"));
  const expenseDelta = $derived(prevExpense > 0 ? (expense - prevExpense) / prevExpense : null);

  const categories = $derived.by(() => {
    const totals = new Map<string, number>();
    for (const e of monthItems) {
      if (e.kind !== "expense" || isSaving(e.category)) continue;
      const c = e.category ?? UNCATEGORIZED;
      totals.set(c, (totals.get(c) ?? 0) + e.amount);
    }
    // A category whose refunds cancel it out has nothing to show.
    return [...totals].filter(([, total]) => total !== 0).map(([name, total]) => ({ name, total })).sort((a, b) => b.total - a.total);
  });
  const maxCategory = $derived(Math.max(0, categories[0]?.total ?? 0));

  // ------------------------------------------------------------- budgets
  /** Spending in a budget category, nested categories included (#교통 covers #교통/택시). */
  const spentIn = (name: string) =>
    monthItems
      .filter((e) => e.kind === "expense" && e.category && (e.category === name || e.category.startsWith(`${name}/`))
        && (isSaving(name) || !isSaving(e.category)))
      .reduce((n, e) => n + e.amount, 0);

  interface BudgetRow { name: string; label: string; spent: number; budget: number }
  const budgetRows = $derived.by((): BudgetRow[] => {
    const rows: BudgetRow[] = [];
    if (budgets.total) rows.push({ name: "", label: "전체", spent: expense, budget: budgets.total });
    for (const [name, budget] of Object.entries(budgets.categories)) {
      rows.push({ name, label: `#${name}`, spent: spentIn(name), budget });
    }
    return rows;
  });
  /** How far through the shown month we are (only for the current month). */
  const pace = $derived.by(() => {
    if (month !== localDate().slice(0, 7)) return null;
    const today = parseYmd(localDate());
    return today.getDate() / parseYmd(lastDay(month)).getDate();
  });

  /** A statement picked for import, with its first reading. */
  let phoneSetup = $state(false);
  let importing = $state<{ file: string; preview: ImportPreview; password: string | null } | null>(null);
  /** A protected Excel file waiting for its password (never stored). */
  let askPassword = $state<{ file: string; error: string | null } | null>(null);
  let passwordInput = $state("");

  async function startImport() {
    const file = await openDialog({
      multiple: false,
      title: "거래 내역 파일 고르기",
      filters: [{ name: "거래 내역 (CSV, 엑셀, 농협 PDF)", extensions: ["csv", "txt", "tsv", "xlsx", "xls", "pdf"] }],
    });
    if (typeof file !== "string") return;
    await openStatement(file, null);
  }

  /** Read the file; a protected Excel file asks for its password first. */
  async function openStatement(file: string, password: string | null) {
    try {
      importing = { file, preview: await importCsvPreview(file, password), password };
    } catch (e) {
      const code = String(e);
      if (code === "PASSWORD_REQUIRED") askPassword = { file, error: null };
      else if (code === "WRONG_PASSWORD") askPassword = { file, error: "비밀번호가 맞지 않습니다." };
      else message = { kind: "error", text: code };
    }
  }

  function submitPassword(e: SubmitEvent) {
    e.preventDefault();
    if (!askPassword) return;
    const { file } = askPassword;
    const pw = passwordInput;
    passwordInput = "";
    askPassword = null;
    openStatement(file, pw);
  }

  function imported(r: ImportResult) {
    const extra = [r.duplicates && `이미 있는 ${r.duplicates}건`, r.skipped && `읽지 못한 줄 ${r.skipped}건`].filter(Boolean);
    message = {
      kind: "ok",
      text: `"${r.path}"에 기록 ${r.added}건을 가져왔습니다.${extra.length ? ` (${extra.join(", ")} 제외)` : ""}`,
    };
    load();
  }

  async function exportCsv() {
    const dest = await saveDialog({
      title: "CSV로 내보내기",
      defaultPath: `재무-${month}.csv`,
      filters: [{ name: "CSV", extensions: ["csv"] }],
    });
    if (!dest) return;
    try {
      await beforewrite();
      const rows = await exportFinanceCsv(dest, `${month}-01`, lastDay(month));
      message = { kind: "ok", text: `${monthLabel(month)} 기록 ${rows}건을 ${dest}에 저장했습니다.` };
    } catch (e) {
      message = { kind: "error", text: String(e) };
    }
  }

  const trend = $derived(trendMonths.map((m) => {
    const items = inMonth(m);
    return { month: m, income: sum(items, "income"), expense: sum(items, "expense") };
  }));

  const shownItems = $derived(
    monthItems.filter((e) => selectedCategory === null || (e.category ?? UNCATEGORIZED) === selectedCategory
      && (selectedCategory === UNCATEGORIZED ? e.kind === "expense" : true)),
  );
  /** Rows of the list, newest first, by day. */
  const days = $derived.by(() => {
    const all = data?.entries ?? [];
    const groups: TxGroup[] = combine
      ? groupRecords(shownItems, all)
      : shownItems.map((e) => ({ item: e, children: [], net: e.amount, orphan: e.parent_line !== null }));
    const out: { date: string; groups: TxGroup[]; spent: number }[] = [];
    for (const g of groups.slice().reverse()) {
      const d = g.item.date ?? "";
      let day = out.at(-1);
      if (!day || day.date !== d) {
        day = { date: d, groups: [], spent: 0 };
        out.push(day);
      }
      day.groups.push(g);
      if (g.item.kind === "expense") day.spent += combine ? g.net : g.item.amount;
    }
    return out;
  });
  const rowCount = $derived(days.reduce((n, d) => n + d.groups.length, 0));

  /** The open detail, rebuilt from fresh data after every reload. */
  const detail = $derived.by((): { group: TxGroup; parent: FinanceItem | null } | null => {
    if (!detailKey || !data) return null;
    const all = [...data.entries, ...data.undated];
    const item = all.find((e) => recordKey(e) === detailKey);
    if (!item) return null;
    const children = childrenByParent(all).get(detailKey) ?? [];
    const parent = parentOf(item, all) ?? null;
    return {
      group: { item, children, net: item.amount + children.reduce((n, c) => n + c.amount, 0), orphan: item.parent_line !== null },
      parent,
    };
  });
  /** Open a row: a settlement opens its payment when that is loaded. */
  function openDetail(e: FinanceItem) {
    const parent = data ? parentOf(e, data.entries) : undefined;
    detailKey = recordKey(parent ?? e);
  }

  // ------------------------------------------------------------ formats
  const won = (cents: number) =>
    `${(cents / 100).toLocaleString("ko-KR", { maximumFractionDigits: 2 })}원`;
  /** A record as money in (+) or out (−): a refund is money in, a deduction money out. */
  const signedWon = (e: FinanceItem) =>
    `${(e.kind === "expense") !== (e.amount < 0) ? "−" : "+"}${won(Math.abs(e.amount))}`;
  const compact = new Intl.NumberFormat("ko-KR", { notation: "compact", maximumFractionDigits: 1 });
  /** Share as "25%", or "<1%" for a non-zero sliver (never a misleading "0%"). */
  const pct = (part: number, whole: number) => {
    if (whole <= 0) return "";
    const p = (part / whole) * 100;
    return p > 0 && p < 1 ? "<1%" : `${Math.round(p)}%`;
  };
  const dayLabel = (d: string) => {
    if (!d) return "날짜 없음";
    const x = parseYmd(d);
    const rel = d === localDate() ? " · 오늘" : d === addDays(localDate(), -1) ? " · 어제" : "";
    return `${x.getMonth() + 1}월 ${x.getDate()}일 ${WEEKDAYS[x.getDay()]}요일${rel}`;
  };

  // --------------------------------------------------------- trend chart
  let chartWidth = $state(600);
  let hover = $state<number | null>(null);
  const CHART_H = 180;
  const M = { top: 12, right: 8, bottom: 24, left: 52 };
  const BAR_W = 18; // <= 24px thick
  const BAR_GAP = 2; // surface gap between touching bars

  /** A round axis maximum: 1, 2 or 5 × 10^n above the data. */
  function niceMax(v: number): number {
    if (v <= 0) return 100_00; // 100원, so an empty chart still has an axis
    const exp = Math.pow(10, Math.floor(Math.log10(v)));
    for (const m of [1, 2, 5, 10]) if (m * exp >= v) return m * exp;
    return 10 * exp;
  }
  const yMax = $derived(niceMax(Math.max(...trend.flatMap((t) => [t.income, t.expense]))));
  const innerW = $derived(Math.max(0, chartWidth - M.left - M.right));
  const innerH = CHART_H - M.top - M.bottom;
  const band = $derived(innerW / TREND_MONTHS);
  const y = (v: number) => M.top + innerH - (v / yMax) * innerH;
  const ticks = $derived([0, yMax / 2, yMax]);

  /** Column path: square at the baseline, 4px rounded top. */
  function column(x: number, value: number): string {
    const top = y(value);
    const bottom = M.top + innerH;
    const h = bottom - top;
    if (h <= 0) return "";
    const r = Math.min(4, h, BAR_W / 2);
    return `M${x},${bottom}V${top + r}Q${x},${top} ${x + r},${top}H${x + BAR_W - r}Q${x + BAR_W},${top} ${x + BAR_W},${top + r}V${bottom}Z`;
  }
  const groupX = (i: number) => M.left + band * i + band / 2;

  // ------------------------------------------------------------- actions
  async function add(e: SubmitEvent) {
    e.preventDefault();
    try {
      await beforewrite();
      const path = await addFinance(kind, amount, memo, category, date, localDate(), localTime());
      message = { kind: "ok", text: `"${path}"에 기록했습니다.` };
      amount = "";
      memo = "";
      if (date.slice(0, 7) !== month) month = date.slice(0, 7);
      await load();
    } catch (err) {
      message = { kind: "error", text: String(err) };
    }
  }

  function goToday() {
    month = localDate().slice(0, 7);
    selectedCategory = null;
  }
</script>

<section class="finance view-in" aria-label="재무">
  <header class="head">
    <h1>{monthLabel(month)}</h1>
    <div class="seg tabs" role="tablist" aria-label="재무 메뉴" use:slider={{ selector: ".on" }}>
      <button role="tab" aria-selected={tab === "book"} class:on={tab === "book"} onclick={() => (tab = "book")}>가계부</button>
      <button role="tab" aria-selected={tab === "fixed"} class:on={tab === "fixed"} onclick={() => (tab = "fixed")}>고정지출</button>
      <button role="tab" aria-selected={tab === "assets"} class:on={tab === "assets"} onclick={() => (tab = "assets")}>자산</button>
    </div>
    <div class="nav">
      <button onclick={() => { month = shiftMonth(month, -1); selectedCategory = null; }} aria-label="이전 달"><Icon name="chevron-left" /></button>
      <button onclick={goToday}>이번 달</button>
      <button onclick={() => { month = shiftMonth(month, 1); selectedCategory = null; }} aria-label="다음 달"><Icon name="chevron-right" /></button>
      <button onclick={startImport} title="은행·카드사·페이에서 받은 거래 내역(CSV, 엑셀, 암호 걸린 엑셀, 농협 PDF)을 기록으로 가져오기">내역 가져오기</button>
      <button onclick={() => (phoneSetup = true)} title="휴대폰에 온 은행·카드·페이 알림을 가계부에 자동으로 적기">휴대폰 알림</button>
      <button onclick={exportCsv} title="이 달의 기록을 CSV 파일로 저장 (엑셀에서 열림)">CSV 내보내기</button>
    </div>
  </header>

  <div class="scroll">
    {#if tab === "fixed"}
      <FixedView {month} {version} categories={data?.categories ?? []} {onopen} />
    {:else if tab === "assets"}
      <AssetsView {version} {onopen} {beforewrite} {savings} onsavings={(list) => (savings = list)} />
    {:else}
    <div class="view-in">
    <form class="add" onsubmit={add}>
      <div class="seg" role="radiogroup" aria-label="종류" use:slider={{ selector: ".on" }}>
        <button type="button" role="radio" aria-checked={kind === "expense"} class:on={kind === "expense"}
          onclick={() => (kind = "expense")}><Icon name="arrow-up-right" size={15} /> 지출</button>
        <button type="button" role="radio" aria-checked={kind === "income"} class:on={kind === "income"}
          onclick={() => (kind = "income")}><Icon name="arrow-down-left" size={15} /> 수입</button>
      </div>
      <input class="amount" bind:value={amount} inputmode="decimal" placeholder="금액" aria-label="금액" />
      <input class="memo" bind:value={memo} placeholder="내용" aria-label="내용" />
      <input class="cat" bind:value={category} list="finance-categories" placeholder="#분류" aria-label="분류" />
      <datalist id="finance-categories">
        {#each data?.categories ?? [] as c (c)}<option value={c}></option>{/each}
      </datalist>
      <input class="date" type="date" bind:value={date} aria-label="날짜" />
      <button class="submit" type="submit" disabled={!amount.trim()}>기록</button>
    </form>
    <p class="hint">오늘 데일리 노트에 <code>- 💸 12,000 점심 #식비</code> 형식으로 추가됩니다. 노트 어디에 직접 써도 집계됩니다.</p>

    {#if message}
      <p class={["message", message.kind]} role={message.kind === "error" ? "alert" : "status"}>
        {message.text}
        <button class="dismiss" onclick={() => (message = null)} aria-label="닫기"><Icon name="close" size={14} /></button>
      </p>
    {/if}

    <div class="tiles">
      <div class="tile">
        <span class="label">수입</span>
        <span class="value">{won(income)}</span>
      </div>
      <div class="tile">
        <span class="label">지출</span>
        <span class="value">{won(expense)}</span>
        {#if expenseDelta !== null}
          <span class={["delta", expenseDelta > 0 ? "bad" : "good"]}>
            <Icon name={expenseDelta > 0 ? "arrow-up" : "arrow-down"} size={12} /> 지난달보다 {Math.abs(Math.round(expenseDelta * 100))}% {expenseDelta > 0 ? "증가" : "감소"}
          </span>
        {/if}
      </div>
      <div class="tile">
        <span class="label">저축</span>
        <span class="value">{won(saved)}</span>
        <span class="sub" title="이 분류의 기록은 지출이 아니라 저축으로 셉니다. 자산 탭에서 바꿀 수 있습니다.">#{savings.join(" #")}</span>
      </div>
      <div class="tile">
        <span class="label">남은 돈</span>
        <span class="value">{income - expense - saved < 0 ? "−" : ""}{won(Math.abs(income - expense - saved))}</span>
        <span class="sub">수입 − 지출 − 저축</span>
      </div>
    </div>

    <section class="panel budget" aria-label="예산">
      <div class="panel-head">
        <h2>예산</h2>
        <button class="link-btn" onclick={() => (showBudgets = true)}>예산 설정</button>
      </div>
      {#if budgetRows.length === 0}
        <p class="empty">월 예산을 정하면 분류별로 얼마나 썼고 얼마 남았는지 보여줍니다.</p>
      {:else}
        <ul class="budget-rows">
          {#each budgetRows as b (b.name)}
            {@const ratio = b.spent / b.budget}
            {@const over = b.spent > b.budget}
            <li class="budget-row" class:over>
              <span class="b-name">{b.label}</span>
              <span class="b-track" title="{b.label}: {won(b.spent)} / {won(b.budget)}">
                <span class="b-bar" style:width="{Math.min(ratio, 1) * 100}%"></span>
                {#if pace !== null}<span class="b-pace" style:left="{pace * 100}%" title="오늘까지 기간의 {Math.round(pace * 100)}%"></span>{/if}
              </span>
              <span class="b-amount">{won(b.spent)} <span class="muted">/ {won(b.budget)}</span></span>
              <span class="b-status">
                {#if over}
                  <Icon name="arrow-up" size={12} /> {won(b.spent - b.budget)} 초과
                {:else}
                  {won(b.budget - b.spent)} 남음
                {/if}
              </span>
            </li>
          {/each}
        </ul>
        {#if pace !== null}<p class="pace-note"><span class="pace-key"></span> 이번 달 지난 기간 ({Math.round(pace * 100)}%) — 막대가 이 선을 넘으면 빠르게 쓰는 중</p>{/if}
      {/if}
    </section>

    <div class="panels">
      <section class="panel" aria-label="분류별 지출">
        <h2>분류별 지출</h2>
        {#if categories.length === 0}
          <p class="empty">이번 달 지출 기록이 없습니다.</p>
        {:else}
          <ul class="cats">
            {#each categories as c (c.name)}
              <li>
                <button class="cat-row" class:active={selectedCategory === c.name} aria-pressed={selectedCategory === c.name}
                  onclick={() => (selectedCategory = selectedCategory === c.name ? null : c.name)}
                  title="{c.name}: {won(c.total)} ({pct(c.total, expense)})">
                  <span class="cat-name">{c.name}</span>
                  <span class="cat-track"><span class="cat-bar" style:width="{maxCategory > 0 ? (Math.max(0, c.total) / maxCategory) * 100 : 0}%"></span></span>
                  <span class="cat-amount">{won(c.total)}</span>
                  <span class="cat-pct">{pct(c.total, expense)}</span>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </section>

      <section class="panel" aria-label="최근 {TREND_MONTHS}개월">
        <div class="panel-head">
          <h2>최근 {TREND_MONTHS}개월</h2>
          <span class="legend">
            <span class="key"><span class="swatch income"></span>수입</span>
            <span class="key"><span class="swatch expense"></span>지출</span>
          </span>
        </div>
        <div class="chart" bind:clientWidth={chartWidth}>
          <svg width={chartWidth} height={CHART_H} role="img"
            aria-label="최근 {TREND_MONTHS}개월 수입과 지출. 자세한 값은 아래 표로 보기에 있습니다.">
            {#each ticks as t (t)}
              <line class="grid" x1={M.left} x2={M.left + innerW} y1={y(t)} y2={y(t)} />
              <text class="tick" x={M.left - 6} y={y(t)} dy="0.32em" text-anchor="end">{compact.format(t / 100)}</text>
            {/each}
            {#each trend as t, i (t.month)}
              {@const cx = groupX(i)}
              <path class="bar income" d={column(cx - BAR_W - BAR_GAP / 2, t.income)} />
              <path class="bar expense" d={column(cx + BAR_GAP / 2, t.expense)} />
              <text class={["xlabel", { current: t.month === month }]} x={cx} y={CHART_H - 6} text-anchor="middle">
                {monthLabel(t.month, false)}
              </text>
              <rect class="hit" x={M.left + band * i} y={M.top} width={band} height={innerH}
                role="button" tabindex="0" aria-label="{monthLabel(t.month)}: 수입 {won(t.income)}, 지출 {won(t.expense)}"
                onmouseenter={() => (hover = i)} onmouseleave={() => (hover = null)}
                onfocus={() => (hover = i)} onblur={() => (hover = null)}
                onclick={() => { month = t.month; selectedCategory = null; }}
                onkeydown={(e) => { if (e.key === "Enter") { month = t.month; selectedCategory = null; } }} />
            {/each}
            <line class="axis" x1={M.left} x2={M.left + innerW} y1={M.top + innerH} y2={M.top + innerH} />
          </svg>
          {#if hover !== null}
            {@const t = trend[hover]}
            <div class="tooltip" style:left="{Math.min(Math.max(groupX(hover), 90), chartWidth - 90)}px">
              <strong>{monthLabel(t.month)}</strong>
              <span><span class="swatch income"></span>수입 {won(t.income)}</span>
              <span><span class="swatch expense"></span>지출 {won(t.expense)}</span>
              <span class="muted">합계 {t.income - t.expense < 0 ? "−" : ""}{won(Math.abs(t.income - t.expense))}</span>
            </div>
          {/if}
        </div>
        <details class="table-view">
          <summary>표로 보기</summary>
          <table>
            <thead><tr><th>월</th><th>수입</th><th>지출</th><th>합계</th></tr></thead>
            <tbody>
              {#each trend as t (t.month)}
                <tr><td>{monthLabel(t.month)}</td><td>{won(t.income)}</td><td>{won(t.expense)}</td>
                  <td>{t.income - t.expense < 0 ? "−" : ""}{won(Math.abs(t.income - t.expense))}</td></tr>
              {/each}
            </tbody>
          </table>
        </details>
      </section>
    </div>

    <section class="list" aria-label="거래 내역">
      <div class="list-head">
        <h2>
          거래 내역 <span class="n">{rowCount}</span>
          {#if selectedCategory}
            <button class="filter-chip" onclick={() => (selectedCategory = null)} aria-label="#{selectedCategory} 필터 해제">#{selectedCategory} <Icon name="close" size={11} /></button>
          {/if}
        </h2>
        <div class="seg small" role="radiogroup" aria-label="정산 보기" use:slider={{ selector: ".on" }}>
          <button role="radio" aria-checked={combine} class:on={combine} onclick={() => setCombine(true)}
            title="결제와 그 정산·환불을 한 줄로 (내 몫)">합쳐 보기</button>
          <button role="radio" aria-checked={!combine} class:on={!combine} onclick={() => setCombine(false)}
            title="모든 기록을 따로">따로 보기</button>
        </div>
      </div>
      {#if rowCount === 0}
        <p class="empty">기록이 없습니다.</p>
      {:else}
        {#each days as day (day.date)}
          <div class="day">
            <div class="day-head">
              <span>{dayLabel(day.date)}</span>
              {#if day.spent !== 0}<span class="day-total">지출 {won(day.spent)}</span>{/if}
            </div>
            <ul>
              {#each day.groups as g (recordKey(g.item))}
                {@const m = splitMemo(g.item.memo)}
                {@const back = g.net - g.item.amount}
                <li>
                  <button class={["txr", { settlement: g.item.parent_line !== null }]} onclick={() => openDetail(g.item)}>
                    <span class={["mark", g.item.kind, { minus: g.item.amount < 0 }]} aria-hidden="true">
                      <Icon name={g.item.parent_line !== null ? "link" : g.item.kind === "expense" ? "arrow-up-right" : "arrow-down-left"} size={14} />
                    </span>
                    <span class="txr-main">
                      <span class="txr-title">{m.title || (g.item.kind === "income" ? "수입" : "지출")}</span>
                      <span class="txr-sub">
                        {#if g.item.category}<span>#{g.item.category}</span>{/if}
                        {#if m.account}<span>{m.account}</span>{/if}
                        {#if g.children.length > 0}<span class="badge">정산 {g.children.length}건</span>{/if}
                        {#if g.item.parent_line !== null}<span class="badge muted">정산</span>{/if}
                      </span>
                    </span>
                    <span class="txr-right">
                      <span class={["txr-amount", { plus: (g.item.kind === "expense") === (g.net < 0) }]}>
                        {(g.item.kind === "expense") !== (g.net < 0) ? "−" : "+"}{won(Math.abs(g.net))}
                      </span>
                      {#if g.children.length > 0}
                        <span class="txr-was">{won(g.item.amount)}{back < 0 ? ` 중 ${won(-back)} 돌려받음` : ` + ${won(back)}`}</span>
                      {/if}
                    </span>
                    <Icon name="chevron-right" size={14} />
                  </button>
                </li>
              {/each}
            </ul>
          </div>
        {/each}
      {/if}
    </section>

    {#if data && data.undated.length > 0}
      <details class="undated">
        <summary>날짜를 알 수 없는 기록 {data.undated.length}개 (합계에서 제외) — 데일리 노트에 쓰거나 📅 날짜를 붙이세요</summary>
        <ul>
          {#each data.undated as e (`${e.path}:${e.line}`)}
            <li class="tx">
              <span class="tx-body"><span class="tx-memo">{e.memo}</span></span>
              <span class="tx-amount">{signedWon(e)}</span>
              <button class="source" onclick={() => onopen(e.path)} title={e.path}>{e.title}</button>
              <button class="tx-edit" onclick={() => (editing = e)} title="편집" aria-label="기록 편집"><Icon name="pencil" size={14} /></button>
            </li>
          {/each}
        </ul>
      </details>
    {/if}
    </div>
    {/if}
  </div>
</section>

{#if detail}
  <TxDetail group={detail.group} parent={detail.parent} onclose={() => (detailKey = null)}
    onedit={(e) => (editing = e)} {onopen} onshow={(e) => (detailKey = recordKey(e))}
    ondone={(text) => { message = { kind: "ok", text }; load(); }} {beforewrite} />
{/if}
{#if editing}
  <FinanceEditor entry={editing} categories={data?.categories ?? []} onclose={() => (editing = null)}
    {beforewrite} {onopen} ondone={(text) => { message = { kind: "ok", text }; load(); }} />
{/if}
{#if phoneSetup}
  <PhoneDialog {beforewrite} onclose={() => (phoneSetup = false)} onrecorded={load} />
{/if}
{#if importing}
  <ImportDialog file={importing.file} preview={importing.preview} password={importing.password} categories={data?.categories ?? []}
    onclose={() => (importing = null)} ondone={imported} {beforewrite} />
{/if}
{#if askPassword}
  <div class="pw-backdrop" role="presentation" onpointerdown={(e) => e.target === e.currentTarget && (askPassword = null)}>
    <div role="dialog" aria-modal="true" aria-labelledby="pw-title" tabindex="-1"
      onkeydown={(e) => { if (e.key === "Escape") { e.preventDefault(); askPassword = null; } e.stopPropagation(); }}>
    <form class="pw-dialog" onsubmit={submitPassword}>
      <h2 id="pw-title">암호가 걸린 파일</h2>
      <p class="pw-file" title={askPassword.file}>{askPassword.file.split(/[\\/]/).pop()}</p>
      <!-- svelte-ignore a11y_autofocus -->
      <input type="password" bind:value={passwordInput} placeholder="파일 비밀번호" autocomplete="off" autofocus aria-label="파일 비밀번호" />
      {#if askPassword.error}<p class="pw-error" role="alert">{askPassword.error}</p>{/if}
      <p class="pw-note">파일을 여는 데만 쓰고 저장하지 않습니다.</p>
      <div class="pw-actions">
        <button type="button" onclick={() => (askPassword = null)}>취소</button>
        <button type="submit" class="primary" disabled={!passwordInput}>열기</button>
      </div>
    </form>
    </div>
  </div>
{/if}
{#if showBudgets}
  <BudgetDialog {budgets} categories={data?.categories ?? []} onclose={() => (showBudgets = false)}
    onsaved={(b) => { budgets = b; message = { kind: "ok", text: "예산을 저장했습니다." }; }} />
{/if}

<style>
  .pw-backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: rgb(0 0 0 / 0.25);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .pw-dialog {
    width: min(360px, calc(100vw - 32px));
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 18px 20px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: 0 16px 48px rgb(0 0 0 / 0.25);
  }
  .pw-dialog h2 {
    margin: 0;
    font-size: 16px;
  }
  .pw-file,
  .pw-note {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
    overflow-wrap: anywhere;
  }
  .pw-note {
    color: var(--text-faint);
  }
  .pw-dialog input {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 14px;
    padding: 7px 9px;
    outline: none;
  }
  .pw-dialog input:focus {
    border-color: var(--accent);
  }
  .pw-error {
    margin: 0;
    color: var(--danger);
    font-size: 13px;
  }
  .pw-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .pw-actions button {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    padding: 6px 14px;
    cursor: pointer;
  }
  .pw-actions .primary {
    border-color: var(--accent);
    background: var(--accent);
    color: #fff;
  }
  .pw-actions .primary:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .finance {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 20px max(16px, calc((100% - 980px) / 2)) 0;
    /* chart roles — validated palette, slots 1 (blue) and 2 (orange) */
    --series-income: #2a78d6;
    --series-expense: #eb6834;
    --seq: #2a78d6;
    --grid: #e1e0d9;
    --axis: #c3c2b7;
    --good: #006300;
    --bad: #d03b3b;
  }
  :global(:root[data-theme="dark"]) .finance {
    --series-income: #3987e5;
    --series-expense: #d95926;
    --seq: #3987e5;
    --grid: #2c2c2a;
    --axis: #383835;
    --good: #0ca30c;
    --bad: #e66767;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px 16px;
  }
  .tabs {
    margin-right: auto;
  }
  .seg.tabs button {
    padding: 5px 14px;
    font-size: 14px;
  }
  h1 {
    margin: 0;
    font-size: 22px;
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
    margin-top: 14px;
    padding-bottom: 40px;
  }
  .add {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .add input {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 14px;
    padding: 7px 10px;
    outline: none;
    min-width: 0;
  }
  .add input:focus {
    border-color: var(--accent);
  }
  .amount {
    width: 120px;
    text-align: right;
  }
  .memo {
    flex: 1;
    min-width: 140px !important;
  }
  .cat {
    width: 120px;
  }
  .date {
    color-scheme: light dark;
  }
  .seg {
    display: flex;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  .seg button {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    border: 0;
    background: none;
    transition: color 0.2s;
    color: var(--text-muted);
    font: inherit;
    font-size: 13px;
    padding: 0 12px;
    cursor: pointer;
  }
  .seg button.on {
    /* the sliding highlight (lib/slider.ts) draws the background */
    color: var(--text);
    font-weight: 600;
  }
  .seg.small button {
    padding: 3px 10px;
    font-size: 12px;
  }
  .submit {
    border: 0;
    border-radius: 6px;
    background: var(--accent);
    color: #fff;
    font: inherit;
    padding: 0 16px;
    cursor: pointer;
  }
  .submit:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .hint {
    margin: 6px 0 0;
    font-size: 12px;
    color: var(--text-faint);
  }
  .hint code {
    font-family: var(--font-mono);
    background: var(--code-bg);
    border-radius: 3px;
    padding: 0 4px;
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
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 12px;
    margin-top: 18px;
  }
  .tile {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 14px 16px;
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  .label {
    font-size: 13px;
    color: var(--text-muted);
  }
  .value {
    font-size: 22px;
    font-weight: 600;
  }
  .tile .sub {
    font-size: 12px;
    color: var(--text-faint);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .delta {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 12px;
  }
  .delta.bad {
    color: var(--bad);
  }
  .delta.good {
    color: var(--good);
  }
  .panels {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
    gap: 12px;
    margin-top: 12px;
  }
  .panel {
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 14px 16px;
    min-width: 0;
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
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .n {
    font-weight: 400;
    color: var(--text-faint);
  }
  .empty {
    color: var(--text-faint);
    font-size: 13px;
    margin: 8px 0;
  }
  .cats,
  .list ul,
  .undated ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .cat-row {
    display: grid;
    grid-template-columns: minmax(60px, 110px) 1fr auto 36px;
    align-items: center;
    gap: 10px;
    width: 100%;
    border: 0;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 13px;
    text-align: left;
    padding: 6px 4px;
    border-radius: 4px;
    cursor: pointer;
  }
  .cat-row:hover {
    background: var(--hover);
  }
  .cat-row.active {
    background: var(--active);
  }
  .cat-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cat-track {
    height: 10px;
  }
  .cat-bar {
    display: block;
    height: 100%;
    min-width: 2px;
    background: var(--seq);
    border-radius: 0 4px 4px 0; /* square at the baseline, rounded data end */
  }
  .cat-amount {
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .cat-pct {
    font-size: 12px;
    color: var(--text-muted);
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .legend {
    display: flex;
    gap: 12px;
    font-size: 12px;
    color: var(--text-muted);
  }
  .key {
    display: flex;
    align-items: center;
    gap: 5px;
  }
  .swatch {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 2px;
    margin-right: 4px;
    vertical-align: middle;
  }
  .swatch.income {
    background: var(--series-income);
  }
  .swatch.expense {
    background: var(--series-expense);
  }
  .chart {
    position: relative;
    width: 100%;
  }
  svg {
    display: block;
    overflow: visible;
  }
  .grid {
    stroke: var(--grid);
    stroke-width: 1;
  }
  .axis {
    stroke: var(--axis);
    stroke-width: 1;
  }
  .tick,
  .xlabel {
    font-size: 11px;
    fill: var(--text-faint);
    font-variant-numeric: tabular-nums;
  }
  .xlabel.current {
    fill: var(--text);
    font-weight: 600;
  }
  .bar.income {
    fill: var(--series-income);
  }
  .bar.expense {
    fill: var(--series-expense);
  }
  .hit {
    fill: transparent;
    cursor: pointer;
    outline: none;
  }
  .hit:hover,
  .hit:focus-visible {
    fill: var(--hover);
    fill-opacity: 0.5;
  }
  .tooltip {
    position: absolute;
    top: 0;
    transform: translateX(-50%);
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    box-shadow: 0 4px 12px rgb(0 0 0 / 0.12);
    font-size: 12px;
    pointer-events: none;
    white-space: nowrap;
  }
  .muted {
    color: var(--text-muted);
  }
  .table-view {
    margin-top: 8px;
    font-size: 12px;
    color: var(--text-muted);
  }
  .table-view summary {
    cursor: pointer;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    margin-top: 6px;
    font-variant-numeric: tabular-nums;
  }
  th,
  td {
    text-align: right;
    padding: 3px 6px;
    border-bottom: 1px solid var(--border);
  }
  th:first-child,
  td:first-child {
    text-align: left;
  }
  .list {
    margin-top: 20px;
  }
  .list-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-bottom: 4px;
  }
  .list-head h2 {
    margin: 0;
  }
  .day + .day {
    margin-top: 6px;
  }
  .day-head {
    position: sticky;
    top: 0;
    z-index: 1;
    display: flex;
    justify-content: space-between;
    gap: 8px;
    padding: 10px 8px 4px;
    background: var(--bg);
    border-bottom: 1px solid var(--border);
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
  }
  .day-total {
    font-weight: 400;
    font-variant-numeric: tabular-nums;
  }
  .txr {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 9px 8px;
    border: 0;
    border-radius: 7px;
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .txr:hover {
    background: var(--hover);
  }
  .txr:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .txr > :global(.icon:last-child) {
    flex: none;
    color: var(--text-faint);
    opacity: 0;
  }
  .txr:hover > :global(.icon:last-child),
  .txr:focus-visible > :global(.icon:last-child) {
    opacity: 1;
  }
  .mark {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    background: var(--bg-side);
    color: var(--text-muted);
  }
  .mark.income {
    color: var(--series-income);
  }
  .mark.expense {
    color: var(--series-expense);
  }
  .txr.settlement .mark,
  .mark.minus {
    color: var(--text-faint);
  }
  .txr-main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .txr-title {
    font-size: 14px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .txr-sub {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 8px;
    font-size: 12px;
    color: var(--text-faint);
  }
  .badge {
    padding: 0 6px;
    border-radius: 9px;
    background: var(--accent-bg);
    color: var(--accent);
    font-size: 11px;
  }
  .badge.muted {
    background: var(--bg-side);
    color: var(--text-muted);
  }
  .txr-right {
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 2px;
  }
  .txr-amount {
    font-size: 14px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .txr-amount.plus {
    color: var(--series-income);
  }
  .txr-was {
    font-size: 11px;
    color: var(--text-faint);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .filter-chip {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    border: 0;
    background: var(--accent-bg);
    color: var(--accent);
    border-radius: 10px;
    font: inherit;
    font-size: 12px;
    font-weight: 400;
    padding: 1px 8px;
    cursor: pointer;
  }
  .tx {
    display: flex;
    align-items: baseline;
    gap: 12px;
    padding: 7px 8px;
    border-radius: 6px;
    font-size: 14px;
  }
  .tx:hover {
    background: var(--hover);
  }
  .tx-date {
    width: 76px;
    flex-shrink: 0;
    font-size: 12px;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
  }
  .tx-body {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: 8px;
    align-items: baseline;
  }
  .tx-memo {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tx-cat {
    font-size: 12px;
    color: var(--text-muted);
    white-space: nowrap;
  }
  .tx-amount {
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .source {
    border: 0;
    background: none;
    padding: 0;
    font: inherit;
    font-size: 12px;
    color: var(--text-faint);
    text-decoration: underline;
    text-decoration-color: var(--border);
    cursor: pointer;
    white-space: nowrap;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .source:hover {
    color: var(--accent);
  }
  .undated {
    margin-top: 20px;
    font-size: 13px;
    color: var(--text-muted);
  }
  .undated summary {
    cursor: pointer;
  }
  .tx-edit {
    align-self: center;
    display: inline-flex;
    border: 0;
    background: none;
    color: var(--text-faint);
    padding: 3px;
    border-radius: 4px;
    cursor: pointer;
    opacity: 0;
  }
  .tx:hover .tx-edit,
  .tx-edit:focus-visible {
    opacity: 1;
  }
  .tx-edit:hover {
    color: var(--text);
    background: var(--active);
  }
  /* ---------------------------------------------------------------- budget */
  .budget {
    margin-top: 16px;
  }
  .link-btn {
    border: 0;
    background: none;
    color: var(--accent);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
    padding: 0;
  }
  .budget-rows {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .budget-row {
    display: grid;
    grid-template-columns: 96px minmax(80px, 1fr) auto 110px;
    align-items: center;
    gap: 12px;
    font-size: 13px;
  }
  .b-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* The track is the budget (100%); the bar is what was spent. */
  .b-track {
    position: relative;
    height: 10px;
    border-radius: 0 4px 4px 0;
    background: color-mix(in srgb, var(--seq) 14%, transparent);
  }
  .b-bar {
    display: block;
    height: 100%;
    min-width: 2px;
    background: var(--seq);
    border-radius: 0 4px 4px 0;
  }
  .budget-row.over .b-bar {
    background: var(--bad);
  }
  .b-pace,
  .pace-key {
    position: absolute;
    top: -3px;
    bottom: -3px;
    width: 2px;
    background: var(--text-muted);
  }
  .b-amount {
    font-variant-numeric: tabular-nums;
    text-align: right;
    white-space: nowrap;
  }
  .b-status {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    justify-content: flex-end;
    font-size: 12px;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .budget-row.over .b-status {
    color: var(--bad);
  }
  .pace-note {
    margin: 10px 0 0;
    font-size: 12px;
    color: var(--text-faint);
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .pace-key {
    position: static;
    display: inline-block;
    height: 12px;
  }
</style>
