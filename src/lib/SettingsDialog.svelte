<script lang="ts">
  import { onMount } from "svelte";
  import HotkeySettings, { type HotkeyCommand } from "./HotkeySettings.svelte";
  import NoteSettings from "./NoteSettings.svelte";
  import { hotkeys } from "./hotkeys.svelte";
  import Icon from "./Icon.svelte";
  import { slider } from "./slider";
  import { FONT_MAX, FONT_MIN, settings, type Theme } from "./settings.svelte";
  import { updates } from "./updates.svelte";

  interface Props {
    /** Commands for the hotkeys tab. */
    commands: HotkeyCommand[];
    hasVault: boolean;
    onclose: () => void;
    /** Template or daily-note settings were saved. */
    onnotesettings?: () => void;
  }

  let { commands, hasVault, onclose, onnotesettings }: Props = $props();
  let dialog: HTMLDivElement | undefined = $state();
  let tab = $state<"general" | "notes" | "hotkeys">("general");

  const THEMES: { value: Theme; label: string }[] = [
    { value: "system", label: "시스템" },
    { value: "light", label: "라이트" },
    { value: "dark", label: "다크" },
  ];

  onMount(() => {
    // Another device may have changed them since the vault opened.
    if (hasVault) hotkeys.load();
    updates.loadVersion();
    const before = document.activeElement as HTMLElement | null;
    dialog?.querySelector<HTMLElement>("[aria-checked='true']")?.focus();
    return () => {
      if (before?.isConnected) before.focus();
    };
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
    e.stopPropagation();
  }
</script>

<div class="backdrop" role="presentation" onpointerdown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="settings-title" tabindex="-1"
    bind:this={dialog} {onkeydown}>
    <header>
      <h2 id="settings-title">설정</h2>
      <div class="segmented tabs" role="tablist" aria-label="설정 항목" use:slider={{ selector: ".on", color: "var(--bg)", shadow: "0 1px 2px rgb(0 0 0 / 0.15)" }}>
        <button role="tab" aria-selected={tab === "general"} class:on={tab === "general"} onclick={() => (tab = "general")}>일반</button>
        <button role="tab" aria-selected={tab === "notes"} class:on={tab === "notes"} onclick={() => (tab = "notes")}>템플릿·데일리 노트</button>
        <button role="tab" aria-selected={tab === "hotkeys"} class:on={tab === "hotkeys"} onclick={() => (tab = "hotkeys")}>단축키</button>
      </div>
      <button class="close" onclick={onclose} aria-label="닫기"><Icon name="close" size={16} /></button>
    </header>

    {#if tab === "hotkeys"}
      <HotkeySettings {commands} {hasVault} />
    {:else if tab === "notes"}
      <NoteSettings {hasVault} onsaved={onnotesettings} />
    {:else}
    <section>
      <div class="row">
        <div class="what">
          <span class="name">테마</span>
          <span class="desc">시스템을 고르면 Windows 설정을 따릅니다.</span>
        </div>
        <div class="segmented" role="radiogroup" aria-label="테마" use:slider={{ selector: ".on", color: "var(--bg)", shadow: "0 1px 2px rgb(0 0 0 / 0.15)" }}>
          {#each THEMES as t (t.value)}
            <button role="radio" aria-checked={settings.theme === t.value} class:on={settings.theme === t.value}
              onclick={() => (settings.theme = t.value)}>{t.label}</button>
          {/each}
        </div>
      </div>

      <div class="row">
        <div class="what">
          <label class="name" for="live-preview">라이브 미리보기</label>
          <span class="desc">편집할 때 커서가 없는 줄의 마크다운 기호를 숨깁니다. 끄면 소스 모드.</span>
        </div>
        <input id="live-preview" class="switch" type="checkbox" role="switch" bind:checked={settings.livePreview} />
      </div>

      <div class="row">
        <div class="what">
          <label class="name" for="inline-title">노트 제목·날짜 보기</label>
          <span class="desc">노트 맨 위에 파일 이름(고치면 이름이 바뀜)과 만든 날·고친 날을 보여줍니다.</span>
        </div>
        <input id="inline-title" class="switch" type="checkbox" role="switch" bind:checked={settings.inlineTitle} />
      </div>

      <div class="row">
        <div class="what">
          <label class="name" for="notifications">알림</label>
          <span class="desc">앱이 열려 있을 때 <code>⏰ 날짜 시간</code> 알림, 시간이 적힌 할일의 시작, 고정지출 결제일(재무 → 고정지출에서 켬)을 Windows 알림으로 알려줍니다.</span>
        </div>
        <input id="notifications" class="switch" type="checkbox" role="switch" bind:checked={settings.notifications} />
      </div>

      <div class="row">
        <div class="what">
          <label class="name" for="font-size">편집기 글자 크기</label>
          <span class="desc">{settings.fontSize}px</span>
        </div>
        <input id="font-size" type="range" min={FONT_MIN} max={FONT_MAX} step="1" bind:value={settings.fontSize} />
      </div>

      <div class="row">
        <div class="what">
          <label class="name" for="readable">읽기 좋은 줄 너비</label>
          <span class="desc">끄면 편집기가 창 너비를 다 씁니다.</span>
        </div>
        <input id="readable" class="switch" type="checkbox" role="switch" bind:checked={settings.readableWidth} />
      </div>

      <div class="row">
        <div class="what">
          <span class="name">업데이트</span>
          <span class="desc" class:bad={updates.status === "error"}>
            {#if updates.status === "checking"}
              확인하는 중…
            {:else if updates.status === "latest"}
              최신 버전입니다 ({updates.version}).
            {:else if updates.status === "available" && updates.update}
              새 버전 {updates.update.version}이 있습니다 (지금 {updates.version}).
            {:else if updates.status === "installing"}
              {updates.progress === null ? "내려받는 중…" : `내려받는 중… ${Math.round(updates.progress * 100)}%`} 끝나면 다시 시작합니다.
            {:else if updates.status === "error"}
              확인하지 못했습니다: {updates.error}
            {:else}
              지금 {updates.version || "…"}. 앱을 켤 때 GitHub에서 새 버전을 확인합니다.
            {/if}
          </span>
        </div>
        {#if updates.status === "available"}
          <button class="action primary" onclick={() => updates.install()}>설치하고 다시 시작</button>
        {:else}
          <button class="action" onclick={() => updates.check()}
            disabled={updates.status === "checking" || updates.status === "installing"}>업데이트 확인</button>
        {/if}
      </div>
    </section>
    {/if}
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
  header {
    display: flex;
    align-items: center;
    padding: 14px 14px 10px 20px;
    border-bottom: 1px solid var(--border);
  }
  h2 {
    margin: 0;
    font-size: 16px;
  }
  .tabs {
    margin: 0 auto 0 16px;
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
    color: var(--text);
  }
  section {
    padding: 6px 20px 14px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 12px 0;
    border-bottom: 1px solid var(--border);
  }
  .row:last-child {
    border-bottom: 0;
  }
  .what {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .name {
    font-size: 14px;
  }
  .desc {
    font-size: 12px;
    color: var(--text-faint);
  }
  .desc code {
    font-family: var(--font-mono);
    background: var(--code-bg);
    padding: 0 3px;
    border-radius: 3px;
  }
  .segmented {
    display: flex;
    padding: 2px;
    background: var(--bg-side);
    border: 1px solid var(--border);
    border-radius: 7px;
  }
  .segmented button {
    border: 0;
    background: none;
    color: var(--text-muted);
    font: inherit;
    font-size: 13px;
    padding: 5px 12px;
    border-radius: 5px;
    cursor: pointer;
  }
  .segmented button {
    transition: color 0.2s;
  }
  .segmented button.on {
    color: var(--text);
  }
  .desc.bad {
    color: var(--danger);
  }
  .action {
    flex: none;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    padding: 6px 12px;
    cursor: pointer;
  }
  .action:hover:not(:disabled) {
    background: var(--hover);
  }
  .action:disabled {
    color: var(--text-faint);
    cursor: default;
  }
  .action.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: #fff;
  }
  .action.primary:hover {
    background: var(--accent);
    filter: brightness(1.08);
  }
  .action:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  input[type="range"] {
    width: 180px;
    accent-color: var(--accent);
  }
  .switch {
    appearance: none;
    width: 36px;
    height: 20px;
    margin: 0;
    border-radius: 10px;
    background: var(--border);
    position: relative;
    cursor: pointer;
    transition: background 0.15s;
  }
  .switch::after {
    content: "";
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.3);
    transition: left 0.15s;
  }
  .switch:checked {
    background: var(--accent);
  }
  .switch:checked::after {
    left: 18px;
  }
  .switch:focus-visible,
  .segmented button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
