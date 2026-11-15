<script lang="ts">
  import { onMount } from "svelte";

  interface Props {
    /** Name as shown (notes without ".md"). */
    initial: string;
    /** Called once: with the new name, or null when cancelled / unchanged. */
    ondone: (name: string | null) => void;
    indent: number;
  }

  let { initial, ondone, indent }: Props = $props();
  let input: HTMLInputElement | undefined = $state();
  let finished = false;

  onMount(() => {
    if (!input) return;
    input.focus();
    // Select the name without its extension, like Explorer does.
    const dot = initial.lastIndexOf(".");
    input.setSelectionRange(0, dot > 0 ? dot : initial.length);
  });

  function finish(save: boolean) {
    if (finished) return;
    finished = true;
    const name = input?.value.trim() ?? "";
    ondone(save && name && name !== initial ? name : null);
  }

  /** Enter pressed while a Korean syllable was still composing. */
  let enterWhileComposing = false;

  function onkeydown(e: KeyboardEvent) {
    e.stopPropagation();
    // The IME owns keys mid-composition; save once it hands over the last syllable.
    if (e.isComposing) {
      if (e.key === "Enter" || e.keyCode === 13) enterWhileComposing = true;
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      finish(true);
    } else if (e.key === "Escape") {
      e.preventDefault();
      finish(false);
    }
  }
</script>

<input bind:this={input} class="rename" value={initial} style:margin-left="{indent}px"
  style:width="calc(100% - {indent + 4}px)"
  {onkeydown} onblur={() => finish(true)} aria-label="새 이름" spellcheck="false"
  oncompositionend={() => {
    if (enterWhileComposing) setTimeout(() => finish(true));
  }} />

<style>
  .rename {
    display: block;
    box-sizing: border-box;
    margin-top: 1px;
    margin-bottom: 1px;
    padding: 2px 6px;
    border: 1px solid var(--accent);
    border-radius: 4px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    outline: none;
  }
</style>
