<script lang="ts">
  import { outgoingLinks, type OutLink } from "./api";
  import Icon from "./Icon.svelte";

  interface Props {
    path: string;
    /** Bumped when vault files change; reloads. */
    version: number;
    onopen: (path: string) => void;
    /** A link to a note that doesn't exist yet: follow it (creates the note). */
    onfollow: (target: string) => void;
    count?: number | null;
  }

  let { path, version, onopen, onfollow, count = $bindable(null) }: Props = $props();
  let links = $state<OutLink[]>([]);
  let loaded = $state(false);
  let seq = 0;

  $effect(() => {
    const p = path;
    version;
    const s = ++seq;
    if (!p) {
      links = [];
      loaded = true;
      return;
    }
    outgoingLinks(p)
      .then((r) => {
        if (s === seq) {
          links = r;
          loaded = true;
        }
      })
      .catch(() => {
        if (s === seq) links = [];
      });
  });

  const existing = $derived(links.filter((l) => l.resolved));
  const missing = $derived(links.filter((l) => !l.resolved));
  $effect(() => {
    count = loaded ? links.length : null;
  });
  const name = (p: string) => (p.split("/").pop() ?? p).replace(/\.md$/i, "");
</script>

<section class="outgoing" aria-label="나가는 링크">
  {#if loaded && links.length === 0}
    <p class="none">이 노트에는 링크가 없습니다.</p>
  {/if}
  {#if existing.length}
    <ul>
      {#each existing as l (l.target)}
        <li>
          <button onclick={() => onopen(l.resolved!)} title={l.resolved}>
            <Icon name={l.embed ? "file" : "link"} size={13} />
            <span class="name">{name(l.resolved!)}</span>
            {#if l.lines.length > 1}<span class="n">{l.lines.length}</span>{/if}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
  {#if missing.length}
    <h3>아직 없는 노트</h3>
    <ul>
      {#each missing as l (l.target)}
        <li>
          <button class="missing" onclick={() => onfollow(l.target)} title="눌러서 만들기">
            <Icon name="file-plus" size={13} />
            <span class="name">{l.target}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .outgoing {
    padding: 12px;
    font-size: 13px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  button {
    display: flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    border: 0;
    border-radius: 5px;
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    padding: 5px 6px;
    cursor: pointer;
  }
  button:hover {
    background: var(--hover);
  }
  button > :global(.icon) {
    flex: none;
    color: var(--text-muted);
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .missing {
    color: var(--text-muted);
  }
  .n {
    color: var(--text-faint);
    font-size: 11px;
  }
  h3 {
    margin: 14px 6px 4px;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-faint);
  }
  .none {
    margin: 0;
    color: var(--text-faint);
  }
</style>
