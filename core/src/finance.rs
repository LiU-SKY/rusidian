//! Money records written as list items anywhere in the vault:
//!
//! ```text
//! - 💸 12,000 점심 김밥 #식비            expense
//! - 💰 3,000,000원 월급 #급여            income
//! - 💸 ₩45,000 택시 #교통/택시 📅 2026-11-10
//! - 💸 -23,000 무신사 환불 #옷             a refund: lowers spending
//! - 💰 -300,800 연구실 비용 #인건비         a deduction: lowers income
//! ```
//!
//! Records indented under another record are its settlements (money back
//! from people who shared the bill, refunds). They count on their own, so
//! totals stay right, and take the parent's category and date unless they
//! have their own:
//!
//! ```text
//! - 💸 30,600 맘스터치 #식비 📅 2026-10-28
//!     - 💸 -10,100 김민준 정산
//!     - 💸 -10,100 박서준 정산 📅 2026-10-29
//! ```
//!
//! The first `#tag` is the category. The date is the `📅` date if present,
//! otherwise it comes from the note (see `index.rs`). Amounts are kept in
//! hundredths so cents work too, without floating point.

use crate::date::Date;
use crate::markdown::prose_lines;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Expense,
    Income,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Entry {
    pub line: usize,
    /// The line as written; edits are refused if it changed since.
    pub raw: String,
    pub kind: EntryKind,
    /// Amount × 100 (12,000원 → 1_200_000). Negative for a refund or a
    /// deduction, which lowers the total of its kind and category.
    pub amount: i64,
    pub memo: String,
    /// First tag without `#`, e.g. "교통/택시".
    pub category: Option<String>,
    /// Explicit `📅` date on the line (or the parent record's).
    pub date: Option<String>,
    /// Line of the record this one is indented under (a settlement of it).
    pub parent: Option<usize>,
}

static LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^[ \t]*(?:>[ \t]*)*(?:[-*+][ \t]+)?(💸|💰)\x{FE0F}?[ \t]*([-−]?)[ \t]*₩?[ \t]*(\d[\d,]*(?:\.\d{1,2})?)[ \t]*(?:원)?(?:[ \t]+(.*))?$",
    )
    .unwrap()
});
static DATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:📅|📆|🗓)\x{FE0F}?\s*(\d{4}-\d{2}-\d{2})").unwrap());
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|\s)#([\p{L}\p{N}_/-]+)").unwrap());
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s{2,}").unwrap());

/// One record with its note and its final date.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FinanceItem {
    pub path: String,
    pub title: String,
    pub line: usize,
    pub raw: String,
    pub kind: EntryKind,
    pub amount: i64,
    pub memo: String,
    pub category: Option<String>,
    /// `📅` on the line, else the note's date; `None` if neither is known.
    pub date: Option<String>,
    /// The date was written on the line (`📅`), not taken from the note.
    pub explicit_date: bool,
    /// Line of the record this one settles (same note), if it is indented under one.
    pub parent_line: Option<usize>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct FinanceData {
    /// Records dated inside the requested range, oldest first.
    pub entries: Vec<FinanceItem>,
    /// Records whose date can't be determined (never counted in totals).
    pub undated: Vec<FinanceItem>,
    /// Every category used in the vault, most used first (for suggestions).
    pub categories: Vec<String>,
}

pub fn parse_entries(text: &str) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    // Records that can still take settlements: (indent, index in `out`).
    let mut open: Vec<(usize, usize)> = Vec::new();
    for (n, line) in prose_lines(text) {
        if line.trim().is_empty() {
            continue; // loose lists keep their structure across blank lines
        }
        let Some(indent) = list_indent(line) else {
            open.clear(); // a paragraph or heading ends the list
            continue;
        };
        while open.last().is_some_and(|&(i, _)| i >= indent) {
            open.pop();
        }
        let Some(mut e) = parse_line(n, line) else { continue };
        if let Some(&(_, at)) = open.last() {
            let parent = &out[at];
            e.parent = Some(parent.line);
            if e.category.is_none() {
                e.category = parent.category.clone();
            }
            if e.date.is_none() {
                e.date = parent.date.clone();
            }
        }
        open.push((indent, out.len()));
        out.push(e);
    }
    out
}

/// Width of the indent before a list marker (tab = 4), or `None` for a
/// line that is not a list item.
fn list_indent(line: &str) -> Option<usize> {
    let body = line.trim_start_matches([' ', '\t']);
    let marker = body.chars().next()?;
    let after = body.get(1..)?;
    if !matches!(marker, '-' | '*' | '+') || !(after.starts_with(' ') || after.starts_with('\t')) {
        return None;
    }
    Some(line[..line.len() - body.len()].chars().map(|c| if c == '\t' { 4 } else { 1 }).sum())
}

pub fn parse_line(line_no: usize, line: &str) -> Option<Entry> {
    let caps = LINE.captures(line)?;
    let kind = if &caps[1] == "💸" { EntryKind::Expense } else { EntryKind::Income };
    let amount = parse_amount(&caps[3])?;
    let amount = if caps[2].is_empty() { amount } else { -amount };
    let rest = caps.get(4).map(|m| m.as_str()).unwrap_or("");

    let date = DATE.captures(rest).map(|c| c[1].to_string()).filter(|d| Date::parse(d).is_some());
    let category = TAG
        .captures_iter(rest)
        .map(|c| c[1].to_string())
        .find(|t| !t.chars().all(|c| c.is_ascii_digit()));
    let memo = DATE.replace_all(rest, " ");
    let memo = TAG.replace_all(&memo, " ");
    let memo = SPACES.replace_all(memo.trim(), " ").into_owned();

    Some(Entry { line: line_no, raw: line.to_string(), kind, amount, memo, category, date, parent: None })
}

/// List marker / indent / quote before the emoji, kept when a line is rewritten.
static PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[ \t]*(?:>[ \t]*)*(?:[-*+][ \t]+)?").unwrap());

/// What the edit dialog can change on a record.
#[derive(Debug, Clone, Deserialize)]
pub struct EntryEdit {
    pub kind: EntryKind,
    pub amount: String,
    pub memo: String,
    pub category: String,
    /// Written as `📅`; `None` keeps the note's date.
    pub date: Option<String>,
}

/// Rebuild record `line` from `edit`, keeping its list prefix and any tags
/// after the category (`#야근`).
pub fn render_edit(line: &str, edit: &EntryEdit) -> Result<String, String> {
    let caps = LINE.captures(line).ok_or("재무 기록 줄이 아닙니다.")?;
    let prefix = PREFIX.find(line).map(|m| m.as_str()).unwrap_or("");
    let date = match edit.date.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(d) => Some(Date::parse(d).ok_or_else(|| format!("잘못된 날짜: {d}"))?),
        None => None,
    };
    let fresh = entry_line(edit.kind, &edit.amount, &edit.memo, &edit.category, date)?;
    let body = fresh.strip_prefix("- ").unwrap_or(&fresh);
    // Extra tags of the old line (all but its category) stay.
    let rest = caps.get(4).map(|m| m.as_str()).unwrap_or("");
    let old_category = parse_line(0, line).and_then(|e| e.category);
    let extra: Vec<String> = TAG
        .captures_iter(rest)
        .map(|c| c[1].to_string())
        .filter(|t| Some(t) != old_category.as_ref() && !body.contains(&format!("#{t}")))
        .map(|t| format!("#{t}"))
        .collect();
    let mut out = format!("{prefix}{body}");
    if !extra.is_empty() {
        // Before the date, like the category.
        let at = out.find(" 📅").unwrap_or(out.len());
        out.insert_str(at, &format!(" {}", extra.join(" ")));
    }
    Ok(out)
}

/// `text` with the record on `line_no` rewritten from `edit`.
pub fn edit(text: &str, line_no: usize, expected: &str, edit: &EntryEdit) -> Result<String, String> {
    let new_line = render_edit(expected, edit)?;
    crate::markdown::replace_line(text, line_no, expected, Some(&new_line))
}

/// `text` without the record on `line_no`.
pub fn delete(text: &str, line_no: usize, expected: &str) -> Result<String, String> {
    parse_line(line_no, expected).ok_or("재무 기록 줄이 아닙니다.")?;
    crate::markdown::replace_line(text, line_no, expected, None)
}

/// `text` with a settlement (money back, a refund, a deduction) written under
/// the record on `line_no`, after the settlements it already has. `amount` is
/// what came back, so it is written negative. Refused unless the record still
/// reads `expected` and is a list item (only list items can nest).
pub fn insert_settlement(
    text: &str,
    line_no: usize,
    expected: &str,
    amount: &str,
    memo: &str,
    date: Option<Date>,
) -> Result<String, String> {
    let lines = crate::markdown::split_lines(text);
    let parent_line = crate::markdown::checked_line(&lines, line_no, expected)?;
    let parent = parse_line(line_no, parent_line).ok_or("재무 기록 줄이 아닙니다.")?;
    let indent = list_indent(parent_line).ok_or("'- '로 시작하는 목록 줄에만 정산을 붙일 수 있습니다.")?;
    let back = parse_signed_amount(amount.trim().trim_end_matches('원'))
        .filter(|a| *a > 0)
        .ok_or("돌려받은 금액을 확인하세요. (예: 10,000)")?;
    let date = date.filter(|d| parent.date.as_deref() != Some(&d.to_string()));
    let body = entry_line(parent.kind, &format!("-{}", format_amount(back)), memo, "", date)?;
    let lead = &parent_line[..parent_line.len() - parent_line.trim_start_matches([' ', '\t']).len()];
    let new_line = format!("{lead}    {body}");

    // After the record's own nested lines (deeper indent, no blank line in between).
    let mut at = line_no + 1;
    while let Some((content, _)) = lines.get(at) {
        let width: usize = content.chars().take_while(|c| *c == ' ' || *c == '\t').map(|c| if c == '\t' { 4 } else { 1 }).sum();
        if content.trim().is_empty() || width <= indent {
            break;
        }
        at += 1;
    }
    let eol = lines.iter().map(|(_, e)| *e).find(|e| !e.is_empty()).unwrap_or("\n");
    let mut out = String::with_capacity(text.len() + new_line.len() + 2);
    for (i, (content, end)) in lines.iter().enumerate() {
        if i == at {
            out.push_str(&new_line);
            out.push_str(eol);
        }
        out.push_str(content);
        // The last line may lack a newline; it needs one before the new line.
        out.push_str(if end.is_empty() && i + 1 == at { eol } else { end });
    }
    if at >= lines.len() {
        out.push_str(&new_line);
        out.push_str(if text.ends_with('\n') { eol } else { "" });
    }
    Ok(out)
}

/// Monthly budgets in hundredths (like amounts), kept in
/// `.obsidian/rusidian.json` under "budgets" so they travel with the vault.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Budgets {
    /// Spending limit for the whole month.
    #[serde(default)]
    pub total: Option<i64>,
    /// Per category (without `#`).
    #[serde(default)]
    pub categories: std::collections::BTreeMap<String, i64>,
}

const BUDGET_FILE: &str = "rusidian.json";
const BUDGET_KEY: &str = "budgets";

pub fn load_budgets(root: &std::path::Path) -> Budgets {
    crate::obsidian_config::read_key(root, BUDGET_FILE, BUDGET_KEY)
}

/// Save budgets; zero or negative amounts and blank categories are dropped.
pub fn save_budgets(root: &std::path::Path, budgets: &Budgets) -> Result<Budgets, String> {
    let clean = Budgets {
        total: budgets.total.filter(|v| *v > 0),
        categories: budgets
            .categories
            .iter()
            .map(|(k, v)| (k.trim().trim_start_matches('#').to_string(), *v))
            .filter(|(k, v)| !k.is_empty() && *v > 0)
            .collect(),
    };
    crate::obsidian_config::write_key(root, BUDGET_FILE, BUDGET_KEY, &clean)?;
    Ok(clean)
}

/// Records as CSV (UTF-8 with BOM, so Excel shows Korean right).
pub fn to_csv(items: &[FinanceItem]) -> String {
    fn cell(s: &str) -> String {
        if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
    }
    let mut out = String::from("\u{feff}날짜,구분,금액,분류,내용,노트\r\n");
    for e in items {
        let sign = if e.amount < 0 { "-" } else { "" };
        let amount = match e.amount.abs() % 100 {
            0 => format!("{sign}{}", e.amount.abs() / 100),
            frac => format!("{sign}{}.{frac:02}", e.amount.abs() / 100),
        };
        let row = [
            e.date.as_deref().unwrap_or(""),
            if e.kind == EntryKind::Expense { "지출" } else { "수입" },
            &amount,
            e.category.as_deref().unwrap_or(""),
            &e.memo,
            &e.path,
        ];
        out.push_str(&row.iter().map(|c| cell(c)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    out
}

/// "12,000" → 1_200_000; "9.5" → 950. Zero is not a record.
pub fn parse_amount(s: &str) -> Option<i64> {
    let s: String = s.chars().filter(|&c| c != ',').collect();
    let (whole, frac) = s.split_once('.').unwrap_or((&s, ""));
    if whole.is_empty() || frac.len() > 2 || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let whole: i64 = whole.parse().ok()?;
    let frac: i64 = if frac.is_empty() { 0 } else { format!("{frac:0<2}").parse().ok()? };
    let total = whole.checked_mul(100)?.checked_add(frac)?;
    (total > 0).then_some(total)
}

/// Like `parse_amount`, but a leading `-` or `−` makes a refund / deduction.
pub fn parse_signed_amount(s: &str) -> Option<i64> {
    let s = s.trim();
    match s.strip_prefix('-').or_else(|| s.strip_prefix('−')) {
        Some(rest) => parse_amount(rest.trim().trim_start_matches('₩').trim()).map(|a| -a),
        None => parse_amount(s.trim_start_matches('₩').trim()),
    }
}

/// Build a record line from the quick-add form.
/// `date` is written only when it differs from the note's own date.
pub fn entry_line(
    kind: EntryKind,
    amount: &str,
    memo: &str,
    category: &str,
    date: Option<Date>,
) -> Result<String, String> {
    let cents = parse_signed_amount(amount.trim().trim_end_matches('원'))
        .ok_or("금액을 확인하세요. (예: 12000, 12,000, 9.50, 환불은 -12,000)")?;
    let memo = memo.split_whitespace().collect::<Vec<_>>().join(" ");
    let category: String = category
        .trim()
        .trim_start_matches('#')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-");

    let mut line = format!(
        "- {} {}",
        if kind == EntryKind::Expense { "💸" } else { "💰" },
        format_amount(cents)
    );
    if !memo.is_empty() {
        line.push(' ');
        line.push_str(&memo);
    }
    if !category.is_empty() {
        line.push_str(" #");
        line.push_str(&category);
    }
    if let Some(d) = date {
        line.push_str(&format!(" 📅 {d}"));
    }
    Ok(line)
}

/// 1_200_000 → "12,000"; 950 → "9.50"; -230_000 → "-2,300".
pub fn format_amount(cents: i64) -> String {
    if cents < 0 {
        return format!("-{}", format_amount(-cents));
    }
    let whole = (cents / 100).to_string();
    let mut grouped = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    match cents % 100 {
        0 => grouped,
        frac => format!("{grouped}.{frac:02}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(line: &str) -> Entry {
        parse_line(0, line).unwrap_or_else(|| panic!("not an entry: {line}"))
    }

    #[test]
    fn parses_expenses_and_income() {
        let e = one("- 💸 12,000 점심 김밥 #식비");
        assert_eq!((e.kind, e.amount), (EntryKind::Expense, 1_200_000));
        assert_eq!(e.memo, "점심 김밥");
        assert_eq!(e.category.as_deref(), Some("식비"));
        assert_eq!(e.date, None);

        let e = one("* 💰 3,000,000원 월급 #급여");
        assert_eq!((e.kind, e.amount), (EntryKind::Income, 300_000_000));
        assert_eq!(e.memo, "월급");

        let e = one("- 💸 ₩45000 택시 #교통/택시 #야근 📅 2026-11-10");
        assert_eq!(e.amount, 4_500_000);
        assert_eq!(e.category.as_deref(), Some("교통/택시"));
        assert_eq!(e.memo, "택시");
        assert_eq!(e.date.as_deref(), Some("2026-11-10"));
    }

    #[test]
    fn line_forms() {
        assert_eq!(one("💸 5000").memo, ""); // no list marker, no memo
        assert_eq!(one("  > - 💸 9.5 coffee #food").amount, 950);
        assert_eq!(one("- 💸\u{FE0F} 1,234.56 x").amount, 123_456);
        assert_eq!(one("- 💸 3000 #2026 is not a category #생활").category.as_deref(), Some("생활"));
        assert_eq!(one("- 💸 3000 bad date 📅 2026-13-01").date, None);
    }

    #[test]
    fn non_entries() {
        for line in [
            "- 점심 💸 12000",        // emoji must lead
            "- 💸 점심",              // amount required
            "- 💸 0 free",            // zero
            "- 💸 1.234 too precise",
            "- [ ] 💸 5000 a task",
            "text 💰 100",
        ] {
            assert!(parse_line(0, line).is_none(), "{line:?}");
        }
        let text = "- 💸 1000 real\n```\n- 💸 2000 in code\n```";
        assert_eq!(parse_entries(text).len(), 1);
    }

    #[test]
    fn settlements_nest_under_their_payment() {
        let text = concat!(
            "# 10월\n",
            "\n",
            "- 💸 30,600 맘스터치 #식비 📅 2026-10-28\n",
            "    - 💸 -10,100 김민준 정산\n",
            "    - 메모만 있는 줄\n",
            "\n",
            "\t- 💸 -10,100 박서준 정산 📅 2026-10-29\n",
            "- 💸 5,000 편의점 #편의점\n",
            "  - 💸 -5,000 취소 #기타\n",
            "\n",
            "다음 문단\n",
            "    - 💸 1,000 따로\n",
        );
        let es = parse_entries(text);
        let got: Vec<_> = es.iter().map(|e| (e.line, e.amount / 100, e.parent, e.category.as_deref(), e.date.as_deref())).collect();
        assert_eq!(got, [
            (2, 30_600, None, Some("식비"), Some("2026-10-28")),
            (3, -10_100, Some(2), Some("식비"), Some("2026-10-28")),       // inherits category and date
            (6, -10_100, Some(2), Some("식비"), Some("2026-10-29")),       // own date; blank line and a note line between
            (7, 5_000, None, Some("편의점"), None),
            (8, -5_000, Some(7), Some("기타"), None),                       // own category wins
            (11, 1_000, None, None, None),                                 // after a paragraph: not nested
        ]);
    }

    #[test]
    fn settlement_goes_under_the_payment() {
        let text = "- 💸 30,600 맘스터치 #식비 📅 2026-10-28\n    - 💸 -10,100 김민준 정산\n- 💸 5,000 커피 #카페\n";
        let d = |s| Date::parse(s);
        // After the existing settlement; the parent's date is not repeated.
        let out = insert_settlement(text, 0, "- 💸 30,600 맘스터치 #식비 📅 2026-10-28", "10,100원", "박서준 정산", d("2026-10-28")).unwrap();
        assert_eq!(out, "- 💸 30,600 맘스터치 #식비 📅 2026-10-28\n    - 💸 -10,100 김민준 정산\n    - 💸 -10,100 박서준 정산\n- 💸 5,000 커피 #카페\n");
        let es = parse_entries(&out);
        assert_eq!((es[2].parent, es[2].amount, es[2].category.as_deref()), (Some(0), -1_010_000, Some("식비")));
        // Last line without a newline, CRLF kept, another date written.
        let out = insert_settlement("# x\r\n  - 💸 5,000 커피", 1, "  - 💸 5,000 커피", "2500", "반 돌려받음", d("2026-11-02")).unwrap();
        assert_eq!(out, "# x\r\n  - 💸 5,000 커피\r\n      - 💸 -2,500 반 돌려받음 📅 2026-11-02");
        // Refusals: changed line, not a list item, bad amount.
        assert!(insert_settlement(text, 2, "- 💸 9,999 바뀜", "1", "", None).is_err());
        assert!(insert_settlement("💸 5,000 x\n", 0, "💸 5,000 x", "1", "", None).is_err());
        assert!(insert_settlement(text, 2, "- 💸 5,000 커피 #카페", "-3", "", None).is_err());
    }

    #[test]
    fn negative_records() {
        let e = one("- 💸 -23,000 무신사 환불 #옷");
        assert_eq!((e.kind, e.amount, e.memo.as_str(), e.category.as_deref()), (EntryKind::Expense, -2_300_000, "무신사 환불", Some("옷")));
        assert_eq!(one("- 💰 −300,800원 연구실 비용 #인건비 📅 2026-10-26").amount, -30_080_000);
        assert_eq!(one("- 💸 - ₩5,000 x").amount, -500_000);
        assert_eq!(parse_signed_amount("-12,000"), Some(-1_200_000));
        assert_eq!(parse_signed_amount("−₩9.5"), Some(-950));
        assert_eq!(parse_signed_amount("12,000"), Some(1_200_000));
        assert_eq!(parse_signed_amount("-0"), None);
        assert_eq!(format_amount(-2_300_000), "-23,000");
        assert_eq!(format_amount(-950), "-9.50");
        let line = entry_line(EntryKind::Expense, "-10,100원", "맘스터치 몫 돌려받음", "식비", None).unwrap();
        assert_eq!(line, "- 💸 -10,100 맘스터치 몫 돌려받음 #식비");
        assert_eq!(parse_line(0, &line).unwrap().amount, -1_010_000);
        // Editing a refund keeps its sign handling and extra tags.
        let e = edit_of(EntryKind::Expense, "-6,000", "돌려받음", "식비", None);
        assert_eq!(render_edit("- 💸 -5,000 x #식비 #정산", &e).unwrap(), "- 💸 -6,000 돌려받음 #식비 #정산");
    }

    #[test]
    fn amounts() {
        assert_eq!(parse_amount("12,000"), Some(1_200_000));
        assert_eq!(parse_amount("9.5"), Some(950));
        assert_eq!(parse_amount("9.05"), Some(905));
        assert_eq!(parse_amount("0.00"), None);
        assert_eq!(parse_amount("99999999999999999999"), None); // overflow
        assert_eq!(format_amount(1_200_000), "12,000");
        assert_eq!(format_amount(300_000_000), "3,000,000");
        assert_eq!(format_amount(950), "9.50");
        assert_eq!(format_amount(100), "1");
    }

    #[test]
    fn builds_lines_that_parse_back() {
        let d = Date::parse("2026-11-10").unwrap();
        let line = entry_line(EntryKind::Expense, "45000원", "  야근  택시 ", "#교통/택시", Some(d)).unwrap();
        assert_eq!(line, "- 💸 45,000 야근 택시 #교통/택시 📅 2026-11-10");
        let back = one(&line);
        assert_eq!((back.amount, back.memo.as_str(), back.category.as_deref()), (4_500_000, "야근 택시", Some("교통/택시")));

        let line = entry_line(EntryKind::Income, "3,000,000", "월급", "급여 보너스", None).unwrap();
        assert_eq!(line, "- 💰 3,000,000 월급 #급여-보너스");

        assert!(entry_line(EntryKind::Expense, "abc", "", "", None).is_err());
        assert!(entry_line(EntryKind::Expense, "0", "", "", None).is_err());
    }

    fn edit_of(kind: EntryKind, amount: &str, memo: &str, category: &str, date: Option<&str>) -> EntryEdit {
        EntryEdit { kind, amount: amount.into(), memo: memo.into(), category: category.into(), date: date.map(Into::into) }
    }

    #[test]
    fn edit_keeps_prefix_and_extra_tags() {
        let line = "  > * 💸 45,000 택시 #교통/택시 #야근 📅 2026-11-10";
        let e = edit_of(EntryKind::Expense, "52000", "심야 택시", "교통", Some("2026-11-11"));
        assert_eq!(render_edit(line, &e).unwrap(), "  > * 💸 52,000 심야 택시 #교통 #야근 📅 2026-11-11");
        // Income, no date (the note's date applies), category cleared.
        let e = edit_of(EntryKind::Income, "10,000", "환불", "", None);
        assert_eq!(render_edit("- 💸 3000 x #식비", &e).unwrap(), "- 💰 10,000 환불");
        assert!(render_edit(line, &edit_of(EntryKind::Expense, "0", "", "", None)).is_err());
        assert!(render_edit(line, &edit_of(EntryKind::Expense, "1", "", "", Some("2026-02-30"))).is_err());
    }

    #[test]
    fn edit_and_delete_in_file() {
        let text = "# 11/13\r\n- 💸 5000 커피 #식비\r\n- 💰 100 이자\r\n";
        let e = edit_of(EntryKind::Expense, "5500", "커피", "식비", None);
        assert_eq!(
            edit(text, 1, "- 💸 5000 커피 #식비", &e).unwrap(),
            "# 11/13\r\n- 💸 5,500 커피 #식비\r\n- 💰 100 이자\r\n"
        );
        assert_eq!(delete(text, 2, "- 💰 100 이자").unwrap(), "# 11/13\r\n- 💸 5000 커피 #식비\r\n");
        assert!(delete(text, 1, "- 💸 9999 바뀜").is_err());
        assert!(delete(text, 0, "# 11/13").is_err());
    }

    #[test]
    fn csv_export() {
        let item = |amount, memo: &str, kind| FinanceItem {
            path: "Daily/2026-11-13.md".into(),
            title: "2026-11-13".into(),
            line: 0,
            raw: String::new(),
            kind,
            amount,
            memo: memo.into(),
            category: Some("식비".into()),
            date: Some("2026-11-13".into()),
            explicit_date: false,
            parent_line: None,
        };
        let csv = to_csv(&[item(1_200_000, "점심, 김밥", EntryKind::Expense), item(950, "say \"hi\"", EntryKind::Income)]);
        assert_eq!(
            csv,
            "\u{feff}날짜,구분,금액,분류,내용,노트\r\n\
             2026-11-13,지출,12000,식비,\"점심, 김밥\",Daily/2026-11-13.md\r\n\
             2026-11-13,수입,9.50,식비,\"say \"\"hi\"\"\",Daily/2026-11-13.md\r\n"
        );
    }
}
