<script lang="ts">
  import { tick } from "svelte";
  import type { Frontmatter, Property, PropertyValue } from "./api";
  import Icon from "./Icon.svelte";

  interface Props {
    fm: Frontmatter;
    /** Called with the full new property list after any edit. */
    onchange: (properties: Property[]) => void;
  }

  let { fm, onchange }: Props = $props();
  let open = $state(true);
  /** Typing the key of a new property. */
  let adding = $state(false);
  let newKeyInput: HTMLInputElement | undefined = $state();

  /** Keys Obsidian treats as lists even when written as `a, b`. */
  const LIST_KEYS = new Set(["tags", "tag", "aliases", "alias", "cssclasses"]);
  const TAG_KEYS = new Set(["tags", "tag"]);
  const DATE = /^\d{4}-\d{2}-\d{2}$/;
  const DATETIME = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2})?$/;

  type Kind = "list" | "checkbox" | "number" | "date" | "datetime" | "object" | "text";

  const isObject = (v: PropertyValue) => typeof v === "object" && v !== null;

  function kindOf(key: string, v: PropertyValue): Kind {
    if (Array.isArray(v)) return v.some(isObject) ? "object" : "list";
    if (LIST_KEYS.has(key.toLowerCase()) && (typeof v === "string" || v === null)) return "list";
    if (typeof v === "boolean") return "checkbox";
    if (typeof v === "number") return "number";
    if (typeof v === "string" && DATE.test(v)) return "date";
    if (typeof v === "string" && DATETIME.test(v)) return "datetime";
    if (isObject(v)) return "object";
    return "text";
  }

  function listOf(v: PropertyValue): PropertyValue[] {
    if (Array.isArray(v)) return v;
    if (typeof v === "string") return v.split(/[,\s]+/).filter(Boolean);
    return [];
  }

  const text = (v: PropertyValue) => (v === null ? "" : typeof v === "object" ? JSON.stringify(v) : String(v));

  /** Open the panel with a new-property field (also for notes without properties). */
  export async function startAdding() {
    open = true;
    adding = true;
    await tick();
    newKeyInput?.focus();
  }

  // ------------------------------------------------------------- edits
  function update(i: number, prop: Property) {
    onchange(fm.properties.map((p, j) => (j === i ? prop : p)));
  }

  function setValue(i: number, value: PropertyValue) {
    update(i, { key: fm.properties[i].key, value });
  }

  function rename(i: number, input: HTMLInputElement) {
    const key = input.value.trim();
    const taken = fm.properties.some((p, j) => j !== i && p.key === key);
    if (!key || taken || key === fm.properties[i].key) {
      input.value = fm.properties[i].key;
      return;
    }
    update(i, { key, value: fm.properties[i].value });
  }

  function remove(i: number) {
    onchange(fm.properties.filter((_, j) => j !== i));
  }

  function addProperty(input: HTMLInputElement) {
    // Enter commits, then the field's removal may also fire blur.
    if (!adding) return;
    const key = input.value.trim();
    adding = false;
    if (!key) return;
    if (fm.properties.some((p) => p.key === key)) return;
    const value: PropertyValue = LIST_KEYS.has(key.toLowerCase()) ? [] : null;
    onchange([...fm.properties, { key, value }]);
  }

  function addListItem(i: number, key: string, input: HTMLInputElement) {
    let item = input.value.trim();
    if (TAG_KEYS.has(key.toLowerCase())) item = item.replace(/^#/, "");
    input.value = "";
    if (!item) return;
    const list = listOf(fm.properties[i].value);
    if (list.some((x) => String(x) === item)) return;
    setValue(i, [...list, item]);
  }

  function removeListItem(i: number, j: number) {
    setValue(i, listOf(fm.properties[i].value).filter((_, k) => k !== j));
  }

  function onText(i: number, value: string) {
    setValue(i, value.trim() === "" ? null : value);
  }

  function onNumber(i: number, input: HTMLInputElement) {
    const n = input.valueAsNumber;
    setValue(i, Number.isFinite(n) ? n : null);
  }

  /** Enter in a value field commits it without leaving the panel. */
  function enterBlurs(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.isComposing) (e.currentTarget as HTMLInputElement).blur();
  }

  const chipLabel = (key: string, v: PropertyValue) =>
    TAG_KEYS.has(key.toLowerCase()) ? `#${text(v).replace(/^#/, "")}` : text(v);
</script>

{#if fm.present || adding}
  <section class="props" aria-label="속성">
    <button class="head" onclick={() => (open = !open)} aria-expanded={open}>
      <span class="chevron" class:open><Icon name="chevron-right" size={12} /></span>
      속성
      <span class="count">{fm.properties.length}</span>
    </button>

    {#if open}
      {#if fm.error}
        <p class="error">속성 YAML을 읽을 수 없어 편집할 수 없습니다: {fm.error}</p>
      {:else}
        <div class="rows">
          {#each fm.properties as { key, value }, i (`${i}:${key}`)}
            {@const kind = kindOf(key, value)}
            <div class="row">
              <input class="key" value={key} aria-label="속성 이름" spellcheck="false"
                onchange={(e) => rename(i, e.currentTarget)} onkeydown={enterBlurs} />
              <div class="value">
                {#if kind === "list"}
                  <div class="chips">
                    {#each listOf(value) as item, j (j)}
                      <span class="chip">
                        {chipLabel(key, item)}
                        <button class="chip-x" onclick={() => removeListItem(i, j)} aria-label="{text(item)} 빼기"><Icon name="close" size={11} /></button>
                      </span>
                    {/each}
                    <input class="chip-add" placeholder="추가" aria-label="{key}에 항목 추가" spellcheck="false"
                      onkeydown={(e) => {
                        if (e.key === "Enter" && !e.isComposing) {
                          e.preventDefault();
                          addListItem(i, key, e.currentTarget);
                        }
                      }}
                      onblur={(e) => addListItem(i, key, e.currentTarget)} />
                  </div>
                {:else if kind === "checkbox"}
                  <input type="checkbox" checked={value === true} aria-label={key}
                    onchange={(e) => setValue(i, e.currentTarget.checked)} />
                {:else if kind === "number"}
                  <input class="field" type="number" value={value as number} aria-label={key}
                    onchange={(e) => onNumber(i, e.currentTarget)} onkeydown={enterBlurs} />
                {:else if kind === "date"}
                  <input class="field" type="date" value={value as string} aria-label={key}
                    onchange={(e) => onText(i, e.currentTarget.value)} />
                {:else if kind === "datetime"}
                  <input class="field" type="datetime-local" value={(value as string).slice(0, 16)} aria-label={key}
                    onchange={(e) => onText(i, e.currentTarget.value)} />
                {:else if kind === "object"}
                  <code title="중첩된 값은 소스에서 편집하세요">{text(value)}</code>
                {:else}
                  <input class="field" value={text(value)} placeholder="비어 있음" aria-label={key} spellcheck="false"
                    onchange={(e) => onText(i, e.currentTarget.value)} onkeydown={enterBlurs} />
                {/if}
              </div>
              <button class="remove" onclick={() => remove(i)} title="속성 삭제" aria-label="{key} 속성 삭제"><Icon name="close" size={13} /></button>
            </div>
          {/each}
        </div>

        {#if adding}
          <input bind:this={newKeyInput} class="key new-key" placeholder="속성 이름" aria-label="새 속성 이름" spellcheck="false"
            onkeydown={(e) => {
              if (e.isComposing) return;
              if (e.key === "Enter") addProperty(e.currentTarget);
              else if (e.key === "Escape") adding = false;
            }}
            onblur={(e) => addProperty(e.currentTarget)} />
        {:else}
          <button class="add" onclick={startAdding}><Icon name="plus" size={13} /> 속성 추가</button>
        {/if}
      {/if}
    {/if}
  </section>
{/if}

<style>
  .props {
    max-height: 40%;
    overflow: auto;
    border-bottom: 1px solid var(--border);
    padding: 8px max(24px, calc((100% - var(--line-width, 800px)) / 2 + 24px));
    font-size: 13px;
    flex-shrink: 0;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 6px;
    border: 0;
    background: none;
    color: var(--text-muted);
    font: inherit;
    font-weight: 600;
    padding: 2px 0;
    cursor: pointer;
  }
  .chevron {
    display: inline-flex;
    width: 12px;
    transition: transform 0.1s;
    color: var(--text-faint);
  }
  .chevron.open {
    transform: rotate(90deg);
  }
  .count {
    color: var(--text-faint);
    font-weight: 400;
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: 1px;
    margin: 6px 0 2px;
  }
  .row {
    display: grid;
    grid-template-columns: minmax(80px, 160px) 1fr 24px;
    align-items: center;
    gap: 8px;
    border-radius: 5px;
  }
  .row:hover {
    background: var(--hover);
  }
  input {
    font: inherit;
    color: var(--text);
  }
  .key,
  .field {
    min-width: 0;
    border: 1px solid transparent;
    background: none;
    padding: 3px 6px;
    border-radius: 4px;
    outline: none;
  }
  .key {
    color: var(--text-muted);
  }
  .key:focus,
  .field:focus,
  .chip-add:focus {
    border-color: var(--accent);
    background: var(--bg);
  }
  .field {
    width: 100%;
    box-sizing: border-box;
  }
  .field::placeholder,
  .chip-add::placeholder {
    color: var(--text-faint);
  }
  .field[type="date"],
  .field[type="datetime-local"] {
    width: auto;
    color-scheme: inherit;
  }
  .value {
    min-width: 0;
    padding: 2px 0;
  }
  .value input[type="checkbox"] {
    margin: 4px 6px;
    accent-color: var(--accent);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
    padding: 0 4px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    background: var(--accent-bg);
    color: var(--accent);
    border-radius: 10px;
    padding: 0 4px 0 8px;
  }
  .chip-x {
    display: inline-flex;
    border: 0;
    background: none;
    color: inherit;
    padding: 2px;
    border-radius: 50%;
    cursor: pointer;
    opacity: 0.6;
  }
  .chip-x:hover {
    opacity: 1;
  }
  .chip-add {
    width: 70px;
    border: 1px solid transparent;
    background: none;
    padding: 2px 6px;
    border-radius: 4px;
    outline: none;
  }
  .remove {
    display: inline-flex;
    justify-content: center;
    border: 0;
    background: none;
    color: var(--text-faint);
    padding: 3px;
    border-radius: 4px;
    cursor: pointer;
    opacity: 0;
  }
  .row:hover .remove,
  .remove:focus-visible {
    opacity: 1;
  }
  .remove:hover {
    color: var(--danger);
  }
  code {
    font-family: var(--font-mono);
    font-size: 12px;
    background: var(--code-bg);
    border-radius: 3px;
    padding: 0 4px;
    overflow-wrap: anywhere;
  }
  .add {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border: 0;
    background: none;
    color: var(--text-faint);
    font: inherit;
    padding: 4px 6px;
    border-radius: 4px;
    cursor: pointer;
  }
  .add:hover {
    color: var(--text);
    background: var(--hover);
  }
  .new-key {
    margin: 2px 0;
    width: 160px;
    border-color: var(--accent);
    background: var(--bg);
  }
  .error {
    margin: 4px 0;
    color: var(--danger);
  }
</style>
