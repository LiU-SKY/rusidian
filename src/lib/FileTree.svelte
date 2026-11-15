<script lang="ts">
  import FileTree from "./FileTree.svelte";
  import Icon from "./Icon.svelte";
  import RenameField from "./RenameField.svelte";
  import { displayName, type TreeNode } from "./api";
  import type { TreeActions, TreeUi } from "./treeUi.svelte";

  interface Props {
    nodes: TreeNode[];
    selected: string | null;
    ui: TreeUi;
    actions: TreeActions;
    onmenu: (e: MouseEvent, node: TreeNode) => void;
    depth?: number;
  }

  let { nodes, selected, ui, actions, onmenu, depth = 0 }: Props = $props();

  /** Hovering a closed folder while dragging opens it after a moment. */
  const OPEN_ON_HOVER_MS = 700;
  let hoverTimer: ReturnType<typeof setTimeout> | null = null;

  const parentOf = (path: string) => (path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "");
  const isNote = (node: TreeNode) => !node.is_dir && node.name.toLowerCase().endsWith(".md");

  function ondragstart(e: DragEvent, node: TreeNode) {
    e.dataTransfer?.setData("text/plain", node.path);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
    ui.dragging = node.path;
  }

  function ondragend() {
    ui.dragging = null;
    ui.dropTarget = null;
    clearHover();
  }

  /** `folder` is where a drop on this row lands (a file row means its folder). */
  function ondragover(e: DragEvent, folder: string, node: TreeNode) {
    e.stopPropagation();
    if (!ui.dragging || !ui.canDrop(ui.dragging, folder)) {
      ui.dropTarget = null;
      return;
    }
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    ui.dropTarget = folder;
    if (node.is_dir && !ui.expanded[node.path] && !hoverTimer) {
      hoverTimer = setTimeout(() => {
        if (ui.dropTarget === node.path) ui.expanded[node.path] = true;
        hoverTimer = null;
      }, OPEN_ON_HOVER_MS);
    }
  }

  function clearHover() {
    if (hoverTimer) clearTimeout(hoverTimer);
    hoverTimer = null;
  }

  function ondrop(e: DragEvent, folder: string) {
    e.preventDefault();
    e.stopPropagation();
    const path = ui.dragging;
    ondragend();
    if (path && ui.canDrop(path, folder)) actions.move(path, folder);
  }

  function onkeydown(e: KeyboardEvent, node: TreeNode) {
    if (e.key === "F2") {
      e.preventDefault();
      ui.renaming = node.path;
    } else if (e.key === "Delete") {
      e.preventDefault();
      actions.trash(node.path, node.is_dir);
    }
  }

  function oncontextmenu(e: MouseEvent, node: TreeNode) {
    e.preventDefault();
    e.stopPropagation();
    onmenu(e, node);
  }

  function renamed(node: TreeNode, name: string | null) {
    ui.renaming = null;
    if (name) actions.rename(node.path, node.is_dir, name);
  }
</script>

<ul class="tree" role={depth === 0 ? "tree" : "group"}>
  {#each nodes as node (node.path)}
    {@const target = node.is_dir ? node.path : parentOf(node.path)}
    <li role="treeitem" aria-expanded={node.is_dir ? !!ui.expanded[node.path] : undefined}
      aria-selected={!node.is_dir && node.path === selected}>
      {#if ui.renaming === node.path}
        <RenameField initial={isNote(node) ? displayName(node.name) : node.name}
          indent={(node.is_dir ? 8 : 22) + depth * 14 - 6} ondone={(name) => renamed(node, name)} />
      {:else if node.is_dir}
        <button class="row folder" class:drop={ui.dropTarget === node.path} class:dragged={ui.dragging === node.path}
          style:padding-left="{8 + depth * 14}px" title={node.path} draggable="true"
          onclick={() => (ui.expanded[node.path] = !ui.expanded[node.path])}
          ondragstart={(e) => ondragstart(e, node)} {ondragend}
          ondragover={(e) => ondragover(e, target, node)} ondragleave={clearHover}
          ondrop={(e) => ondrop(e, target)}
          onkeydown={(e) => onkeydown(e, node)} oncontextmenu={(e) => oncontextmenu(e, node)}>
          <span class="chevron" class:open={ui.expanded[node.path]}><Icon name="chevron-right" size={12} /></span>
          <span class="label">{node.name}</span>
        </button>
      {:else}
        <button class="row file" class:active={node.path === selected} class:dragged={ui.dragging === node.path}
          style:padding-left="{22 + depth * 14}px" title={node.path} draggable="true"
          onclick={(e) => actions.open(node.path, e.ctrlKey || e.metaKey)}
          onauxclick={(e) => e.button === 1 && actions.open(node.path, true)}
          ondragstart={(e) => ondragstart(e, node)} {ondragend}
          ondragover={(e) => ondragover(e, target, node)}
          ondrop={(e) => ondrop(e, target)}
          onkeydown={(e) => onkeydown(e, node)} oncontextmenu={(e) => oncontextmenu(e, node)}>
          <span class="label">{displayName(node.name)}</span>
          {#if !isNote(node)}
            <span class="ext">{node.name.split(".").pop()}</span>
          {/if}
        </button>
      {/if}
      {#if node.is_dir && ui.expanded[node.path]}
        <div class="children" class:drop={ui.dropTarget === node.path}>
          <FileTree nodes={node.children} {selected} {ui} {actions} {onmenu} depth={depth + 1} />
        </div>
      {/if}
    </li>
  {/each}
</ul>

<style>
  .tree {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    border: 0;
    background: none;
    color: var(--text-muted);
    font: inherit;
    font-size: 13px;
    text-align: left;
    padding-top: 3px;
    padding-bottom: 3px;
    padding-right: 8px;
    border-radius: 4px;
    cursor: pointer;
  }
  .row:hover {
    background: var(--hover);
    color: var(--text);
  }
  .row.active {
    background: var(--active);
    color: var(--text);
  }
  .row.dragged {
    opacity: 0.45;
  }
  /* The folder a drop would land in: its row and its open contents. */
  .row.drop {
    background: var(--accent-bg);
    color: var(--text);
    box-shadow: inset 0 0 0 1px var(--accent-soft);
  }
  .children.drop {
    background: var(--accent-bg);
    border-radius: 0 0 4px 4px;
  }
  .folder {
    color: var(--text);
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
  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ext {
    margin-left: auto;
    font-size: 10px;
    text-transform: uppercase;
    color: var(--text-faint);
    border: 1px solid var(--border);
    border-radius: 3px;
    padding: 0 4px;
  }
</style>
