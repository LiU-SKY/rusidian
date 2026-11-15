//! In-memory vault index: link resolution, backlinks and substring search.
//!
//! The files on disk stay the source of truth; this is a cache rebuilt on
//! open and kept fresh by the watcher (`watch.rs`) and by our own saves.

use crate::daily::DailyMatcher;
use crate::date::Date;
use crate::finance::{Entry, FinanceData, FinanceItem, parse_entries};
use crate::frontmatter::{self, Frontmatter};
use crate::links::{Link, extract_links};
use crate::tags::{self as tagging, note_tags_with};
use crate::tasks::{StatusKind, Task, parse_tasks};
use crate::vault::is_hidden_path;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::Path;

/// Notes bigger than this are listed but not indexed for content.
const MAX_NOTE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_COUNT_PER_NOTE: usize = 999;
const SNIPPETS_PER_NOTE: usize = 3;
const SNIPPET_BEFORE: usize = 40;
const SNIPPET_AFTER: usize = 80;

struct Note {
    content: String,
    links: Vec<Link>,
    tasks: Vec<Task>,
    money: Vec<Entry>,
    balances: Vec<crate::assets::Balance>,
    /// `date:` frontmatter property, if it holds a valid date.
    fm_date: Option<Date>,
    /// Tags without `#` (see `tags.rs`).
    tags: Vec<String>,
    /// `aliases:` frontmatter property (other names of the note).
    aliases: Vec<String>,
    /// Lines before this are the frontmatter block.
    body_line: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TagCount {
    /// Without `#`, as first written in the vault.
    pub tag: String,
    /// Notes that have the tag.
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TaskItem {
    pub path: String,
    pub title: String,
    #[serde(flatten)]
    pub task: Task,
}

/// Every (non-hidden) file of the vault and Obsidian's link resolution over
/// them. Cloneable so a planned move can be checked against the "after" state.
#[derive(Default, Clone)]
pub struct PathTable {
    /// Lowercase path -> actual path.
    files: BTreeMap<String, String>,
    /// Lowercase file name (with extension) -> actual paths.
    by_name: HashMap<String, Vec<String>>,
}

impl PathTable {
    /// Adds a file; false if it was already there.
    pub fn add(&mut self, rel: &str) -> bool {
        let added = self.files.insert(rel.to_lowercase(), rel.to_string()).is_none();
        if added {
            self.by_name.entry(file_name(rel).to_lowercase()).or_default().push(rel.to_string());
        }
        added
    }

    /// Removes a file (matched case-insensitively); returns its actual path.
    pub fn remove(&mut self, rel: &str) -> Option<String> {
        let actual = self.files.remove(&rel.to_lowercase())?;
        let key = file_name(&actual).to_lowercase();
        if let Some(list) = self.by_name.get_mut(&key) {
            list.retain(|p| p != &actual);
            if list.is_empty() {
                self.by_name.remove(&key);
            }
        }
        Some(actual)
    }

    /// How many files have this file name (case-insensitive), in any folder.
    pub fn named(&self, name: &str) -> usize {
        self.by_name.get(&name.to_lowercase()).map_or(0, Vec::len)
    }

    /// Every file's actual path.
    pub fn all(&self) -> impl Iterator<Item = &str> {
        self.files.values().map(String::as_str)
    }

    /// The actual path of an existing file, matched case-insensitively.
    pub fn get(&self, rel: &str) -> Option<&str> {
        self.files.get(&rel.to_lowercase()).map(String::as_str)
    }

    /// Actual paths of every file inside folder `dir` (any depth).
    pub fn under(&self, dir: &str) -> Vec<String> {
        let prefix = format!("{}/", dir.to_lowercase());
        self.files
            .range(prefix.clone()..)
            .take_while(|(k, _)| k.starts_with(&prefix))
            .map(|(_, v)| v.clone())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Resolve a link target written in `source` to an existing file path:
    /// exact path, then relative to the source's folder, then by file name /
    /// path suffix with the shortest path winning (Obsidian's order).
    pub fn resolve(&self, source: &str, target: &str) -> Option<String> {
        let target = target.trim().trim_start_matches("./").trim_start_matches('/');
        if target.is_empty() {
            return None;
        }
        let with_md = format!("{target}.md");
        let candidates: &[&str] = if is_note(target) { &[target] } else { &[&with_md, target] };
        let source_dir = source.rsplit_once('/').map(|(d, _)| d).unwrap_or("");

        for cand in candidates {
            let lower = cand.to_lowercase();
            // 1. Exact vault-relative path.
            if let Some(p) = self.files.get(&lower) {
                return Some(p.clone());
            }
            // 2. Relative to the linking note's folder.
            if !source_dir.is_empty() {
                if let Some(joined) = normalize(&format!("{source_dir}/{cand}")) {
                    if let Some(p) = self.files.get(&joined.to_lowercase()) {
                        return Some(p.clone());
                    }
                }
            }
            // 3. By file name / path suffix; shortest path wins.
            if let Some(paths) = self.by_name.get(file_name(&lower)) {
                let suffix = format!("/{lower}");
                let best = paths
                    .iter()
                    .filter(|p| {
                        let pl = p.to_lowercase();
                        pl == lower || pl.ends_with(&suffix)
                    })
                    .min_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
                if let Some(p) = best {
                    return Some(p.clone());
                }
            }
        }
        None
    }
}

#[derive(Default)]
pub struct VaultIndex {
    paths: PathTable,
    /// Markdown notes with content, keyed by actual path.
    notes: BTreeMap<String, Note>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LineRef {
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Backlink {
    pub source: String,
    pub title: String,
    pub lines: Vec<LineRef>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Snippet {
    pub line: usize,
    pub before: String,
    pub hit: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SearchHit {
    pub path: String,
    pub title: String,
    /// All terms appear in the file path.
    pub path_match: bool,
    pub count: usize,
    pub snippets: Vec<Snippet>,
}

/// A link written in a note, with what it points to.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OutLink {
    /// As written (without alias / heading).
    pub target: String,
    /// The file it opens, or `None` for a note that doesn't exist yet.
    pub resolved: Option<String>,
    pub embed: bool,
    /// 0-based lines where it appears.
    pub lines: Vec<usize>,
}

/// A note that names another note in plain text.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Mention {
    pub source: String,
    pub title: String,
    pub lines: Vec<MentionLine>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MentionLine {
    pub line: usize,
    /// Trimmed, for display.
    pub text: String,
    /// As in the file (what `link_mention` checks before rewriting).
    pub raw: String,
    /// Which name was found (the title or an alias), for linking it.
    pub term: String,
}

/// `aliases: [a, b]`, `aliases: a`, `alias: a`.
fn aliases_of(fm: &Frontmatter) -> Vec<String> {
    let value = fm.get("aliases").or_else(|| fm.get("alias"));
    let list: Vec<String> = match value {
        Some(serde_json::Value::Array(items)) => items.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    };
    list.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// First line after a `---` frontmatter block (0 if there is none).
fn body_start_line(content: &str) -> usize {
    let mut lines = content.lines();
    if lines.next().map(|l| l.trim_start_matches('\u{feff}').trim_end()) != Some("---") {
        return 0;
    }
    lines.position(|l| l.trim_end() == "---").map_or(0, |i| i + 2)
}

fn is_note(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".md")
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// "Folder/My Note.md" -> "My Note"
pub fn title_of(path: &str) -> String {
    let name = file_name(path);
    if is_note(name) { name[..name.len() - 3].to_string() } else { name.to_string() }
}

impl VaultIndex {
    pub fn build(root: &Path) -> io::Result<Self> {
        let mut index = VaultIndex::default();
        index.scan_dir(root, root)?;
        Ok(index)
    }

    pub fn note_count(&self) -> usize {
        self.notes.len()
    }

    fn scan_dir(&mut self, root: &Path, dir: &Path) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let ft = entry.file_type()?;
            if ft.is_symlink() || entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let full = entry.path();
            if ft.is_dir() {
                self.scan_dir(root, &full)?;
            } else if ft.is_file() {
                let rel = rel_string(root, &full);
                self.load_file(&full, &rel);
            }
        }
        Ok(())
    }

    fn load_file(&mut self, full: &Path, rel: &str) {
        self.add_file_entry(rel);
        if is_note(rel) {
            let small = fs::metadata(full).map(|m| m.len() <= MAX_NOTE_BYTES).unwrap_or(false);
            let content = if small { fs::read_to_string(full).unwrap_or_default() } else { String::new() };
            self.set_note(rel, content);
        }
    }

    fn add_file_entry(&mut self, rel: &str) {
        self.paths.add(rel);
    }

    fn set_note(&mut self, rel: &str, content: String) {
        let links = extract_links(&content);
        let tasks = parse_tasks(&content);
        let money = parse_entries(&content);
        let balances = crate::assets::parse_balances(&content);
        let fm = if content.trim_start_matches('\u{feff}').starts_with("---") {
            frontmatter::parse(&content)
        } else {
            Frontmatter::default()
        };
        let fm_date = fm.get("date").and_then(|v| v.as_str()).and_then(|s| s.get(..10)).and_then(Date::parse);
        let tags = note_tags_with(&content, &fm);
        let aliases = aliases_of(&fm);
        let body_line = body_start_line(&content);
        self.notes.insert(rel.to_string(), Note { content, links, tasks, money, balances, fm_date, tags, aliases, body_line });
    }

    /// Links written in `path`, one entry per target (first appearance first).
    pub fn outgoing(&self, path: &str) -> Vec<OutLink> {
        let Some(note) = self.notes.get(path) else { return Vec::new() };
        let mut out: Vec<OutLink> = Vec::new();
        for l in &note.links {
            let key = l.target.to_lowercase();
            if let Some(o) = out.iter_mut().find(|o| o.target.to_lowercase() == key) {
                if !o.lines.contains(&l.line) {
                    o.lines.push(l.line);
                }
                continue;
            }
            out.push(OutLink { target: l.target.clone(), resolved: self.resolve(path, &l.target), embed: l.embed, lines: vec![l.line] });
        }
        out
    }

    /// Other notes that name `path` (its title or an alias, 2+ characters)
    /// in plain text, outside links and code, ordered by path.
    pub fn unlinked_mentions(&self, path: &str) -> Vec<Mention> {
        let mut terms = vec![title_of(path)];
        if let Some(note) = self.notes.get(path) {
            for a in &note.aliases {
                if !terms.iter().any(|t| t.to_lowercase() == a.to_lowercase()) {
                    terms.push(a.clone());
                }
            }
        }
        terms.retain(|t| t.chars().count() >= 2);
        if terms.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut sources: Vec<&String> = self.notes.keys().filter(|p| p.as_str() != path).collect();
        sources.sort();
        for source in sources {
            let note = &self.notes[source];
            let mut lines = Vec::new();
            for (n, line) in crate::markdown::prose_lines(&note.content) {
                if n < note.body_line {
                    continue;
                }
                if let Some(term) = terms.iter().find(|t| crate::mentions::find_mention(line, t).is_some()) {
                    lines.push(MentionLine { line: n, text: line.trim().to_string(), raw: line.to_string(), term: term.clone() });
                }
            }
            if !lines.is_empty() {
                out.push(Mention { source: source.clone(), title: title_of(source), lines });
            }
        }
        out
    }

    /// How to link to `path` from another note: its title when no other
    /// file has the same name, else its path (without `.md`).
    pub fn link_target(&self, path: &str) -> String {
        let name = file_name(path);
        if self.paths.named(name) <= 1 {
            title_of(path)
        } else {
            path.strip_suffix(".md").unwrap_or(path).to_string()
        }
    }

    /// Money records dated within `start..=end`, plus undated ones.
    /// A note's date comes from its daily-note path (`daily`), else its
    /// `date:` property, else a `YYYY-MM-DD` prefix in the file name.
    pub fn finance(&self, daily: Option<&DailyMatcher>, start: &str, end: &str) -> FinanceData {
        let mut data = FinanceData::default();
        let mut category_counts: HashMap<&str, usize> = HashMap::new();

        for (path, note) in &self.notes {
            if note.money.is_empty() {
                continue;
            }
            let note_date = daily
                .and_then(|m| m.date_of(path))
                .or(note.fm_date)
                .or_else(|| file_name(path).get(..10).and_then(Date::parse))
                .map(|d| d.to_string());

            for e in &note.money {
                if let Some(c) = &e.category {
                    *category_counts.entry(c).or_default() += 1;
                }
                let item = FinanceItem {
                    path: path.clone(),
                    title: title_of(path),
                    line: e.line,
                    raw: e.raw.clone(),
                    kind: e.kind,
                    amount: e.amount,
                    memo: e.memo.clone(),
                    category: e.category.clone(),
                    date: e.date.clone().or_else(|| note_date.clone()),
                    explicit_date: e.date.is_some(),
                    parent_line: e.parent,
                };
                match item.date.as_deref() {
                    None => data.undated.push(item),
                    Some(d) if d >= start && d <= end => data.entries.push(item),
                    Some(_) => {}
                }
            }
        }

        data.entries.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.path.cmp(&b.path)).then(a.line.cmp(&b.line)));
        let mut cats: Vec<(&str, usize)> = category_counts.into_iter().collect();
        cats.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        data.categories = cats.into_iter().map(|(c, _)| c.to_string()).collect();
        data
    }

    /// Every account balance record (`- 🏦 …`), oldest first. The date is
    /// the line's `📅`, else the note's date (as for money records).
    pub fn balances(&self, daily: Option<&DailyMatcher>) -> Vec<crate::assets::BalanceItem> {
        let mut out = Vec::new();
        for (path, note) in &self.notes {
            if note.balances.is_empty() {
                continue;
            }
            let note_date = daily
                .and_then(|m| m.date_of(path))
                .or(note.fm_date)
                .or_else(|| file_name(path).get(..10).and_then(Date::parse))
                .map(|d| d.to_string());
            for b in &note.balances {
                out.push(crate::assets::BalanceItem {
                    path: path.clone(),
                    title: title_of(path),
                    line: b.line,
                    raw: b.raw.clone(),
                    amount: b.amount,
                    account: b.account.clone(),
                    group: b.group.clone(),
                    date: b.date.clone().or_else(|| note_date.clone()),
                });
            }
        }
        out.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.path.cmp(&b.path)).then(a.line.cmp(&b.line)));
        out
    }

    /// Every task in the vault (open ones only unless `include_done`),
    /// ordered by note path and line.
    pub fn tasks(&self, include_done: bool) -> Vec<TaskItem> {
        self.tasks_where(|t| include_done || matches!(t.kind, StatusKind::Todo | StatusKind::InProgress))
    }

    /// Tasks (any status) with a due, scheduled, start or done date in
    /// `start..=end` (`YYYY-MM-DD` strings compare chronologically).
    pub fn tasks_between(&self, start: &str, end: &str) -> Vec<TaskItem> {
        self.tasks_where(|t| {
            [&t.due, &t.scheduled, &t.start, &t.done]
                .into_iter()
                .flatten()
                .any(|d| d.as_str() >= start && d.as_str() <= end)
        })
    }

    fn tasks_where(&self, keep: impl Fn(&Task) -> bool) -> Vec<TaskItem> {
        self.notes
            .iter()
            .flat_map(|(path, note)| {
                note.tasks
                    .iter()
                    .filter(|t| keep(t))
                    .map(move |t| TaskItem { path: path.clone(), title: title_of(path), task: t.clone() })
            })
            .collect()
    }

    /// Tags of a note (without `#`); empty for unknown paths.
    pub fn note_tags(&self, path: &str) -> &[String] {
        self.notes.get(path).map_or(&[], |n| n.tags.as_slice())
    }

    /// Every note with the links written in it, in path order.
    pub fn note_links(&self) -> impl Iterator<Item = (&str, &[Link])> {
        self.notes.iter().map(|(path, note)| (path.as_str(), note.links.as_slice()))
    }

    /// The actual path of an existing file, matched case-insensitively
    /// (Windows and macOS file systems usually are).
    pub fn file_path(&self, rel: &str) -> Option<&str> {
        self.paths.get(rel)
    }

    /// The vault's files and link resolution.
    pub fn paths(&self) -> &PathTable {
        &self.paths
    }

    fn remove_file(&mut self, rel: &str) -> bool {
        let Some(actual) = self.paths.remove(rel) else {
            return false;
        };
        self.notes.remove(&actual);
        true
    }

    /// Drop a file, or a folder with everything below it, from the index.
    pub fn forget(&mut self, rel: &str) {
        self.remove_file(rel);
        for path in self.paths.under(rel) {
            self.remove_file(&path);
        }
    }

    /// Re-sync after we moved `from` to `to` ourselves. Forgetting first
    /// makes a case-only rename show the new casing.
    pub fn moved(&mut self, root: &Path, from: &str, to: &str) {
        self.forget(from);
        self.refresh_path(root, to);
    }

    /// Update the index after our own save, without touching the disk.
    pub fn upsert_note(&mut self, rel: &str, content: String) {
        self.add_file_entry(rel);
        if is_note(rel) {
            self.set_note(rel, content);
        }
    }

    /// Re-sync one path from disk (file, folder, or something that vanished).
    /// Returns true if the index changed.
    pub fn refresh_path(&mut self, root: &Path, rel: &str) -> bool {
        if rel.is_empty() || is_hidden_path(rel) {
            return false;
        }
        let full = root.join(rel);
        match fs::symlink_metadata(&full) {
            Ok(m) if m.is_file() => {
                // Replace so a case-only rename is picked up too.
                self.remove_file(rel);
                self.load_file(&full, rel);
                true
            }
            Ok(m) if m.is_dir() => {
                let before = self.paths.len();
                let _ = self.scan_dir(root, &full);
                self.paths.len() != before
            }
            _ => {
                // Gone: drop the file itself, or everything below a removed folder.
                let mut changed = self.remove_file(rel);
                for path in self.paths.under(rel) {
                    changed |= self.remove_file(&path);
                }
                changed
            }
        }
    }

    // ---------------------------------------------------------------- links

    /// Resolve a link target written in `source` to an existing file path.
    pub fn resolve(&self, source: &str, target: &str) -> Option<String> {
        self.paths.resolve(source, target)
    }

    /// Notes that link to `target_path`, with the lines containing the links.
    pub fn backlinks(&self, target_path: &str) -> Vec<Backlink> {
        let mut out = Vec::new();
        for (source, note) in &self.notes {
            if source == target_path {
                continue;
            }
            let mut lines: Vec<usize> = note
                .links
                .iter()
                .filter(|l| self.resolve(source, &l.target).as_deref() == Some(target_path))
                .map(|l| l.line)
                .collect();
            if lines.is_empty() {
                continue;
            }
            lines.dedup();
            let all_lines: Vec<&str> = note.content.lines().collect();
            out.push(Backlink {
                source: source.clone(),
                title: title_of(source),
                lines: lines
                    .into_iter()
                    .map(|n| LineRef { line: n, text: clip(all_lines.get(n).copied().unwrap_or("").trim(), 200) })
                    .collect(),
            });
        }
        out
    }

    // --------------------------------------------------------------- search

    /// Tags in the vault with the number of notes carrying each, sorted
    /// by name. Case variants count as one tag (first spelling wins).
    pub fn tag_counts(&self) -> Vec<TagCount> {
        let mut counts: BTreeMap<String, TagCount> = BTreeMap::new();
        for note in self.notes.values() {
            for tag in &note.tags {
                counts
                    .entry(tag.to_lowercase())
                    .or_insert_with(|| TagCount { tag: tag.clone(), count: 0 })
                    .count += 1;
            }
        }
        counts.into_values().collect()
    }

    /// Case-insensitive (ASCII) substring search. Every term must appear in
    /// the note's path or content. `"quoted phrases"` are one term.
    /// Operators: `tag:#x` (also nested tags), `path:x`, `file:x`, `-x` (exclude).
    pub fn search(&self, query: &str, limit: usize) -> Vec<SearchHit> {
        let q = Query::parse(query);
        if q.is_empty() {
            return Vec::new();
        }
        let terms = &q.terms;
        let mut hits = Vec::new();

        for (lower_path, path) in &self.paths.files {
            let note = self.notes.get(path);
            if !q.tags.is_empty() {
                let Some(n) = note else { continue };
                if !q.tags.iter().all(|qt| n.tags.iter().any(|t| tagging::matches(t, qt))) {
                    continue;
                }
            }
            if !q.paths.iter().all(|p| lower_path.contains(p.as_str()))
                || !q.files.iter().all(|f| file_name(lower_path).contains(f.as_str()))
            {
                continue;
            }
            let content = note.map(|n| n.content.as_str()).unwrap_or("");
            if q.exclude.iter().any(|x| lower_path.contains(x.as_str()) || find_all_ci(content, x).next().is_some()) {
                continue;
            }
            let path_match = !terms.is_empty() && terms.iter().all(|t| lower_path.contains(t.as_str()));

            let mut count = 0;
            let mut positions: Vec<(usize, usize)> = Vec::new(); // (byte pos, term len)
            let mut all_found = true;
            for t in terms {
                let mut found_here = 0;
                for pos in find_all_ci(content, t) {
                    found_here += 1;
                    if positions.len() < 64 {
                        positions.push((pos, t.len()));
                    }
                    if found_here >= MAX_COUNT_PER_NOTE {
                        break;
                    }
                }
                if found_here == 0 && !lower_path.contains(t.as_str()) {
                    all_found = false;
                    break;
                }
                count += found_here;
            }
            if !all_found || (!terms.is_empty() && count == 0 && !path_match) {
                continue;
            }
            // Show where a searched tag is written (property tags have no `#`).
            for t in &q.tags {
                let needle = format!("#{t}");
                positions.extend(find_all_ci(content, &needle).take(8).map(|p| (p, needle.len())));
            }
            positions.sort_unstable();
            hits.push(SearchHit {
                path: path.clone(),
                title: title_of(path),
                path_match,
                count,
                snippets: snippets(content, &positions),
            });
        }

        hits.sort_by(|a, b| {
            b.path_match
                .cmp(&a.path_match)
                .then_with(|| b.count.cmp(&a.count))
                .then_with(|| a.path.cmp(&b.path))
        });
        hits.truncate(limit);
        hits
    }
}

/// A parsed search query. Every value is lowercased (ASCII).
#[derive(Debug, Default, PartialEq)]
struct Query {
    /// Must each appear in the path or the content.
    terms: Vec<String>,
    /// Must not appear in the path or the content.
    exclude: Vec<String>,
    /// Tag names without `#`; nested tags match too.
    tags: Vec<String>,
    /// Must appear in the path.
    paths: Vec<String>,
    /// Must appear in the file name.
    files: Vec<String>,
}

impl Query {
    fn parse(query: &str) -> Self {
        let mut q = Query::default();
        for (token, quoted) in tokens(query) {
            let token = token.to_ascii_lowercase();
            let op = |prefix: &str| (!quoted).then(|| token.strip_prefix(prefix)).flatten();
            let (list, value) = if let Some(v) = op("tag:") {
                (&mut q.tags, v.trim_start_matches('#'))
            } else if let Some(v) = op("path:") {
                (&mut q.paths, v)
            } else if let Some(v) = op("file:") {
                (&mut q.files, v)
            } else if let Some(v) = op("-") {
                (&mut q.exclude, v)
            } else {
                (&mut q.terms, token.as_str())
            };
            if !value.is_empty() {
                list.push(value.to_string());
            }
        }
        q
    }

    fn is_empty(&self) -> bool {
        self.terms.is_empty() && self.exclude.is_empty() && self.tags.is_empty() && self.paths.is_empty() && self.files.is_empty()
    }
}

/// Whitespace-separated tokens; `"…"` keeps spaces (also after an
/// operator: `path:"my folder"`). The flag is set for tokens that start
/// with a quote, which are never operators.
fn tokens(query: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted_start = false;
    let mut in_quote = false;
    let mut started = false;
    for c in query.chars() {
        if c == '"' {
            if !started {
                quoted_start = true;
                started = true;
            }
            in_quote = !in_quote;
        } else if c.is_whitespace() && !in_quote {
            if started {
                out.push((std::mem::take(&mut cur).trim().to_string(), quoted_start));
            }
            started = false;
            quoted_start = false;
        } else {
            cur.push(c);
            started = true;
        }
    }
    if started {
        out.push((cur.trim().to_string(), quoted_start));
    }
    out.retain(|(t, _)| !t.is_empty());
    out
}

/// Byte offsets of ASCII-case-insensitive matches of `needle_lower`.
/// Offsets are always char boundaries (a UTF-8 lead byte never equals a
/// continuation byte), so they can be used to slice `hay`.
fn find_all_ci<'a>(hay: &'a str, needle_lower: &'a str) -> impl Iterator<Item = usize> + 'a {
    let h = hay.as_bytes();
    let n = needle_lower.as_bytes();
    let first = n.first().copied().unwrap_or(0);
    let upper = first.to_ascii_uppercase();
    let mut pos = 0;
    std::iter::from_fn(move || {
        if n.is_empty() {
            return None;
        }
        while pos + n.len() <= h.len() {
            let found = memchr::memchr2(first, upper, &h[pos..=h.len() - n.len()])?;
            let at = pos + found;
            if h[at..at + n.len()].eq_ignore_ascii_case(n) {
                pos = at + n.len();
                return Some(at);
            }
            pos = at + 1;
        }
        None
    })
}

fn snippets(content: &str, positions: &[(usize, usize)]) -> Vec<Snippet> {
    let mut out: Vec<Snippet> = Vec::new();
    let mut last_line_start = usize::MAX;
    for &(pos, len) in positions {
        let line_start = content[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
        if line_start == last_line_start {
            continue; // one snippet per line
        }
        last_line_start = line_start;
        let line_end = content[pos..].find('\n').map(|i| pos + i).unwrap_or(content.len());
        let before = &content[line_start..pos];
        let after = &content[pos + len..line_end];
        out.push(Snippet {
            line: content[..line_start].matches('\n').count(),
            before: tail_chars(before.trim_start(), SNIPPET_BEFORE),
            hit: content[pos..pos + len].to_string(),
            after: clip(after.trim_end(), SNIPPET_AFTER),
        });
        if out.len() >= SNIPPETS_PER_NOTE {
            break;
        }
    }
    out
}

fn clip(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

fn tail_chars(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    let skip = s.char_indices().nth(count - max_chars).map(|(i, _)| i).unwrap_or(0);
    format!("…{}", &s[skip..])
}

/// Collapse `.` and `..` in a `/`-separated relative path. `None` if it escapes.
fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

pub fn rel_string(root: &Path, full: &Path) -> String {
    full.strip_prefix(root)
        .unwrap_or(full)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index_of(files: &[(&str, &str)]) -> (tempfile::TempDir, VaultIndex) {
        let dir = tempfile::tempdir().unwrap();
        for (path, content) in files {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, content).unwrap();
        }
        let index = VaultIndex::build(dir.path()).unwrap();
        (dir, index)
    }

    #[test]
    fn resolves_like_obsidian() {
        let (_d, idx) = index_of(&[
            ("Note.md", ""),
            ("A/Note.md", ""),
            ("A/B/Deep.md", ""),
            ("A/Sibling.md", ""),
            ("img/pic.png", ""),
            (".obsidian/hidden.md", ""),
        ]);
        // exact path, with or without .md
        assert_eq!(idx.resolve("x.md", "A/Note").as_deref(), Some("A/Note.md"));
        assert_eq!(idx.resolve("x.md", "A/Note.md").as_deref(), Some("A/Note.md"));
        // bare name: shortest path wins
        assert_eq!(idx.resolve("A/B/Deep.md", "Note").as_deref(), Some("Note.md"));
        // name anywhere in vault, case-insensitive
        assert_eq!(idx.resolve("x.md", "deep").as_deref(), Some("A/B/Deep.md"));
        // relative to the source folder
        assert_eq!(idx.resolve("A/B/Deep.md", "../Sibling").as_deref(), Some("A/Sibling.md"));
        // path suffix
        assert_eq!(idx.resolve("x.md", "B/Deep").as_deref(), Some("A/B/Deep.md"));
        // attachments
        assert_eq!(idx.resolve("x.md", "pic.png").as_deref(), Some("img/pic.png"));
        // unresolved & hidden
        assert_eq!(idx.resolve("x.md", "Missing"), None);
        assert_eq!(idx.resolve("x.md", "hidden"), None);
    }

    #[test]
    fn backlinks_with_line_context() {
        let (_d, idx) = index_of(&[
            ("Target.md", "self link [[Target]]"),
            ("One.md", "intro\nsee [[Target]] here\n\nand [[target|again]]"),
            ("Two.md", "embed ![[Target#Heading]]"),
            ("Three.md", "no links, just Target text"),
            ("Code.md", "```\n[[Target]]\n```"),
        ]);
        let bl = idx.backlinks("Target.md");
        let sources: Vec<_> = bl.iter().map(|b| b.source.as_str()).collect();
        assert_eq!(sources, ["One.md", "Two.md"]);
        assert_eq!(bl[0].title, "One");
        assert_eq!(
            bl[0].lines,
            [
                LineRef { line: 1, text: "see [[Target]] here".into() },
                LineRef { line: 3, text: "and [[target|again]]".into() },
            ]
        );
    }

    #[test]
    fn search_korean_substring_and_case() {
        let (_d, idx) = index_of(&[
            ("회의록.md", "주간 회의록을 정리했다.\nBudget review는 다음 주."),
            ("Other.md", "회의 없음. budget only"),
            ("Plain.md", "nothing here"),
        ]);
        // Korean substring inside a longer word
        let hits = idx.search("회의", 10);
        let paths: Vec<_> = hits.iter().map(|h| h.path.as_str()).collect();
        // file-name match ranks first
        assert_eq!(paths, ["회의록.md", "Other.md"]);
        assert!(hits[0].path_match);
        assert_eq!(hits[0].snippets[0].hit, "회의");
        assert_eq!(hits[0].snippets[0].before, "주간 ");
        assert_eq!(hits[0].snippets[0].after, "록을 정리했다.");

        // ASCII case-insensitive, original casing kept in the snippet
        let hits = idx.search("BUDGET", 10);
        assert_eq!(hits.len(), 2);
        let first = hits.iter().find(|h| h.path == "회의록.md").unwrap();
        assert_eq!(first.snippets[0].hit, "Budget");
        assert_eq!(first.snippets[0].line, 1);
    }

    #[test]
    fn search_requires_all_terms_and_supports_phrases() {
        let (_d, idx) = index_of(&[
            ("a.md", "apple banana"),
            ("b.md", "apple only"),
            ("c.md", "banana apple"),
        ]);
        let paths = |q: &str| idx.search(q, 10).into_iter().map(|h| h.path).collect::<Vec<_>>();
        assert_eq!(paths("apple banana"), ["a.md", "c.md"]);
        assert_eq!(paths("\"apple banana\""), ["a.md"]);
        assert!(paths("   ").is_empty());
        assert!(paths("zzz").is_empty());
    }

    #[test]
    fn query_tokens_and_operators() {
        let q = Query::parse("Apple tag:#Work path:\"my folder\" file:log -draft \"-not op\" tag:");
        assert_eq!(q.terms, ["apple", "-not op"]);
        assert_eq!(q.tags, ["work"]);
        assert_eq!(q.paths, ["my folder"]);
        assert_eq!(q.files, ["log"]);
        assert_eq!(q.exclude, ["draft"]);
    }

    #[test]
    fn search_operators_filter() {
        let (_d, idx) = index_of(&[
            ("Work/plan.md", "---\ntags: [project]\n---\nplan #work/meeting draft"),
            ("Work/log.md", "log #work"),
            ("Home/list.md", "groceries #home #workshop"),
        ]);
        let paths = |q: &str| idx.search(q, 10).into_iter().map(|h| h.path).collect::<Vec<_>>();
        assert_eq!(paths("tag:work"), ["Work/log.md", "Work/plan.md"]);
        assert_eq!(paths("tag:#work/meeting"), ["Work/plan.md"]);
        assert_eq!(paths("tag:project"), ["Work/plan.md"]);
        assert_eq!(paths("tag:work -draft"), ["Work/log.md"]);
        assert_eq!(paths("path:home"), ["Home/list.md"]);
        assert_eq!(paths("file:plan"), ["Work/plan.md"]);
        assert_eq!(paths("path:work log"), ["Work/log.md"]);
        let hit = &idx.search("tag:work", 10)[0];
        assert_eq!(hit.snippets[0].hit, "#work");
    }

    #[test]
    fn tag_counts_merge_case() {
        let (_d, idx) = index_of(&[("a.md", "#Idea #x"), ("b.md", "#idea"), ("c.md", "none")]);
        let tags = idx.tag_counts();
        assert_eq!(
            tags,
            [TagCount { tag: "Idea".into(), count: 2 }, TagCount { tag: "x".into(), count: 1 }]
        );
    }

    #[test]
    fn search_finds_attachments_by_name() {
        let (_d, idx) = index_of(&[("files/영수증-11월.pdf", "%PDF")]);
        let hits = idx.search("영수증", 10);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].path_match);
        assert!(hits[0].snippets.is_empty());
    }

    #[test]
    fn refresh_tracks_disk_changes() {
        let (d, mut idx) = index_of(&[("Target.md", ""), ("Dir/Linker.md", "[[Target]]")]);
        let root = d.path();
        assert_eq!(idx.backlinks("Target.md").len(), 1);

        // edit: link removed
        fs::write(root.join("Dir/Linker.md"), "no more link").unwrap();
        assert!(idx.refresh_path(root, "Dir/Linker.md"));
        assert!(idx.backlinks("Target.md").is_empty());

        // new file
        fs::write(root.join("New.md"), "[[Target]]").unwrap();
        idx.refresh_path(root, "New.md");
        assert_eq!(idx.backlinks("Target.md")[0].source, "New.md");

        // folder deleted -> everything below goes
        fs::remove_dir_all(root.join("Dir")).unwrap();
        assert!(idx.refresh_path(root, "Dir"));
        assert_eq!(idx.resolve("x.md", "Linker"), None);

        // hidden paths ignored
        assert!(!idx.refresh_path(root, ".obsidian/workspace.json"));
        assert_eq!(idx.note_count(), 2);
    }

    #[test]
    fn collects_tasks_across_notes() {
        let (d, mut idx) = index_of(&[
            ("B.md", "- [ ] b1 📅 2026-11-20\n- [x] b2 ✅ 2026-11-01"),
            ("A/Daily.md", "text\n- [/] a1\n```\n- [ ] in code\n```"),
        ]);
        let open: Vec<_> = idx.tasks(false).into_iter().map(|t| (t.path, t.task.line, t.task.text)).collect();
        assert_eq!(open, [("A/Daily.md".into(), 1, "a1".into()), ("B.md".into(), 0, "b1".into())]);
        assert_eq!(idx.tasks(true).len(), 3);

        fs::write(d.path().join("B.md"), "- [x] b1").unwrap();
        idx.refresh_path(d.path(), "B.md");
        assert_eq!(idx.tasks(false).len(), 1);
        let json = serde_json::to_value(&idx.tasks(false)[0]).unwrap();
        assert_eq!(json["path"], "A/Daily.md");
        assert_eq!(json["kind"], "in_progress");
        assert_eq!(json["status"], "/");
    }

    #[test]
    fn outgoing_links_and_unlinked_mentions() {
        let (_d, idx) = index_of(&[
            ("회의록.md", "---\naliases: [미팅]\n---\n[[프로젝트]] 진행, [[없는 노트]], ![[그림.png]]\n다시 [[프로젝트|PJ]]"),
            ("프로젝트.md", "회의록 정리했다\n[[회의록]] 링크와 회의록 둘 다\n`회의록` 코드"),
            ("일기.md", "---\ntitle: 회의록 이야기\n---\n오늘 미팅이 길었다"),
            ("그림.png", ""),
            ("다른/회의록.md", ""),
        ]);
        let out: Vec<_> = idx.outgoing("회의록.md").iter().map(|o| (o.target.clone(), o.resolved.clone(), o.embed, o.lines.clone())).collect();
        assert_eq!(out, [
            ("프로젝트".to_string(), Some("프로젝트.md".to_string()), false, vec![3, 4]),
            ("없는 노트".to_string(), None, false, vec![3]),
            ("그림.png".to_string(), Some("그림.png".to_string()), true, vec![3]),
        ]);
        let m: Vec<_> = idx
            .unlinked_mentions("회의록.md")
            .iter()
            .map(|m| (m.source.clone(), m.lines.iter().map(|l| (l.line, l.term.clone())).collect::<Vec<_>>()))
            .collect();
        assert_eq!(m, [
            // the other 회의록 note mentions nothing; 일기's frontmatter title is skipped, the alias is found
            ("일기.md".to_string(), vec![(3, "미팅".to_string())]),
            ("프로젝트.md".to_string(), vec![(0, "회의록".to_string()), (1, "회의록".to_string())]),
        ]);
        // Two files named 회의록.md: links need the path (at the root that is the title anyway).
        assert_eq!(idx.link_target("다른/회의록.md"), "다른/회의록");
        assert_eq!(idx.link_target("회의록.md"), "회의록");
        assert_eq!(idx.link_target("프로젝트.md"), "프로젝트");
    }

    #[test]
    fn balances_are_listed_oldest_first_with_note_dates() {
        let (_d, idx) = index_of(&[
            ("2026-11-14 자산.md", "- 🏦 2,700,000 청년도약 #저축\n- 🏦 5,300 카카오페이 #페이 📅 2026-11-13\n- 💸 1,000 커피"),
            ("Loose.md", "- 🏦 100 어디 #기타"),
            ("Old.md", "- 🏦 2,400,000 청년도약 #저축 📅 2026-10-14"),
        ]);
        let got: Vec<_> = idx.balances(None).iter().map(|b| (b.date.clone(), b.account.clone(), b.amount / 100)).collect();
        assert_eq!(got, [
            (None, "어디".to_string(), 100),
            (Some("2026-10-14".to_string()), "청년도약".to_string(), 2_400_000),
            (Some("2026-11-13".to_string()), "카카오페이".to_string(), 5_300),
            (Some("2026-11-14".to_string()), "청년도약".to_string(), 2_700_000), // the file name's date
        ]);
    }

    #[test]
    fn finance_dates_come_from_line_then_note() {
        let (_d, idx) = index_of(&[
            ("Daily/2026-11-13.md", "- 💸 12,000 점심 #식비\n- 💸 5000 어제 커피 #카페 📅 2026-11-12"),
            ("Trip.md", "---\ndate: 2026-11-05\n---\n- 💸 30000 숙소 #여행"),
            ("2026-11-20 회의.md", "- 💰 50000 강의료 #부수입"),
            ("Loose.md", "- 💸 9000 언제인지 모름 #식비"),
            ("Old/2026-10-01.md", "- 💸 1000 지난달 #식비"),
            ("Code.md", "```\n- 💸 1 not real\n```"),
        ]);
        let cfg = crate::daily::DailyConfig { folder: "Daily".into(), ..Default::default() };
        let m = cfg.matcher().unwrap();
        let data = idx.finance(Some(&m), "2026-11-01", "2026-11-30");

        let got: Vec<_> = data.entries.iter().map(|e| (e.date.clone().unwrap(), e.memo.as_str(), e.amount)).collect();
        assert_eq!(
            got,
            [
                ("2026-11-05".to_string(), "숙소", 3_000_000),   // frontmatter date
                ("2026-11-12".to_string(), "어제 커피", 500_000), // explicit 📅 wins over the daily note
                ("2026-11-13".to_string(), "점심", 1_200_000),   // daily note path
                ("2026-11-20".to_string(), "강의료", 5_000_000), // file name prefix
            ]
        );
        assert_eq!(data.undated.len(), 1);
        assert_eq!(data.undated[0].path, "Loose.md");
        // "Old/2026-10-01.md" is dated by its file name, outside the range
        // all-time categories, most used first ("식비" 3 times), ties alphabetical
        assert_eq!(data.categories, ["식비", "부수입", "여행", "카페"]);
    }

    #[test]
    fn find_all_ci_is_boundary_safe() {
        let hay = "aÅa AA 가a";
        let found: Vec<_> = find_all_ci(hay, "a").collect();
        for p in &found {
            assert!(hay.is_char_boundary(*p));
        }
        assert_eq!(found.len(), 5);
    }
}
