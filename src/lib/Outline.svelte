<script lang="ts">
  import { headings } from "./outline";

  interface Props {
    /** The note text as currently edited. */
    content: string;
    onjump: (line: number) => void;
  }

  let { content, onjump }: Props = $props();

  let list = $derived(headings(content));
  /** Indent relative to the note's top heading level. */
  let base = $derived(Math.min(...list.map((h) => h.level)));
</script>

{#if list.length}
  <ul>
    {#each list as h (h.line)}
      <li>
        <button style:padding-left="{10 + (h.level - base) * 14}px" class="h{h.level}"
          onclick={() => onjump(h.line)} title={h.text}>{h.text}</button>
      </li>
    {/each}
  </ul>
{:else}
  <p class="none">제목이 없습니다. <code># 제목</code> 줄을 쓰면 여기에 나옵니다.</p>
{/if}

<style>
  ul {
    list-style: none;
    margin: 0;
    padding: 4px 6px;
  }
  button {
    display: block;
    width: 100%;
    border: 0;
    background: none;
    color: var(--text-muted);
    font: inherit;
    font-size: 13px;
    text-align: left;
    padding: 4px 8px;
    border-radius: 4px;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  button:hover {
    background: var(--hover);
    color: var(--text);
  }
  .h1 {
    color: var(--text);
    font-weight: 600;
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
