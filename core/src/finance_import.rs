//! Importing bank and card statements (CSV) as money records.
//!
//! Korean banks and card companies mostly export CP949 (EUC-KR) CSV with a
//! few lines of account information above the table, and either separate
//! withdrawal / deposit columns (banks) or one amount column (cards). The
//! header row and the columns are guessed; the UI can correct them. Rows
//! that already exist in the vault (same date, kind, amount and memo) are
//! skipped, so importing the same file twice adds nothing.

use crate::date::Date;
use crate::finance::{EntryKind, FinanceItem, entry_line};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Rows read from one file at most.
pub const MAX_ROWS: usize = 10_000;

// ---------------------------------------------------------------- reading

/// Text of a CSV file: UTF-8 (with or without BOM), else CP949 / EUC-KR.
/// Returns the text and the encoding's name.
pub fn decode(bytes: &[u8]) -> (String, &'static str) {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    if let Ok(s) = std::str::from_utf8(bytes) {
        return (s.to_string(), "UTF-8");
    }
    if bytes.starts_with(b"\xFF\xFE") || bytes.starts_with(b"\xFE\xFF") {
        let enc = if bytes[0] == 0xFF { encoding_rs::UTF_16LE } else { encoding_rs::UTF_16BE };
        return (enc.decode_with_bom_removal(bytes).0.into_owned(), "UTF-16");
    }
    (encoding_rs::EUC_KR.decode(bytes).0.into_owned(), "CP949")
}

/// The delimiter used most consistently over the first lines: `,`, tab or `;`.
fn guess_delimiter(text: &str) -> char {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).take(30).collect();
    [',', '\t', ';']
        .into_iter()
        .max_by_key(|&d| {
            let counts: Vec<usize> = lines.iter().map(|l| l.matches(d).count()).collect();
            // Lines sharing the most common count, weighted by that count.
            let mut freq: HashMap<usize, usize> = HashMap::new();
            for c in counts.iter().filter(|c| **c > 0) {
                *freq.entry(*c).or_default() += 1;
            }
            freq.into_iter().map(|(c, n)| n * 10 + c).max().unwrap_or(0)
        })
        .unwrap_or(',')
}

/// RFC 4180 CSV: quoted fields, `""` escapes, newlines inside quotes.
pub fn parse_csv(text: &str, delimiter: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.trim().is_empty() => {
                field.clear();
                quoted = true;
            }
            c if c == delimiter => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                if rows.len() >= MAX_ROWS {
                    return rows;
                }
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows.into_iter()
        .map(|r| r.into_iter().map(|f| f.trim().to_string()).collect::<Vec<_>>())
        .filter(|r: &Vec<String>| r.iter().any(|f| !f.is_empty()))
        .collect()
}

// --------------------------------------------------------------- guessing

/// Which column holds what (0-based), as guessed or set in the dialog.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Mapping {
    pub date: Option<usize>,
    pub memo: Option<usize>,
    /// Money out (bank "출금").
    pub withdrawal: Option<usize>,
    /// Money in (bank "입금").
    pub deposit: Option<usize>,
    /// One amount column (cards, or banks with signed amounts).
    pub amount: Option<usize>,
    /// For `amount`: positive numbers are spending (card statements);
    /// otherwise negative numbers are spending.
    #[serde(default)]
    pub positive_is_expense: bool,
    pub category: Option<usize>,
    /// "입금" / "출금" next to an unsigned `amount` (e.g. Kakao Pay's 입출구분).
    #[serde(default)]
    pub direction: Option<usize>,
    /// A status column; rows saying "취소" (cancelled) are skipped.
    #[serde(default)]
    pub status: Option<usize>,
}

const DATE_WORDS: &[&str] = &["거래일시", "거래일자", "거래일", "이용일시", "이용일자", "이용일", "승인일시", "승인일자", "승인일", "일자", "날짜", "date"];
const MEMO_WORDS: &[&str] = &["가맹점명", "가맹점", "이용하신곳", "이용처", "사용처", "적요", "거래적요", "거래내용", "내용", "기재내용", "받는분", "보낸분", "메모", "description", "memo"];
const OUT_WORDS: &[&str] = &["출금액", "출금", "찾으신금액", "지출", "withdrawal", "debit"];
const IN_WORDS: &[&str] = &["입금액", "입금", "맡기신금액", "수입", "deposit", "credit"];
const CARD_AMOUNT_WORDS: &[&str] = &["이용금액", "결제금액", "승인금액", "결제예정금액"];
const AMOUNT_WORDS: &[&str] = &["거래금액", "금액", "amount"];
const CATEGORY_WORDS: &[&str] = &["업종", "분류", "카테고리", "category"];
const DIRECTION_WORDS: &[&str] = &["입출구분", "입출금구분", "입출금", "거래구분"];
const STATUS_WORDS: &[&str] = &["상태", "취소여부", "승인상태", "거래상태"];

fn norm(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace() && !"()[]_".contains(*c)).collect::<String>().to_lowercase()
}

/// The first free column whose header starts with one of `words`, trying
/// the words in order (more specific words come first).
fn find(header: &[String], words: &[&str], taken: &[Option<usize>]) -> Option<usize> {
    let cells: Vec<String> = header.iter().map(|h| norm(h)).collect();
    words.iter().find_map(|w| {
        cells
            .iter()
            .enumerate()
            .position(|(i, c)| !taken.contains(&Some(i)) && c.starts_with(w))
    })
}

fn is_header_like(row: &[String]) -> usize {
    let all: Vec<&[&str]> = vec![DATE_WORDS, MEMO_WORDS, OUT_WORDS, IN_WORDS, CARD_AMOUNT_WORDS, AMOUNT_WORDS];
    row.iter()
        .map(|c| norm(c))
        .filter(|c| all.iter().any(|ws| ws.iter().any(|w| c.starts_with(w))))
        .count()
}

/// The header row: the first row that names at least two known columns
/// (else the first row).
pub fn guess_header(rows: &[Vec<String>]) -> usize {
    rows.iter().take(30).position(|r| is_header_like(r) >= 2).unwrap_or(0)
}

pub fn guess_mapping(header: &[String]) -> Mapping {
    let mut m = Mapping::default();
    m.date = find(header, DATE_WORDS, &[]);
    m.withdrawal = find(header, OUT_WORDS, &[m.date]);
    m.deposit = find(header, IN_WORDS, &[m.date, m.withdrawal]);
    if m.withdrawal.is_none() || m.deposit.is_none() {
        if let Some(i) = find(header, CARD_AMOUNT_WORDS, &[m.date]) {
            m.amount = Some(i);
            m.positive_is_expense = true;
        } else if let Some(i) = find(header, AMOUNT_WORDS, &[m.date]) {
            m.amount = Some(i);
        }
        if m.amount.is_some() {
            m.withdrawal = None;
            m.deposit = None;
        }
    }
    if m.amount.is_some() {
        m.direction = find(header, DIRECTION_WORDS, &[m.date, m.amount]);
    }
    let taken = [m.date, m.withdrawal, m.deposit, m.amount, m.direction];
    m.memo = find(header, MEMO_WORDS, &taken);
    m.category = find(header, CATEGORY_WORDS, &[m.date, m.withdrawal, m.deposit, m.amount, m.memo, m.direction]);
    m.status = find(header, STATUS_WORDS, &[m.date, m.withdrawal, m.deposit, m.amount, m.memo, m.direction, m.category]);
    m
}

/// What the dialog first shows.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Preview {
    pub encoding: String,
    pub rows: Vec<Vec<String>>,
    pub header: usize,
    pub mapping: Mapping,
}

/// A CSV file's rows and the encoding it was read in.
pub fn csv_rows(bytes: &[u8]) -> (Vec<Vec<String>>, &'static str) {
    let (text, encoding) = decode(bytes);
    (parse_csv(&text, guess_delimiter(&text)), encoding)
}

pub fn preview(bytes: &[u8]) -> Preview {
    let (rows, encoding) = csv_rows(bytes);
    preview_rows(rows, encoding)
}

/// The first guess for rows read from any statement file (`format` is shown).
pub fn preview_rows(rows: Vec<Vec<String>>, format: &str) -> Preview {
    let header = guess_header(&rows);
    let mapping = rows.get(header).map(|h| guess_mapping(h)).unwrap_or_default();
    Preview { encoding: format.to_string(), rows, header, mapping }
}

// --------------------------------------------------------------- planning

/// `2026-11-13`, `2026.11.13`, `2026/11/13`, `20261113`, `26.11.13`, with
/// an optional time after it.
pub fn parse_date(s: &str) -> Option<Date> {
    let s = s.trim();
    let digits_only: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits_only.len() >= 8 {
        let d = &digits_only[..8];
        return Date::new(d[..4].parse().ok()?, d[4..6].parse().ok()?, d[6..8].parse().ok()?);
    }
    let head = s.split([' ', 'T']).next()?;
    let parts: Vec<&str> = head.split(['-', '.', '/']).map(str::trim).filter(|p| !p.is_empty()).collect();
    let [y, m, d] = parts.as_slice() else { return None };
    let year: i32 = y.parse().ok()?;
    let year = if y.len() == 2 { 2000 + year } else { year };
    Date::new(year, m.parse().ok()?, d.parse().ok()?)
}

/// `12,000`, `-12,000원`, `(12,000)`, `₩12000.50` → hundredths (signed).
pub fn parse_money(s: &str) -> Option<i64> {
    let t: String = s.chars().filter(|c| !matches!(c, ',' | ' ' | '원' | '₩' | '\u{a0}')).collect();
    if t.is_empty() {
        return None;
    }
    let (neg, t) = if let Some(inner) = t.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        (true, inner.to_string())
    } else if let Some(rest) = t.strip_prefix('-') {
        (true, rest.to_string())
    } else {
        (false, t.strip_prefix('+').unwrap_or(&t).to_string())
    };
    let (whole, frac) = t.split_once('.').unwrap_or((&t, ""));
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let frac: i64 = format!("{:0<2}", &frac[..frac.len().min(2)]).parse().ok()?;
    let v = whole.parse::<i64>().ok()?.checked_mul(100)?.checked_add(frac)?;
    Some(if neg { -v } else { v })
}

/// Memo text safe on a record line (no `#` tags or `📅` taking it over).
fn clean_memo(s: &str) -> String {
    s.replace(['#', '\n', '\r'], " ").replace('📅', "").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A category as a tag body: letters, digits, `_`, `-`, `/`.
pub fn clean_category(s: &str) -> String {
    s.trim()
        .trim_start_matches('#')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '/'))
        .collect()
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RowStatus {
    /// Will be imported.
    Ok,
    /// Already in the vault.
    Duplicate,
    /// Can't be read (no date / amount) — e.g. a total line.
    Skipped,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlannedRow {
    /// Index into the file's rows.
    pub row: usize,
    pub status: RowStatus,
    /// Why it was skipped.
    pub reason: Option<String>,
    pub date: Option<String>,
    pub kind: Option<EntryKind>,
    /// Hundredths, positive.
    pub amount: Option<i64>,
    pub memo: String,
    pub category: Option<String>,
}

fn key(date: &str, kind: EntryKind, amount: i64, memo: &str) -> String {
    format!("{date}|{kind:?}|{amount}|{}", memo.to_lowercase())
}

/// Each data row (after `header`) as it would be imported. `existing` are
/// the vault's dated records, for the duplicate check (counted, so two
/// identical coffees on one day still import if the vault has only one).
pub fn plan(rows: &[Vec<String>], header: usize, m: &Mapping, default_category: &str, existing: &[FinanceItem]) -> Vec<PlannedRow> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    // Records written from phone notifications name the shop their own way,
    // so they match on day and money alone: (date, money in − out) → count.
    let mut auto: HashMap<(String, i64), usize> = HashMap::new();
    let auto_tag = format!("#{}", crate::phone::AUTO_TAG);
    let flow = |kind: EntryKind, amount: i64| if kind == EntryKind::Income { amount } else { -amount };
    for e in existing {
        if let Some(d) = &e.date {
            if e.raw.split_whitespace().any(|w| w == auto_tag) {
                *auto.entry((d.clone(), flow(e.kind, e.amount))).or_default() += 1;
            } else {
                *seen.entry(key(d, e.kind, e.amount, &clean_memo(&e.memo))).or_default() += 1;
            }
        }
    }
    let default_category = clean_category(default_category);
    let cell = |r: &Vec<String>, i: Option<usize>| i.and_then(|i| r.get(i)).map(|s| s.trim().to_string()).unwrap_or_default();

    rows.iter()
        .enumerate()
        .skip(header + 1)
        .map(|(i, r)| {
            let skip = |reason: &str| PlannedRow {
                row: i,
                status: RowStatus::Skipped,
                reason: Some(reason.to_string()),
                date: None,
                kind: None,
                amount: None,
                memo: clean_memo(&cell(r, m.memo)),
                category: None,
            };
            let Some(date) = parse_date(&cell(r, m.date)) else { return skip("날짜를 읽을 수 없음") };
            if cell(r, m.status).contains("취소") {
                return skip("취소된 거래");
            }
            let direction = cell(r, m.direction);
            let signed = if m.amount.is_some() && !direction.is_empty() {
                // Unsigned amount with 입금/출금 beside it.
                let v = parse_money(&cell(r, m.amount)).map(i64::abs);
                if direction.contains('출') || direction.contains("지급") {
                    v.map(|v| -v)
                } else if direction.contains('입') {
                    v
                } else {
                    parse_money(&cell(r, m.amount))
                }
            } else if m.amount.is_some() {
                parse_money(&cell(r, m.amount)).map(|v| if m.positive_is_expense { -v } else { v })
            } else {
                let out = parse_money(&cell(r, m.withdrawal)).unwrap_or(0).abs();
                let inn = parse_money(&cell(r, m.deposit)).unwrap_or(0).abs();
                match (out, inn) {
                    (0, 0) => None,
                    (o, 0) => Some(-o),
                    (0, n) => Some(n),
                    (o, n) => Some(n - o),
                }
            };
            let Some(signed) = signed.filter(|v| *v != 0) else { return skip("금액이 없음") };
            let kind = if signed < 0 { EntryKind::Expense } else { EntryKind::Income };
            let amount = signed.abs();
            let memo = clean_memo(&cell(r, m.memo));
            let category = Some(clean_category(&cell(r, m.category)))
                .filter(|c| !c.is_empty())
                .or_else(|| (!default_category.is_empty()).then(|| default_category.clone()));
            let k = key(&date.to_string(), kind, amount, &memo);
            let status = match seen.get_mut(&k) {
                Some(n) if *n > 0 => {
                    *n -= 1;
                    RowStatus::Duplicate
                }
                _ => match auto.get_mut(&(date.to_string(), signed)) {
                    Some(n) if *n > 0 => {
                        *n -= 1;
                        RowStatus::Duplicate
                    }
                    _ => RowStatus::Ok,
                },
            };
            PlannedRow {
                row: i,
                status,
                reason: None,
                date: Some(date.to_string()),
                kind: Some(kind),
                amount: Some(amount),
                memo,
                category,
            }
        })
        .collect()
}

/// Record lines for the importable rows, oldest first.
pub fn lines(planned: &[PlannedRow]) -> Vec<String> {
    let mut ok: Vec<&PlannedRow> = planned.iter().filter(|p| p.status == RowStatus::Ok).collect();
    ok.sort_by(|a, b| a.date.cmp(&b.date).then(a.row.cmp(&b.row)));
    ok.into_iter()
        .filter_map(|p| {
            let date = Date::parse(p.date.as_deref()?)?;
            let amount = crate::finance::format_amount(p.amount?);
            entry_line(p.kind?, &amount, &p.memo, p.category.as_deref().unwrap_or(""), Some(date)).ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finance::parse_line;

    fn rows(csv: &str) -> Vec<Vec<String>> {
        parse_csv(csv, guess_delimiter(csv))
    }

    #[test]
    fn decodes_cp949_and_utf8() {
        let (cp949, _, _) = encoding_rs::EUC_KR.encode("거래일자,적요\n2026.11.13,커피\n");
        let (text, enc) = decode(&cp949);
        assert_eq!(enc, "CP949");
        assert!(text.starts_with("거래일자,적요"));
        let (text, enc) = decode("\u{feff}날짜,금액\n".as_bytes());
        assert_eq!((text.as_str(), enc), ("날짜,금액\n", "UTF-8"));
    }

    #[test]
    fn parses_quoted_csv_and_delimiters() {
        let r = rows("a,\"b, c\",\"say \"\"hi\"\"\"\r\n1,\"multi\nline\",3\r\n\r\n");
        assert_eq!(r, [vec!["a", "b, c", "say \"hi\""], vec!["1", "multi\nline", "3"]]);
        assert_eq!(guess_delimiter("a\tb\tc\n1\t2\t3\n"), '\t');
        assert_eq!(guess_delimiter("a;b;c\n1;2,5;3\n"), ';');
    }

    #[test]
    fn bank_statement_with_preamble() {
        let csv = "계좌번호,123-45-678\n조회기간,2026.11.01~2026.11.30\n\n\
                   거래일시,적요,출금액,입금액,잔액,거래점\n\
                   2026.11.03 12:10:22,스타벅스,\"5,600\",0,\"994,400\",인터넷\n\
                   2026.11.10 09:00:00,월급,0,\"3,000,000\",\"3,994,400\",\n\
                   합계,,\"5,600\",\"3,000,000\",,\n";
        let r = rows(csv);
        let header = guess_header(&r);
        assert_eq!(header, 2);
        let m = guess_mapping(&r[header]);
        assert_eq!((m.date, m.memo, m.withdrawal, m.deposit, m.amount), (Some(0), Some(1), Some(2), Some(3), None));

        let p = plan(&r, header, &m, "", &[]);
        assert_eq!(p.len(), 3);
        assert_eq!((p[0].date.as_deref(), p[0].kind, p[0].amount, p[0].memo.as_str()), (Some("2026-11-03"), Some(EntryKind::Expense), Some(560_000), "스타벅스"));
        assert_eq!((p[1].kind, p[1].amount), (Some(EntryKind::Income), Some(300_000_000)));
        assert_eq!(p[2].status, RowStatus::Skipped); // the total line
        assert_eq!(
            lines(&p),
            ["- 💸 5,600 스타벅스 📅 2026-11-03", "- 💰 3,000,000 월급 📅 2026-11-10"]
        );
    }

    #[test]
    fn card_statement_and_categories() {
        let csv = "이용일자,이용하신 곳,이용금액,업종\n26.11.05,배달의 민족 #야식,\"23,000\",음식점 일반\n26.11.06,환불,\"-5,000\",\n";
        let r = rows(csv);
        let m = guess_mapping(&r[guess_header(&r)]);
        assert_eq!((m.amount, m.positive_is_expense, m.category), (Some(2), true, Some(3)));
        let p = plan(&r, 0, &m, "카드", &[]);
        assert_eq!((p[0].kind, p[0].memo.as_str(), p[0].category.as_deref()), (Some(EntryKind::Expense), "배달의 민족 야식", Some("음식점-일반")));
        assert_eq!((p[1].kind, p[1].category.as_deref()), (Some(EntryKind::Income), Some("카드")));
        // The written line reads back as the same record.
        let back = parse_line(0, &lines(&p)[0]).unwrap();
        assert_eq!((back.amount, back.memo.as_str(), back.category.as_deref()), (2_300_000, "배달의 민족 야식", Some("음식점-일반")));
    }

    #[test]
    fn direction_column_and_cancelled_rows() {
        // Kakao Pay money statement: unsigned amount + 입출구분; a status column marks cancellations.
        let csv = "거래일자,거래시간,거래(적요)명,입출구분,거래금액,상태\n2026-11-13,16:58:43,PAY일반출금,출금,10000,\n2026-11-13,16:58:43,PAY충전입금,입금,10000,\n2026-11-14,09:00:00,무신사,출금,23000,취소\n";
        let r = rows(csv);
        let m = guess_mapping(&r[guess_header(&r)]);
        assert_eq!((m.date, m.memo, m.direction, m.amount, m.status), (Some(0), Some(2), Some(3), Some(4), Some(5)));
        let p = plan(&r, 0, &m, "", &[]);
        assert_eq!((p[0].kind, p[0].amount), (Some(EntryKind::Expense), Some(1_000_000)));
        assert_eq!((p[1].kind, p[1].amount), (Some(EntryKind::Income), Some(1_000_000)));
        assert_eq!((p[2].status, p[2].reason.as_deref()), (RowStatus::Skipped, Some("취소된 거래")));
        // Signed amounts and no direction column (Toss): unchanged.
        let toss = rows("거래 일시,적요,거래 유형,거래 금액\n2026.11.13 16:58:42,카카오페이,오픈뱅킹,-10000\n");
        let mt = guess_mapping(&toss[0]);
        assert_eq!((mt.amount, mt.direction), (Some(3), None));
        assert_eq!(plan(&toss, 0, &mt, "", &[])[0].kind, Some(EntryKind::Expense));
    }

    #[test]
    fn duplicates_are_counted() {
        let csv = "날짜,내용,금액\n2026-11-13,커피,-4500\n2026-11-13,커피,-4500\n2026-11-14,커피,-4500\n";
        let r = rows(csv);
        let m = guess_mapping(&r[0]);
        let existing = [FinanceItem {
            path: "x.md".into(),
            title: "x".into(),
            line: 0,
            raw: String::new(),
            kind: EntryKind::Expense,
            amount: 450_000,
            memo: "커피".into(),
            category: None,
            date: Some("2026-11-13".into()),
            explicit_date: true,
            parent_line: None,
        }];
        let status: Vec<RowStatus> = plan(&r, 0, &m, "", &existing).iter().map(|p| p.status).collect();
        assert_eq!(status, [RowStatus::Duplicate, RowStatus::Ok, RowStatus::Ok]);

        // A record from a phone notification matches whatever its memo says,
        // and a friend's share written as money back matches the deposit.
        let csv = "날짜,내용,금액\n2026-11-14,스타벅스 대전점,-4500\n2026-11-14,박서준,10000\n2026-11-14,박서준,10000\n";
        let r = rows(csv);
        let auto = |kind, amount, memo: &str| FinanceItem {
            raw: format!("- 💸 x {memo} #기타 #자동"),
            kind,
            amount,
            memo: memo.into(),
            date: Some("2026-11-14".into()),
            ..existing[0].clone()
        };
        let existing = [
            auto(EntryKind::Expense, 450_000, "스타벅스 [토스]"),
            auto(EntryKind::Expense, -1_000_000, "박서준에게서 돌려받음 [토스]"),
        ];
        let status: Vec<RowStatus> = plan(&r, 0, &m, "", &existing).iter().map(|p| p.status).collect();
        assert_eq!(status, [RowStatus::Duplicate, RowStatus::Duplicate, RowStatus::Ok]);
    }

    #[test]
    fn dates_and_money() {
        for s in ["2026-11-13", "2026.11.13", "2026/11/13", "20261113", "26.11.13", "2026-11-13 14:22", "2026.11.13 14:22:10"] {
            assert_eq!(parse_date(s), Date::parse("2026-11-13"), "{s}");
        }
        assert_eq!(parse_date("합계"), None);
        assert_eq!(parse_money("12,000"), Some(1_200_000));
        assert_eq!(parse_money("-12,000원"), Some(-1_200_000));
        assert_eq!(parse_money("(3,000)"), Some(-300_000));
        assert_eq!(parse_money("₩9.5"), Some(950));
        assert_eq!(parse_money(""), None);
        assert_eq!(parse_money("abc"), None);
    }
}
