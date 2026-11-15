<script lang="ts">
  import { getBacklinks, linkMention, unlinkedMentions, type Backlink, type Mention } from "./api";
  import Icon from "./Icon.svelte";

  interface Props {
    path: string;
    /** Bumped when vault files change; reloads the list. */
    version: number;
    onopen: (path: string) => void;
    /** Number of linking lines once loaded (shown by the parent's header). */
    count?: number | null;
    /** Called before rewriting another note (flushes unsaved edits). */
    beforewrite?: () => Promise<void>;
    onerror?: (message: string) => void;
  }

  let { path, version, onopen, count = $bindable(null), beforewrite, onerror }: Props = $props();

  // Unlinked mentions: searched only when the section is opened.
  let showMentions = $state(false);
  let mentions = $state<Mention[] | null>(null);
  let mseq = 0;
  $effect(() => {
    const target = path;
    version;
    if (!showMentions || !target) return;
    const s = ++mseq;
    unlinkedMentions(target)
      .then((r) => {
        if (s === mseq) mentions = r;
      })
      .catch(() => {
        if (s === mseq) mentions = [];
      });
  });
  const mentionCount = $derived(mentions?.reduce((n, m) => n + m.lines.length, 0) ?? null);

  async function link(m: Mention, l: Mention["lines"][number]) {
    try {
      await beforewrite?.();
      await linkMention(m.source, l.line, l.raw, l.term, path);
      // The vault watcher reloads the lists; drop this line meanwhile.
      mentions = (mentions ?? [])
        .map((x) => (x === m ? { ...x, lines: x.lines.filter((y) => y !== l) } : x))
        .filter((x) => x.lines.length > 0);
    } catch (e) {
      onerror?.(String(e));
    }
  }
  let links = $state<Backlink[]>([]);
  let loaded = $state(false);
  let seq = 0;

  $effect(() => {
    const target = path;
    version;
    const s = ++seq;
    getBacklinks(target)
      .then((result) => {
        if (s === seq) {
          links = result;
          loaded = true;
        }
      })
      .catch(() => {
        if (s === seq) links = [];
      });
  });

  let total = $derived(links.reduce((n, l) => n + l.lines.length, 0));
  $effect(() => {
    count = loaded ? total : null;
  });
</script>

<section class="backlinks" aria-label="백링크">
  {#if loaded && links.length === 0}
    <p class="none">이 노트를 가리키는 링크가 없습니다.</p>
  {/if}
  <ul>
    {#each links as link (link.source)}
      <li>
        <button class="source" onclick={() => onopen(link.source)} title={link.source}>{link.title}</button>
        {#each link.lines as l (l.line)}
          <button class="line" onclick={() => onopen(link.source)}>{l.text}</button>
        {/each}
      </li>
    {/each}
  </ul>

  <button class="mentions-head" onclick={() => (showMentions = !showMentions)} aria-expanded={showMentions}>
    <Icon name={showMentions ? "chevron-down" : "chevron-right"} size={13} />
    링크 없는 언급
    {#if showMentions && mentionCount !== null}<span class="n">{mentionCount}</span>{/if}
  </button>
  {#if showMentions}
    {#if mentions === null}
      <p class="none">찾는 중…</p>
    {:else if mentions.length === 0}
      <p class="none">이 노트의 이름(별칭 포함)을 링크 없이 쓴 노트가 없습니다.</p>
    {:else}
      <ul>
        {#each mentions as m (m.source)}
          <li>
            <button class="source" onclick={() => onopen(m.source)} title={m.source}>{m.title}</button>
            {#each m.lines as l (l.line)}
              <div class="mention">
                <button class="line" onclick={() => onopen(m.source)}>{l.text}</button>
                <button class="link-btn" onclick={() => link(m, l)} title="'{l.term}'을(를) [[링크]]로 바꾸기">링크</button>
              </div>
            {/each}
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</section>

<style>
  .backlinks {
    padding: 12px;
    font-size: 13px;
  }
  .none {
    margin: 0;
    color: var(--text-faint);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  button {
    display: block;
    width: 100%;
    border: 0;
    background: none;
    font: inherit;
    text-align: left;
    cursor: pointer;
    border-radius: 4px;
  }
  .source {
    color: var(--text);
    font-weight: 600;
    padding: 2px 6px;
  }
  .line {
    color: var(--text-muted);
    font-size: 12px;
    padding: 4px 6px 4px 12px;
    border-left: 2px solid var(--border);
    border-radius: 0 4px 4px 0;
    margin-top: 2px;
    overflow-wrap: anywhere;
  }
  button:hover {
    background: var(--hover);
  }
  .mentions-head {
    display: flex;
    align-items: center;
    gap: 5px;
    margin: 18px 0 6px;
    padding: 4px 6px;
    color: var(--text-muted);
    font-size: 12px;
    font-weight: 600;
  }
  .mentions-head .n {
    font-weight: 400;
    color: var(--text-faint);
  }
  .mention {
    display: flex;
    align-items: flex-start;
    gap: 4px;
  }
  .mention .line {
    flex: 1;
    min-width: 0;
  }
  .link-btn {
    flex: none;
    width: auto;
    margin-top: 4px;
    padding: 2px 8px;
    border: 1px solid var(--border);
    color: var(--text-muted);
    font-size: 11px;
  }
  .link-btn:hover {
    color: var(--accent);
    border-color: var(--accent);
  }
</style>
