<script lang="ts">
  import { onMount } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { phoneGet, phoneProcess, phoneReset, phoneSet, type PhoneInfo, type PhoneLogItem, type PhoneSetup } from "./api";
  import { localDate } from "./dates";
  import Icon from "./Icon.svelte";

  interface Props {
    /** Save unsaved notes before records are written. */
    beforewrite: () => Promise<void>;
    onclose: () => void;
    /** Records were written (the view reloads). */
    onrecorded: () => void;
  }

  let { beforewrite, onclose, onrecorded }: Props = $props();

  const DEFAULT: PhoneSetup = {
    enabled: true,
    file: "",
    note: "가계부/YYYY-MM",
    rules: { own: ["카카오페이"], atm_lab_cost: true, settle_below: 100_000 },
  };

  let info = $state<PhoneInfo | null>(null);
  let enabled = $state(true);
  let file = $state("");
  let note = $state(DEFAULT.note);
  let own = $state(DEFAULT.rules.own.join(", "));
  let atm = $state(true);
  let settleBelow = $state(DEFAULT.rules.settle_below);
  let error = $state<string | null>(null);
  let status = $state<string | null>(null);
  let busy = $state(false);
  let copied = $state(false);
  let dialog: HTMLDivElement | undefined = $state();

  onMount(() => {
    const before = document.activeElement as HTMLElement | null;
    load(true);
    dialog?.focus();
    return () => {
      if (before?.isConnected) before.focus();
    };
  });

  async function load(fill: boolean) {
    try {
      info = await phoneGet();
      if (fill) {
        const s = info.setup ?? DEFAULT;
        enabled = info.setup ? s.enabled : true;
        file = s.file;
        note = s.note || DEFAULT.note;
        own = s.rules.own.join(", ");
        atm = s.rules.atm_lab_cost;
        settleBelow = s.rules.settle_below;
      }
    } catch (e) {
      error = String(e);
    }
  }

  const setup = (): PhoneSetup => ({
    enabled,
    file: file.trim(),
    note: note.trim(),
    rules: {
      own: own.split(/[,\n]/).map((s) => s.trim()).filter(Boolean),
      atm_lab_cost: atm,
      settle_below: Math.max(0, Math.round(Number(settleBelow) || 0)),
    },
  });

  async function pickFile() {
    const picked = await openDialog({
      multiple: false,
      title: "휴대폰이 알림을 적는 파일 고르기",
      filters: [{ name: "텍스트 파일", extensions: ["txt", "log", "md"] }],
    });
    if (typeof picked === "string") file = picked;
  }

  async function save(close: boolean) {
    error = null;
    try {
      await phoneSet(setup());
      await load(false);
      if (close) onclose();
      else status = "저장했습니다.";
    } catch (e) {
      error = String(e);
    }
  }

  async function turnOff() {
    error = null;
    try {
      await phoneSet(null);
      onclose();
    } catch (e) {
      error = String(e);
    }
  }

  /** Save, then read the file now. */
  async function checkNow() {
    error = null;
    busy = true;
    try {
      await phoneSet(setup());
      await beforewrite();
      const res = await phoneProcess(localDate());
      status = res.log.length
        ? `새 알림 ${res.log.length}건 중 ${res.added}건을 적었습니다.`
        : "새 알림이 없습니다.";
      if (res.added) onrecorded();
      await load(false);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function readAgain() {
    error = null;
    try {
      await phoneReset();
      await checkNow();
    } catch (e) {
      error = String(e);
    }
  }

  async function copyTemplate() {
    if (!info) return;
    try {
      await navigator.clipboard.writeText(info.template);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      error = "복사할 수 없습니다. 아래 글을 직접 선택해 복사하세요.";
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
    e.stopPropagation();
  }

  const LABEL: Record<PhoneLogItem["status"], string> = {
    added: "기록함",
    duplicate: "이미 있음",
    skipped: "건너뜀",
    unreadable: "읽지 못함",
  };
</script>

<div class="backdrop" role="presentation" onpointerdown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="phone-title" tabindex="-1" bind:this={dialog} {onkeydown}>
    <header>
      <h2 id="phone-title">휴대폰 알림 자동 기록</h2>
      <span class="status" aria-live="polite">{status ?? ""}</span>
      <button class="close" onclick={onclose} aria-label="닫기"><Icon name="close" size={16} /></button>
    </header>

    <div class="body">
      <p class="help lead">
        휴대폰에 온 은행·카드·페이 알림을 휴대폰이 파일 하나에 적어 두면, 그 파일을 이 컴퓨터로 동기화해 Rusidian이 <b>30초마다</b> 읽고
        가계부에 적습니다. 알림 내용은 휴대폰과 이 컴퓨터 사이에서만 오가고, 외부 서버에는 저장되지 않습니다.
      </p>
      {#if error}<p class="warn" role="alert">{error}</p>{/if}

      <section>
        <label class="item"><input type="checkbox" bind:checked={enabled} /> <span><b>켜기</b> — 이 컴퓨터에서 이 vault로 알림을 받아 적습니다</span></label>
        <label class="field">알림 파일 (휴대폰에서 동기화되어 오는 파일)
          <span class="pick">
            <input class="mono" bind:value={file} placeholder="C:\Users\…\Sync\가계부 알림.txt" spellcheck="false" />
            <button onclick={pickFile}>고르기</button>
          </span>
        </label>
        {#if info?.setup?.file && info.setup.file === file.trim()}
          <p class={["help", { bad: !info.file_found }]}>
            {info.file_found ? "파일을 찾았습니다." : "아직 파일이 없습니다. 휴대폰에서 알림이 한 번 적히고 동기화되면 생깁니다."}
          </p>
        {/if}
        <label class="field">기록할 노트
          <input class="mono" bind:value={note} placeholder="가계부/YYYY-MM" spellcheck="false" />
        </label>
        <p class="help"><code>YYYY</code>·<code>MM</code>은 거래한 해와 달로 바뀝니다. 적힌 기록에는 <code>#자동</code> 태그가 붙어, 나중에 확인하고 고칠 수 있습니다.</p>
      </section>

      <section>
        <h3>기록 규칙</h3>
        <label class="field">내 계좌로 볼 이름 (쉼표로 구분)
          <input bind:value={own} placeholder="카카오페이, 본인 이름" spellcheck="false" />
        </label>
        <p class="help">보낸·받은 곳이 이 이름이면 내 계좌끼리 옮긴 돈으로 보고 적지 않습니다. 카카오페이 충전 알림도 적지 않습니다.
          <b>본인 이름</b>을 넣어 두면 내 다른 계좌로 보낸 돈도 빠집니다.</p>
        <label class="item"><input type="checkbox" bind:checked={atm} /> <span>ATM 현금 인출은 <b>연구실 비용</b> — 인건비에서 빼는 기록(<code>💰 -금액 #인건비</code>)으로 적기</span></label>
        <label class="field narrow">사람이 보낸 돈이 이 금액보다 적으면 식비 정산으로 적기 (원, 0이면 끔)
          <input type="number" min="0" step="10000" bind:value={settleBelow} />
        </label>
        <p class="help">같이 먹고 나눠 낸 돈은 <code>💸 -금액 ○○에게서 돌려받음 #식비/정산</code>으로 적혀 식비에서 빠집니다.
          분류는 같은 가게의 이전 기록을 따르고, 처음 보는 곳은 <code>#기타</code>로 적습니다.</p>
      </section>

      <details>
        <summary>휴대폰 준비하기 (안드로이드)</summary>
        <ol class="steps">
          <li><b>Syncthing</b>을 컴퓨터와 휴대폰에 설치합니다 (휴대폰은 Play 스토어의 <i>Syncthing-Fork</i>).
            <button class="link" onclick={() => openUrl("https://syncthing.net/downloads/")}>내려받기</button>
            휴대폰에 빈 폴더(예: <code>Rusidian</code>)를 만들어 컴퓨터와 공유하세요. 계정 없이 두 기기끼리 암호화해서 주고받고, LTE에서도 됩니다.
            이 폴더에는 알림 파일만 두세요.</li>
          <li><b>MacroDroid</b>에서 매크로를 만듭니다.
            <ul>
              <li>트리거: <i>알림 → 알림 수신</i>, 앱은 NH콕뱅크(또는 NH스마트뱅킹)·토스·카카오페이처럼 돈 알림이 오는 앱</li>
              <li>동작: <i>파일 → 파일에 쓰기</i>, 위 폴더의 <code>가계부 알림.txt</code>, <b>추가(덧붙이기)</b>, 쓸 내용은 아래 글</li>
            </ul>
            <div class="template">
              <pre>{info?.template ?? ""}</pre>
              <button onclick={copyTemplate}>{copied ? "복사됨" : "복사"}</button>
            </div>
            <p class="help">대괄호 이름이 MacroDroid 버전에 따라 다르면, 글 입력란의 <i>…(매직 텍스트)</i> 버튼에서 같은 뜻(연·월·일·시·분, 앱 이름, 알림 제목, 알림 내용)을 골라 넣으세요.
              첫 줄은 <code>@@</code>로 시작해야 합니다.</p>
          </li>
          <li>휴대폰 설정에서 MacroDroid에 <b>알림 접근</b>을 허용하고, MacroDroid와 Syncthing을 <b>배터리 최적화에서 빼</b> 두세요.</li>
          <li>컴퓨터의 Syncthing 폴더에 생긴 <code>가계부 알림.txt</code>를 위에서 고르고 저장합니다. 컴퓨터가 꺼져 있던 동안의 알림은 다음에 켤 때 한꺼번에 적힙니다.</li>
        </ol>
      </details>

      <section class="log">
        <div class="log-head">
          <h3>최근 알림</h3>
          <button onclick={checkNow} disabled={busy || !file.trim()}>{busy ? "읽는 중…" : "지금 확인"}</button>
          <button class="link" onclick={readAgain} disabled={busy || !file.trim()}
            title="처리한 알림 목록을 지우고 파일 전체를 다시 읽습니다. 이미 적힌 기록은 '이미 있음'으로 건너뜁니다.">처음부터 다시 읽기</button>
        </div>
        {#if info?.log.length}
          <ul>
            {#each info.log as item (item.key)}
              <li class={item.status}>
                <span class="chip">{LABEL[item.status]}</span>
                <span class="when">{item.when}</span>
                <span class="text selectable" title={item.text}><b>{item.app}</b> {item.text}</span>
                <span class="result selectable">{item.result}</span>
              </li>
            {/each}
          </ul>
        {:else}
          <p class="help">아직 읽은 알림이 없습니다.</p>
        {/if}
      </section>
    </div>

    <footer>
      {#if info?.setup}
        <button class="danger" onclick={turnOff} title="이 컴퓨터에서 휴대폰 알림 기록을 끕니다 (적힌 기록과 휴대폰의 파일은 그대로)">설정 지우기</button>
      {/if}
      <span class="spacer"></span>
      <button onclick={onclose}>닫기</button>
      <button class="primary" onclick={() => save(true)}>저장</button>
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
    width: min(720px, calc(100vw - 32px));
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
    align-items: center;
    gap: 12px;
    padding: 14px 14px 10px 20px;
    border-bottom: 1px solid var(--border);
  }
  h2 {
    margin: 0;
    font-size: 16px;
  }
  .status {
    flex: 1;
    min-width: 0;
    font-size: 12px;
    color: var(--text-faint);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .close {
    margin-left: auto;
    display: inline-flex;
    border: 0;
    background: none;
    color: var(--text-muted);
    padding: 5px;
    border-radius: 5px;
    cursor: pointer;
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 10px 20px 14px;
  }
  section {
    padding: 10px 0;
    border-bottom: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  h3 {
    margin: 0;
    font-size: 13px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 4px;
  }
  .field.narrow input {
    width: 160px;
  }
  .pick {
    display: flex;
    gap: 6px;
  }
  .pick input {
    flex: 1;
    min-width: 0;
  }
  input:not([type="checkbox"]) {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    padding: 6px 8px;
    outline: none;
  }
  input:focus {
    border-color: var(--accent);
  }
  .mono {
    font-family: var(--font-mono);
  }
  input[type="checkbox"] {
    accent-color: var(--accent);
  }
  .item {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    font-size: 13px;
    padding: 2px 0;
  }
  .item input {
    margin-top: 3px;
  }
  .help {
    margin: 0;
    font-size: 12px;
    line-height: 1.6;
    color: var(--text-faint);
  }
  .help.lead {
    margin-bottom: 4px;
    color: var(--text-muted);
  }
  .help b {
    color: var(--text-muted);
  }
  .help.bad {
    color: var(--danger);
  }
  code {
    font-family: var(--font-mono);
    background: var(--code-bg);
    padding: 0 3px;
    border-radius: 3px;
  }
  details {
    padding: 10px 0;
    border-bottom: 1px solid var(--border);
    font-size: 13px;
  }
  summary {
    cursor: pointer;
    font-weight: 600;
  }
  .steps {
    margin: 8px 0 0;
    padding-left: 20px;
    line-height: 1.7;
  }
  .steps li + li {
    margin-top: 6px;
  }
  .steps ul {
    margin: 2px 0;
    padding-left: 18px;
  }
  .template {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin: 6px 0;
  }
  .template pre {
    flex: 1;
    min-width: 0;
    margin: 0;
    padding: 8px 10px;
    border-radius: 6px;
    background: var(--code-bg);
    font-family: var(--font-mono);
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-all;
    user-select: text;
  }
  .log-head {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .log-head h3 {
    flex: 1;
  }
  .log-head .link {
    font-size: 12px;
  }
  .log ul {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 260px;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  .log li {
    display: grid;
    grid-template-columns: auto auto 1fr;
    gap: 2px 8px;
    align-items: baseline;
    padding: 6px 10px;
    font-size: 12px;
    border-bottom: 1px solid var(--border);
  }
  .log li:last-child {
    border-bottom: 0;
  }
  .chip {
    font-size: 11px;
    padding: 0 6px;
    border-radius: 9px;
    background: var(--hover);
    color: var(--text-muted);
    white-space: nowrap;
  }
  .added .chip {
    background: var(--accent);
    color: #fff;
  }
  .unreadable .chip {
    background: var(--danger-bg);
    color: var(--danger);
  }
  .when {
    color: var(--text-faint);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-muted);
  }
  .result {
    grid-column: 2 / -1;
    color: var(--text);
  }
  .skipped .result,
  .duplicate .result {
    color: var(--text-faint);
  }
  .warn {
    margin: 0 0 6px;
    padding: 8px 10px;
    border-radius: 6px;
    background: var(--danger-bg);
    color: var(--danger);
    font-size: 12px;
  }
  .link {
    border: 0;
    background: none;
    color: var(--accent);
    font: inherit;
    padding: 0;
    cursor: pointer;
  }
  .link:disabled {
    opacity: 0.5;
    cursor: default;
  }
  section button:not(.link),
  .template button,
  .pick button {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 12px;
    padding: 5px 12px;
    cursor: pointer;
    white-space: nowrap;
  }
  section button:disabled {
    opacity: 0.5;
    cursor: default;
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
  footer .danger {
    color: var(--danger);
  }
</style>
