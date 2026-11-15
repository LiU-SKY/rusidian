<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { BookmarkItem } from "./api";
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";

  interface Props {
    /** `.obsidian/bookmarks.json` items, shared with Obsidian. */
    items: BookmarkItem[];
    currentPath: string;
    onopen: (path: string) => void;
    /** A heading bookmark: open the note at that heading. */
    onheading: (path: string, heading: string) => void;
    onsearch: (query: string) => void;
    onreveal: (path: string) => void;
    onremove: (at: number[]) => void;
  }

  let { items, currentPath, onopen, onheading, onsearch, onreveal, onremove }: Props = $props();

  /** Collapsed groups, by their position. */
  let closed = $state<Record<string, boolean>>({});

  const base = (p: string) => p.split("/").pop() ?? p;
  const noExt = (p: string) => base(p).replace(/\.md$/i, "");

  function label(b: BookmarkItem): string {
    if (b.title) return b.title;
    switch (b.type) {
      case "file":
        return noExt(b.path ?? "");
      case "folder":
        return base(b.path ?? "") || "vault";
      case "heading":
      case "block":
        return `${noExt(b.path ?? "")} › ${(b.subpath ?? "").replace(/^#\^?/, "")}`;
      case "search":
        return b.query ?? "";
      case "url":
        return b.url ?? "";
      case "graph":
        return "그래프";
      default:
        return b.type;
    }
  }

  function icon(b: BookmarkItem): IconName {
    switch (b.type) {
      case "folder":
        return "folder-open";
      case "search":
        return "search";
      case "heading":
      case "block":
        return "outline";
      case "url":
        return "link";
      case "graph":
        return "graph";
      default:
        return "file";
    }
  }

  function activate(b: BookmarkItem, key: string) {
    switch (b.type) {
      case "group":
        closed[key] = !closed[key];
        break;
      case "file":
        if (b.path) onopen(b.path);
        break;
      case "heading":
      case "block":
        if (b.path) onheading(b.path, (b.subpath ?? "").replace(/^#/, ""));
        break;
      case "folder":
        if (b.path) onreveal(b.path);
        break;
      case "search":
        if (b.query) onsearch(b.query);
        break;
      case "url":
        if (b.url) openUrl(b.url).catch(() => {});
        break;
    }
  }
</script>

{#snippet list(nodes: BookmarkItem[], at: number[])}
  <ul>
    {#each nodes as b, i (`${at.join(".")}.${i}`)}
      {@const pos = [...at, i]}
      {@const key = pos.join(".")}
      <li>
        <div class="row" class:active={b.type === "file" && b.path === currentPath} style:padding-left="{6 + at.length * 14}px">
          <button class="open" onclick={() => activate(b, key)} title={b.path ?? b.query ?? b.url ?? ""}>
            {#if b.type === "group"}
              <Icon name={closed[key] ? "chevron-right" : "chevron-down"} size={13} />
            {:else}
              <Icon name={icon(b)} size={14} />
            {/if}
            <span class="name">{label(b)}</span>
            {#if b.type === "group"}<span class="n">{b.items?.length ?? 0}</span>{/if}
          </button>
          <button class="remove" onclick={() => onremove(pos)} title="북마크에서 빼기" aria-label="{label(b)} 북마크에서 빼기">
            <Icon name="close" size={12} />
          </button>
        </div>
        {#if b.type === "group" && b.items?.length && !closed[key]}
          {@render list(b.items, pos)}
        {/if}
      </li>
    {/each}
  </ul>
{/snippet}

<section class="bookmarks" aria-label="북마크">
  {#if items.length === 0}
    <p class="none">
      북마크가 없습니다. 노트 위의 <Icon name="bookmark" size={13} /> 버튼이나 명령 팔레트의 "북마크"로 추가하세요.
      Obsidian과 같은 <code>.obsidian/bookmarks.json</code>을 씁니다.
    </p>
  {:else}
    {@render list(items, [])}
  {/if}
</section>

<style>
  .bookmarks {
    font-size: 13px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .row {
    display: flex;
    align-items: center;
    border-radius: 5px;
  }
  .row:hover {
    background: var(--hover);
  }
  .row.active {
    background: var(--active);
  }
  .open {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 7px;
    border: 0;
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    padding: 5px 4px;
    cursor: pointer;
  }
  .open > :global(.icon) {
    flex: none;
    color: var(--text-muted);
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .n {
    color: var(--text-faint);
    font-size: 11px;
  }
  .remove {
    flex: none;
    display: inline-flex;
    border: 0;
    background: none;
    color: var(--text-faint);
    padding: 4px 6px;
    border-radius: 4px;
    cursor: pointer;
    opacity: 0;
  }
  .row:hover .remove,
  .remove:focus-visible {
    opacity: 1;
  }
  .remove:hover {
    color: var(--text);
    background: var(--active);
  }
  .none {
    margin: 8px 6px;
    color: var(--text-faint);
    line-height: 1.6;
  }
  code {
    font-family: var(--font-mono);
    font-size: 12px;
  }
</style>
