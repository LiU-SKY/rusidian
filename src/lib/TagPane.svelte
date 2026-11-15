<script lang="ts">
  import { listTags, type TagCount } from "./api";
  import Icon from "./Icon.svelte";

  interface Props {
    /** Bumped when vault files change; reloads the tags. */
    version: number;
    /** Clicked tag, without `#` (nested: "work/meeting"). */
    onsearch: (tag: string) => void;
    /** Number of distinct tags, for the pane header. */
    count?: number | null;
  }

  let { version, onsearch, count = $bindable(null) }: Props = $props();

  interface TagNode {
    name: string;
    full: string;
    /** Notes with exactly this tag (null: only nested tags exist). */
    count: number | null;
    /** For sorting by size: own count plus nested ones. */
    total: number;
    children: TagNode[];
  }

  let tags = $state<TagCount[]>([]);
  let loaded = $state(false);
  let byCount = $state(false);
  let open = $state<Record<string, boolean>>({});
  let seq = 0;

  $effect(() => {
    version;
    const s = ++seq;
    listTags()
      .then((t) => {
        if (s !== seq) return;
        tags = t;
        loaded = true;
      })
      .catch(() => {});
  });

  $effect(() => {
    count = loaded ? tags.length : null;
  });

  function build(list: TagCount[], sortByCount: boolean): TagNode[] {
    const root: TagNode[] = [];
    for (const { tag, count } of list) {
      const parts = tag.split("/").filter(Boolean);
      let level = root;
      let full = "";
      parts.forEach((part, i) => {
        full = full ? `${full}/${part}` : part;
        let node = level.find((n) => n.name.toLowerCase() === part.toLowerCase());
        if (!node) {
          node = { name: part, full, count: null, total: 0, children: [] };
          level.push(node);
        }
        node.total += count;
        if (i === parts.length - 1) node.count = (node.count ?? 0) + count;
        level = node.children;
      });
    }
    const sort = (nodes: TagNode[]) => {
      nodes.sort((a, b) => (sortByCount && b.total - a.total) || a.name.localeCompare(b.name, "ko"));
      nodes.forEach((n) => sort(n.children));
    };
    sort(root);
    return root;
  }

  let tree = $derived(build(tags, byCount));
</script>

{#snippet branch(nodes: TagNode[], depth: number)}
  {#each nodes as node (node.full)}
    <li>
      <div class="row" style:padding-left="{4 + depth * 14}px">
        {#if node.children.length}
          <button class="twisty" aria-label={open[node.full] ? "접기" : "펼치기"} aria-expanded={!!open[node.full]}
            onclick={() => (open[node.full] = !open[node.full])}>
            <Icon name={open[node.full] ? "chevron-down" : "chevron-right"} size={13} />
          </button>
        {:else}
          <span class="twisty-space"></span>
        {/if}
        <button class="tag" onclick={() => onsearch(node.full)} title="#{node.full} 검색">
          <span class="name">#{node.name}</span>
          {#if node.count !== null}<span class="count">{node.count}</span>{/if}
        </button>
      </div>
      {#if node.children.length && open[node.full]}
        <ul>{@render branch(node.children, depth + 1)}</ul>
      {/if}
    </li>
  {/each}
{/snippet}

{#if loaded && !tags.length}
  <p class="none">태그가 없습니다. 노트에 <code>#태그</code>를 쓰거나 <code>tags</code> 속성을 넣으세요.</p>
{:else if loaded}
  <div class="bar">
    <button class:on={!byCount} onclick={() => (byCount = false)}>이름순</button>
    <button class:on={byCount} onclick={() => (byCount = true)}>노트 수순</button>
  </div>
  <ul class="root">{@render branch(tree, 0)}</ul>
{/if}

<style>
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .root {
    padding: 0 6px 6px;
  }
  .bar {
    display: flex;
    gap: 4px;
    padding: 6px 12px;
  }
  .bar button {
    border: 0;
    background: none;
    color: var(--text-faint);
    font: inherit;
    font-size: 12px;
    padding: 2px 6px;
    border-radius: 4px;
    cursor: pointer;
  }
  .bar button.on {
    color: var(--text);
    background: var(--hover);
  }
  .row {
    display: flex;
    align-items: center;
  }
  .twisty,
  .twisty-space {
    flex: none;
    width: 20px;
    height: 20px;
  }
  .twisty {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 0;
    background: none;
    color: var(--text-faint);
    padding: 0;
    border-radius: 4px;
    cursor: pointer;
  }
  .twisty:hover {
    color: var(--text);
  }
  .tag {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    border: 0;
    background: none;
    color: var(--text-muted);
    font: inherit;
    font-size: 13px;
    text-align: left;
    padding: 4px 6px;
    border-radius: 4px;
    cursor: pointer;
  }
  .tag:hover {
    background: var(--hover);
    color: var(--text);
  }
  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    font-size: 11px;
    color: var(--text-faint);
  }
  .none {
    margin: 0;
    padding: 10px 14px;
    font-size: 13px;
    color: var(--text-faint);
  }
  code {
    font-family: var(--font-mono);
    background: var(--code-bg);
    padding: 0 4px;
    border-radius: 3px;
  }
</style>
