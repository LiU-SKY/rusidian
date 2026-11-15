//! Fixed monthly costs (rent, phone, subscriptions, savings transfers) and
//! whether this month's payment has shown up in the money records.
//!
//! The list lives in `.obsidian/rusidian.json` under "fixed_expenses", next
//! to the budgets, so it travels with the vault. A payment is recognised by
//! its keyword appearing in a record's memo.

use crate::date::{days_in_month, Date};
use crate::finance::{EntryKind, FinanceItem};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

const FILE: &str = "rusidian.json";
const KEY: &str = "fixed_expenses";
/// How many days before the due day to remind (missing: no reminders).
const REMIND_KEY: &str = "fixed_remind_days";
/// Suggestions the user hid (their keywords, lowercase).
const IGNORED_KEY: &str = "fixed_ignored";
/// A due day this many days past with no payment is still reminded of.
pub const LATE_DAYS: i64 = 3;
/// A payment this many days before the due day still pays it.
const EARLY_DAYS: i64 = 15;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FixedExpense {
    pub name: String,
    /// Expected amount × 100, like record amounts.
    pub amount: i64,
    /// Day of the month it goes out (1–31; short months use their last day).
    pub day: u32,
    /// Text in a record's memo that marks the payment (case-insensitive).
    pub keyword: String,
    #[serde(default)]
    pub category: Option<String>,
    /// The account it is paid from (shown only).
    #[serde(default)]
    pub account: Option<String>,
}

pub fn load(root: &Path) -> Vec<FixedExpense> {
    crate::obsidian_config::read_key(root, FILE, KEY)
}

/// Save the list, cleaned: blank names, non-positive amounts and bad days are
/// dropped; a blank keyword falls back to the name. Sorted by day.
pub fn save(root: &Path, list: &[FixedExpense]) -> Result<Vec<FixedExpense>, String> {
    let blank = |s: Option<&String>| s.map(|s| s.trim().trim_start_matches('#').to_string()).filter(|s| !s.is_empty());
    let mut clean: Vec<FixedExpense> = list
        .iter()
        .filter(|f| !f.name.trim().is_empty() && f.amount > 0 && (1..=31).contains(&f.day))
        .map(|f| FixedExpense {
            name: f.name.trim().to_string(),
            amount: f.amount,
            day: f.day,
            keyword: Some(f.keyword.trim()).filter(|k| !k.is_empty()).unwrap_or(f.name.trim()).to_string(),
            category: blank(f.category.as_ref()),
            account: blank(f.account.as_ref()),
        })
        .collect();
    clean.sort_by(|a, b| a.day.cmp(&b.day).then_with(|| a.name.cmp(&b.name)));
    crate::obsidian_config::write_key(root, FILE, KEY, &clean)?;
    Ok(clean)
}

pub fn load_remind_days(root: &Path) -> Option<u32> {
    crate::obsidian_config::read_key(root, FILE, REMIND_KEY)
}

/// Remind `days` before each due day (0: on the day); None turns reminders off.
pub fn save_remind_days(root: &Path, days: Option<u32>) -> Result<(), String> {
    match days {
        Some(d) if d > 31 => Err("31일 이내로 고르세요.".into()),
        Some(d) => crate::obsidian_config::write_key(root, FILE, REMIND_KEY, &d),
        None => crate::obsidian_config::remove_key(root, FILE, REMIND_KEY),
    }
}

pub fn load_ignored(root: &Path) -> Vec<String> {
    crate::obsidian_config::read_key(root, FILE, IGNORED_KEY)
}

/// Hide the suggestion with this keyword (None: show every hidden one again).
pub fn ignore(root: &Path, keyword: Option<&str>) -> Result<Vec<String>, String> {
    let Some(keyword) = keyword.map(|k| k.trim().to_lowercase()).filter(|k| !k.is_empty()) else {
        crate::obsidian_config::remove_key(root, FILE, IGNORED_KEY)?;
        return Ok(Vec::new());
    };
    let mut list = load_ignored(root);
    if !list.contains(&keyword) {
        list.push(keyword);
        list.sort();
        crate::obsidian_config::write_key(root, FILE, IGNORED_KEY, &list)?;
    }
    Ok(list)
}

/// A fixed expense whose due day is near (or just past) with no payment yet.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FixedReminder {
    pub name: String,
    pub amount: i64,
    pub due: String,
    /// Days until the due day: 0 today, negative when it has passed.
    pub days: i64,
}

/// Fixed expenses due within `before` days of `today`, or up to
/// LATE_DAYS ago, that no record has paid yet. A payment counts from
/// EARLY_DAYS before its due day, across month ends (the 31st's bill
/// paid on the 2nd).
pub fn reminders(list: &[FixedExpense], items: &[FinanceItem], today: Date, before: u32) -> Vec<FixedReminder> {
    let mut out = Vec::new();
    for f in list {
        let keyword = f.keyword.to_lowercase();
        for shift in -1..=1 {
            let month = today.add_months(shift);
            let Some(due) = due_date(&format!("{:04}-{:02}", month.year, month.month), f.day) else { continue };
            let days = today.days_until(due);
            if days < -LATE_DAYS || days > before as i64 {
                continue;
            }
            let from = due.add_days(-EARLY_DAYS);
            let until = if today > due { today } else { due };
            let paid = !keyword.is_empty()
                && items.iter().filter(|e| is_payment(e)).any(|e| {
                    e.memo.to_lowercase().contains(&keyword)
                        && e.date.as_deref().and_then(Date::parse).is_some_and(|d| d >= from && d <= until)
                });
            if !paid {
                out.push(FixedReminder { name: f.name.clone(), amount: f.amount, due: due.to_string(), days });
            }
        }
    }
    out.sort_by(|a, b| a.due.cmp(&b.due).then_with(|| a.name.cmp(&b.name)));
    out
}

/// The record that paid a fixed expense this month.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Paid {
    pub date: String,
    pub amount: i64,
    pub memo: String,
    pub path: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FixedStatus {
    /// Position in the saved list.
    pub index: usize,
    /// When it is due in the month asked for.
    pub due: String,
    pub paid: Option<Paid>,
}

fn due_date(month: &str, day: u32) -> Option<Date> {
    let year: i32 = month.get(..4)?.parse().ok()?;
    let m: u32 = month.get(5..7)?.parse().ok()?;
    let last = days_in_month(year, m);
    Date::new(year, m, day.min(last))
}

/// A payment, not a settlement or refund.
fn is_payment(e: &FinanceItem) -> bool {
    e.kind == EntryKind::Expense && e.amount > 0 && e.parent_line.is_none()
}

/// This month's state of each fixed expense: the record that paid it (the
/// matching one closest to the due day; a record pays at most one item).
pub fn status(list: &[FixedExpense], items: &[FinanceItem], month: &str) -> Vec<FixedStatus> {
    let mut used: HashSet<(&str, usize)> = HashSet::new();
    let mut out = Vec::new();
    for (index, f) in list.iter().enumerate() {
        let Some(due) = due_date(month, f.day) else { continue };
        let keyword = f.keyword.to_lowercase();
        let best = items
            .iter()
            .filter(|e| is_payment(e) && !keyword.is_empty())
            .filter(|e| e.date.as_deref().is_some_and(|d| d.starts_with(month)))
            .filter(|e| e.memo.to_lowercase().contains(&keyword))
            .filter(|e| !used.contains(&(e.path.as_str(), e.line)))
            .min_by_key(|e| e.date.as_deref().and_then(Date::parse).map_or(i64::MAX, |d| d.days_until(due).abs()));
        let paid = best.map(|e| {
            used.insert((e.path.as_str(), e.line));
            Paid { date: e.date.clone().unwrap_or_default(), amount: e.amount, memo: e.memo.clone(), path: e.path.clone(), line: e.line }
        });
        out.push(FixedStatus { index, due: due.to_string(), paid });
    }
    out
}

/// A payment that looks monthly: the same memo start in two or more months,
/// once a month, on about the same day (±5), for about the same amount (±20%,
/// bills vary). A name paid three or more times in some month is a shop you
/// visit, not a bill, and is never suggested.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Suggestion {
    pub name: String,
    pub keyword: String,
    pub amount: i64,
    pub day: u32,
    pub category: Option<String>,
    pub account: Option<String>,
    /// Months it was seen in.
    pub months: usize,
}

/// "청약저축53회 [농협]" → ("청약저축", Some("농협")): the memo up to its first
/// digit (dates, counters and order numbers change every month), and the
/// `[account]` written at the end, if any.
fn keyword_of(memo: &str) -> (String, Option<String>) {
    let (text, account) = match memo.trim_end().strip_suffix(']').and_then(|m| m.rsplit_once(" [")) {
        Some((text, account)) => (text, Some(account.trim().to_string())),
        None => (memo, None),
    };
    let head = text.split(|c: char| c.is_ascii_digit()).next().unwrap_or("");
    let head = head.trim().trim_end_matches(|c: char| !c.is_alphanumeric() && c != ')').trim();
    (head.to_string(), account.filter(|a| !a.is_empty()))
}

/// Days between two days of the month, wrapping at the month's end (30th and 2nd are 3 apart).
fn day_gap(a: u32, b: u32) -> u32 {
    let d = a.abs_diff(b);
    d.min(31 - d)
}

/// `ignored`: keywords of suggestions the user hid.
pub fn suggest(items: &[FinanceItem], existing: &[FixedExpense], ignored: &[String]) -> Vec<Suggestion> {
    let taken: Vec<String> = existing.iter().map(|f| f.keyword.to_lowercase()).filter(|k| !k.is_empty()).collect();
    // keyword → payments (date, amount, record)
    let mut groups: BTreeMap<String, Vec<(Date, &FinanceItem, Option<String>)>> = BTreeMap::new();
    for e in items.iter().filter(|e| is_payment(e)) {
        let Some(date) = e.date.as_deref().and_then(Date::parse) else { continue };
        let (keyword, account) = keyword_of(&e.memo);
        if keyword.chars().count() < 2
            || taken.iter().any(|t| e.memo.to_lowercase().contains(t))
            || ignored.contains(&keyword.to_lowercase())
        {
            continue;
        }
        groups.entry(keyword).or_default().push((date, e, account));
    }

    let mut out = Vec::new();
    for (keyword, payments) in groups {
        let mut per_month: BTreeMap<(i32, u32), usize> = BTreeMap::new();
        for (d, _, _) in &payments {
            *per_month.entry((d.year, d.month)).or_default() += 1;
        }
        if per_month.values().any(|&n| n >= 3) {
            continue;
        }
        // Try each payment's amount as the centre; keep the largest regular set.
        let mut best: Option<Vec<&(Date, &FinanceItem, Option<String>)>> = None;
        for (_, centre, _) in &payments {
            let near: Vec<_> = payments
                .iter()
                .filter(|(_, e, _)| (e.amount - centre.amount).abs() * 5 <= centre.amount)
                .collect();
            let months: BTreeSet<(i32, u32)> = near.iter().map(|(d, _, _)| (d.year, d.month)).collect();
            let once_a_month = months.len() == near.len();
            let days: Vec<u32> = near.iter().map(|(d, _, _)| d.day).collect();
            let steady = days.iter().all(|&a| days.iter().all(|&b| day_gap(a, b) <= 5));
            if months.len() >= 2 && once_a_month && steady && best.as_ref().is_none_or(|b| near.len() > b.len()) {
                best = Some(near);
            }
        }
        let Some(mut set) = best else { continue };
        set.sort_by_key(|(d, _, _)| *d);
        let (last_date, last, account) = set.last().unwrap();
        out.push(Suggestion {
            name: keyword.clone(),
            keyword,
            amount: last.amount,
            day: last_date.day,
            category: last.category.clone(),
            account: account.clone(),
            months: set.len(),
        });
    }
    out.sort_by(|a, b| a.day.cmp(&b.day).then_with(|| a.name.cmp(&b.name)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(date: &str, amount: i64, memo: &str, category: &str) -> FinanceItem {
        FinanceItem {
            path: "가계부.md".into(),
            title: "가계부".into(),
            line: 0,
            raw: String::new(),
            kind: EntryKind::Expense,
            amount: amount * 100,
            memo: memo.into(),
            category: Some(category.into()),
            date: Some(date.into()),
            explicit_date: true,
            parent_line: None,
        }
    }

    fn numbered(mut items: Vec<FinanceItem>) -> Vec<FinanceItem> {
        for (i, e) in items.iter_mut().enumerate() {
            e.line = i;
        }
        items
    }

    fn sample() -> Vec<FinanceItem> {
        numbered(vec![
            item("2026-09-15", 10_958, "DISCORD* NITROMONTHLY [토스]", "구독"),
            item("2026-10-15", 10_948, "DISCORD* NITROMONTHLY [토스]", "구독"),
            item("2026-09-15", 20_000, "청약저축52회 [농협]", "저축/주택청약"),
            item("2026-10-15", 20_000, "청약저축53회 [농협]", "저축/주택청약"),
            item("2026-09-30", 7_610, "씨엔씨티에너지 [농협]", "공과금"),
            item("2026-11-02", 6_520, "씨엔씨티에너지 [농협]", "공과금"),
            // A shop visited often: never fixed, even if one amount repeats monthly.
            item("2026-10-02", 3_600, "씨유(CU)대전대서문스타점 [토스]", "편의점"),
            item("2026-10-03", 2_400, "씨유(CU)대전대서문스타점 [토스]", "편의점"),
            item("2026-10-06", 1_900, "씨유(CU)대전대서문스타점 [토스]", "편의점"),
            item("2026-11-06", 3_200, "씨유(CU)대전대서문스타점 [토스]", "편의점"),
            // Same shop, monthly, but the day wanders: not suggested.
            item("2026-09-23", 130_430, "LG U+ [농협]", "통신"),
            item("2026-10-12", 133_080, "LG U+ [농협]", "통신"),
            // Two amounts under one name: the monthly 990 is found, the one-off 3,990 is not.
            item("2026-09-28", 990, "Google·Youtube Sponsorship [카카오페이]", "구독"),
            item("2026-10-02", 3_990, "Google·Youtube Sponsorship [카카오페이]", "구독"),
            item("2026-10-28", 990, "Google·Youtube Sponsorship [카카오페이]", "구독"),
        ])
    }

    #[test]
    fn keywords_drop_counters_and_account() {
        assert_eq!(keyword_of("청약저축53회 [농협]"), ("청약저축".into(), Some("농협".into())));
        assert_eq!(keyword_of("Apple Services·App Store & Apple Music: 10.11 구입 항목 [카카오페이]").0, "Apple Services·App Store & Apple Music");
        assert_eq!(keyword_of("부족한돈 자동환전 (USD) ANTHROPIC* CLA"), ("부족한돈 자동환전 (USD) ANTHROPIC* CLA".into(), None));
        assert_eq!(day_gap(30, 2), 3);
        assert_eq!(day_gap(15, 16), 1);
    }

    #[test]
    fn suggests_monthly_payments_only() {
        let s = suggest(&sample(), &[], &[]);
        let got: Vec<_> = s.iter().map(|s| (s.name.as_str(), s.amount / 100, s.day, s.months, s.account.as_deref())).collect();
        assert_eq!(got, [
            ("씨엔씨티에너지", 6_520, 2, 2, Some("농협")),
            ("DISCORD* NITROMONTHLY", 10_948, 15, 2, Some("토스")),
            ("청약저축", 20_000, 15, 2, Some("농협")),
            ("Google·Youtube Sponsorship", 990, 28, 2, Some("카카오페이")),
        ]);
        // Already on the list: not suggested again.
        let existing = [FixedExpense { name: "디스코드".into(), amount: 1_095_000, day: 15, keyword: "discord".into(), category: None, account: None }];
        assert!(suggest(&sample(), &existing, &[]).iter().all(|s| !s.keyword.contains("DISCORD")));
        // Hidden by the user.
        let hidden = suggest(&sample(), &[], &["청약저축".into(), "discord* nitromonthly".into()]);
        assert_eq!(hidden.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["씨엔씨티에너지", "Google·Youtube Sponsorship"]);
    }

    #[test]
    fn reminders_before_and_after_the_due_day() {
        let list = vec![
            FixedExpense { name: "청약".into(), amount: 2_000_000, day: 15, keyword: "청약저축".into(), category: None, account: None },
            FixedExpense { name: "가스".into(), amount: 700_000, day: 31, keyword: "씨엔씨티".into(), category: None, account: None },
            FixedExpense { name: "통신".into(), amount: 13_000_000, day: 1, keyword: "lg u+".into(), category: None, account: None },
        ];
        let items = sample();
        let at = |d: &str, before: u32| -> Vec<(String, String, i64)> {
            reminders(&list, &items, Date::parse(d).unwrap(), before).into_iter().map(|r| (r.name, r.due, r.days)).collect()
        };
        let r = |n: &str, d: &str, days: i64| (n.to_string(), d.to_string(), days);
        // 11/13: 청약 is due in 2 days and not paid in November.
        assert_eq!(at("2026-11-13", 3), [r("청약", "2026-11-15", 2)]);
        assert!(at("2026-11-13", 1).is_empty());
        assert_eq!(at("2026-11-14", 1), [r("청약", "2026-11-15", 1)]);
        // October's was paid on 10/15 (a record already dated the due day counts too).
        assert!(at("2026-10-14", 1).is_empty());
        assert!(at("2026-10-16", 0).is_empty());
        // Gas due 10/31, paid 11/2: reminded on 11/1 (a day late), not after the payment.
        assert_eq!(at("2026-11-01", 0), [r("가스", "2026-10-31", -1), r("통신", "2026-11-01", 0)]);
        assert!(at("2026-11-02", 0).iter().all(|x| x.0 != "가스"));
        // Due on the 1st, reminded from the end of the month before.
        assert!(at("2026-11-29", 3).contains(&r("통신", "2026-12-01", 2)));
        // Long past: no more reminders.
        assert!(at("2026-11-05", 0).iter().all(|x| x.0 != "통신"));
    }

    #[test]
    fn settings_are_kept_in_the_vault() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(load_remind_days(root), None);
        save_remind_days(root, Some(3)).unwrap();
        assert_eq!(load_remind_days(root), Some(3));
        assert!(save_remind_days(root, Some(40)).is_err());
        save_remind_days(root, None).unwrap();
        assert_eq!(load_remind_days(root), None);

        assert_eq!(ignore(root, Some(" 청약저축 ")).unwrap(), ["청약저축"]);
        assert_eq!(ignore(root, Some("DISCORD")).unwrap(), ["discord", "청약저축"]);
        assert_eq!(ignore(root, Some("discord")).unwrap().len(), 2);
        assert!(ignore(root, None).unwrap().is_empty());
        assert!(load_ignored(root).is_empty());
    }

    #[test]
    fn status_finds_this_months_payment() {
        let list = vec![
            FixedExpense { name: "청약".into(), amount: 2_000_000, day: 15, keyword: "청약저축".into(), category: None, account: Some("농협".into()) },
            FixedExpense { name: "가스".into(), amount: 700_000, day: 31, keyword: "씨엔씨티".into(), category: None, account: None },
            FixedExpense { name: "통신".into(), amount: 13_000_000, day: 23, keyword: "lg u+".into(), category: None, account: None },
        ];
        let items = sample();
        let s = status(&list, &items, "2026-10");
        assert_eq!(s[0].paid.as_ref().map(|p| (p.date.as_str(), p.amount)), Some(("2026-10-15", 2_000_000)));
        assert_eq!(s[1].due, "2026-10-31");
        assert!(s[1].paid.is_none()); // October's gas went out on 11/2
        assert_eq!(s[2].paid.as_ref().map(|p| p.date.as_str()), Some("2026-10-12")); // case-insensitive
        // February: day 31 is due on the last day.
        assert_eq!(status(&list, &items, "2027-02")[1].due, "2027-02-28");
        // Settlements and refunds never count as the payment.
        let mut refund = item("2026-11-15", -20_000, "청약저축 취소", "저축");
        refund.kind = EntryKind::Expense;
        assert!(status(&list, &[refund], "2026-11")[0].paid.is_none());
    }
}
