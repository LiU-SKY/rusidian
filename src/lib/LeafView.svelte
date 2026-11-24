<script lang="ts" module>
  /** What a tab's content can ask the app to do. */
  export interface LeafActions {
    /** Open a file (in a new tab when `newTab`). */
    open: (path: string, newTab?: boolean) => void;
    followLink: (target: string, heading: string | null, source: string) => void;
    searchTag: (tag: string) => void;
    /** Save unsaved notes before something writes files. */
    beforewrite: () => Promise<void>;
    reveal: (path: string) => void;
    pickVault: () => void;
    /** Open a vault from the recent list. */
    openVault: (path: string) => void;
    quickSwitch: () => void;
    newNote: () => void;
    today: () => void;
    error: (message: string) => void;
    imageUrl: (source: string, target: string) => Promise<string | null>;
    /** Save files pasted or dropped into `note`; the texts to insert. */
    attach: (note: string, files: File[], pasted: boolean) => Promise<string[]>;
    /** Rename a note in place (`name` without ".md"). */
    rename: (path: string, name: string) => Promise<void>;
  }
</script>

<script lang="ts">
  import { tick } from "svelte";
  import { displayName, mediaKind, setProperties, vaultUrl, type Property, type VaultEntry } from "./api";
  import { textStats } from "./textStats";
  import Icon from "./Icon.svelte";
  import CalendarView from "./CalendarView.svelte";
  import CanvasView from "./CanvasView.svelte";
  import type { Doc } from "./docs.svelte";
  import Editor from "./Editor.svelte";
  import FinanceView from "./FinanceView.svelte";
  import GraphView from "./GraphView.svelte";
  import NoteHeader from "./NoteHeader.svelte";
  import { MOBILE } from "./platform";
  import Properties from "./Properties.svelte";
  import Reading from "./Reading.svelte";
  import { settings } from "./settings.svelte";
  import TaskView from "./TaskView.svelte";
  import type { Leaf } from "./workspace.svelte";

  interface Props {
    leaf: Leaf;
    /** The open text file (null for views, media and empty tabs). */
    doc: Doc | null;
    /** This tab is the focused one (its editor takes the keyboard). */
    active: boolean;
    vaultOpen: boolean;
    noteCount: number;
    version: number;
    actions: LeafActions;
    /** Shown on the start screen when no vault is open. */
    recentVaults?: VaultEntry[];
  }

  let { leaf, doc, active, vaultOpen, noteCount, version, actions, recentVaults = [] }: Props = $props();

  type EditorApi = {
    gotoLine: (line: number) => void;
    applyText: (text: string) => void;
    insertAtCursor: (text: string) => void;
    focus: () => void;
  };
  let editor: EditorApi | undefined = $state();
  let reader: { gotoLine: (line: number) => void } | undefined = $state();
  let propsPanel: { startAdding: () => void } | undefined = $state();

  const reading = $derived(leaf.reading && !!doc?.isNote);
  /** Text selected in the editor ("" for none): the status bar counts it instead of the note. */
  let selected = $state("");
  $effect(() => {
    doc; // another note (or a reload) starts with nothing selected
    selected = "";
  });
  const title = $derived(leaf.path ? displayName(leaf.path.split("/").pop()!) : "");

  export function gotoLine(line: number) {
    (reading ? reader : editor)?.gotoLine(line);
  }

  export function startAddingProperty() {
    propsPanel?.startAdding();
  }

  export function focus() {
    editor?.focus();
  }

  /** Insert text at the cursor, leaving reading view first. */
  export async function insertAtCursor(text: string) {
    if (leaf.reading) {
      leaf.reading = false;
      await tick();
    }
    editor?.insertAtCursor(text);
  }

  /**
   * Change the text from outside the editor (property edits, reading-view
   * checkboxes). Through the editor when there is one, so it can be undone.
   */
  export function replaceContent(next: string) {
    if (!doc || next === doc.content) return;
    if (editor && !reading) editor.applyText(next);
    else doc.edit(next);
  }

  export async function applyProperties(properties: Property[]) {
    if (!doc) return;
    // Show the edit at once; the parsed result follows shortly.
    if (doc.fm) doc.fm = { ...doc.fm, present: properties.length > 0, properties };
    try {
      replaceContent(await setProperties(doc.content, properties));
    } catch (e) {
      actions.error(String(e));
    }
  }

  /** Reading view checkbox: `[ ]` ↔ `[x]` on that line. */
  function toggleTaskLine(line: number) {
    if (!doc) return;
    const lines = doc.content.split("\n");
    const text = lines[line];
    if (text === undefined) return;
    lines[line] = text.replace(/\[( |x|X)\]/, (_, c: string) => (c === " " ? "[x]" : "[ ]"));
    replaceContent(lines.join("\n"));
  }

  const openHere = (path: string) => actions.open(path);
</script>

{#if !vaultOpen}
  <div class="empty">
    <h1>Rusidian</h1>
    <p>기존 Obsidian vault를 그대로 열 수 있습니다.</p>
    {#if recentVaults.length}
      <ul class="recent" aria-label="최근 vault">
        {#each recentVaults as v (v.path)}
          <li>
            <button onclick={() => actions.openVault(v.path)} disabled={!v.exists} title={v.path}>
              <Icon name="folder-open" size={16} />
              <span class="r-text">
                <span class="r-name">{v.name}</span>
                <span class="r-path">{v.exists ? v.path : `${v.path} (폴더 없음)`}</span>
              </span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
    <button class="primary" onclick={actions.pickVault}>{recentVaults.length ? "다른 폴더 열기" : "Vault 폴더 열기"}</button>
  </div>
{:else if leaf.kind === "tasks"}
  <TaskView {version} onopen={openHere} beforewrite={actions.beforewrite} />
{:else if leaf.kind === "calendar"}
  <CalendarView {version} onopen={openHere} beforewrite={actions.beforewrite} />
{:else if leaf.kind === "finance"}
  <FinanceView {version} onopen={openHere} beforewrite={actions.beforewrite} />
{:else if leaf.kind === "graph"}
  <GraphView {version} currentPath={leaf.path} bind:local={leaf.graphLocal} onopen={openHere} />
{:else if leaf.kind === "empty" || !leaf.path}
  <div class="empty">
    <p>새 탭</p>
    <div class="quick">
      <button onclick={actions.quickSwitch}>파일 열기 <kbd>Ctrl+O</kbd></button>
      <button onclick={actions.newNote}>새 노트 <kbd>Ctrl+N</kbd></button>
      <button onclick={actions.today}>오늘의 노트</button>
    </div>
    <p class="hint">노트 {noteCount}개 · Ctrl+P 명령 팔레트</p>
  </div>
{:else if doc}
  {#if doc.failed}
    <div class="empty"><p>{doc.failed}</p></div>
  {:else if doc.loaded && doc.path.toLowerCase().endsWith(".canvas")}
    {#key doc}
      <CanvasView {doc} onopen={(path, newTab) => actions.open(path, newTab)} onlink={actions.followLink} onerror={actions.error} />
    {/key}
  {:else if doc.loaded}
    {#if doc.isNote && settings.inlineTitle}
      <NoteHeader path={doc.path} saved={`${doc.dirty}:${version}`}
        onrename={(name) => actions.rename(doc!.path, name)}
        ondone={async () => {
          if (reading) return;
          await tick();
          editor?.focus();
        }} />
    {/if}
    {#if doc.isNote && doc.fm}
      <Properties bind:this={propsPanel} fm={doc.fm} onchange={applyProperties} />
    {/if}
    {#if reading}
      <Reading bind:this={reader} content={doc.content} path={doc.path}
        onlink={actions.followLink} ontag={actions.searchTag} ontoggle={toggleTaskLine} />
    {:else}
      {#key doc}
        <Editor bind:this={editor} doc={doc.content} onchange={(text) => doc?.edit(text)} autofocus={active && !MOBILE}
          onlink={(target, heading) => doc && actions.followLink(target, heading, doc.path)}
          livePreview={doc.isNote && settings.livePreview}
          imageUrl={(target) => actions.imageUrl(doc?.path ?? "", target)}
          onfiles={doc.isNote ? (files, pasted) => actions.attach(doc!.path, files, pasted) : undefined}
          onselection={(text) => (selected = text)} />
      {/key}
    {/if}
    {#if doc.isNote}
      {@const st = selected && !reading ? textStats(selected, false) : textStats(doc.content)}
      <div class="status" aria-live="off">
        {#if selected && !reading}<span class="sel">선택</span>{/if}
        <span>단어 {st.words.toLocaleString()}</span>
        <span title="공백 포함 (줄바꿈 제외)">글자 {st.chars.toLocaleString()}</span>
        <span title="공백 제외">공백 제외 {st.nonSpace.toLocaleString()}</span>
      </div>
    {/if}
  {/if}
{:else}
  {@const kind = mediaKind(leaf.path)}
  {@const url = vaultUrl(leaf.path)}
  {#if kind === "image"}
    <div class="media"><img src={url} alt={title} /></div>
  {:else if kind === "pdf"}
    <iframe class="media-frame" src={url} {title}></iframe>
  {:else if kind === "audio"}
    <div class="media"><audio src={url} controls></audio></div>
  {:else if kind === "video"}
    <!-- svelte-ignore a11y_media_has_caption -->
    <div class="media"><video src={url} controls></video></div>
  {:else}
    <div class="empty">
      <p>이 파일 형식은 미리 볼 수 없습니다.</p>
      <button class="primary" onclick={() => leaf.path && actions.reveal(leaf.path)}>탐색기에서 보기</button>
    </div>
  {/if}
{/if}

<style>
  .status {
    flex: none;
    display: flex;
    justify-content: flex-end;
    gap: 14px;
    padding: 3px 14px 4px;
    border-top: 1px solid var(--border);
    font-size: 11px;
    color: var(--text-faint);
    font-variant-numeric: tabular-nums;
    user-select: none;
  }
  .status .sel {
    color: var(--accent);
  }
  .empty {
    flex: 1;
    min-height: 0;
    overflow: auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 16px;
    color: var(--text-muted);
    text-align: center;
  }
  .empty p {
    margin: 0;
  }
  .empty h1 {
    margin: 0;
    color: var(--text);
  }
  .hint {
    color: var(--text-faint);
    font-size: 13px;
  }
  /* Recent vaults on the start screen. */
  .recent {
    list-style: none;
    margin: 12px 0 0;
    padding: 6px;
    width: min(440px, 100%);
    border: 1px solid var(--border);
    border-radius: 8px;
    text-align: left;
  }
  .recent button {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    border: 0;
    background: none;
    color: var(--text);
    font: inherit;
    padding: 8px 10px;
    border-radius: 6px;
    cursor: pointer;
    text-align: left;
  }
  .recent button:hover:not(:disabled) {
    background: var(--hover);
  }
  .recent button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .recent :global(.icon) {
    flex: none;
    color: var(--accent);
  }
  .r-text {
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .r-name {
    font-weight: 600;
    font-size: 14px;
  }
  .r-path {
    font-size: 12px;
    color: var(--text-faint);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .quick {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 8px 0;
  }
  .quick button {
    display: flex;
    justify-content: space-between;
    gap: 24px;
    min-width: 200px;
    border: 0;
    background: none;
    color: var(--accent);
    font: inherit;
    font-size: 14px;
    padding: 6px 10px;
    border-radius: 6px;
    cursor: pointer;
  }
  .quick button:hover {
    background: var(--hover);
  }
  kbd {
    font-family: inherit;
    font-size: 12px;
    color: var(--text-faint);
  }
  .primary {
    margin-top: 8px;
    border: 0;
    background: var(--accent);
    color: #fff;
    font: inherit;
    padding: 8px 18px;
    border-radius: 6px;
    cursor: pointer;
  }
  .media {
    flex: 1;
    min-height: 0;
    overflow: auto;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
  }
  .media img,
  .media video {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
  }
  .media-frame {
    flex: 1;
    min-height: 0;
    border: 0;
  }
</style>
