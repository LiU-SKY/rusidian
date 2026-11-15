<script lang="ts">
  import ContextMenu, { type MenuItem } from "./ContextMenu.svelte";
  import FileTree from "./FileTree.svelte";
  import type { TreeNode } from "./api";
  import type { TreeActions, TreeUi } from "./treeUi.svelte";

  interface Props {
    tree: TreeNode[];
    selected: string | null;
    ui: TreeUi;
    actions: TreeActions;
  }

  let { tree, selected, ui, actions }: Props = $props();
  let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);

  /** Right-click menu for a row, or for the empty area (`node` null = vault root). */
  function openMenu(e: MouseEvent, node: TreeNode | null) {
    e.preventDefault();
    const items: MenuItem[] = [];
    if (node && !node.is_dir) {
      items.push({ label: "새 탭에서 열기", icon: "plus", shortcut: "Ctrl+클릭", action: () => actions.open(node.path, true) });
    }
    if (!node || node.is_dir) {
      const folder = node?.path ?? "";
      items.push(
        { label: "새 노트", icon: "file-plus", action: () => actions.newNote(folder) },
        { label: "새 폴더", icon: "folder-plus", action: () => actions.newFolder(folder) },
        { label: "새 캔버스", icon: "columns", action: () => actions.newCanvas(folder) },
      );
    }
    if (node) {
      items.push({
        label: "이름 바꾸기", icon: "pencil", shortcut: "F2", separated: node.is_dir,
        action: () => (ui.renaming = node.path),
      });
    }
    items.push({
      label: node ? "탐색기에서 보기" : "vault 폴더 열기", icon: "folder-open", separated: !node,
      action: () => actions.reveal(node?.path ?? ""),
    });
    if (node && !node.is_dir && /\.md$/i.test(node.path)) {
      items.push({ label: "이전 버전 보기", icon: "clock", action: () => actions.history(node.path) });
    }
    if (node) {
      items.push({
        label: "삭제", icon: "trash", shortcut: "Del", danger: true, separated: true,
        action: () => actions.trash(node.path, node.is_dir),
      });
    }
    menu = { x: e.clientX, y: e.clientY, items };
  }

  // The empty area below the tree is the vault root as a drop target.
  function ondragover(e: DragEvent) {
    if (!ui.dragging || !ui.canDrop(ui.dragging, "")) {
      ui.dropTarget = null;
      return;
    }
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    ui.dropTarget = "";
  }

  function ondrop(e: DragEvent) {
    e.preventDefault();
    const path = ui.dragging;
    ui.dragging = null;
    ui.dropTarget = null;
    if (path && ui.canDrop(path, "")) actions.move(path, "");
  }

  function ondragleave(e: DragEvent) {
    if (!(e.currentTarget as Element).contains(e.relatedTarget as Node)) ui.dropTarget = null;
  }
</script>

<div class="explorer" class:drop={ui.dropTarget === ""} role="presentation"
  oncontextmenu={(e) => openMenu(e, null)} {ondragover} {ondrop} {ondragleave}>
  {#if tree.length}
    <FileTree nodes={tree} {selected} {ui} {actions} onmenu={openMenu} />
  {:else}
    <p class="hint">빈 vault입니다. 오른쪽 클릭으로 노트나 폴더를 만드세요.</p>
  {/if}
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menu.items} onclose={() => (menu = null)} />
{/if}

<style>
  .explorer {
    min-height: 100%;
    border-radius: 6px;
  }
  .explorer.drop {
    background: var(--accent-bg);
    box-shadow: inset 0 0 0 1px var(--accent-soft);
  }
  .hint {
    color: var(--text-faint);
    font-size: 13px;
    padding: 8px;
    margin: 0;
  }
</style>
