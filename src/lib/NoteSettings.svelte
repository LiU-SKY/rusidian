<script lang="ts">
  // Settings tab for Obsidian's core Templates and Daily notes plugins. They
  // live in the vault's `.obsidian/templates.json` and `daily-notes.json`,
  // so Obsidian and Rusidian share them. Each field saves when it is left
  // (or on Enter); the preview below it updates while typing.
  import { onMount } from "svelte";
  import {
    noteSettings, previewNoteSettings, setNoteSettings,
    type DailySettings, type NoteSettingsPreview, type TemplateSettings,
  } from "./api";
  import { localDate, localTime } from "./dates";

  interface Props {
    hasVault: boolean;
    /** The settings were saved (views that show daily notes should refresh). */
    onsaved?: () => void;
  }

  let { hasVault, onsaved }: Props = $props();

  let templates = $state<TemplateSettings>({ folder: "", dateFormat: "", timeFormat: "" });
  let daily = $state<DailySettings>({ folder: "", format: "", template: "" });
  let folders = $state<string[]>([]);
  let notes = $state<string[]>([]);
  let preview = $state<NoteSettingsPreview | null>(null);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  let saved = $state(false);
  /** What is on disk, so leaving an unchanged field doesn't rewrite the files. */
  let stored = "";

  const snapshot = () => JSON.stringify([templates, daily]);

  onMount(async () => {
    if (!hasVault) return;
    try {
      const s = await noteSettings();
      templates = s.templates;
      daily = s.daily;
      folders = s.folders;
      notes = s.notes;
      stored = snapshot();
      loaded = true;
    } catch (e) {
      error = String(e);
    }
  });
  // Closing the dialog (Esc) while a field still has focus: keep what was typed.
  onMount(() => () => {
    save();
  });

  // Preview while typing; stale answers are dropped.
  let seq = 0;
  $effect(() => {
    if (!loaded) return;
    const t = { ...templates };
    const d = { ...daily };
    const mine = ++seq;
    const timer = setTimeout(async () => {
      try {
        const p = await previewNoteSettings(t, d, localDate(), localTime());
        if (mine === seq) preview = p;
      } catch {
        // Keep the last preview.
      }
    }, 150);
    return () => clearTimeout(timer);
  });

  async function save() {
    if (!loaded || snapshot() === stored) return;
    try {
      await setNoteSettings(templates, daily);
      stored = snapshot();
      error = null;
      saved = true;
      setTimeout(() => (saved = false), 1500);
      onsaved?.();
    } catch (e) {
      error = String(e);
    }
  }

  function onkeydown(e: KeyboardEvent) {
    // Korean input: the Enter that ends a syllable is not "done".
    if (e.key === "Enter" && !e.isComposing) (e.currentTarget as HTMLInputElement).blur();
  }

  /** Templates to suggest: notes in the templates folder, else every note. */
  const templateChoices = $derived.by(() => {
    const folder = templates.folder.replace(/\\/g, "/").trim().replace(/^\/+|\/+$/g, "");
    const inside = folder ? notes.filter((n) => n.startsWith(folder + "/")) : [];
    return inside.length ? inside : notes;
  });
</script>

{#if !hasVault}
  <p class="empty">vault를 열면 템플릿과 데일리 노트를 설정할 수 있습니다.</p>
{:else}
  <section>
    <datalist id="ns-folders">
      {#each folders as f (f)}<option value={f}></option>{/each}
    </datalist>
    <datalist id="ns-templates">
      {#each templateChoices as n (n)}<option value={n}></option>{/each}
    </datalist>

    <p class="intro">
      Obsidian의 <b>템플릿</b>·<b>데일리 노트</b> 설정과 같은 파일(<code>.obsidian/templates.json</code>,
      <code>.obsidian/daily-notes.json</code>)에 저장되어 Obsidian과 함께 쓰입니다. 칸을 벗어나면 저장됩니다.
      {#if saved}<span class="saved">저장했습니다</span>{/if}
    </p>
    {#if error}<p class="error">{error}</p>{/if}

    <h3>템플릿</h3>
    <div class="row">
      <div class="what">
        <label class="name" for="ns-tfolder">템플릿 폴더</label>
        <span class="desc">템플릿 삽입(Alt+T)에서 고를 노트가 있는 폴더.</span>
        {#if preview?.template_folder_missing}<span class="warn">vault에 이 폴더가 없습니다.</span>{/if}
      </div>
      <input id="ns-tfolder" list="ns-folders" bind:value={templates.folder} onchange={save} {onkeydown}
        placeholder="예: 템플릿" spellcheck="false" disabled={!loaded} />
    </div>
    <div class="row">
      <div class="what">
        <label class="name" for="ns-tdate">날짜 형식</label>
        <span class="desc"><code>{"{{date}}"}</code> 자리에 들어갈 모양. 지금: <b>{preview?.template_date ?? "…"}</b></span>
      </div>
      <input id="ns-tdate" class="mono" bind:value={templates.dateFormat} onchange={save} {onkeydown}
        placeholder="YYYY-MM-DD" spellcheck="false" disabled={!loaded} />
    </div>
    <div class="row">
      <div class="what">
        <label class="name" for="ns-ttime">시간 형식</label>
        <span class="desc"><code>{"{{time}}"}</code> 자리에 들어갈 모양. 지금: <b>{preview?.template_time ?? "…"}</b></span>
      </div>
      <input id="ns-ttime" class="mono" bind:value={templates.timeFormat} onchange={save} {onkeydown}
        placeholder="HH:mm" spellcheck="false" disabled={!loaded} />
    </div>

    <h3>데일리 노트</h3>
    <div class="row">
      <div class="what">
        <label class="name" for="ns-dformat">날짜 형식</label>
        <span class="desc">
          노트 이름(<code>/</code>를 넣으면 폴더). <code>YYYY</code> 연도 · <code>MM</code> 월 · <code>DD</code> 일 ·
          <code>ddd</code> 요일 — 글자 그대로 쓸 부분은 <code>[Daily]</code>처럼 대괄호로.
        </span>
      </div>
      <input id="ns-dformat" class="mono" bind:value={daily.format} onchange={save} {onkeydown}
        placeholder="YYYY-MM-DD" spellcheck="false" disabled={!loaded} />
    </div>
    <div class="row">
      <div class="what">
        <label class="name" for="ns-dfolder">새 파일 위치</label>
        <span class="desc">비우면 vault 맨 위.</span>
        {#if preview?.daily_folder_missing}<span class="warn">vault에 이 폴더가 없습니다 (첫 데일리 노트를 만들 때 생김).</span>{/if}
      </div>
      <input id="ns-dfolder" list="ns-folders" bind:value={daily.folder} onchange={save} {onkeydown}
        placeholder="vault 맨 위" spellcheck="false" disabled={!loaded} />
    </div>
    <div class="row">
      <div class="what">
        <label class="name" for="ns-dtemplate">템플릿 파일</label>
        <span class="desc">새 데일리 노트를 이 노트 내용으로 시작. <code>{"{{date}}"}</code> <code>{"{{title}}"}</code> 등을 채움.</span>
        {#if preview?.daily_template_missing}<span class="warn">이 노트가 없습니다. 새 데일리 노트가 빈 채로 만들어집니다.</span>{/if}
      </div>
      <input id="ns-dtemplate" list="ns-templates" bind:value={daily.template} onchange={save} {onkeydown}
        placeholder="예: 템플릿/Daily" spellcheck="false" disabled={!loaded} />
    </div>
    <div class="result" class:bad={!!preview?.daily_error}>
      {#if preview?.daily_error}
        {preview.daily_error}
      {:else if preview?.daily_path}
        오늘의 노트: <code>{preview.daily_path}</code>
      {:else}
        …
      {/if}
    </div>
  </section>
{/if}

<style>
  section {
    flex: 1;
    min-height: 0;
    padding: 6px 20px 14px;
    overflow-y: auto;
  }
  .empty,
  .intro {
    font-size: 12px;
    line-height: 1.6;
    color: var(--text-faint);
  }
  .empty {
    padding: 20px;
    margin: 0;
  }
  .intro {
    margin: 8px 0 4px;
  }
  .saved {
    margin-left: 6px;
    color: var(--accent);
  }
  .error,
  .warn {
    color: var(--danger);
  }
  .error {
    font-size: 12px;
    margin: 4px 0;
  }
  .warn {
    font-size: 12px;
  }
  h3 {
    margin: 16px 0 0;
    font-size: 13px;
    color: var(--text-muted);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 10px 0;
    border-bottom: 1px solid var(--border);
  }
  .what {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .name {
    font-size: 14px;
  }
  .desc {
    font-size: 12px;
    color: var(--text-faint);
  }
  code {
    font-family: var(--font-mono);
    background: var(--code-bg);
    padding: 0 3px;
    border-radius: 3px;
  }
  input {
    width: 220px;
    flex: none;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    padding: 6px 8px;
    outline: none;
  }
  input.mono {
    font-family: var(--font-mono);
  }
  input:focus {
    border-color: var(--accent);
  }
  .result {
    margin-top: 10px;
    padding: 8px 10px;
    border-radius: 6px;
    background: var(--bg-side);
    font-size: 12px;
    line-height: 1.6;
    color: var(--text-muted);
  }
  .result.bad {
    background: var(--danger-bg);
    color: var(--danger);
  }
</style>
