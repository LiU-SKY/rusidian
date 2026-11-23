<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { ask, open } from "@tauri-apps/plugin-dialog";
  import FileExplorer from "$lib/FileExplorer.svelte";
  import { TreeUi, type TreeActions } from "$lib/treeUi.svelte";
  import Backlinks from "$lib/Backlinks.svelte";
  import BookmarksPane from "$lib/BookmarksPane.svelte";
  import OutgoingLinks from "$lib/OutgoingLinks.svelte";
  import Icon from "$lib/Icon.svelte";
  import { slider } from "$lib/slider";
  import { localDate, localTime } from "$lib/dates";
  import LeafView, { type LeafActions } from "$lib/LeafView.svelte";
  import Outline from "$lib/Outline.svelte";
  import Palette, { type PaletteItem } from "$lib/Palette.svelte";
  import Search from "$lib/Search.svelte";
  import SettingsDialog from "$lib/SettingsDialog.svelte";
  import SyncDialog from "$lib/SyncDialog.svelte";
  import FileRecovery from "$lib/FileRecovery.svelte";
  import ConflictDialog from "$lib/ConflictDialog.svelte";
  import PrintView from "$lib/PrintView.svelte";
  import TagPane from "$lib/TagPane.svelte";
  import WindowControls from "$lib/WindowControls.svelte";
  import { DocStore, type Doc } from "$lib/docs.svelte";
  import { fuzzyMatch } from "$lib/fuzzy";
  import type { IconName } from "$lib/icons";
  import { findHeading } from "$lib/outline";
  import { startReminders } from "$lib/reminders";
  import { onUpdateFlush, updates } from "$lib/updates.svelte";
  import { settings } from "$lib/settings.svelte";
  import { hk, hotkeys, label as hotkeyLabel, matches } from "$lib/hotkeys.svelte";
  import { Leaf, MAX_PANES, Pane, Workspace, type ViewKind } from "$lib/workspace.svelte";
  import {
    createNote, displayName, filePaths, focusVaultWindow, forgetVault, isTextFile, listTemplates, movePath,
    newCanvas, newFolder, newNote, onVaultChanged, openDailyNote, openVault, openWindow, readNote, recentVaults, refreshTree,
    renamePath, renderTemplate, resolveLink, revealPath, startupVault, trashPath, vaultUrl, getSync, setSync,
    getBookmarks, toggleBookmark, removeBookmark, type BookmarkItem,
    saveAttachment, attachmentStamp, ATTACHMENT_MAX_BYTES, phoneChanged, phoneProcess,
    syncNow, syncStamp, type MoveResult, type Property, type PropertyValue, type SyncConfig, type SyncInfo, type VaultEntry,
    type VaultInfo, type Hotkey,
    everythingOpen, everythingSearch, everythingStatus, mediaKind, type EverythingHit,
  } from "$lib/api";

  const TREE_REFRESH_MS = 200;
  const FLASH_MS = 4000;
  /** After startup settles, so the check doesn't compete with opening the vault. */
  const UPDATE_CHECK_DELAY_MS = 5000;
  const RECENT_MAX = 30;
  const WORKSPACE_KEY = "rusidian.workspace:";
  /** Recently opened files, per vault. */
  const RECENT_KEY = "rusidian.recent:";

  let vault = $state<VaultInfo | null>(null);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let sideTab = $state<"files" | "search" | "bookmarks">("files");
  let showRight = $state(true);
  let rightTab = $state<"backlinks" | "outgoing" | "outline" | "tags">("backlinks");
  /** Bumped on every on-disk change so search/backlinks/views refresh. */
  let vaultVersion = $state(0);
  let searchPanel: { focus: () => void; setQuery: (q: string) => void } | undefined = $state();
  /** Open modal: quick switcher (Ctrl+O), command palette (Ctrl+P), template picker, vault switcher. */
  let palette = $state<"switcher" | "commands" | "templates" | "vaults" | "everything" | null>(null);
  /** Vaults opened before, newest first. */
  let vaults = $state<VaultEntry[]>([]);
  let templateFiles = $state<string[]>([]);
  let showSettings = $state(false);
  /** Recently opened files, newest first (quick switcher order). */
  let recent = $state<string[]>([]);
  let treeTimer: ReturnType<typeof setTimeout> | null = null;

  // Theme, font size and line width: push into the document and remember.
  $effect.pre(() => {
    settings.apply();
    settings.save();
  });

  // -------------------------------------------------- workspace and documents
  const docs = new DocStore(
    (message) => (error = message),
    // A save found the note changed elsewhere (sync, Obsidian): it kept that
    // version and put the unsaved text in a copy, to merge from.
    (path, copy) => {
      notice = `"${displayName(path.split("/").pop()!)}" 노트가 다른 곳에서 바뀌어 있어 덮어쓰지 않았습니다. 저장하지 않은 내용은 사본으로 남겼습니다.`;
      noticeAction = { notice, label: "비교해서 합치기", run: () => (conflicts = { start: copy }) };
      vaultVersion++;
    },
  );
  const workspace = new Workspace();

  interface LeafApi {
    gotoLine: (line: number) => void;
    startAddingProperty: () => void;
    insertAtCursor: (text: string) => Promise<void>;
    applyProperties: (properties: Property[]) => Promise<void>;
  }
  /** The mounted content of each pane's active tab, by leaf id. */
  let leafViews: Record<string, LeafApi | null> = $state({});

  const isTextLeaf = (l: Leaf) => l.kind === "file" && !!l.path && isTextFile(l.path);
  const docFor = (l: Leaf): Doc | null => (isTextLeaf(l) ? (docs.find(l.path!) ?? null) : null);

  const leaf = $derived(workspace.leaf);
  /** The file in the focused tab. */
  const currentPath = $derived(leaf.kind === "file" ? leaf.path : null);
  const activeDoc = $derived(docFor(leaf));
  const isNote = $derived(currentPath?.toLowerCase().endsWith(".md") ?? false);

  // Load a doc for every tab showing a text file; drop docs no tab shows.
  $effect(() => {
    const paths = new Set(workspace.leaves().filter(isTextLeaf).map((l) => l.path!));
    untrack(() => {
      for (const path of paths) docs.open(path);
      docs.keepOnly(paths);
    });
  });

  // Remember the tab layout per vault.
  $effect(() => {
    if (!vault) return;
    const key = WORKSPACE_KEY + vault.root;
    const json = workspace.serialize();
    try {
      localStorage.setItem(key, json);
    } catch {
      // Not remembered; harmless.
    }
  });

  async function flushSave() {
    await docs.flushAll();
  }

  async function refreshVaults() {
    try {
      vaults = await recentVaults();
    } catch {
      vaults = [];
    }
  }

  function readStored(key: string): string | null {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  }

  async function loadVault(path: string) {
    try {
      // One window per vault: if another window has it, go there instead.
      if (path !== vault?.root && (await focusVaultWindow(path).catch(() => false))) {
        flash("이 vault는 다른 창에 열려 있어 그 창을 앞으로 가져왔습니다.");
        return;
      }
      await docs.clear();
      const info = await openVault(path);
      vault = info;
      hotkeys.load();
      const files = new Set(filePaths(info.tree));
      // Tabs, recent files and open folders belong to each vault.
      const saved = readStored(WORKSPACE_KEY + info.root);
      if (!saved || !workspace.restore(saved, (p) => files.has(p))) workspace.reset();
      try {
        const list = JSON.parse(readStored(RECENT_KEY + info.root) ?? "[]");
        recent = Array.isArray(list) ? list.filter((p): p is string => typeof p === "string" && files.has(p)) : [];
      } catch {
        recent = [];
      }
      treeUi.expanded = {};
      treeUi.renaming = null;
      sideTab = "files";
      error = null;
      notice = info.watching ? null : "이 폴더는 변경 감지를 할 수 없습니다. 바깥에서 바꾼 내용은 사이드바 위쪽의 새로고침 버튼으로 반영하세요.";
      syncStatus = null;
      await loadSyncInfo();
      if (syncInfo?.config) runSync(true);
      await reloadBookmarks();
    } catch (e) {
      error = String(e);
    } finally {
      refreshVaults();
    }
  }

  // ------------------------------------------------------------- git sync
  /** Settings last sent/received by sync, per vault (to send only changes). */
  const SYNCED_UI_KEY = "rusidian.syncedUi:";
  let syncInfo = $state<SyncInfo | null>(null);
  let syncRunning = $state(false);
  let syncStatus = $state<{ ok: boolean; text: string } | null>(null);
  let showSync = $state(false);
  /** File recovery is open, starting at this note (null: the newest change). */
  let recovery = $state<{ start: string | null } | null>(null);
  /** Sync-conflict merging is open, starting at this copy. */
  let conflicts = $state<{ start: string | null } | null>(null);
  /** A button on the notice it was made for. */
  let noticeAction = $state<{ notice: string; label: string; run: () => void } | null>(null);
  /** The note being printed (rendered off screen until then). */
  let printing = $state<{ path: string; content: string } | null>(null);
  let printView: PrintView | undefined = $state();

  async function loadSyncInfo() {
    try {
      syncInfo = vault ? await getSync() : null;
    } catch {
      syncInfo = null;
    }
  }

  const pad2 = (n: number) => String(n).padStart(2, "0");

  /** Sync this vault now. `quiet`: automatic runs only update the status. */
  async function runSync(quiet = false) {
    if (!vault || !syncInfo?.config || syncRunning) return;
    const root = vault.root;
    syncRunning = true;
    try {
      await flushSave();
      const current = settings.snapshot();
      const sent = readStored(SYNCED_UI_KEY + root);
      const report = await syncNow(JSON.stringify(current) !== sent ? { ...current } : null, syncStamp());
      if (report.ui) settings.applySnapshot(report.ui);
      try {
        localStorage.setItem(SYNCED_UI_KEY + root, JSON.stringify(settings.snapshot()));
      } catch {
        // Settings will simply be sent again next time.
      }
      if (report.pulled.length) {
        await reload();
        vaultVersion++;
        hotkeys.load(); // another device may have changed them
      }
      const d = new Date();
      const parts = [
        report.pulled.length ? `받음 ${report.pulled.length}` : "",
        report.pushed ? "올림" : "",
        report.conflicts.length ? `충돌 사본 ${report.conflicts.length}` : "",
      ].filter(Boolean);
      syncStatus = { ok: true, text: `${pad2(d.getHours())}:${pad2(d.getMinutes())} 동기화됨${parts.length ? ` (${parts.join(", ")})` : ""}` };
      if (report.conflicts.length) {
        notice = `동기화: 양쪽에서 바뀐 파일이 있어 다른 쪽 내용을 사본으로 남겼습니다 — ${report.conflicts.join(", ")}`;
        const first = report.conflicts[0];
        noticeAction = { notice, label: "비교해서 합치기", run: () => (conflicts = { start: first }) };
      } else if (!quiet) {
        flash(parts.length ? `동기화했습니다: ${parts.join(", ")}` : "동기화했습니다. 바뀐 것이 없습니다.");
      }
      if (report.large.length && !quiet) {
        flash(`50MB가 넘는 파일 ${report.large.length}개는 동기화하지 않았습니다.`);
      }
    } catch (e) {
      syncStatus = { ok: false, text: `동기화 실패: ${e}` };
      if (!quiet) error = `동기화하지 못했습니다: ${e}`;
    } finally {
      syncRunning = false;
    }
  }

  // -------------------------------------------------------------- bookmarks
  /** `.obsidian/bookmarks.json`, shared with Obsidian. */
  let bookmarks = $state<BookmarkItem[]>([]);
  async function reloadBookmarks() {
    try {
      bookmarks = vault ? await getBookmarks() : [];
    } catch {
      bookmarks = [];
    }
  }
  /** Every bookmarked file path (groups included). */
  const bookmarked = $derived.by(() => {
    const out = new Set<string>();
    const walk = (items: BookmarkItem[]) => {
      for (const b of items) {
        if (b.type === "file" && b.path) out.add(b.path);
        if (b.items) walk(b.items);
      }
    };
    walk(bookmarks);
    return out;
  });
  async function toggleBookmarkOf(path: string) {
    try {
      const on = await toggleBookmark(path);
      await reloadBookmarks();
      flash(on ? "북마크에 추가했습니다." : "북마크에서 뺐습니다.");
    } catch (e) {
      error = String(e);
    }
  }
  async function removeBookmarkAt(at: number[]) {
    try {
      await removeBookmark(at);
    } catch (e) {
      error = String(e);
    }
    await reloadBookmarks();
  }
  // Obsidian (or a sync) may change the file too.
  $effect(() => {
    vaultVersion;
    if (vault) reloadBookmarks();
  });
  async function searchFor(query: string) {
    sideTab = "search";
    await tick();
    searchPanel?.setQuery(query);
  }

  async function saveSync(config: SyncConfig | null, now: boolean) {
    try {
      await setSync(config);
      await loadSyncInfo();
      showSync = false;
      if (config && now) await runSync();
      else flash(config ? "동기화 설정을 저장했습니다." : "이 기기에서 동기화를 껐습니다. 파일과 원격 저장소는 그대로입니다.");
    } catch (e) {
      error = String(e);
    }
  }

  // Automatic sync while the window is open.
  $effect(() => {
    const minutes = syncInfo?.config?.interval_minutes ?? 0;
    if (!minutes) return;
    const id = setInterval(() => runSync(true), minutes * 60_000);
    return () => clearInterval(id);
  });

  // Phone notifications (set up in the finance view): the inbox file is
  // checked every 30 s and when the window comes back to the front.
  const PHONE_POLL_MS = 30_000;
  let phoneBusy = false;
  let phoneError: string | null = null;
  async function checkPhone() {
    if (!vault || phoneBusy) return;
    phoneBusy = true;
    try {
      if (!(await phoneChanged())) return;
      await flushSave(); // records are appended to notes that may be open
      const res = await phoneProcess(localDate());
      phoneError = null;
      if (res.added) {
        await handleVaultChanged(res.notes);
        flash(`휴대폰 알림 ${res.added}건을 가계부에 적었습니다.`);
      }
    } catch (e) {
      // Shown once, not every 30 s.
      if (String(e) !== phoneError) {
        phoneError = String(e);
        flash(`휴대폰 알림: ${phoneError}`);
      }
    } finally {
      phoneBusy = false;
    }
  }
  $effect(() => {
    if (!vault) return;
    untrack(checkPhone);
    const id = setInterval(checkPhone, PHONE_POLL_MS);
    const onFocus = () => checkPhone();
    window.addEventListener("focus", onFocus);
    return () => {
      clearInterval(id);
      window.removeEventListener("focus", onFocus);
    };
  });

  // The window title names the vault, so windows can be told apart in the taskbar.
  $effect(() => {
    const title = vault ? `${vault.name} - Rusidian` : "Rusidian";
    document.title = title;
    import("@tauri-apps/api/window")
      .then(({ getCurrentWindow }) => getCurrentWindow().setTitle(title))
      .catch(() => {});
  });

  // Remember the recently opened files of this vault.
  $effect(() => {
    if (!vault) return;
    const json = JSON.stringify(recent);
    try {
      localStorage.setItem(RECENT_KEY + vault.root, json);
    } catch {
      // Not remembered; harmless.
    }
  });

  // ------------------------------------------------------- vault switcher
  const OPEN_FOLDER_ID = "\u0000open-folder";

  // Re-read the list each time the switcher opens (folders may have moved).
  $effect(() => {
    if (palette === "vaults") untrack(refreshVaults);
  });

  function vaultItems(query: string): PaletteItem[] {
    const current = vault?.root;
    const toItem = (v: VaultEntry, marks?: number[]): PaletteItem => ({
      id: v.path,
      label: v.name,
      marks,
      detail: v.path,
      icon: "folder-open",
      hint: v.path === current ? "열려 있음" : v.exists ? undefined : "폴더 없음",
      disabled: !v.exists,
      removable: v.path !== current,
    });
    const q = query.trim();
    const items = q
      ? vaults
          .map((v) => ({ v, m: fuzzyMatch(q, v.name) ?? fuzzyMatch(q, v.path) }))
          .filter((x) => x.m)
          .sort((a, b) => b.m!.score - a.m!.score)
          .map((x) => toItem(x.v, fuzzyMatch(q, x.v.name)?.indices))
      : vaults.map((v) => toItem(v));
    items.push({ id: OPEN_FOLDER_ID, label: "다른 폴더를 vault로 열기…", icon: "folder-plus" });
    return items;
  }

  /** Enter: open here. Ctrl+Enter / Ctrl+click: open in a new window. */
  async function pickVaultItem(item: PaletteItem, _query: string, keys: { shift: boolean; mod: boolean }) {
    if (item.id === OPEN_FOLDER_ID) return pickVault(keys.mod);
    const entry = vaults.find((v) => v.path === item.id);
    if (entry && !entry.exists) {
      error = `"${entry.path}" 폴더가 없습니다. 옮겼다면 "다른 폴더를 vault로 열기"로 새 위치를 여세요.`;
      return;
    }
    if (keys.mod) {
      if (item.id === vault?.root) return;
      await openWindow(item.id).catch((e) => (error = String(e)));
      return;
    }
    if (item.id === vault?.root) return;
    await loadVault(item.id);
  }

  async function removeVaultItem(item: PaletteItem) {
    try {
      vaults = await forgetVault(item.id);
    } catch (e) {
      error = String(e);
    }
  }

  /** Pick a folder and open it as a vault (in a new window when `newWindow`). */
  async function pickVault(newWindow = false) {
    const picked = await open({ directory: true, multiple: false, title: "Vault 폴더 선택" });
    if (typeof picked !== "string") return;
    if (newWindow) await openWindow(picked).catch((e) => (error = String(e)));
    else await loadVault(picked);
  }

  async function reload() {
    if (!vault) return;
    try {
      vault.tree = await refreshTree();
    } catch (e) {
      error = String(e);
    }
  }

  /**
   * Open a file in the focused tab, or in a new tab when asked (Ctrl+click)
   * or when the focused tab shows a view (calendar, tasks, …) worth keeping.
   */
  async function openPath(path: string, newTab = false) {
    let target = workspace.leaf;
    if (newTab || (target.kind !== "file" && target.kind !== "empty")) target = workspace.newTab();
    else workspace.focus(workspace.pane);
    target.kind = "file";
    target.path = path;
    notice = null;
    recent = [path, ...recent.filter((p) => p !== path)].slice(0, RECENT_MAX);
    await tick();
  }

  /** A view (calendar, tasks, …): focus its tab if one is open, else open one. */
  function showView(kind: ViewKind, opts: { local?: boolean } = {}) {
    if (!vault) return;
    const center = currentPath;
    const found = workspace.find(kind);
    let target: Leaf;
    if (found) {
      workspace.focus(found.pane, found.index);
      target = found.pane.tabs[found.index];
    } else if (workspace.leaf.kind === "empty") {
      target = workspace.leaf;
    } else {
      target = workspace.newTab();
    }
    target.kind = kind;
    if (kind === "graph") {
      target.graphLocal = !!opts.local && !!center;
      if (center) target.path = center;
    }
  }

  function newTab(pane = workspace.pane) {
    workspace.focus(pane);
    workspace.newTab(pane);
  }

  function closeTab(pane: Pane, index: number) {
    workspace.closeTab(pane, index);
  }

  function cycleTab(delta: number) {
    const pane = workspace.pane;
    workspace.focus(pane, (pane.active + delta + pane.tabs.length) % pane.tabs.length);
  }

  function splitRight() {
    if (!workspace.splitRight()) flash(`창은 ${MAX_PANES}개까지 나눌 수 있습니다.`);
  }

  async function openTodayNote() {
    try {
      const path = await openDailyNote(localDate(), localTime());
      await reload();
      await openPath(path);
    } catch (e) {
      error = String(e);
    }
  }

  /** Scroll the focused tab to 0-based `line`. */
  function gotoLine(line: number) {
    leafViews[workspace.leaf.id]?.gotoLine(line);
  }

  /** Scroll the focused note to a heading (text as written after `#`). */
  async function jumpToHeading(heading: string) {
    await tick();
    const doc = docFor(workspace.leaf);
    if (!doc) return;
    if (!doc.loaded) await doc.load();
    await tick();
    const line = findHeading(doc.content, heading);
    if (line === null) flash(`"${heading}" 제목을 찾을 수 없습니다.`);
    else gotoLine(line);
  }

  /**
   * [[target#heading]] clicked in `source`: open it (creating a missing
   * note), then go to the heading.
   */
  async function followLink(target: string, heading: string | null, source: string) {
    if (!target) {
      if (source !== currentPath) await openPath(source);
      if (heading) await jumpToHeading(heading);
      return;
    }
    try {
      let path = await resolveLink(source, target);
      if (!path) {
        const ext = target.match(/\.([a-z0-9]{1,5})$/i)?.[1]?.toLowerCase();
        if (ext && ext !== "md") {
          error = `파일을 찾을 수 없습니다: ${target}`;
          return;
        }
        path = await createNote(target);
        await reload();
      }
      await openPath(path);
      if (heading) await jumpToHeading(heading);
    } catch (e) {
      error = String(e);
    }
  }

  const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

  /** Files changed on disk: refresh tree/search/backlinks, reload open notes that are untouched. */
  async function handleVaultChanged(paths: string[]) {
    vaultVersion++;
    if (treeTimer) clearTimeout(treeTimer);
    treeTimer = setTimeout(reload, TREE_REFRESH_MS);

    for (const doc of docs.all()) {
      if (!paths.includes(doc.path) || doc.dirty) continue;
      const path = doc.path;
      let text: string | null = null;
      // Some apps save by delete + recreate; retry once before treating it as deleted.
      for (let attempt = 0; attempt < 2 && text === null; attempt++) {
        try {
          text = await readNote(path);
        } catch {
          await sleep(300);
        }
      }
      if (doc.path !== path || doc.dirty) continue;
      if (text === null) {
        docs.removed(path);
        if (workspace.removed(path)) {
          notice = `"${displayName(path.split("/").pop()!)}" 노트가 다른 곳에서 삭제되었거나 이동되었습니다.`;
        }
      } else {
        doc.replaceFromDisk(text);
      }
    }
  }

  // ------------------------------------------------------ file management
  const treeUi = new TreeUi();

  /** A notice that clears itself. */
  function flash(text: string) {
    notice = text;
    setTimeout(() => {
      if (notice === text) notice = null;
    }, FLASH_MS);
  }

  /** `from` now lives at `res.path`: follow it in the tree, the tabs and the docs. */
  async function afterMove(from: string, res: MoveResult) {
    treeUi.moved(from, res.path);
    docs.moved(from, res.path);
    workspace.moved(from, res.path);
    recent = recent.map((p) => (p === from ? res.path : p));
    // Open notes may have had links rewritten.
    for (const path of res.updated) {
      const doc = docs.find(path);
      if (doc && !doc.dirty) doc.replaceFromDisk(await readNote(path));
    }
    await reload();
    vaultVersion++;
    if (res.failed.length) {
      error = `링크를 고치지 못한 노트가 있습니다: ${res.failed.join(", ")}`;
    } else if (res.updated.length) {
      flash(`노트 ${res.updated.length}개의 링크를 새 위치에 맞게 고쳤습니다.`);
    }
  }

  async function renameItem(path: string, isDir: boolean, typed: string) {
    const oldName = path.split("/").pop()!;
    const isMd = !isDir && oldName.toLowerCase().endsWith(".md");
    // Notes are edited without ".md"; it is added back here.
    const newName = isMd && !typed.toLowerCase().endsWith(".md") ? `${typed}.md` : typed;
    if (newName === oldName) return;
    try {
      await flushSave();
      await afterMove(path, await renamePath(path, newName));
    } catch (e) {
      error = String(e);
    }
  }

  async function moveItem(path: string, folder: string) {
    try {
      await flushSave();
      const res = await movePath(path, folder);
      if (folder) treeUi.expanded[folder] = true;
      await afterMove(path, res);
    } catch (e) {
      error = String(e);
    }
  }

  async function trashItem(path: string, isDir: boolean) {
    const name = displayName(path.split("/").pop()!);
    try {
      const ok = await ask(
        isDir ? `"${name}" 폴더와 그 안의 모든 파일을 휴지통으로 옮길까요?` : `"${name}"을(를) 휴지통으로 옮길까요?`,
        { title: "삭제", kind: "warning", okLabel: "휴지통으로", cancelLabel: "취소" },
      );
      if (!ok) return;
      await flushSave();
      await trashPath(path);
      docs.removed(path);
      workspace.removed(path);
      await reload();
      vaultVersion++;
    } catch (e) {
      error = String(e);
    }
  }

  /** New "제목 없음" note in `folder`, opened, with its name ready to type. */
  async function newNoteIn(folder: string) {
    try {
      await flushSave();
      const path = await newNote(folder);
      await reload();
      sideTab = "files";
      treeUi.reveal(path);
      await openPath(path);
      treeUi.renaming = path;
    } catch (e) {
      error = String(e);
    }
  }

  async function newCanvasIn(folder: string) {
    try {
      await flushSave();
      const path = await newCanvas(folder);
      await reload();
      sideTab = "files";
      treeUi.reveal(path);
      await openPath(path);
      treeUi.renaming = path;
    } catch (e) {
      error = String(e);
    }
  }

  async function newFolderIn(parent: string) {
    try {
      const path = await newFolder(parent);
      await reload();
      sideTab = "files";
      treeUi.reveal(path);
      treeUi.renaming = path;
    } catch (e) {
      error = String(e);
    }
  }

  const treeActions: TreeActions = {
    open: (path, newTab) => openPath(path, newTab),
    rename: renameItem,
    move: moveItem,
    trash: trashItem,
    newNote: newNoteIn,
    newFolder: newFolderIn,
    newCanvas: newCanvasIn,
    reveal: (path) => {
      revealPath(path).catch((e) => (error = String(e)));
    },
    history: (path) => (recovery = { start: path }),
  };

  const leafActions: LeafActions = {
    open: (path, newTab) => openPath(path, newTab),
    followLink,
    searchTag: (tag) => searchTag(tag),
    beforewrite: flushSave,
    reveal: treeActions.reveal,
    pickVault: () => pickVault(),
    openVault: (path) => loadVault(path),
    quickSwitch: () => (palette = "switcher"),
    newNote: () => newNoteIn(""),
    today: openTodayNote,
    error: (message) => (error = message),
    imageUrl: async (source, target) => {
      const path = await resolveLink(source, target).catch(() => null);
      return path ? vaultUrl(path) : null;
    },
    attach: attachFiles,
    rename: (path, name) => renameItem(path, false, name),
  };

  /** Save pasted / dropped files next to a note (Obsidian's attachment folder). */
  async function attachFiles(note: string, files: File[], pasted: boolean): Promise<string[]> {
    const embeds: string[] = [];
    const stamp = attachmentStamp();
    for (const file of files) {
      if (file.size > ATTACHMENT_MAX_BYTES) {
        error = `${file.name || "파일"}: 50MB가 넘는 파일은 첨부할 수 없습니다.`;
        continue;
      }
      try {
        const saved = await saveAttachment(note, file, pasted, stamp);
        embeds.push(saved.embed);
      } catch (e) {
        error = `${file.name || "붙여 넣은 이미지"}을(를) 저장하지 못했습니다: ${e}`;
      }
    }
    if (embeds.length) {
      flash(embeds.length === 1 ? "첨부 파일을 저장했습니다." : `첨부 파일 ${embeds.length}개를 저장했습니다.`);
      reload();
    }
    return embeds;
  }

  async function showSearch() {
    sideTab = "search";
    await tick();
    searchPanel?.focus();
  }

  /** Search the vault for a tag (clicked in the tag pane or a note). */
  async function searchTag(tag: string) {
    sideTab = "search";
    await tick();
    searchPanel?.setQuery(`tag:#${tag}`);
  }

  // -------------------------------------------------------------- templates
  async function insertTemplate() {
    if (!activeDoc?.isNote) {
      flash("템플릿은 열려 있는 노트에 넣을 수 있습니다.");
      return;
    }
    try {
      const list = await listTemplates();
      if (!list.folder) {
        error = "템플릿 폴더가 정해져 있지 않습니다. .obsidian/templates.json에 \"folder\"를 적거나 Obsidian 설정 › 템플릿에서 지정하세요.";
        return;
      }
      if (!list.files.length) {
        flash(`"${list.folder}" 폴더에 템플릿 노트가 없습니다.`);
        return;
      }
      templateFiles = list.files;
      palette = "templates";
    } catch (e) {
      error = String(e);
    }
  }

  function templateItems(query: string): PaletteItem[] {
    const toItem = (path: string, marks?: number[]): PaletteItem =>
      ({ id: path, label: displayName(baseName(path)), marks, detail: folderOf(path), icon: "template" });
    if (!query.trim()) return templateFiles.map((p) => toItem(p));
    return templateFiles
      .map((p) => ({ p, m: fuzzyMatch(query, displayName(baseName(p))) }))
      .filter((x) => x.m)
      .sort((a, b) => b.m!.score - a.m!.score)
      .map((x) => toItem(x.p, x.m!.indices));
  }

  /** Template properties go into the note's (lists merged, existing values kept); the body goes at the cursor. */
  function mergeProperties(note: Property[], template: Property[]): Property[] {
    const out = note.map((p) => ({ ...p }));
    for (const t of template) {
      const existing = out.find((p) => p.key === t.key);
      if (!existing) out.push(t);
      else if (Array.isArray(existing.value) && Array.isArray(t.value)) {
        const seen = new Set(existing.value.map((v) => JSON.stringify(v)));
        existing.value = [...existing.value, ...t.value.filter((v: PropertyValue) => !seen.has(JSON.stringify(v)))];
      } else if (existing.value === null || existing.value === "") {
        existing.value = t.value;
      }
    }
    return out;
  }

  async function pickTemplate(item: PaletteItem) {
    const doc = activeDoc;
    const view = leafViews[workspace.leaf.id];
    if (!doc || !view) return;
    try {
      const rendered = await renderTemplate(item.id, displayName(baseName(doc.path)), localDate(), localTime());
      await view.insertAtCursor(rendered.body);
      if (rendered.properties?.length) {
        await doc.refreshProps();
        if (doc.fm?.error) {
          flash("노트의 속성 YAML에 오류가 있어 템플릿의 속성은 넣지 않았습니다.");
          return;
        }
        await view.applyProperties(mergeProperties(doc.fm?.properties ?? [], rendered.properties));
      }
    } catch (e) {
      error = String(e);
    }
  }

  // ------------------------------------------------- quick switcher (Ctrl+O)
  const PALETTE_MAX = 60;
  const CREATE_ID = "\u0000create";
  let allFiles = $derived(vault ? filePaths(vault.tree) : []);

  const baseName = (path: string) => path.slice(path.lastIndexOf("/") + 1);
  const folderOf = (path: string) => path.slice(0, Math.max(0, path.lastIndexOf("/")));

  function fileItem(path: string, marks?: number[]): PaletteItem {
    return { id: path, label: displayName(baseName(path)), marks, detail: folderOf(path) || undefined };
  }

  function switcherItems(query: string): PaletteItem[] {
    const q = query.trim();
    if (!q) {
      const exists = new Set(allFiles);
      const first = recent.filter((p) => exists.has(p));
      const seen = new Set(first);
      return [...first, ...allFiles.filter((p) => !seen.has(p))].slice(0, PALETTE_MAX).map((p) => fileItem(p));
    }
    const scored: { path: string; score: number; marks?: number[] }[] = [];
    for (const path of allFiles) {
      const bonus = recent.includes(path) ? 5 : 0;
      const byName = fuzzyMatch(q, displayName(baseName(path)));
      if (byName) {
        scored.push({ path, score: byName.score + 15 + bonus, marks: byName.indices });
        continue;
      }
      const byPath = fuzzyMatch(q, path);
      if (byPath) scored.push({ path, score: byPath.score + bonus });
    }
    scored.sort((a, b) => b.score - a.score);
    const items = scored.slice(0, PALETTE_MAX).map((s) => fileItem(s.path, s.marks));
    const wanted = q.replace(/\.md$/i, "").toLowerCase();
    const exact = allFiles.some((p) => p.replace(/\.md$/i, "").toLowerCase() === wanted
      || displayName(baseName(p)).toLowerCase() === wanted);
    if (!exact) items.push({ id: CREATE_ID, label: `"${q}" 노트 만들기`, icon: "file-plus", hint: "Shift+Enter" });
    return items;
  }

  async function pickFile(item: PaletteItem, query: string, keys: { shift: boolean; mod: boolean }) {
    if (item.id !== CREATE_ID && !keys.shift) {
      await openPath(item.id, keys.mod);
      return;
    }
    if (!query) return;
    try {
      await flushSave();
      const path = await createNote(query.replace(/\.md$/i, ""));
      await reload();
      treeUi.reveal(path);
      await openPath(path, keys.mod);
    } catch (e) {
      error = String(e);
    }
  }

  // ------------------------------------- Everything: whole-PC search (Ctrl+Shift+E)
  const EVERYTHING_DEBOUNCE_MS = 120;
  let everythingHits = $state<EverythingHit[]>([]);
  let everythingTotal = $state(0);
  /** What the list says when it has no hits (help, Everything not running, errors). */
  let everythingEmpty = $state("");
  let everythingTimer: ReturnType<typeof setTimeout> | null = null;
  /** Only the newest request's answer is shown. */
  let everythingSeq = 0;

  const EVERYTHING_HELP = "PC 전체에서 파일 이름을 찾습니다 (Everything 문법: ext:pdf, \"두 단어\", C:\\work\\ 보고서)";

  async function everythingIdle(seq: number) {
    try {
      const s = await everythingStatus();
      if (seq !== everythingSeq) return;
      everythingEmpty = !s.running
        ? "Everything이 실행 중이 아닙니다. Everything(voidtools.com)을 설치·실행하면 PC 전체를 찾을 수 있습니다."
        : !s.db_loaded
          ? `Everything ${s.version ?? ""}이 색인을 만드는 중입니다. 잠시 뒤 다시 시도하세요.`
          : EVERYTHING_HELP;
    } catch (e) {
      if (seq === everythingSeq) everythingEmpty = String(e);
    }
  }

  function onEverythingQuery(query: string) {
    const q = query.trim();
    const seq = ++everythingSeq;
    if (everythingTimer) clearTimeout(everythingTimer);
    if (!q) {
      everythingHits = [];
      everythingTotal = 0;
      everythingEmpty = EVERYTHING_HELP;
      everythingIdle(seq);
      return;
    }
    everythingTimer = setTimeout(async () => {
      everythingTimer = null;
      try {
        const found = await everythingSearch(q);
        if (seq !== everythingSeq) return;
        everythingHits = found.hits;
        everythingTotal = found.total;
        everythingEmpty = "일치하는 파일 없음";
      } catch (e) {
        if (seq !== everythingSeq) return;
        everythingHits = [];
        everythingTotal = 0;
        everythingEmpty = String(e);
      }
    }, EVERYTHING_DEBOUNCE_MS);
  }

  function fileSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    const units = ["KB", "MB", "GB", "TB"];
    let v = bytes / 1024;
    let i = 0;
    while (v >= 1024 && i < units.length - 1) {
      v /= 1024;
      i++;
    }
    return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
  }

  function everythingItems(_query: string): PaletteItem[] {
    return everythingHits.map((h): PaletteItem => {
      const parent = h.path.length > h.name.length ? h.path.slice(0, h.path.length - h.name.length).replace(/\\$/, "") : "";
      const facts = [
        h.vault_path !== null ? "vault" : null,
        !h.folder && h.size !== null ? fileSize(h.size) : null,
        h.modified !== null ? new Date(h.modified).toLocaleDateString("ko-KR") : null,
      ].filter(Boolean);
      return {
        id: h.path,
        label: h.name,
        detail: parent || undefined,
        icon: h.folder ? "folder-open" : h.vault_path !== null ? "note-pen" : "file",
        hint: facts.join(" · ") || undefined,
      };
    });
  }

  const everythingFooter = $derived(
    `${everythingTotal > everythingHits.length ? `${everythingTotal.toLocaleString()}개 중 ${everythingHits.length}개 · ` : ""}`
      + "↑↓ 이동 · Enter 열기 · Ctrl+Enter 새 탭 · Shift+Enter 탐색기에서 보기 · Esc 닫기",
  );

  /** Vault files open here (notes, canvases, images, PDFs …); everything else with its own app. */
  async function pickEverything(item: PaletteItem, _query: string, keys: { shift: boolean; mod: boolean }) {
    const hit = everythingHits.find((h) => h.path === item.id);
    if (!hit) return;
    try {
      if (keys.shift) {
        await everythingOpen(hit.path, true);
      } else if (hit.vault_path !== null && hit.folder) {
        // Open the folder itself too, not just the ones above it.
        sideTab = "files";
        treeUi.reveal(hit.vault_path);
        treeUi.expanded[hit.vault_path] = true;
      } else if (hit.vault_path !== null && (isTextFile(hit.vault_path) || mediaKind(hit.vault_path))) {
        await openPath(hit.vault_path, keys.mod);
      } else {
        await everythingOpen(hit.path);
      }
    } catch (e) {
      error = String(e);
    }
  }

  // ------------------------------------------------ command palette (Ctrl+P)
  interface Command {
    id: string;
    /** Obsidian's id for the same command, so keys set there work here. */
    obsidian?: string;
    /** Default keys (Obsidian's defaults where it has the command). */
    keys?: Hotkey[];
    label: string;
    icon: IconName;
    /** Shown only when this holds (e.g. a note is open). */
    when?: () => boolean;
    run: () => void;
  }

  const hasVault = () => !!vault;
  const hasFile = () => !!vault && !!currentPath;
  const hasNote = () => !!vault && isNote;
  const showRightTab = (tab: typeof rightTab) => () => {
    showRight = true;
    rightTab = tab;
  };

  const COMMANDS: Command[] = [
    { id: "palette", obsidian: "command-palette:open", keys: [hk("Mod+P")], label: "명령 팔레트 열기", icon: "command",
      run: () => (palette = "commands") },
    { id: "switcher", obsidian: "switcher:open", keys: [hk("Mod+O")], label: "빠른 전환: 파일 열기", icon: "search", when: hasVault,
      run: () => (palette = "switcher") },
    { id: "everything", keys: [hk("Mod+Shift+E")], label: "Everything: PC 전체에서 파일 찾기", icon: "search",
      run: () => (palette = "everything") },
    { id: "new-note", obsidian: "file-explorer:new-file", keys: [hk("Mod+N")], label: "새 노트", icon: "file-plus", when: hasVault, run: () => newNoteIn("") },
    { id: "new-canvas", obsidian: "canvas:new-file", label: "새 캔버스", icon: "columns", when: hasVault, run: () => newCanvasIn("") },
    { id: "new-folder", obsidian: "file-explorer:new-folder", label: "새 폴더", icon: "folder-plus", when: hasVault, run: () => newFolderIn("") },
    { id: "today", obsidian: "daily-notes", label: "오늘의 노트 열기", icon: "note-pen", when: hasVault, run: openTodayNote },
    { id: "template", obsidian: "insert-template", keys: [hk("Alt+T")], label: "템플릿 삽입", icon: "template", when: hasNote, run: insertTemplate },
    { id: "search", obsidian: "global-search:open", keys: [hk("Mod+Shift+F")], label: "vault 검색", icon: "search", when: hasVault, run: showSearch },
    { id: "new-tab", obsidian: "workspace:new-tab", keys: [hk("Mod+T")], label: "새 탭", icon: "plus", when: hasVault, run: () => newTab() },
    { id: "close-tab", obsidian: "workspace:close", keys: [hk("Mod+W")], label: "현재 탭 닫기", icon: "close", when: hasVault,
      run: () => closeTab(workspace.pane, workspace.pane.active) },
    { id: "next-tab", obsidian: "workspace:next-tab", keys: [hk("Mod+Tab"), hk("Mod+PageDown")], label: "다음 탭", icon: "chevron-right", when: hasVault, run: () => cycleTab(1) },
    { id: "prev-tab", obsidian: "workspace:previous-tab", keys: [hk("Mod+Shift+Tab"), hk("Mod+PageUp")], label: "이전 탭", icon: "chevron-left", when: hasVault, run: () => cycleTab(-1) },
    { id: "split", obsidian: "workspace:split-vertical", keys: [hk("Mod+\\")], label: "오른쪽으로 분할", icon: "columns", when: () => !!vault && workspace.panes.length < MAX_PANES,
      run: splitRight },
    { id: "unsplit", label: "분할 닫기 (탭은 한 창으로 모음)", icon: "columns", when: () => workspace.panes.length > 1,
      run: () => workspace.unsplit() },
    { id: "calendar", label: "캘린더 열기", icon: "calendar", when: hasVault, run: () => showView("calendar") },
    { id: "tasks", label: "할일 열기", icon: "check-square", when: hasVault, run: () => showView("tasks") },
    { id: "finance", label: "재무 열기", icon: "wallet", when: hasVault, run: () => showView("finance") },
    { id: "graph", obsidian: "graph:open", label: "그래프 열기", icon: "graph", when: hasVault, run: () => showView("graph") },
    { id: "reading", obsidian: "markdown:toggle-preview", keys: [hk("Mod+E")], label: "읽기 보기 / 편집 전환", icon: "book-open", when: hasNote,
      run: () => toggleReading() },
    { id: "print", obsidian: "workspace:export-pdf", keys: [hk("Mod+Shift+P")], label: "PDF로 내보내기 / 인쇄", icon: "printer", when: hasNote, run: printNote },
    { id: "add-property", obsidian: "markdown:add-metadata-property", label: "속성 추가", icon: "plus", when: hasNote,
      run: async () => {
        if (workspace.leaf.reading) workspace.leaf.reading = false;
        await tick();
        leafViews[workspace.leaf.id]?.startAddingProperty();
      } },
    { id: "live-preview", obsidian: "editor:toggle-source", label: "라이브 미리보기 / 소스 모드 전환", icon: "pencil",
      run: () => (settings.livePreview = !settings.livePreview) },
    { id: "local-graph", obsidian: "graph:open-local", label: "현재 노트 주변 그래프", icon: "graph", when: hasNote,
      run: () => showView("graph", { local: true }) },
    { id: "rename", obsidian: "workspace:edit-file-title", label: "현재 파일 이름 바꾸기", icon: "pencil", when: hasFile,
      run: () => {
        if (!currentPath) return;
        sideTab = "files";
        treeUi.reveal(currentPath);
        treeUi.renaming = currentPath;
      } },
    { id: "trash", obsidian: "app:delete-file", label: "현재 파일 삭제", icon: "trash", when: hasFile,
      run: () => currentPath && trashItem(currentPath, false) },
    { id: "recovery", obsidian: "file-recovery:open", label: "파일 복구: 이전 버전 보기", icon: "clock", when: hasVault,
      run: () => (recovery = { start: isNote ? currentPath : null }) },
    { id: "reveal", obsidian: "open-with-default-app:show", label: "현재 파일을 탐색기에서 보기", icon: "folder-open", when: hasFile,
      run: () => currentPath && treeActions.reveal(currentPath) },
    { id: "right", obsidian: "app:toggle-right-sidebar", label: "오른쪽 패널 열기/닫기", icon: "panel-right", when: hasVault,
      run: () => (showRight = !showRight) },
    { id: "backlinks", obsidian: "backlink:open", label: "백링크 보기", icon: "link", when: hasNote, run: showRightTab("backlinks") },
    { id: "outline", obsidian: "outline:open", label: "개요 보기", icon: "outline", when: hasNote, run: showRightTab("outline") },
    { id: "tags", obsidian: "tag-pane:open", label: "태그 보기", icon: "hash", when: hasNote, run: showRightTab("tags") },
    { id: "theme", label: "라이트/다크 테마 전환", icon: "sliders",
      run: () => (settings.theme = settings.resolved === "dark" ? "light" : "dark") },
    { id: "settings", obsidian: "app:open-settings", keys: [hk("Mod+,")], label: "설정 열기", icon: "sliders", run: () => (showSettings = true) },
    { id: "save", obsidian: "editor:save-file", keys: [hk("Mod+S")], label: "저장", icon: "check", when: hasNote, run: () => activeDoc?.save() },
    { id: "refresh", label: "파일 목록 새로고침", icon: "refresh", when: hasVault, run: reload },
    { id: "vaults", obsidian: "app:open-vault", keys: [hk("Mod+Shift+O")], label: "vault 전환", icon: "folder-open", run: () => (palette = "vaults") },
    { id: "sync-now", label: "지금 동기화", icon: "sync", when: () => !!vault && !!syncInfo?.config, run: () => runSync() },
    { id: "conflicts", label: "동기화 충돌 합치기", icon: "sync", when: hasVault, run: () => (conflicts = { start: null }) },
    { id: "sync-settings", label: "Git 동기화 설정", icon: "sync", when: hasVault, run: () => (showSync = true) },
    { id: "bookmark", obsidian: "bookmarks:bookmark-current-view", label: "현재 노트 북마크 추가/빼기", icon: "bookmark", when: () => !!currentPath, run: () => { if (currentPath) toggleBookmarkOf(currentPath); } },
    { id: "bookmarks", obsidian: "bookmarks:open", label: "북마크 보기", icon: "bookmark", when: hasVault, run: () => (sideTab = "bookmarks") },
    { id: "outgoing", obsidian: "outgoing-links:open", label: "나가는 링크 보기", icon: "arrow-up-right", when: hasVault, run: () => { showRight = true; rightTab = "outgoing"; } },
    { id: "vault", label: "다른 폴더를 vault로 열기", icon: "folder-plus", run: () => pickVault() },
    { id: "new-window", obsidian: "workspace:new-window", keys: [hk("Mod+Shift+N")], label: "새 창", icon: "columns",
      run: () => openWindow(null).catch((e) => (error = String(e))) },
    { id: "vault-window", label: "다른 폴더를 새 창에서 vault로 열기", icon: "folder-plus", run: () => pickVault(true) },
    { id: "reveal-vault", label: "vault 폴더를 탐색기에서 보기", icon: "folder-open", when: hasVault,
      run: () => treeActions.reveal("") },
  ];

  /** The id a command's keys are saved under in hotkeys.json. */
  const hotkeyId = (c: Command) => c.obsidian ?? `rusidian:${c.id}`;
  const keysOf = (c: Command) => hotkeys.keysFor(hotkeyId(c), c.keys ?? []);

  /** Commands as the hotkey settings list them. */
  const hotkeyCommands = COMMANDS.map((c) => ({ id: hotkeyId(c), label: c.label, icon: c.icon, defaults: c.keys ?? [] }));

  function commandItems(query: string): PaletteItem[] {
    const available = COMMANDS.filter((c) => !c.when || c.when());
    const toItem = (c: Command, marks?: number[]): PaletteItem => {
      const keys = keysOf(c);
      return { id: c.id, label: c.label, icon: c.icon, hint: keys.length ? hotkeyLabel(keys[0]) : undefined, marks };
    };
    if (!query.trim()) return available.map((c) => toItem(c));
    return available
      .map((c) => ({ c, m: fuzzyMatch(query, c.label) }))
      .filter((x) => x.m)
      .sort((a, b) => b.m!.score - a.m!.score)
      .map((x) => toItem(x.c, x.m!.indices));
  }

  function runCommand(item: PaletteItem) {
    COMMANDS.find((c) => c.id === item.id)?.run();
  }

  /** Print the current note, or save it as PDF from the print dialog. */
  async function printNote() {
    const doc = activeDoc;
    if (!doc?.isNote || !doc.loaded) return;
    try {
      await doc.save();
      printing = { path: doc.path, content: doc.content };
      await tick();
      await printView?.settled();
      // The PDF's suggested file name.
      const title = document.title;
      document.title = displayName(doc.path.split("/").pop()!);
      window.addEventListener("afterprint", () => {
        printing = null;
        document.title = title;
      }, { once: true });
      window.print();
    } catch (e) {
      printing = null;
      error = String(e);
    }
  }

  async function toggleReading(target = workspace.leaf) {
    await docFor(target)?.save();
    target.reading = !target.reading;
  }

  function onKeydown(e: KeyboardEvent) {
    // Plain keys are typing; only combinations (and F-keys) run commands.
    if (!e.ctrlKey && !e.metaKey && !e.altKey && !/^F\d{1,2}$/.test(e.key)) return;
    for (const c of COMMANDS) {
      if (!keysOf(c).some((h) => matches(e, h))) continue;
      // Taken even when the command can't run now (e.g. Ctrl+P never prints the page).
      e.preventDefault();
      if (!c.when || c.when()) c.run();
      return;
    }
  }

  onMount(() => {
    let unlisten: (() => void) | undefined;
    let destroyed = false;
    onVaultChanged(handleVaultChanged)
      .then((u) => (destroyed ? u() : (unlisten = u)))
      .catch(() => {});
    refreshVaults();
    // The vault this window was opened for (the main window: the last one).
    startupVault()
      .then((path) => {
        if (path) loadVault(path);
      })
      .catch(() => {});
    const stopReminders = startReminders(() => settings.notifications && !!vault);
    // Save edits before the window closes mid-autosave. Tauri waits for this
    // handler before destroying the window (and with it this window's vault on
    // the Rust side), so the last keystrokes are written first.
    let unlistenClose: (() => void) | undefined;
    import("@tauri-apps/api/window")
      .then(({ getCurrentWindow }) =>
        getCurrentWindow().onCloseRequested(async () => {
          try {
            // Bounded, so a stuck save can never keep the window open.
            await Promise.race([docs.flushAll(), sleep(3000)]);
          } catch {
            // Closing anyway; the error toast can't be seen now.
          }
        }),
      )
      .then((u) => (destroyed ? u() : (unlistenClose = u)))
      .catch(() => {});
    // A reload (not a close) still tries.
    const onBeforeUnload = () => {
      docs.flushAll();
    };
    window.addEventListener("beforeunload", onBeforeUnload);
    // An update installing (from any window) ends the app: save first.
    let unlistenUpdate: (() => void) | undefined;
    onUpdateFlush(() => docs.flushAll())
      .then((u) => (destroyed ? u() : (unlistenUpdate = u)))
      .catch(() => {});
    // Look for a new version once, from the main window only (not in `npm run
    // tauri dev`, where it would install a release over the dev build).
    const updateTimer = setTimeout(async () => {
      if (import.meta.env.DEV) return;
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      if (destroyed || getCurrentWindow().label !== "main") return;
      const u = await updates.check();
      if (!u || destroyed) return;
      notice = `새 버전 ${u.version}이 나왔습니다 (지금 ${u.currentVersion}).`;
      noticeAction = {
        notice,
        label: "설치하고 다시 시작",
        run: async () => {
          notice = "새 버전을 내려받는 중입니다…";
          await updates.install();
          // Still here: it failed (on success the app restarts).
          if (updates.error) notice = `업데이트하지 못했습니다: ${updates.error}`;
        },
      };
    }, UPDATE_CHECK_DELAY_MS);
    return () => {
      destroyed = true;
      clearTimeout(updateTimer);
      unlistenUpdate?.();
      unlisten?.();
      unlistenClose?.();
      stopReminders();
      window.removeEventListener("beforeunload", onBeforeUnload);
    };
  });

  // ------------------------------------------------------------- tab labels
  const VIEW_HEADS: Record<ViewKind, { label: string; icon: IconName }> = {
    tasks: { label: "할일", icon: "check-square" },
    calendar: { label: "캘린더", icon: "calendar" },
    finance: { label: "재무", icon: "wallet" },
    graph: { label: "그래프", icon: "graph" },
  };

  function tabTitle(l: Leaf): string {
    if (l.kind === "file") return l.path ? displayName(baseName(l.path)) : "새 탭";
    if (l.kind === "empty") return "새 탭";
    const head = VIEW_HEADS[l.kind].label;
    return l.kind === "graph" && l.graphLocal && l.path ? `${head}: ${displayName(baseName(l.path))}` : head;
  }

  function tabIcon(l: Leaf): IconName {
    if (l.kind === "file" || l.kind === "empty") return "file";
    return VIEW_HEADS[l.kind].icon;
  }

  let rightPanel = $derived(!!vault && isNote && showRight);
  const RIGHT_TABS: { id: typeof rightTab; label: string; icon: IconName }[] = [
    { id: "backlinks", label: "백링크", icon: "link" },
    { id: "outgoing", label: "나가는 링크", icon: "arrow-up-right" },
    { id: "outline", label: "개요", icon: "outline" },
    { id: "tags", label: "태그", icon: "hash" },
  ];
  let tagCount = $state<number | null>(null);
  let backlinkCount = $state<number | null>(null);
  let outgoingCount = $state<number | null>(null);
  const viewActive = (kind: ViewKind) => leaf.kind === kind;
</script>

<svelte:window onkeydown={onKeydown} />

<!-- No native title bar: the top row of each pane is the drag area and the
     window buttons sit at the right end of the rightmost top row. -->
<div class="app" class:with-right={rightPanel}>
  <WindowControls />
  <aside class="sidebar">
    <header class="sidebar-head top-row" data-drag>
      <button class="vault-name" onclick={() => (palette = "vaults")}
        title="{vault ? `${vault.root}\n` : ""}vault 전환 (Ctrl+Shift+O)" aria-label="vault 전환">
        <span class="vault-label">{vault?.name ?? "Rusidian"}</span>
        <Icon name="chevron-down" size={13} />
      </button>
      {#if vault}
        <button class="icon" onclick={() => newNoteIn("")} title="새 노트 (Ctrl+N)" aria-label="새 노트"><Icon name="file-plus" size={17} /></button>
        <button class="icon" onclick={() => newFolderIn("")} title="새 폴더" aria-label="새 폴더"><Icon name="folder-plus" size={17} /></button>
        <button class="icon" onclick={reload} title="새로고침" aria-label="새로고침"><Icon name="refresh" size={17} /></button>
      {/if}
      <button class="icon" onclick={() => pickVault()} title="Vault 열기" aria-label="Vault 열기"><Icon name="folder-open" size={17} /></button>
    </header>

    {#if vault}
      <nav class="views" aria-label="보기" use:slider={{ selector: ".active" }}>
        <button onclick={openTodayNote} title="오늘 데일리 노트 (없으면 만들기)"><Icon name="note-pen" /> 오늘의 노트</button>
        {#each ["calendar", "tasks", "finance", "graph"] as const as kind (kind)}
          <button class:active={viewActive(kind)} aria-current={viewActive(kind) ? "page" : undefined}
            onclick={() => showView(kind)}><Icon name={VIEW_HEADS[kind].icon} /> {VIEW_HEADS[kind].label}</button>
        {/each}
      </nav>
      <div class="tabs" role="tablist" use:slider={{ selector: ".active", underline: true }}>
        <button role="tab" aria-selected={sideTab === "files"} class:active={sideTab === "files"}
          onclick={() => (sideTab = "files")}>파일</button>
        <button role="tab" aria-selected={sideTab === "search"} class:active={sideTab === "search"}
          onclick={showSearch} title="Ctrl+Shift+F">검색</button>
        <button role="tab" aria-selected={sideTab === "bookmarks"} class:active={sideTab === "bookmarks"}
          onclick={() => (sideTab = "bookmarks")}>북마크</button>
      </div>
    {/if}

    <nav class="side-body" hidden={sideTab !== "files" && !!vault}>
      {#if vault}
        <FileExplorer tree={vault.tree} selected={currentPath} ui={treeUi} actions={treeActions} />
      {:else}
        <p class="hint">Obsidian vault 폴더를 열어주세요.</p>
      {/if}
    </nav>
    {#if vault}
      <div class="side-body" hidden={sideTab !== "search"}>
        <Search bind:this={searchPanel} onopen={openPath} version={vaultVersion} selected={currentPath} />
      </div>
      <div class="side-body" hidden={sideTab !== "bookmarks"}>
        <BookmarksPane items={bookmarks} currentPath={currentPath ?? ""} onopen={openPath}
          onheading={(path, heading) => followLink("", heading, path)} onsearch={searchFor}
          onreveal={(path) => { sideTab = "files"; treeUi.reveal(path); }} onremove={removeBookmarkAt} />
      </div>
    {/if}
    <footer class="sidebar-foot">
      <button class="icon" onclick={() => (showSettings = true)} title="설정 (Ctrl+,)" aria-label="설정"><Icon name="sliders" size={17} /></button>
      <button class="icon" onclick={() => (palette = "commands")} title="명령 팔레트 (Ctrl+P)" aria-label="명령 팔레트"><Icon name="command" size={17} /></button>
      {#if vault}
        <button class="icon" onclick={() => (palette = "switcher")} title="빠른 전환 (Ctrl+O)" aria-label="빠른 전환"><Icon name="search" size={17} /></button>
        <span class="foot-spacer"></span>
        <button class={["icon", "sync-btn", { spinning: syncRunning }]}
          onclick={() => (syncInfo?.config ? runSync() : (showSync = true))}
          oncontextmenu={(e) => { e.preventDefault(); showSync = true; }}
          title={syncInfo?.config
            ? `${syncStatus?.text ?? "Git 동기화"} — 클릭: 지금 동기화, 오른쪽 클릭: 설정`
            : "Git 동기화 설정"}
          aria-label="Git 동기화">
          <Icon name="sync" size={17} />
          {#if syncInfo?.config && syncStatus}
            <span class={["sync-dot", syncStatus.ok ? "ok" : "bad"]} aria-hidden="true"></span>
          {/if}
        </button>
        {#if syncStatus && !syncStatus.ok}
          <button class="sync-error" onclick={() => (showSync = true)} title={syncStatus.text}>동기화 실패</button>
        {/if}
      {/if}
    </footer>
  </aside>

  <main class="main">
    {#each workspace.panes as pane, pi (pane.id)}
      {@const shown = pane.leaf}
      {@const shownDoc = docFor(shown)}
      {@const shownIsNote = shown.kind === "file" && !!shown.path?.toLowerCase().endsWith(".md")}
      <section class="pane" class:focused={pi === workspace.activePane && workspace.panes.length > 1}
        onpointerdowncapture={() => (workspace.activePane = pi)} aria-label="창 {pi + 1}">
        <header class="pane-head top-row" data-drag>
          {#if vault}
            <div class="tabbar" role="tablist" aria-label="탭">
              {#each pane.tabs as t, ti (t.id)}
                {@const tDoc = docFor(t)}
                <div class="tab" class:active={ti === pane.active} role="tab" tabindex="0" aria-selected={ti === pane.active}
                  title={t.path ?? tabTitle(t)}
                  onclick={() => workspace.focus(pane, ti)}
                  onkeydown={(e) => (e.key === "Enter" || e.key === " ") && workspace.focus(pane, ti)}
                  onauxclick={(e) => e.button === 1 && closeTab(pane, ti)}>
                  <Icon name={tabIcon(t)} size={14} />
                  <span class="tab-label">{tabTitle(t)}</span>
                  {#if tDoc?.dirty}<span class="unsaved" title="저장 중" aria-label="저장 중"></span>{/if}
                  <button class="tab-x" onclick={(e) => { e.stopPropagation(); closeTab(pane, ti); }}
                    title="탭 닫기 (Ctrl+W)" aria-label="{tabTitle(t)} 탭 닫기"><Icon name="close" size={12} /></button>
                </div>
              {/each}
              <button class="icon new-tab" onclick={() => newTab(pane)} title="새 탭 (Ctrl+T)" aria-label="새 탭"><Icon name="plus" size={15} /></button>
            </div>
            <div class="pane-tools">
              {#if shownIsNote}
                {@const marked = bookmarked.has(shown.path ?? "")}
                <button class={["icon", "toggle", { on: marked }]} aria-pressed={marked} onclick={() => shown.path && toggleBookmarkOf(shown.path)}
                  title={marked ? "북마크에서 빼기" : "북마크에 추가"} aria-label="북마크">
                  <Icon name={marked ? "bookmark-check" : "bookmark"} size={17} />
                </button>
                <button class="icon toggle" class:on={shown.reading} aria-pressed={shown.reading} onclick={() => toggleReading(shown)}
                  title={shown.reading ? "편집하기 (Ctrl+E)" : "읽기 보기 (Ctrl+E)"} aria-label="읽기 보기">
                  <Icon name={shown.reading ? "pencil" : "book-open"} size={17} />
                </button>
                <button class="icon" onclick={() => { workspace.focus(pane); showView("graph", { local: true }); }}
                  title="이 노트 주변 그래프" aria-label="이 노트 주변 그래프"><Icon name="graph" size={17} /></button>
              {/if}
              {#if workspace.panes.length < MAX_PANES}
                <button class="icon" onclick={() => { workspace.focus(pane); splitRight(); }}
                  title="오른쪽으로 분할 (Ctrl+\)" aria-label="오른쪽으로 분할"><Icon name="columns" size={17} /></button>
              {:else}
                <button class="icon" onclick={() => { workspace.focus(pane); workspace.unsplit(); }}
                  title="분할 닫기 (탭은 한 창으로 모음)" aria-label="분할 닫기"><Icon name="columns" size={17} /></button>
              {/if}
              {#if pi === workspace.panes.length - 1 && isNote}
                <button class="icon toggle" class:on={showRight} aria-pressed={showRight}
                  onclick={() => (showRight = !showRight)} title="오른쪽 패널 (백링크 · 개요 · 태그)" aria-label="오른쪽 패널"><Icon name="panel-right" size={17} /></button>
              {/if}
            </div>
          {/if}
        </header>
        {#key shown.id}
          <LeafView bind:this={leafViews[shown.id]} leaf={shown} doc={shownDoc}
            active={pi === workspace.activePane} vaultOpen={!!vault} noteCount={vault?.note_count ?? 0}
            version={vaultVersion} actions={leafActions} recentVaults={vaults} />
        {/key}
      </section>
    {/each}

    <div class="toasts" aria-live="polite">
      {#if error}
        <div class="toast error" role="alert">
          <span>{error}</span>
          <button class="dismiss" onclick={() => (error = null)} aria-label="닫기"><Icon name="close" size={14} /></button>
        </div>
      {/if}
      {#if notice}
        <div class="toast notice" role="status">
          <span>{notice}</span>
          {#if noticeAction?.notice === notice}
            <button class="notice-action" onclick={() => { const action = noticeAction; notice = null; action?.run(); }}>{noticeAction.label}</button>
          {/if}
          <button class="dismiss" onclick={() => (notice = null)} aria-label="닫기"><Icon name="close" size={14} /></button>
        </div>
      {/if}
    </div>
  </main>

  {#if rightPanel && currentPath}
    <aside class="rightbar">
      <header class="right-head top-row" data-drag>
        <div class="right-tabs" role="tablist" aria-label="오른쪽 패널" use:slider={{ selector: ".on", color: "var(--accent-bg)" }}>
          {#each RIGHT_TABS as t (t.id)}
            <button class="icon" role="tab" class:on={rightTab === t.id} aria-selected={rightTab === t.id}
              onclick={() => (rightTab = t.id)} title={t.label} aria-label={t.label}><Icon name={t.icon} size={17} /></button>
          {/each}
        </div>
      </header>
      <div class="right-title">
        {RIGHT_TABS.find((t) => t.id === rightTab)?.label}
        {#if rightTab === "backlinks" && backlinkCount !== null}<span class="count">{backlinkCount}</span>{/if}
        {#if rightTab === "outgoing" && outgoingCount !== null}<span class="count">{outgoingCount}</span>{/if}
        {#if rightTab === "tags" && tagCount !== null}<span class="count">{tagCount}</span>{/if}
      </div>
      <div class="right-body" role="tabpanel">
        {#if rightTab === "backlinks"}
          <Backlinks path={currentPath} version={vaultVersion} onopen={openPath} bind:count={backlinkCount}
            beforewrite={flushSave} onerror={(m) => (error = m)} />
        {:else if rightTab === "outgoing"}
          <OutgoingLinks path={currentPath} version={vaultVersion} onopen={openPath}
            onfollow={(target) => followLink(target, null, currentPath)} bind:count={outgoingCount} />
        {:else if rightTab === "outline"}
          <Outline content={activeDoc?.content ?? ""} onjump={gotoLine} />
        {:else}
          <TagPane version={vaultVersion} onsearch={searchTag} bind:count={tagCount} />
        {/if}
      </div>
    </aside>
  {/if}
</div>

{#if palette === "switcher"}
  <Palette placeholder="파일 이름을 입력하세요" items={switcherItems} onpick={pickFile}
    onclose={() => (palette = null)} footer="↑↓ 이동 · Enter 열기 · Ctrl+Enter 새 탭 · Shift+Enter 새 노트 · Esc 닫기" />
{:else if palette === "everything"}
  <Palette placeholder="PC 전체에서 찾을 파일 이름 (Everything)" items={everythingItems} onquery={onEverythingQuery}
    onpick={pickEverything} onclose={() => (palette = null)} footer={everythingFooter} empty={everythingEmpty} />
{:else if palette === "commands"}
  <Palette placeholder="명령을 입력하세요" items={commandItems} onpick={runCommand}
    onclose={() => (palette = null)} footer="↑↓ 이동 · Enter 실행 · Esc 닫기" empty="일치하는 명령 없음" />
{:else if palette === "vaults"}
  <Palette placeholder="열 vault를 고르세요" items={vaultItems} onpick={pickVaultItem} onremove={removeVaultItem}
    onclose={() => (palette = null)} footer="↑↓ 이동 · Enter 이 창에서 열기 · Ctrl+Enter 새 창 · × 목록에서 빼기 · Esc 닫기" empty="일치하는 vault 없음" />
{:else if palette === "templates"}
  <Palette placeholder="넣을 템플릿을 고르세요" items={templateItems} onpick={pickTemplate}
    onclose={() => (palette = null)} footer={"↑↓ 이동 · Enter 커서 위치에 넣기 · {{title}} {{date}} {{time}} 채움"} empty="일치하는 템플릿 없음" />
{/if}
{#if showSettings}
  <SettingsDialog commands={hotkeyCommands} hasVault={!!vault} onclose={() => (showSettings = false)}
    onnotesettings={() => vaultVersion++} />
{/if}
{#if printing}
  <PrintView bind:this={printView} path={printing.path} content={printing.content} />
{/if}
{#if conflicts && vault}
  <ConflictDialog start={conflicts.start} beforewrite={flushSave} onclose={() => (conflicts = null)}
    onchanged={(paths) => handleVaultChanged(paths)} />
{/if}
{#if recovery && vault}
  <FileRecovery start={recovery.start} beforewrite={flushSave} onclose={() => (recovery = null)}
    onrestored={(path) => handleVaultChanged([path])} />
{/if}
{#if showSync && vault && syncInfo}
  <SyncDialog info={syncInfo} tree={vault.tree} status={syncStatus?.text ?? null} running={syncRunning}
    onclose={() => (showSync = false)} onsave={saveSync} />
{/if}

<style>
  :global(:root) {
    --bg: #ffffff;
    --bg-side: #f6f6f6;
    --text: #222222;
    --text-muted: #5c5c5c;
    --text-faint: #999999;
    --border: #e3e3e3;
    --hover: #ebebeb;
    --active: #e0e0f8;
    --accent: #7c5cdb;
    --accent-soft: #7c5cdb66;
    --accent-bg: #7c5cdb1a;
    --code-bg: #f0f0f0;
    --selection: #c9bdf280;
    --selection-match: #c9bdf240;
    --yaml-value: #b05f00;
    --danger-bg: #fdecec;
    --danger: #b3261e;
    --notice-bg: #eef3fd;
    --notice: #2856a8;
    --highlight-bg: #fff3a3;
    --sunday: #d1453b;
    --saturday: #2f6fd6;
    --font-text: Inter, "Segoe UI", "Malgun Gothic", system-ui, sans-serif;
    --font-mono: "JetBrains Mono", Consolas, "D2Coding", monospace;
    --editor-font-size: 16px;
    --line-width: 800px;
    color-scheme: light;
  }
  /* data-theme is set from the settings (app.html sets it before first paint). */
  :global(:root[data-theme="dark"]) {
    color-scheme: dark;
    --bg: #1e1e1e;
    --bg-side: #262626;
    --text: #dadada;
    --text-muted: #a3a3a3;
    --text-faint: #6e6e6e;
    --border: #363636;
    --hover: #2f2f2f;
    --active: #3a3550;
    --accent: #9a7ff0;
    --accent-soft: #9a7ff066;
    --accent-bg: #9a7ff024;
    --code-bg: #2a2a2a;
    --selection: #9a7ff04d;
    --selection-match: #9a7ff026;
    --yaml-value: #e5a663;
    --danger-bg: #3b1e1e;
    --danger: #ff8a80;
    --notice-bg: #1f2a3d;
    --notice: #9cc0ff;
    --highlight-bg: #6b5c00;
    --sunday: #ff8a80;
    --saturday: #8ab4ff;
  }
  :global(html, body) {
    margin: 0;
    height: 100%;
    /* The window itself never scrolls; only the panes inside do. */
    overflow: hidden;
    background: var(--bg);
    color: var(--text);
    font-family: var(--font-text);
    /* Like a desktop app: dragging the mouse selects the text you write or
       read, not menus, buttons and labels around it. */
    user-select: none;
    -webkit-user-select: none;
  }
  :global(:is(input, textarea, [contenteditable="true"], .cm-editor, .reading, [role="alert"], .error, .selectable)) {
    user-select: text;
    -webkit-user-select: text;
  }
  /* Views and tabs ease in when they appear. No fill after the end, so no
     transform stays behind (it would trap position: fixed dialogs inside). */
  @keyframes -global-view-in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
  :global(.view-in) {
    animation: view-in 0.2s ease-out backwards;
  }
  @media (prefers-reduced-motion: reduce) {
    :global(.view-in) {
      animation: none;
    }
  }
  :global([hidden]) {
    display: none !important;
  }
  .app {
    /* Top row height of every pane, and the window button width. */
    --top-h: 40px;
    --control-w: 46px;
    --controls: calc(var(--control-w) * 3 + 8px);
    position: relative;
    height: 100vh;
    display: grid;
    grid-template-columns: 260px minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
    overflow: hidden;
  }
  .app.with-right {
    grid-template-columns: 260px minmax(0, 1fr) 280px;
  }
  /* Narrow windows: drop the right panel rather than squeezing the editor. */
  @media (max-width: 900px) {
    .app.with-right {
      grid-template-columns: 260px minmax(0, 1fr);
    }
    .rightbar {
      display: none;
    }
    .pane:last-of-type .pane-head {
      padding-right: var(--controls);
    }
  }
  /* Grid/flex children default to min-height: auto, which lets long content
     stretch them past the window. min-height: 0 keeps them window-sized so
     the overflow: auto panes below do the scrolling. */
  .sidebar {
    display: flex;
    flex-direction: column;
    background: var(--bg-side);
    border-right: 1px solid var(--border);
    min-width: 0;
    min-height: 0;
  }
  /* Every pane starts with a row of the same height; together they form the
     window's drag area (see WindowControls). */
  .top-row {
    flex: none;
    box-sizing: border-box;
    height: var(--top-h);
    display: flex;
    align-items: center;
    border-bottom: 1px solid var(--border);
    user-select: none;
    -webkit-user-select: none;
  }
  .sidebar-head {
    gap: 4px;
    padding: 0 10px 0 14px;
  }
  /* The vault name opens the vault switcher. */
  .vault-name {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 4px;
    margin-left: -6px;
    padding: 4px 6px;
    border: 0;
    border-radius: 5px;
    background: none;
    color: var(--text);
    font: inherit;
    font-weight: 600;
    font-size: 14px;
    text-align: left;
    cursor: pointer;
  }
  .vault-name:hover {
    background: var(--hover);
  }
  .vault-name :global(.icon) {
    flex: none;
    color: var(--text-faint);
  }
  .vault-label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* Icon buttons: muted at rest, full text color on hover, accent when on. */
  .icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 0;
    background: none;
    color: var(--text-muted);
    padding: 5px;
    border-radius: 5px;
    cursor: pointer;
  }
  .icon:hover {
    background: var(--hover);
    color: var(--text);
  }
  .toggle {
    color: var(--text-faint);
  }
  .toggle.on {
    color: var(--accent);
    background: var(--accent-bg);
  }
  .views {
    display: flex;
    flex-direction: column;
    padding: 6px 6px 0;
  }
  .views button {
    display: flex;
    align-items: center;
    gap: 9px;
    border: 0;
    background: none;
    color: var(--text-muted);
    font: inherit;
    font-size: 13px;
    text-align: left;
    padding: 6px 8px;
    border-radius: 4px;
    cursor: pointer;
  }
  .views button:hover {
    background: var(--hover);
    color: var(--text);
  }
  .views button.active {
    /* the sliding highlight (lib/slider.ts) draws the background */
    background: none;
    color: var(--text);
  }
  .views button.active:hover {
    background: var(--hover);
  }
  .views button :global(.icon) {
    transition: color 0.2s;
  }
  .views button.active :global(.icon) {
    color: var(--accent);
  }
  .tabs {
    display: flex;
    gap: 2px;
    padding: 6px 8px 0;
  }
  .tabs button {
    flex: 1;
    border: 0;
    border-bottom: 2px solid transparent;
    background: none;
    color: var(--text-muted);
    font: inherit;
    font-size: 13px;
    padding: 6px 0;
    cursor: pointer;
  }
  .tabs button {
    transition: color 0.2s;
  }
  .tabs button.active {
    color: var(--text);
  }
  .side-body {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 6px;
  }
  .hint {
    color: var(--text-faint);
    font-size: 13px;
    padding: 8px;
  }
  .sidebar-foot {
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px 8px;
    border-top: 1px solid var(--border);
  }
  .foot-spacer {
    flex: 1;
  }
  .sync-btn {
    position: relative;
  }
  .sync-btn.spinning :global(.icon) {
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(-360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .sync-btn.spinning :global(.icon) {
      animation: none;
      opacity: 0.5;
    }
  }
  .sync-dot {
    position: absolute;
    right: 3px;
    bottom: 3px;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    box-shadow: 0 0 0 2px var(--bg-side);
  }
  .sync-dot.ok {
    background: #0ca30c;
  }
  .sync-dot.bad {
    background: var(--danger);
  }
  .sync-error {
    border: 0;
    background: none;
    color: var(--danger);
    font: inherit;
    font-size: 12px;
    padding: 2px 4px;
    cursor: pointer;
  }
  /* ------------------------------------------------------- panes and tabs */
  .main {
    position: relative;
    display: flex;
    min-width: 0;
    min-height: 0;
  }
  .pane {
    flex: 1 1 0;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .pane + .pane {
    border-left: 1px solid var(--border);
  }
  .pane-head {
    gap: 4px;
    padding: 0 8px 0 6px;
    background: var(--bg-side);
  }
  .pane.focused .pane-head {
    box-shadow: inset 0 -2px 0 var(--accent-soft);
  }
  /* The window buttons sit over whichever top row reaches the right edge. */
  .app:not(.with-right) .pane:last-of-type .pane-head {
    padding-right: var(--controls);
  }
  .tabbar {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: flex-end;
    align-self: stretch;
    gap: 1px;
    padding-top: 6px;
    overflow: hidden;
  }
  .tab {
    flex: 0 1 180px;
    min-width: 56px;
    display: flex;
    align-items: center;
    gap: 6px;
    box-sizing: border-box;
    height: 100%;
    padding: 0 4px 0 10px;
    border-radius: 7px 7px 0 0;
    color: var(--text-muted);
    font-size: 13px;
    cursor: default;
    outline: none;
  }
  .tab:hover {
    background: var(--hover);
  }
  .tab.active {
    background: var(--bg);
    color: var(--text);
    box-shadow: 0 0 0 1px var(--border);
    clip-path: inset(-1px -1px 0 -1px);
  }
  .tab:focus-visible {
    box-shadow: inset 0 0 0 2px var(--accent);
  }
  .tab :global(.icon) {
    flex: none;
    color: var(--text-faint);
  }
  .tab-label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .unsaved {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 3px;
    background: var(--accent);
  }
  .tab-x {
    flex: none;
    display: inline-flex;
    border: 0;
    background: none;
    color: var(--text-faint);
    padding: 3px;
    border-radius: 4px;
    cursor: pointer;
    opacity: 0;
  }
  .tab:hover .tab-x,
  .tab.active .tab-x,
  .tab-x:focus-visible {
    opacity: 1;
  }
  .tab-x:hover {
    background: var(--active);
    color: var(--text);
  }
  .new-tab {
    align-self: center;
    margin-left: 2px;
  }
  .pane-tools {
    flex: none;
    display: flex;
    gap: 2px;
  }
  .toasts {
    position: absolute;
    right: 16px;
    bottom: 16px;
    z-index: 30;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: min(520px, calc(100% - 32px));
  }
  .toast {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px 10px 14px;
    border-radius: 8px;
    font-size: 13px;
    box-shadow: 0 6px 20px rgb(0 0 0 / 0.18);
  }
  .toast.error {
    background: var(--danger-bg);
    color: var(--danger);
  }
  .toast.notice {
    background: var(--notice-bg);
    color: var(--notice);
  }
  .toast span {
    flex: 1;
  }
  .notice-action {
    flex: none;
    border: 1px solid currentColor;
    border-radius: 5px;
    background: none;
    color: inherit;
    font: inherit;
    font-size: 12px;
    padding: 2px 8px;
    cursor: pointer;
    white-space: nowrap;
  }
  .dismiss {
    display: inline-flex;
    border: 0;
    background: none;
    color: inherit;
    padding: 2px;
    cursor: pointer;
    opacity: 0.7;
  }
  .dismiss:hover {
    opacity: 1;
  }
  /* ------------------------------------------------------------ right panel */
  .rightbar {
    display: flex;
    flex-direction: column;
    background: var(--bg-side);
    border-left: 1px solid var(--border);
    min-width: 0;
    min-height: 0;
  }
  .right-head {
    padding: 0 var(--controls) 0 8px;
  }
  .right-tabs {
    display: flex;
    gap: 2px;
  }
  .right-tabs .icon.on {
    color: var(--accent);
    background: none;
  }
  .right-title {
    flex: none;
    display: flex;
    gap: 6px;
    padding: 10px 14px 4px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-muted);
  }
  .right-title .count {
    font-weight: 400;
    color: var(--text-faint);
  }
  .right-body {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
</style>
