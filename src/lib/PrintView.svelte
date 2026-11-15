<script lang="ts">
  import { displayName } from "./api";
  import { bodyOf } from "./outline";
  import Reading from "./Reading.svelte";

  interface Props {
    path: string;
    content: string;
  }

  let { path, content }: Props = $props();
  let reader: { settled: () => Promise<void> } | undefined = $state();

  const name = $derived(displayName(path.split("/").pop()!));
  /** The note's name goes on top unless it already starts with that heading. */
  const showTitle = $derived.by(() => {
    const first = bodyOf(content).split(/\r?\n/).find((l) => l.trim() !== "");
    return first?.trim() !== `# ${name}`;
  });

  /** Resolves when everything in the note has loaded. */
  export async function settled() {
    await reader?.settled();
  }

  const none = () => {};
</script>

<!-- Off screen until printed; only this shows on paper. -->
<div class="print-root" aria-hidden="true">
  {#if showTitle}<h1 class="print-title">{name}</h1>{/if}
  <Reading bind:this={reader} {content} {path} print onlink={none} ontag={none} ontoggle={none} />
</div>

<style>
  .print-root {
    /* Printed in light colours whatever the app's theme. */
    color-scheme: light;
    /* Keep highlight and code backgrounds on paper. */
    print-color-adjust: exact;
    -webkit-print-color-adjust: exact;
    --bg: #ffffff;
    --bg-side: #f6f6f6;
    --text: #1a1a1a;
    --text-muted: #4a4a4a;
    --text-faint: #808080;
    --border: #d6d6d6;
    --accent: #5b3cc4;
    --accent-bg: #7c5cdb1a;
    --code-bg: #f2f2f2;
    --highlight-bg: #fff3a3;
    --danger-bg: #fdecec;
    --danger: #b3261e;
    --editor-font-size: 15px;
    --line-width: none;
    position: fixed;
    top: 0;
    left: -10000px;
    width: 700px;
    background: var(--bg);
    color: var(--text);
    font-family: var(--font-text);
  }
  .print-title {
    margin: 0 0 16px;
    font-size: 26px;
    line-height: 1.3;
  }
  .print-root :global(.reading-scroll) {
    overflow: visible;
  }
  .print-root :global(.reading) {
    padding: 0;
    max-width: none;
  }
  /* Pages break between blocks, not through them. */
  .print-root :global(:is(h1, h2, h3, h4, h5, h6)) {
    break-after: avoid;
  }
  .print-root :global(:is(pre, blockquote, table, img, .mermaid-block, .math-block, .callout, .embed-note, li)) {
    break-inside: avoid;
  }
  .print-root :global(a.external-link::after) {
    display: none;
  }
  .print-root :global(.embed-pdf) {
    display: none;
  }

  @page {
    margin: 16mm 15mm;
  }
  @media print {
    /* Everything but the note is hidden on paper. */
    :global(body:has(.print-root) *:not(.print-root, .print-root *, :has(.print-root))) {
      display: none !important;
    }
    :global(html:has(.print-root)),
    :global(body:has(.print-root)) {
      height: auto !important;
      overflow: visible !important;
      background: #fff !important;
    }
    .print-root {
      position: static;
      width: auto;
    }
  }
</style>
