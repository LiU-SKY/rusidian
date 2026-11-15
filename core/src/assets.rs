//! Account balances, written as list items in any note:
//!
//! ```text
//! - 🏦 2,700,000 NH청년도약계좌 #저축 📅 2026-11-14
//! - 🏦 5,300 카카오페이머니 #페이 📅 2026-11-14
//! ```
//!
//! The memo is the account's name, the first tag its group. An account's
//! latest record (by date) is its current balance; earlier ones are its
//! history. Kept in notes, not in `.obsidian`, so sync only carries them in
//! folders the user chose.
//!
//! Also: which money-record categories count as savings (moved into
//! savings, not spent), kept in `.obsidian/rusidian.json`.

use crate::date::Date;
use crate::finance::{format_amount, parse_amount};
use crate::markdown::prose_lines;
use regex::Regex;
use serde::Serialize;
use std::path::Path;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq)]
pub struct Balance {
    pub line: usize,
    pub raw: String,
    /// × 100, like money records; may be 0 or negative (a card's debt).
    pub amount: i64,
    pub account: String,
    pub group: Option<String>,
    /// `📅` on the line.
    pub date: Option<String>,
}

/// A balance record with its note and final date (the note's date if the line has none).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BalanceItem {
    pub path: String,
    pub title: String,
    pub line: usize,
    pub raw: String,
    pub amount: i64,
    pub account: String,
    pub group: Option<String>,
    pub date: Option<String>,
}

static LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[ \t]*(?:>[ \t]*)*(?:[-*+][ \t]+)?🏦\x{FE0F}?[ \t]*([-−]?)[ \t]*₩?[ \t]*(\d[\d,]*(?:\.\d{1,2})?)[ \t]*(?:원)?(?:[ \t]+(.*))?$")
        .unwrap()
});
static DATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:📅|📆|🗓)\x{FE0F}?\s*(\d{4}-\d{2}-\d{2})").unwrap());
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|\s)#([\p{L}\p{N}_/-]+)").unwrap());

/// "0", "0.00" (a zero balance is a real record, unlike a zero payment) or a money amount.
fn balance_amount(digits: &str) -> Option<i64> {
    if digits.chars().any(|c| c.is_ascii_digit()) && digits.chars().all(|c| matches!(c, '0' | ',' | '.')) {
        return Some(0);
    }
    parse_amount(digits)
}

pub fn parse_balance_line(line_no: usize, line: &str) -> Option<Balance> {
    let caps = LINE.captures(line)?;
    let amount = balance_amount(&caps[2])?;
    let amount = if caps[1].is_empty() { amount } else { -amount };
    let rest = caps.get(3).map(|m| m.as_str()).unwrap_or("");
    let date = DATE.captures(rest).map(|c| c[1].to_string()).filter(|d| Date::parse(d).is_some());
    let group = TAG
        .captures_iter(rest)
        .map(|c| c[1].to_string())
        .find(|t| !t.chars().all(|c| c.is_ascii_digit()));
    let account = DATE.replace_all(rest, " ");
    let account = TAG.replace_all(&account, " ");
    let account = account.split_whitespace().collect::<Vec<_>>().join(" ");
    if account.is_empty() {
        return None; // a balance belongs to an account
    }
    Some(Balance { line: line_no, raw: line.to_string(), amount, account, group, date })
}

pub fn parse_balances(text: &str) -> Vec<Balance> {
    prose_lines(text).filter_map(|(n, l)| parse_balance_line(n, l)).collect()
}

/// `- 🏦 2,700,000 NH청년도약계좌 #저축 📅 2026-11-14` from the form.
pub fn balance_line(amount: &str, account: &str, group: &str, date: Date) -> Result<String, String> {
    let text = amount.trim().trim_end_matches('원').trim();
    let (negative, digits) = match text.strip_prefix('-').or_else(|| text.strip_prefix('−')) {
        Some(rest) => (true, rest.trim().trim_start_matches('₩').trim()),
        None => (false, text.trim_start_matches('₩').trim()),
    };
    let cents = balance_amount(digits).ok_or("잔액을 확인하세요. (예: 2,700,000, 0, 빚은 -50,000)")?;
    let cents = if negative { -cents } else { cents };
    let account = account.replace(['#', '📅'], " ").split_whitespace().collect::<Vec<_>>().join(" ");
    if account.is_empty() {
        return Err("계좌 이름을 적으세요.".into());
    }
    let group: String = group.trim().trim_start_matches('#').split_whitespace().collect::<Vec<_>>().join("-");
    let mut line = format!("- 🏦 {} {account}", format_amount(cents));
    if !group.is_empty() {
        line.push_str(&format!(" #{group}"));
    }
    line.push_str(&format!(" 📅 {date}"));
    Ok(line)
}

// ------------------------------------------------------------------ savings

const SAVINGS_FILE: &str = "rusidian.json";
const SAVINGS_KEY: &str = "savings_categories";
pub const DEFAULT_SAVINGS: [&str; 2] = ["저축", "투자"];

/// Categories whose spending records are savings (sub-categories included).
pub fn load_savings(root: &Path) -> Vec<String> {
    let saved: Option<Vec<String>> = crate::obsidian_config::read_key(root, SAVINGS_FILE, SAVINGS_KEY);
    saved.unwrap_or_else(|| DEFAULT_SAVINGS.iter().map(|s| s.to_string()).collect())
}

pub fn save_savings(root: &Path, list: &[String]) -> Result<Vec<String>, String> {
    let mut clean: Vec<String> = Vec::new();
    for c in list {
        let c = c.trim().trim_start_matches('#').trim_end_matches('/').to_string();
        if !c.is_empty() && !clean.contains(&c) {
            clean.push(c);
        }
    }
    crate::obsidian_config::write_key(root, SAVINGS_FILE, SAVINGS_KEY, &clean)?;
    Ok(clean)
}

/// `저축` covers `저축/주택청약`.
pub fn is_saving(category: &str, savings: &[String]) -> bool {
    savings.iter().any(|s| category == s || category.strip_prefix(s.as_str()).is_some_and(|rest| rest.starts_with('/')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_balance_lines() {
        let text = "# 자산\n- 🏦 2,700,000 NH청년도약계좌 #저축 📅 2026-11-14\n- 🏦 0 농협 입출금 #입출금\n- 🏦 -50,000원 카드 대금 #카드\n- 🏦 1,000\n```\n- 🏦 9 코드 안\n```\n- 💸 5,000 커피\n";
        let got: Vec<_> = parse_balances(text).iter().map(|b| (b.line, b.amount / 100, b.account.clone(), b.group.clone(), b.date.clone())).collect();
        assert_eq!(got, [
            (1, 2_700_000, "NH청년도약계좌".to_string(), Some("저축".to_string()), Some("2026-11-14".to_string())),
            (2, 0, "농협 입출금".to_string(), Some("입출금".to_string()), None),
            (3, -50_000, "카드 대금".to_string(), Some("카드".to_string()), None),
        ]); // no account name, code blocks and money records are not balances
    }

    #[test]
    fn writes_balance_lines() {
        let d = Date::parse("2026-11-14").unwrap();
        assert_eq!(balance_line("2700000", " NH청년도약계좌 ", "#저축", d).unwrap(), "- 🏦 2,700,000 NH청년도약계좌 #저축 📅 2026-11-14");
        assert_eq!(balance_line("0원", "농협", "", d).unwrap(), "- 🏦 0 농협 📅 2026-11-14");
        assert_eq!(balance_line("-₩50,000", "카드 #대금", "카드 값", d).unwrap(), "- 🏦 -50,000 카드 대금 #카드-값 📅 2026-11-14");
        assert!(balance_line("abc", "농협", "", d).is_err());
        assert!(balance_line("100", "  ", "", d).is_err());
        let back = parse_balance_line(0, &balance_line("1,234.5", "외화", "외화", d).unwrap()).unwrap();
        assert_eq!((back.amount, back.account.as_str()), (123_450, "외화"));
    }

    #[test]
    fn savings_categories() {
        let s = vec!["저축".to_string(), "투자".to_string()];
        assert!(is_saving("저축", &s));
        assert!(is_saving("저축/주택청약", &s));
        assert!(!is_saving("저축은행이자", &s));
        assert!(!is_saving("식비", &s));
        let dir = std::env::temp_dir().join(format!("rusidian-savings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(load_savings(&dir), s);
        assert_eq!(save_savings(&dir, &[" #저축/ ".into(), "".into(), "저축".into(), "연금".into()]).unwrap(), ["저축", "연금"]);
        assert_eq!(load_savings(&dir), ["저축", "연금"]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
