import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface TreeNode {
  name: string;
  /** Vault-relative path with `/` separators. */
  path: string;
  is_dir: boolean;
  children: TreeNode[];
}

export interface VaultInfo {
  root: string;
  name: string;
  tree: TreeNode[];
  note_count: number;
  /** False if the folder can't be watched; outside edits need a manual refresh. */
  watching: boolean;
}

export interface Snippet {
  line: number;
  before: string;
  hit: string;
  after: string;
}

export interface SearchHit {
  path: string;
  title: string;
  /** All search terms appear in the file path. */
  path_match: boolean;
  count: number;
  snippets: Snippet[];
}

export interface LineRef {
  line: number;
  text: string;
}

export interface Backlink {
  source: string;
  title: string;
  lines: LineRef[];
}

export const searchVault = (query: string) => invoke<SearchHit[]>("search_vault", { query });

export interface TagCount {
  /** Without `#`; nested tags use `/`. */
  tag: string;
  /** Notes that have the tag. */
  count: number;
}

export const listTags = () => invoke<TagCount[]>("list_tags");

/** Every file path in the tree, depth-first. */
export function filePaths(nodes: TreeNode[], out: string[] = []): string[] {
  for (const n of nodes) {
    if (n.is_dir) filePaths(n.children, out);
    else out.push(n.path);
  }
  return out;
}
export const getBacklinks = (path: string) => invoke<Backlink[]>("backlinks", { path });
export const resolveLink = (source: string, target: string) =>
  invoke<string | null>("resolve_link", { source, target });
/** Creates an empty note (".md" appended if missing); returns its path. */
export const createNote = (path: string) => invoke<string>("create_note", { path });

export interface MoveResult {
  /** New path of the moved file or folder. */
  path: string;
  /** Notes whose links were rewritten to follow the move. */
  updated: string[];
  /** Notes whose links could not be rewritten. */
  failed: string[];
}

/** Creates "제목 없음.md" (or "제목 없음 1.md", …) in `folder`; returns its path. */
export const newNote = (folder: string) => invoke<string>("new_note", { folder });
/** Creates an empty "제목 없는 캔버스.canvas" in `folder`; returns its path. */
export const newCanvas = (folder: string) => invoke<string>("new_canvas", { folder });
/** Creates "새 폴더" (or "새 폴더 1", …) in `parent`; returns its path. */
export const newFolder = (parent: string) => invoke<string>("new_folder", { parent });
/** Renames in place; `newName` includes the extension. Links follow. */
export const renamePath = (from: string, newName: string) =>
  invoke<MoveResult>("rename_path", { from, newName });
/** Moves into `folder` ("" = vault root). Links follow. */
export const movePath = (from: string, folder: string) => invoke<MoveResult>("move_path", { from, folder });
/** Sends to the system trash. */
export const trashPath = (path: string) => invoke<void>("trash_path", { path });
/** Shows the file in Explorer ("" = the vault folder). */
export const revealPath = (path: string) => invoke<void>("reveal_path", { path });

/** Where `path` ends up when `from` moves to `to` (null if unaffected). */
export function movedPath(path: string, from: string, to: string): string | null {
  if (path === from) return to;
  return path.startsWith(`${from}/`) ? to + path.slice(from.length) : null;
}

export type TaskKind = "todo" | "in_progress" | "done" | "cancelled";
export type Priority = "highest" | "high" | "medium" | "normal" | "low" | "lowest";

export interface TaskItem {
  path: string;
  title: string;
  line: number;
  /** The line as written; the backend refuses edits if it changed. */
  raw: string;
  status: string;
  kind: TaskKind;
  text: string;
  due: string | null;
  scheduled: string | null;
  start: string | null;
  created: string | null;
  done: string | null;
  cancelled: string | null;
  priority: Priority;
  recurrence: string | null;
  tags: string[];
  /** Start time `HH:MM` written at the start of the line (Day Planner style). */
  time: string | null;
  end_time: string | null;
  /** `YYYY-MM-DD HH:MM` (`⏰ …` or `(@…)`). */
  reminder: string | null;
}

/** What the task edit dialog can change. Empty strings clear a field. */
export interface TaskEdit {
  text: string;
  priority: Priority;
  due: string | null;
  scheduled: string | null;
  start: string | null;
  recurrence: string | null;
  time: string | null;
  end_time: string | null;
  reminder: string | null;
}

export const listTasks = (includeDone: boolean) => invoke<TaskItem[]>("list_tasks", { includeDone });
export const toggleTask = (t: TaskItem, today: string) =>
  invoke<void>("toggle_task", { path: t.path, line: t.line, raw: t.raw, today });
export const updateTask = (t: TaskItem, edit: TaskEdit) =>
  invoke<void>("update_task", { path: t.path, line: t.line, raw: t.raw, edit });
export const deleteTask = (t: TaskItem) => invoke<void>("delete_task", { path: t.path, line: t.line, raw: t.raw });

export interface Reminder extends TaskItem {
  /** `YYYY-MM-DD HH:MM` */
  at: string;
}

/** Open tasks to notify about between `from` and `to` (`YYYY-MM-DD HH:MM`). */
export const taskReminders = (from: string, to: string) => invoke<Reminder[]>("task_reminders", { from, to });
/** Appends to today's daily note; returns that note's path. */
export const addTask = (text: string, due: string | null, today: string, time: string) =>
  invoke<string>("add_task", { text, due, today, time });

export interface CalendarDay {
  date: string;
  /** That day's daily note, if it exists. */
  note: string | null;
}

export interface CalendarData {
  days: CalendarDay[];
  /** Tasks with a due/scheduled/start/done date inside the range. */
  tasks: TaskItem[];
  /** Timed tasks without dates in daily notes, placed on the note's day. */
  daily_tasks: (TaskItem & { date: string })[];
  /** Daily-note settings can't be used (e.g. unsupported date format). */
  daily_error: string | null;
}

export type EntryKind = "expense" | "income";

export interface FinanceItem {
  path: string;
  title: string;
  line: number;
  /** The line as written; edits are refused if it changed. */
  raw: string;
  kind: EntryKind;
  /** Amount × 100 (12,000원 → 1200000). */
  amount: number;
  memo: string;
  category: string | null;
  /** `📅` on the line, else the note's date; null if unknown. */
  date: string | null;
  /** The date is written on the line (not taken from the note). */
  explicit_date: boolean;
  /** Line of the record this one is indented under (a settlement of it), same note. */
  parent_line: number | null;
}

export interface EntryEdit {
  kind: EntryKind;
  amount: string;
  memo: string;
  category: string;
  /** Written as `📅`; null keeps the note's date. */
  date: string | null;
}

/** Monthly budgets, amounts × 100 like FinanceItem.amount. */
export interface Budgets {
  total: number | null;
  categories: Record<string, number>;
}

export const updateFinance = (e: FinanceItem, edit: EntryEdit) =>
  invoke<void>("update_finance", { path: e.path, line: e.line, raw: e.raw, edit });
export const deleteFinance = (e: FinanceItem) =>
  invoke<void>("delete_finance", { path: e.path, line: e.line, raw: e.raw });
export const getBudgets = () => invoke<Budgets>("get_budgets");
/** Write a settlement (money back, refund) indented under a record; returns nothing. */
export const addSettlement = (parent: FinanceItem, amount: string, memo: string, date: string | null) =>
  invoke<void>("add_settlement", { path: parent.path, line: parent.line, raw: parent.raw, amount, memo, date });

/** A monthly fixed cost; amount × 100 like records. */
export interface FixedExpense {
  name: string;
  amount: number;
  /** Day of the month (1–31; short months use their last day). */
  day: number;
  /** Text in a record's memo that marks the payment. */
  keyword: string;
  category: string | null;
  account: string | null;
}
export interface FixedStatus {
  /** Position in the list. */
  index: number;
  due: string;
  paid: { date: string; amount: number; memo: string; path: string; line: number } | null;
}
export interface FixedSuggestion {
  name: string;
  keyword: string;
  amount: number;
  day: number;
  category: string | null;
  account: string | null;
  months: number;
}
export interface FixedOverview {
  list: FixedExpense[];
  status: FixedStatus[];
  suggestions: FixedSuggestion[];
  /** Days before the due day to remind (null: off). */
  remind_days: number | null;
  /** How many suggestions were hidden. */
  ignored: number;
}
/** A fixed expense due soon (or just past) with no payment recorded. */
export interface FixedReminder {
  name: string;
  amount: number;
  due: string;
  /** Days until due: 0 today, negative when past. */
  days: number;
}
export const fixedExpenses = (month: string) => invoke<FixedOverview>("fixed_expenses", { month });
export const fixedReminders = (today: string) => invoke<FixedReminder[]>("fixed_reminders", { today });
export const setFixedRemind = (days: number | null) => invoke<void>("set_fixed_remind", { days });
/** Hide a suggested fixed expense (null: show the hidden ones again). */
export const ignoreFixed = (keyword: string | null) => invoke<void>("ignore_fixed", { keyword });
export const setFixedExpenses = (list: FixedExpense[]) => invoke<FixedExpense[]>("set_fixed_expenses", { list });
export const setBudgets = (budgets: Budgets) => invoke<Budgets>("set_budgets", { budgets });
/** Which CSV column (0-based) holds what. */
export interface ImportMapping {
  date: number | null;
  memo: number | null;
  /** Bank "출금" column. */
  withdrawal: number | null;
  /** Bank "입금" column. */
  deposit: number | null;
  /** One amount column instead of withdrawal/deposit. */
  amount: number | null;
  /** With `amount`: positive numbers are spending (card statements). */
  positive_is_expense: boolean;
  category: number | null;
  /** "입금"/"출금" beside an unsigned `amount` (e.g. Kakao Pay's 입출구분). */
  direction: number | null;
  /** A status column: rows saying "취소" are skipped. */
  status: number | null;
}

export interface ImportPreview {
  encoding: string;
  rows: string[][];
  /** Index of the header row (guessed). */
  header: number;
  mapping: ImportMapping;
}

export interface PlannedRow {
  row: number;
  status: "ok" | "duplicate" | "skipped";
  reason: string | null;
  date: string | null;
  kind: EntryKind | null;
  /** × 100, positive. */
  amount: number | null;
  memo: string;
  category: string | null;
}

export interface ImportResult {
  path: string;
  added: number;
  duplicates: number;
  skipped: number;
}

/**
 * Read a statement (CSV, xlsx, xls, a password-protected Excel file, or NH Bank's emailed PDF). Rejects with
 * "PASSWORD_REQUIRED" or "WRONG_PASSWORD" so the caller can ask for the password;
 * the password is only used to open the file and is never stored.
 */
export const importCsvPreview = (file: string, password: string | null = null) =>
  invoke<ImportPreview>("import_csv_preview", { file, password });
export const importCsvPlan = (file: string, header: number, mapping: ImportMapping, defaultCategory: string, password: string | null = null) =>
  invoke<PlannedRow[]>("import_csv_plan", { file, header, mapping, defaultCategory, password });
export const importCsvCommit = (
  file: string, header: number, mapping: ImportMapping, defaultCategory: string, note: string, password: string | null = null,
) => invoke<ImportResult>("import_csv_commit", { file, header, mapping, defaultCategory, note, password });

/** Writes records dated start..end as CSV to `dest`; returns the row count. */
export const exportFinanceCsv = (dest: string, start: string, end: string) =>
  invoke<number>("export_finance_csv", { dest, start, end });

export interface FinanceData {
  /** Records dated inside the range, oldest first. */
  entries: FinanceItem[];
  /** Records whose date can't be determined (excluded from totals). */
  undated: FinanceItem[];
  /** All categories in the vault, most used first. */
  categories: string[];
}

export const financeRange = (start: string, end: string) =>
  invoke<FinanceData>("finance_range", { start, end });
/** Appends `- 💸 12,000 memo #category` to today's daily note; returns its path. */
export const addFinance = (
  kind: EntryKind, amount: string, memo: string, category: string,
  date: string, today: string, time: string,
) => invoke<string>("add_finance", { kind, amount, memo, category, date, today, time });

export type GraphNodeKind = "note" | "attachment" | "missing";

export interface GraphNode {
  /** Vault path, or `missing:<target>` for links to notes that don't exist. */
  id: string;
  title: string;
  kind: GraphNodeKind;
  /** Distinct neighbours in the whole graph. */
  degree: number;
  /** The note's tags without `#`. */
  tags: string[];
}

/** Obsidian graph color group (`.obsidian/graph.json`). */
export interface ColorGroup {
  query: string;
  /** `#rrggbb` */
  color: string;
}

export const graphGroups = () => invoke<ColorGroup[]>("graph_groups");
export const setGraphGroups = (groups: ColorGroup[]) => invoke<void>("set_graph_groups", { groups });

export interface GraphEdge {
  source: string;
  target: string;
}

export interface GraphData {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export const linkGraph = () => invoke<GraphData>("link_graph");

/** An external calendar subscription (stored in app settings, not the vault). */
export interface CalendarSource {
  id: string;
  name: string;
  /** https / webcal URL or a local .ics path. */
  url: string;
  /** `#rrggbb` */
  color: string;
  enabled: boolean;
}

/** One occurrence of an external event; times are wall-clock in `tz`. */
export interface ExternalEvent {
  /** The subscription id. */
  calendar: string;
  uid: string;
  summary: string;
  location: string | null;
  description: string | null;
  all_day: boolean;
  /** `YYYY-MM-DD` (all-day) or `YYYY-MM-DDTHH:MM:SS`. */
  start: string;
  /** Exclusive end, same form. */
  end: string;
  /** "UTC", an IANA zone, or null for local time. */
  tz: string | null;
}

export interface ExternalEvents {
  events: ExternalEvent[];
  /** Calendars that could not be fetched (an old copy may still be shown). */
  errors: { calendar: string; name: string; message: string }[];
}

export const listCalendars = () => invoke<CalendarSource[]>("list_calendars");
export const setCalendars = (calendars: CalendarSource[]) => invoke<CalendarSource[]>("set_calendars", { calendars });
/** `force` downloads again instead of reusing a recent copy. */
export const externalEvents = (start: string, end: string, force = false) =>
  invoke<ExternalEvents>("external_events", { start, end, force });

export const calendarRange = (start: string, end: string) =>
  invoke<CalendarData>("calendar_range", { start, end });
/** Opens (creating from the template if needed) the daily note; returns its path. */
export const openDailyNote = (date: string, time: string) =>
  invoke<string>("open_daily_note", { date, time });

/** Files changed on disk (outside edits or our own saves). */
export const onVaultChanged = (cb: (paths: string[]) => void): Promise<UnlistenFn> =>
  listen<string[]>("vault-changed", (e) => cb(e.payload));

export type PropertyValue =
  | string | number | boolean | null
  | PropertyValue[]
  | { [key: string]: PropertyValue };

export interface Property {
  key: string;
  value: PropertyValue;
}

export interface Frontmatter {
  present: boolean;
  properties: Property[];
  error: string | null;
  body_start: number;
}

export const parseFrontmatter = (content: string) =>
  invoke<Frontmatter>("parse_frontmatter", { content });
/** `content` with its frontmatter rewritten (removed when `properties` is empty). */
export const setProperties = (content: string, properties: Property[]) =>
  invoke<string>("set_properties", { content, properties });

export interface TemplateList {
  /** `.obsidian/templates.json` folder ("" when templates aren't set up). */
  folder: string;
  files: string[];
}

export interface RenderedTemplate {
  /** Merged into the note's properties (null: the template has none). */
  properties: Property[] | null;
  /** Inserted at the cursor. */
  body: string;
}

/** Obsidian's Templates settings (`.obsidian/templates.json`). */
export interface TemplateSettings {
  folder: string;
  dateFormat: string;
  timeFormat: string;
}
/** Obsidian's Daily notes settings (`.obsidian/daily-notes.json`). */
export interface DailySettings {
  folder: string;
  format: string;
  template: string;
}
export interface NoteSettings {
  templates: TemplateSettings;
  daily: DailySettings;
  /** The vault's folders and notes (without `.md`), to pick from. */
  folders: string[];
  notes: string[];
}
/** What the settings being edited produce today. */
export interface NoteSettingsPreview {
  template_date: string;
  template_time: string;
  template_folder_missing: boolean;
  daily_path: string | null;
  /** Why the format can't name a daily note (letters read as symbols, …). */
  daily_error: string | null;
  daily_folder_missing: boolean;
  daily_template_missing: boolean;
}
export const noteSettings = () => invoke<NoteSettings>("note_settings");
export const setNoteSettings = (templates: TemplateSettings, daily: DailySettings) =>
  invoke<void>("set_note_settings", { templates, daily });
export const previewNoteSettings = (templates: TemplateSettings, daily: DailySettings, date: string, time: string) =>
  invoke<NoteSettingsPreview>("preview_note_settings", { templates, daily, date, time });
export const listTemplates = () => invoke<TemplateList>("list_templates");
export const renderTemplate = (path: string, title: string, date: string, time: string) =>
  invoke<RenderedTemplate>("render_template", { path, title, date, time });

/** URL that loads a vault file (images, PDFs, media) through the `vault://` protocol. */
export const vaultUrl = (path: string) => convertFileSrc(path, "vault");

const IMAGE_EXTENSIONS = ["png", "jpg", "jpeg", "gif", "webp", "avif", "bmp", "svg"];
const AUDIO_EXTENSIONS = ["mp3", "wav", "ogg", "oga", "m4a", "flac"];
const VIDEO_EXTENSIONS = ["mp4", "m4v", "webm", "ogv", "mov"];

export type MediaKind = "image" | "audio" | "video" | "pdf" | null;

export function mediaKind(path: string): MediaKind {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  if (IMAGE_EXTENSIONS.includes(ext)) return "image";
  if (AUDIO_EXTENSIONS.includes(ext)) return "audio";
  if (VIDEO_EXTENSIONS.includes(ext)) return "video";
  if (ext === "pdf") return "pdf";
  return null;
}

export const openVault =(path: string) => invoke<VaultInfo>("open_vault", { path });
export const refreshTree = () => invoke<TreeNode[]>("refresh_tree");
export const lastVault = () => invoke<string | null>("last_vault");

/** A vault opened before (newest first). */
export interface VaultEntry {
  path: string;
  /** Folder name. */
  name: string;
  /** The folder still exists. */
  exists: boolean;
  /** Seconds since 1970 (0 if unknown). */
  opened: number;
}

export const recentVaults = () => invoke<VaultEntry[]>("recent_vaults");

/** Git sync settings of a vault (device part + the folders shared by all devices). */
export interface SyncConfig {
  remote: string;
  branch: string;
  /** Note folders to sync; the settings folder (.obsidian) always syncs. */
  folders: string[];
  /** Also sync calendar subscription URLs (private tokens). */
  include_calendars: boolean;
  /** 0 = only at start and on demand. */
  interval_minutes: number;
}

export interface SyncInfo {
  /** Installed Git version; null if Git is missing. */
  git: string | null;
  /** Null when sync isn't set up on this device. */
  config: SyncConfig | null;
  folders: string[];
  include_calendars: boolean;
}

export interface SyncScan {
  count: number;
  bytes: number;
  files: string[];
  /** Left out for size: [path, bytes]. */
  large: [string, number][];
  per_folder: [string, number][];
}

export interface SyncReport {
  committed: boolean;
  pulled: string[];
  conflicts: string[];
  pushed: boolean;
  large: string[];
  /** Rusidian settings from another device, to apply. */
  ui: Record<string, unknown> | null;
}

export const getSync = () => invoke<SyncInfo>("get_sync");
/** null turns sync off on this device. */
export const setSync = (config: SyncConfig | null) => invoke<void>("set_sync", { config });
export const syncScan = (folders: string[]) => invoke<SyncScan>("sync_scan", { folders });
export const syncNow = (ui: Record<string, unknown> | null, stamp: string) =>
  invoke<SyncReport>("sync_now", { ui, stamp });

/** The vault this window should open at start (null: show the start screen). */
export const startupVault = () => invoke<string | null>("startup_vault");
/** If another window has `path` open, bring it forward and return true. */
export const focusVaultWindow = (path: string) => invoke<boolean>("focus_vault_window", { path });
/** Open a new window, empty or with a vault. */
export const openWindow = (path: string | null) => invoke<void>("open_window", { path });
/** Remove from the recent list (the folder is not touched). */
export const forgetVault = (path: string) => invoke<VaultEntry[]>("forget_vault", { path });
export const readNote = (path: string) => invoke<string>("read_note", { path });

/** When a file was made and last changed (Unix ms; null where the system doesn't say). */
export interface FileTimes {
  created: number | null;
  modified: number | null;
  size: number;
}
export const fileTimes = (path: string) => invoke<FileTimes>("file_times", { path });
export interface WriteResult {
  /** The file had changed elsewhere since `expected`: it was left alone and
   *  the text went into this conflict copy instead. */
  conflict: string | null;
  /** What the file holds now, when there was a conflict. */
  disk: string | null;
}
/**
 * Save a note. With `expected` (the text last read or saved there), a file
 * changed elsewhere meanwhile is not overwritten: the text becomes a conflict
 * copy labelled `stamp`. The file's line endings are kept either way.
 */
export const writeNote = (path: string, content: string, expected?: string, stamp?: string) =>
  invoke<WriteResult>("write_note", { path, content, expected: expected ?? null, stamp: stamp ?? null });

const TEXT_EXTENSIONS = ["md", "txt", "canvas", "json", "csv"];

export function isTextFile(path: string): boolean {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return TEXT_EXTENSIONS.includes(ext);
}

/** "Folder/My note.md" -> "My note" (Obsidian shows notes without .md). */
export function displayName(name: string): string {
  return name.toLowerCase().endsWith(".md") ? name.slice(0, -3) : name;
}

// ---------------------------------------------------------------- attachments

/** Most a pasted or dropped file may weigh (the backend refuses more). */
export const ATTACHMENT_MAX_BYTES = 50 * 1024 * 1024;

const pad2 = (n: number) => String(n).padStart(2, "0");
/** "2026-11-14 0305": labels sync commits and conflict copies. */
export function syncStamp(d = new Date()): string {
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())} ${pad2(d.getHours())}${pad2(d.getMinutes())}`;
}
/** Local time as "YYYYMMDDHHmmss", like Obsidian's pasted-image names. */
export function attachmentStamp(d = new Date()): string {
  return `${d.getFullYear()}${pad2(d.getMonth() + 1)}${pad2(d.getDate())}${pad2(d.getHours())}${pad2(d.getMinutes())}${pad2(d.getSeconds())}`;
}

/**
 * Save a pasted (clipboard image) or dropped file as an attachment of `note`,
 * where Obsidian's attachment setting says. The bytes travel raw, not as JSON.
 * Returns the saved path and the text to insert (`![[…]]` or a markdown link).
 */
export async function saveAttachment(note: string, file: File, pasted: boolean, stamp: string) {
  const bytes = new Uint8Array(await file.arrayBuffer());
  return invoke<{ path: string; embed: string }>("save_attachment", bytes, {
    headers: {
      "x-note": encodeURIComponent(note),
      "x-name": encodeURIComponent(file.name),
      "x-type": encodeURIComponent(file.type),
      "x-stamp": stamp,
      "x-pasted": pasted ? "1" : "0",
    },
  });
}

// ---------------------------------------------------------------- assets

/** An account balance record (`- 🏦 2,700,000 계좌 #분류 📅 날짜`); amount × 100. */
export interface BalanceItem {
  path: string;
  title: string;
  line: number;
  raw: string;
  amount: number;
  account: string;
  group: string | null;
  date: string | null;
}
export const balances = () => invoke<BalanceItem[]>("balances");
/** Append a balance record to `note` (created if missing). */
export const addBalance = (note: string, amount: string, account: string, group: string, date: string) =>
  invoke<void>("add_balance", { note, amount, account, group, date });
/** Money-record categories counted as savings, not spending (sub-categories included). */
export const getSavings = () => invoke<string[]>("get_savings");
export const setSavings = (list: string[]) => invoke<string[]>("set_savings", { list });

// ---------------------------------------------------------------- bookmarks & links

/** An item of Obsidian's `.obsidian/bookmarks.json` (unknown fields kept as they are). */
export interface BookmarkItem {
  type: "file" | "folder" | "group" | "search" | "heading" | "block" | "graph" | "url" | string;
  ctime?: number;
  path?: string;
  /** "#Heading" or "#^block" for heading / block bookmarks. */
  subpath?: string;
  title?: string;
  query?: string;
  url?: string;
  items?: BookmarkItem[];
}
export const getBookmarks = () => invoke<BookmarkItem[]>("get_bookmarks");
/** Bookmark a file, or remove it; resolves to whether it is bookmarked now. */
export const toggleBookmark = (path: string) => invoke<boolean>("toggle_bookmark", { path, ctime: Date.now() });
/** Remove the item at `at` (indices from the top level down into groups). */
export const removeBookmark = (at: number[]) => invoke<void>("remove_bookmark", { at });

export interface OutLink {
  target: string;
  /** The file it opens, or null for a note that doesn't exist yet. */
  resolved: string | null;
  embed: boolean;
  lines: number[];
}
export interface Mention {
  source: string;
  title: string;
  lines: { line: number; text: string; raw: string; term: string }[];
}
export const outgoingLinks = (path: string) => invoke<OutLink[]>("outgoing_links", { path });
export const unlinkedMentions = (path: string) => invoke<Mention[]>("unlinked_mentions", { path });
/** Link the plain mention of `term` on that line of `source` to the note `target`. */
export const linkMention = (source: string, line: number, raw: string, term: string, target: string) =>
  invoke<void>("link_mention", { source, line, raw, term, target });

// ---------------------------------------------------------------- phone notifications

/** How notifications become records (this computer's setup for the vault). */
export interface PhoneRules {
  /** Names meaning one of my own accounts: money moved between them is left out. */
  own: string[];
  /** ATM withdrawals are lab costs: written as money taken off 인건비. */
  atm_lab_cost: boolean;
  /** A person's transfer below this (won) is their share of a meal: money back under 식비/정산. 0 = off. */
  settle_below: number;
}

export interface PhoneSetup {
  enabled: boolean;
  /** The inbox file the phone writes, synced to this computer. */
  file: string;
  /** Note for records; YYYY and MM become the record's year and month. */
  note: string;
  rules: PhoneRules;
}

export interface PhoneLogItem {
  key: string;
  when: string;
  app: string;
  text: string;
  status: "added" | "duplicate" | "skipped" | "unreadable";
  result: string;
}

export interface PhoneInfo {
  setup: PhoneSetup | null;
  log: PhoneLogItem[];
  file_found: boolean;
  template: string;
}

export interface PhoneResult {
  added: number;
  notes: string[];
  log: PhoneLogItem[];
}

export const phoneGet = () => invoke<PhoneInfo>("phone_get");
export const phoneSet = (setup: PhoneSetup | null) => invoke<void>("phone_set", { setup });
/** The inbox file changed since it was last read. */
export const phoneChanged = () => invoke<boolean>("phone_changed");
/** Write the inbox's new notifications into the vault (save open notes first). */
export const phoneProcess = (today: string) => invoke<PhoneResult>("phone_process", { today });
/** Read the whole file again next time (records already written are still recognized). */
export const phoneReset = () => invoke<void>("phone_reset");

// ------------------------------------------------------------ file recovery

/** A note with earlier versions kept on this computer. */
export interface HistoryFile {
  path: string;
  /** Newest version (unix ms). */
  latest: number;
  count: number;
  /** False when the note has been deleted or moved elsewhere. */
  exists: boolean;
}

export interface HistoryVersion {
  /** When it was kept (unix ms); also its id. */
  id: number;
  size: number;
}

export interface DiffLine {
  change: "same" | "added" | "removed";
  text: string;
}

// ------------------------------------------------------------ sync conflicts

/** A copy sync kept when both devices changed a file. */
export interface ConflictCopy {
  copy: string;
  /** The file it is a version of. */
  original: string;
  stamp: string;
  /** The copy is this device's own version (else the other device's). */
  this_device: boolean;
}

export type ConflictBlock =
  | { kind: "same"; lines: string[] }
  | { kind: "differ"; original: string[]; copy: string[] };

export interface ConflictView extends ConflictCopy {
  original_exists: boolean;
  /** Both are text files (else they can't be compared). */
  text: boolean;
  blocks: ConflictBlock[];
  crlf: boolean;
  trailing_newline: boolean;
}

export const listConflicts = () => invoke<ConflictCopy[]>("list_conflicts");
export const conflictBlocks = (copy: string) => invoke<ConflictView>("conflict_blocks", { copy });

export const historyFiles = () => invoke<HistoryFile[]>("history_files");
export const historyVersions = (path: string) => invoke<HistoryVersion[]>("history_versions", { path });
export const historyRead = (path: string, id: number) => invoke<string>("history_read", { path, id });
/** From version `id` to the note as it is now. */
export const historyDiff = (path: string, id: number) => invoke<DiffLine[]>("history_diff", { path, id });
/** Put version `id` back; the current text is kept as a version first. */
export const historyRestore = (path: string, id: number) => invoke<void>("history_restore", { path, id });
/** Forget the versions of one note (null: of the whole vault). */
export const historyClear = (path: string | null) => invoke<void>("history_clear", { path });

// ------------------------------------------------------------------ hotkeys

/** One key combination, as Obsidian writes it in `.obsidian/hotkeys.json`. */
export interface Hotkey {
  /** "Mod" is Ctrl (Cmd on macOS). */
  modifiers: ("Mod" | "Ctrl" | "Meta" | "Shift" | "Alt")[];
  key: string;
}

/** Commands whose keys were changed; an empty list means no key at all. */
export const getHotkeys = () => invoke<Record<string, Hotkey[]>>("get_hotkeys");
/** Give command `id` these keys (null: back to its defaults). */
export const setHotkey = (id: string, keys: Hotkey[] | null) => invoke<void>("set_hotkey", { id, keys });
