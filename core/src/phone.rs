//! Bank and payment notifications forwarded from a phone.
//!
//! An automation app on the phone (MacroDroid on Android) appends each
//! notification of the bank and pay apps to one text file, and a sync app
//! (Syncthing) brings that file to this computer. A record starts with a
//! header line and its text follows up to the next header:
//!
//! ```text
//! @@ 2026-11-14 13:22 | 토스 | 토스뱅크
//! 맘스터치 대전대점에서 8,300원 결제
//! ```
//!
//! (date and time, the app, the notification's title). The file is only
//! read, never written: records already handled are remembered by key on
//! this computer, so the phone may empty the file at any time.

use crate::date::Date;
use crate::finance::{self, EntryKind, FinanceItem};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::LazyLock;

/// Tag on records written from a notification (kept when the record is edited).
pub const AUTO_TAG: &str = "자동";

/// Marks the start of a record in the inbox file.
pub const HEADER: &str = "@@";

#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// Stable id of the record (its header and text), to handle it once.
    pub key: String,
    pub date: Option<Date>,
    /// `HH:MM`, if the header has it.
    pub time: Option<String>,
    pub app: String,
    pub title: String,
    pub text: String,
}

static HEADER_DATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d{4})\s*[-./년]\s*(\d{1,2})\s*[-./월]\s*(\d{1,2})").unwrap());
static HEADER_TIME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(오전|오후|AM|PM|am|pm)?\s*(\d{1,2}):(\d{2})").unwrap());

/// The records of an inbox file, in file order. Lines before the first
/// header are ignored.
pub fn records(file: &str) -> Vec<Record> {
    let file = file.trim_start_matches('\u{feff}');
    let mut out = Vec::new();
    let mut current: Option<(String, Vec<&str>)> = None;
    let finish = |out: &mut Vec<Record>, (header, body): (String, Vec<&str>)| {
        out.push(record(&header, &body.join("\n")));
    };
    for line in file.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix(HEADER) {
            if let Some(done) = current.take() {
                finish(&mut out, done);
            }
            current = Some((rest.trim().to_string(), Vec::new()));
        } else if let Some((_, body)) = current.as_mut() {
            body.push(line);
        }
    }
    if let Some(done) = current {
        finish(&mut out, done);
    }
    out
}

fn record(header: &str, body: &str) -> Record {
    let mut parts = header.split('|').map(str::trim);
    let when = parts.next().unwrap_or("");
    let app = parts.next().unwrap_or("").to_string();
    let title = parts.collect::<Vec<_>>().join(" | ");
    let date = HEADER_DATE.captures(when).and_then(|c| {
        Date::new(c[1].parse().ok()?, c[2].parse().ok()?, c[3].parse().ok()?)
    });
    // The time is looked for after the date, so "2026-11-14" isn't read as one.
    let after_date = HEADER_DATE.find(when).map(|m| &when[m.end()..]).unwrap_or(when);
    let time = HEADER_TIME.captures(after_date).and_then(|c| {
        let mut h: u32 = c[2].parse().ok()?;
        let m: u32 = c[3].parse().ok()?;
        match c.get(1).map(|m| m.as_str()) {
            Some("오후" | "PM" | "pm") if h < 12 => h += 12,
            Some("오전" | "AM" | "am") if h == 12 => h = 0,
            _ => {}
        }
        (h < 24 && m < 60).then(|| format!("{h:02}:{m:02}"))
    });
    let text = body.trim().to_string();
    let key = format!("{:016x}", fnv1a(format!("{header}\n{text}").as_bytes()));
    Record { key, date, time, app, title, text }
}

/// FNV-1a: a hash that stays the same across Rust versions (keys are saved).
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// How notifications are turned into records (set per vault on this computer).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rules {
    /// Names that mean one of my own accounts (my name, "카카오페이"):
    /// money moved between them is left out.
    pub own: Vec<String>,
    /// An ATM withdrawal is cash for lab costs: written as money taken off 인건비.
    pub atm_lab_cost: bool,
    /// Money a person sends below this many won is their share of a meal
    /// I paid: written as money back under 식비/정산 (0 = off).
    pub settle_below: i64,
}

impl Default for Rules {
    fn default() -> Self {
        Rules { own: vec!["카카오페이".into()], atm_lab_cost: false, settle_below: 100_000 }
    }
}

/// A notification read as a money record.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Understood {
    pub kind: EntryKind,
    /// × 100, negative for money back (a refund, a friend's share).
    pub amount: i64,
    pub memo: String,
    pub category: String,
    pub date: String,
}

impl Understood {
    /// The record line, tagged as written from a notification.
    pub fn line(&self) -> String {
        let date = Date::parse(&self.date);
        let line = finance::entry_line(self.kind, &finance::format_amount(self.amount), &self.memo, &self.category, date)
            .unwrap_or_default();
        match line.find(" 📅") {
            Some(at) => format!("{} #{AUTO_TAG}{}", &line[..at], &line[at..]),
            None => format!("{line} #{AUTO_TAG}"),
        }
    }

    /// Money in (+) or out (−) of my accounts, in cents.
    fn flow(&self) -> i64 {
        flow(self.kind, self.amount)
    }
}

fn flow(kind: EntryKind, amount: i64) -> i64 {
    if kind == EntryKind::Income { amount } else { -amount }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Record(Understood),
    /// Not a money record (an ad, a login, money moved between my accounts).
    Skip(String),
    /// Has an amount, but which way the money went can't be told.
    Unreadable,
}

static ADS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\(광고\)|\[광고\]|인증번호|OTP|로그인|비밀번호|이벤트|혜택|쿠폰").unwrap());
static AMOUNT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d{1,3}(?:,\d{3})+|\d+)\s*원").unwrap());
/// Words before an amount that isn't the transaction's own.
const NOT_THE_AMOUNT: &[&str] = &["잔액", "잔고", "누적", "한도", "가능금액", "가능액", "포인트", "적립", "캐시백", "수수료"];
static REFUND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"취소|환불").unwrap());
static FROM_PERSON: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([가-힣A-Za-z]{2,10})\s*님(?:이|으로부터|에게서|께서)").unwrap());
static TO_PERSON: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([가-힣A-Za-z]{2,10})\s*님(?:에게|께)").unwrap());
static IN_OUT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"입금|출금").unwrap());
static IN_WORDS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"받았|받으셨|들어왔|환급|이자").unwrap());
static OUT_WORDS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"결제|승인|이체|송금|보냈|인출|납부|사용|구매|지급").unwrap());
static ATM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"ATM|CD\s*출금|현금\s*인출|자동화기기").unwrap());
static SHOP_AT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([^\s|,\[\]]+(?:[ ][^\s|,\[\]]+){0,3})에서\s*(?:\d{1,3}(?:,\d{3})+|\d+)\s*원").unwrap()
});
/// Bits of a notification that are never the shop or the person.
static NOISE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"\[[^\]]*\]|\([^)]*\d[^)]*\)",               // [NH농협], (1234)
        r"|(?:잔액|잔고|누적|한도)\s*:?\s*-?[\d,]+\s*원?", // the balance
        r"|-?[\d,]+\s*원",                            // amounts
        r"|\d{1,4}[./-]\d{1,2}(?:[./-]\d{1,4})?",      // dates
        r"|\d{1,2}:\d{2}(?::\d{2})?",                  // times
        r"|[\d*]+(?:-[\d*]+)+|\*+\d+|\d+\*+",          // masked accounts and cards
    ))
    .unwrap()
});
const STOP: &[&str] = &[
    "NH농협", "NH", "농협은행", "NH콕뱅크", "토스뱅크", "카카오페이머니", "페이머니",
    "입금", "출금", "결제", "승인", "이체", "송금", "일시불", "할부", "체크", "체크카드", "신용", "카드", "완료", "알림", "거래",
    "사용", "취소", "환불", "원", "님", "잔액", "잔고", "누적", "결제완료", "입금완료", "출금완료", "이체완료", "승인완료",
    "했어요", "됐어요", "되었어요", "되었습니다", "했습니다", "보냈어요", "받았어요", "입출금", "입출금알림", "내", "통장", "계좌",
];
/// Words before 님 that aren't a person's name.
const NOT_NAMES: &[&str] = &["고객", "회원", "사용자"];
const PARTICLES: &[&str] = &["에서", "으로", "에게", "님이", "님께", "님", "로"];

/// Turn one notification into a record, or say why not.
pub fn understand(rec: &Record, rules: &Rules, history: &[FinanceItem], today: Date) -> Outcome {
    let full = format!("{} {}", rec.title, rec.text).split_whitespace().collect::<Vec<_>>().join(" ");
    if ADS.is_match(&full) && !REFUND.is_match(&full) {
        return Outcome::Skip("광고·인증 알림".into());
    }
    let Some(won) = amount(&full) else { return Outcome::Skip("금액이 없는 알림".into()) };
    let date = rec.date.unwrap_or(today).to_string();
    let account = account_of(&rec.app, &rec.title);

    // "입출금" (a title like 입출금알림) says neither way.
    let dir = full.replace("입출금", " ");
    let name = |re: &Regex| re.captures(&dir).map(|c| c[1].to_string()).filter(|n| !NOT_NAMES.contains(&n.as_str()));
    let (from, to) = (name(&FROM_PERSON), name(&TO_PERSON));
    let refund = REFUND.is_match(&dir);
    let incoming = if refund {
        false
    } else if from.is_some() {
        true
    } else if to.is_some() {
        false
    } else if let Some(m) = IN_OUT.find(&dir) {
        m.as_str() == "입금"
    } else {
        match (IN_WORDS.find(&dir), OUT_WORDS.find(&dir)) {
            (Some(i), Some(o)) => i.start() < o.start(),
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) if full.contains("충전") => return Outcome::Skip("충전 (내 계좌끼리 옮긴 돈)".into()),
            (None, None) => return Outcome::Unreadable,
        }
    };
    if incoming && full.contains("충전") {
        return Outcome::Skip("충전 (내 계좌끼리 옮긴 돈)".into());
    }

    let person = from.clone().or(to.clone());
    let memo = person.clone().unwrap_or_else(|| counterparty(rec, &account));
    // "카카오페이" as my own account means nothing in Kakao Pay's own notifications.
    let own = rules.own.iter().map(|o| o.trim()).filter(|o| !o.is_empty() && *o != account).any(|o| memo.contains(o));
    if own {
        return Outcome::Skip("내 계좌끼리 옮긴 돈".into());
    }
    let with_account = |memo: &str| match (memo.is_empty(), account.is_empty()) {
        (_, true) => memo.to_string(),
        (true, false) => format!("[{account}]"),
        (false, false) => format!("{memo} [{account}]"),
    };
    let cents = won * 100;

    if !incoming && !refund && ATM.is_match(&full) {
        let (kind, amount, memo, category) = if rules.atm_lab_cost {
            (EntryKind::Income, -cents, "연구실 비용 (현금 인출)".to_string(), "인건비".to_string())
        } else {
            (EntryKind::Expense, cents, "현금 인출".to_string(), guess_category("현금 인출", EntryKind::Expense, history))
        };
        return Outcome::Record(Understood { kind, amount, memo: with_account(&memo), category, date });
    }
    let looks_like_person = person.is_some()
        || (memo.chars().count() >= 2 && memo.chars().count() <= 4 && memo.chars().all(|c| ('가'..='힣').contains(&c)));
    if incoming && looks_like_person && rules.settle_below > 0 && won < rules.settle_below {
        let memo = format!("{memo}에게서 돌려받음");
        return Outcome::Record(Understood {
            kind: EntryKind::Expense,
            amount: -cents,
            memo: with_account(&memo),
            category: "식비/정산".into(),
            date,
        });
    }
    let kind = if incoming { EntryKind::Income } else { EntryKind::Expense };
    let category = guess_category(&memo, kind, history);
    let amount = if refund { -cents } else { cents };
    let memo = if memo.is_empty() && account.is_empty() { rec.app.clone() } else { memo };
    Outcome::Record(Understood { kind, amount, memo: with_account(&memo), category, date })
}

/// The transaction's amount in won: the first "…원" that isn't a balance.
fn amount(text: &str) -> Option<i64> {
    AMOUNT.captures_iter(text).find_map(|c| {
        let at = c.get(0)?.start();
        let before = text[..at].trim_end().trim_end_matches(':').trim_end();
        if NOT_THE_AMOUNT.iter().any(|w| before.ends_with(w)) {
            return None;
        }
        c[1].replace(',', "").parse::<i64>().ok().filter(|v| *v > 0)
    })
}

/// The account a notification is about, named as in the vault's records.
fn account_of(app: &str, title: &str) -> String {
    let both = format!("{app} {title}");
    if both.contains("농협") || both.contains("NH") {
        "농협".into()
    } else if both.contains("토스") {
        "토스".into()
    } else if both.contains("카카오페이") {
        "카카오페이".into()
    } else if both.contains("카카오뱅크") {
        "카카오뱅크".into()
    } else {
        app.trim().to_string()
    }
}

/// The shop or person: "…에서 8,300원", else the first part of the text
/// left once amounts, dates, account numbers and bank words are taken out.
fn counterparty(rec: &Record, account: &str) -> String {
    let text = if rec.text.is_empty() { &rec.title } else { &rec.text };
    if let Some(c) = SHOP_AT.captures(text) {
        let shop = clean_words(&c[1], account);
        if !shop.is_empty() {
            return shop;
        }
    }
    NOISE
        .replace_all(text, " ")
        .split(['|', '\n', '/'])
        .map(|part| clean_words(part, account))
        .find(|s| !s.is_empty())
        .unwrap_or_default()
}

/// `s` without bank words, the account's own name and trailing particles.
fn clean_words(s: &str, account: &str) -> String {
    let stop = |w: &str| STOP.contains(&w) || (!account.is_empty() && w == account);
    let words: Vec<String> = s
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| matches!(c, ',' | '.' | ':' | '·' | '-' | '|')))
        .filter(|w| !w.is_empty() && !stop(w))
        .map(|w| {
            let mut w = w.to_string();
            for p in PARTICLES {
                if w.chars().count() > p.chars().count() + 1 && w.ends_with(p) {
                    w.truncate(w.len() - p.len());
                    break;
                }
            }
            w
        })
        .filter(|w| !stop(w))
        .collect();
    let joined = words.join(" ");
    joined.chars().take(40).collect::<String>().trim().to_string()
}

/// Memo without the `[계좌]` part, spaces and case, for comparing.
fn memo_base(memo: &str) -> String {
    let memo = match memo.rfind(" [") {
        Some(at) if memo.ends_with(']') => &memo[..at],
        _ => memo,
    };
    memo.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect()
}

const KEYWORDS: &[(&str, &[&str])] = &[
    ("편의점", &["CU", "씨유", "GS25", "지에스25", "세븐일레븐", "이마트24", "미니스톱"]),
    ("배달음식", &["배달의민족", "배민", "요기요", "쿠팡이츠"]),
    ("카페", &["카페", "커피", "스타벅스", "투썸", "이디야", "메가커피", "메가MGC", "컴포즈", "빽다방"]),
    ("교통", &["택시", "카카오T", "버스", "지하철", "코레일", "KORAIL", "SRT", "티머니", "고속"]),
    ("구독", &["넷플릭스", "YouTube", "유튜브", "Spotify", "멜론", "Claude", "ChatGPT", "Apple"]),
    ("이자", &["이자", "예탁금이용료"]),
];

/// The category of earlier records with the same memo (most used), else
/// one from well-known shop names, else 기타.
pub fn guess_category(memo: &str, kind: EntryKind, history: &[FinanceItem]) -> String {
    let base = memo_base(memo);
    if base.chars().count() >= 2 {
        let mut exact: Vec<&str> = Vec::new();
        let mut prefix: Vec<&str> = Vec::new();
        for e in history.iter().filter(|e| e.kind == kind) {
            let Some(c) = e.category.as_deref() else { continue };
            let other = memo_base(&e.memo);
            if other == base {
                exact.push(c);
            } else if other.chars().count() >= 2 && (other.starts_with(&base) || base.starts_with(&other)) {
                prefix.push(c);
            }
        }
        if let Some(c) = most_common(if exact.is_empty() { &prefix } else { &exact }) {
            return c.to_string();
        }
    }
    let lower = memo.to_lowercase();
    KEYWORDS
        .iter()
        .find(|(_, words)| words.iter().any(|w| lower.contains(&w.to_lowercase())))
        .map(|(c, _)| c.to_string())
        .unwrap_or_else(|| "기타".into())
}

fn most_common<'a>(list: &[&'a str]) -> Option<&'a str> {
    let mut best: Option<(&str, usize)> = None;
    for c in list {
        let n = list.iter().filter(|x| *x == c).count();
        if best.is_none_or(|(_, m)| n > m) {
            best = Some((c, n));
        }
    }
    best.map(|(c, _)| c)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Added,
    Duplicate,
    Skipped,
    Unreadable,
}

/// What happened to one notification (shown in the setup dialog).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogItem {
    pub key: String,
    /// `YYYY-MM-DD HH:MM` from the header (or the day it was read).
    pub when: String,
    pub app: String,
    /// Title and text, shortened.
    pub text: String,
    pub status: Status,
    /// The record written, or why nothing was.
    pub result: String,
}

#[derive(Debug, Clone, Default)]
pub struct Batch {
    /// New records, in file order.
    pub records: Vec<Understood>,
    pub log: Vec<LogItem>,
    /// Keys of every record handled now (to skip them next time).
    pub keys: Vec<String>,
}

/// Handle the inbox records not in `seen`. `history` are the vault's dated
/// records: a notification whose record is already there (same day, same
/// money in or out, a similar memo, e.g. from a statement import) is left out.
pub fn process(file: &str, seen: &HashSet<String>, rules: &Rules, history: &[FinanceItem], today: Date) -> Batch {
    let mut batch = Batch::default();
    // Existing records that can still match a notification: (date, flow, memo base).
    let mut pool: Vec<(String, i64, String)> = history
        .iter()
        .filter_map(|e| Some((e.date.clone()?, flow(e.kind, e.amount), memo_base(&e.memo))))
        .collect();
    let mut handled: HashSet<&str> = HashSet::new();
    let all = records(file);
    for rec in &all {
        if seen.contains(&rec.key) || !handled.insert(&rec.key) {
            continue;
        }
        batch.keys.push(rec.key.clone());
        let when = match (rec.date, &rec.time) {
            (Some(d), Some(t)) => format!("{d} {t}"),
            (Some(d), None) => d.to_string(),
            (None, _) => today.to_string(),
        };
        let shown = format!("{} {}", rec.title, rec.text).split_whitespace().collect::<Vec<_>>().join(" ");
        let shown: String = shown.chars().take(120).collect();
        let log = |status, result: String| LogItem {
            key: rec.key.clone(),
            when: when.clone(),
            app: rec.app.clone(),
            text: shown.clone(),
            status,
            result,
        };
        match understand(rec, rules, history, today) {
            Outcome::Record(u) => {
                let base = memo_base(&u.memo);
                let short: String = base.chars().take(2).collect();
                let same = pool.iter().position(|(d, f, m)| {
                    *d == u.date && *f == u.flow() && (m.is_empty() || base.is_empty() || m.starts_with(&short) || base.starts_with(&m.chars().take(2).collect::<String>()))
                });
                if let Some(at) = same {
                    pool.remove(at);
                    batch.log.push(log(Status::Duplicate, "같은 기록이 이미 있음".into()));
                } else {
                    let line = u.line();
                    batch.log.push(log(Status::Added, line.trim_start_matches("- ").to_string()));
                    pool.push((u.date.clone(), u.flow(), base));
                    batch.records.push(u);
                }
            }
            Outcome::Skip(why) => batch.log.push(log(Status::Skipped, why)),
            Outcome::Unreadable => batch.log.push(log(Status::Unreadable, "들어온 돈인지 나간 돈인지 알 수 없음".into())),
        }
    }
    batch
}

/// The note a record of `date` goes to: `YYYY` and `MM` in `pattern` are
/// replaced (default layout: `가계부/YYYY-MM`).
pub fn note_for(pattern: &str, date: &str) -> String {
    let (y, m) = (date.get(0..4).unwrap_or("0000"), date.get(5..7).unwrap_or("01"));
    let p = pattern.trim().trim_matches('/').replace('\\', "/");
    let p = if p.is_empty() { "가계부/YYYY-MM".to_string() } else { p };
    let p = p.replace("YYYY", y).replace("MM", m);
    if p.to_lowercase().ends_with(".md") { p } else { format!("{p}.md") }
}

/// The text the phone's automation app should append for each notification
/// (MacroDroid's magic-text names).
pub const MACRODROID_TEMPLATE: &str =
    "@@ [year]-[month_digit]-[dayofmonth] [hour]:[minute] | [app_name] | [notification_title]\n[notification]";

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> Date {
        Date::new(2026, 11, 14).unwrap()
    }

    fn one(text: &str) -> Outcome {
        let recs = records(text);
        assert_eq!(recs.len(), 1, "{text}");
        let rules = Rules { own: vec!["카카오페이".into(), "홍길동".into()], atm_lab_cost: true, settle_below: 100_000 };
        understand(&recs[0], &rules, &[], today())
    }

    fn rec(text: &str) -> Understood {
        match one(text) {
            Outcome::Record(u) => u,
            other => panic!("{text}: {other:?}"),
        }
    }

    #[test]
    fn records_are_split_at_headers() {
        let file = "\u{feff}junk\n@@ 2026-11-14 13:22 | 토스 | 토스뱅크\n맘스터치에서 8,300원\n결제\n@@ 2026/1/5 오후 1:05 | 카카오페이\n결제 1,000원\n";
        let r = records(file);
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].date, r[0].time.as_deref(), r[0].app.as_str(), r[0].title.as_str()),
            (Date::new(2026, 11, 14), Some("13:22"), "토스", "토스뱅크"));
        assert_eq!(r[0].text, "맘스터치에서 8,300원\n결제");
        assert_eq!((r[1].date, r[1].time.as_deref(), r[1].title.as_str()), (Date::new(2026, 1, 5), Some("13:05"), ""));
        assert_ne!(r[0].key, r[1].key);
        assert_eq!(records(file)[0].key, r[0].key, "keys are stable");
    }

    #[test]
    fn shop_payments() {
        let u = rec("@@ 2026-11-14 13:22 | 토스 | 토스뱅크\n맘스터치 대전대점에서 8,300원 결제했어요");
        assert_eq!((u.kind, u.amount, u.memo.as_str(), u.date.as_str()), (EntryKind::Expense, 830_000, "맘스터치 대전대점 [토스]", "2026-11-14"));
        assert_eq!(u.line(), "- 💸 8,300 맘스터치 대전대점 [토스] #기타 #자동 📅 2026-11-14");

        let u = rec("@@ 2026-11-14 09:00 | 카카오페이 | 카카오페이머니 결제\n씨유(CU)대전대서문스타점 | 3,200원 결제 완료");
        assert_eq!((u.amount, u.memo.as_str(), u.category.as_str()), (320_000, "씨유(CU)대전대서문스타점 [카카오페이]", "편의점"));
    }

    #[test]
    fn bank_style_text() {
        let u = rec("@@ 2026-11-14 10:00 | NH콕뱅크 | 입출금알림\n[NH농협] 출금 7,500원 11/14 10:00 302-****-0000-00 지지고대전대점 잔액 64,616원");
        assert_eq!((u.kind, u.amount, u.memo.as_str()), (EntryKind::Expense, 750_000, "지지고대전대점 [농협]"));
        // A balance before the amount isn't the amount.
        let u = rec("@@ 2026-11-14 10:00 | NH콕뱅크 | 입출금알림\n잔액 64,616원 입금 2,000,000원 연구실인건비");
        assert_eq!((u.kind, u.amount, u.memo.as_str()), (EntryKind::Income, 200_000_000, "연구실인건비 [농협]"));
    }

    #[test]
    fn people_and_settlements() {
        let u = rec("@@ 2026-11-14 12:00 | 토스 | 입금\n박서준님이 10,000원을 보냈어요");
        assert_eq!((u.kind, u.amount, u.memo.as_str(), u.category.as_str()),
            (EntryKind::Expense, -1_000_000, "박서준에게서 돌려받음 [토스]", "식비/정산"));
        let u = rec("@@ 2026-11-14 12:00 | 토스 | 입금\n조현숙님이 500,000원을 보냈어요");
        assert_eq!((u.kind, u.amount, u.memo.as_str()), (EntryKind::Income, 50_000_000, "조현숙 [토스]"));
        let u = rec("@@ 2026-11-14 12:00 | 카카오페이 | 송금\n박서준님에게 11,800원을 보냈어요");
        assert_eq!((u.kind, u.amount, u.memo.as_str()), (EntryKind::Expense, 1_180_000, "박서준 [카카오페이]"));
    }

    #[test]
    fn refunds_atm_and_skips() {
        let u = rec("@@ 2026-11-14 12:00 | 카카오페이 | 결제 취소\n맘스터치에서 8,300원 결제가 취소되었어요");
        assert_eq!((u.kind, u.amount), (EntryKind::Expense, -830_000));
        let u = rec("@@ 2026-11-14 12:00 | NH콕뱅크 | 입출금알림\n농협 출금 300,000원 11/14 12:00 CD출금 잔액 1,000원");
        assert_eq!((u.kind, u.amount, u.memo.as_str(), u.category.as_str()),
            (EntryKind::Income, -30_000_000, "연구실 비용 (현금 인출) [농협]", "인건비"));
        assert_eq!(one("@@ 2026-11-14 12:00 | NH콕뱅크 | 입출금알림\n출금 50,000원 카카오페이"), Outcome::Skip("내 계좌끼리 옮긴 돈".into()));
        assert_eq!(one("@@ 2026-11-14 12:00 | 카카오페이 | 충전\n카카오페이머니 50,000원이 충전되었어요 입금"), Outcome::Skip("충전 (내 계좌끼리 옮긴 돈)".into()));
        assert_eq!(one("@@ 2026-11-14 12:00 | 카카오페이 | 충전\n카카오페이머니 50,000원이 충전되었어요"), Outcome::Skip("충전 (내 계좌끼리 옮긴 돈)".into()));
        // Paying another pay service from the bank is spending.
        let u = rec("@@ 2026-11-14 12:00 | NH콕뱅크 | 입출금알림\n출금 20,000원 네이버페이충전");
        assert_eq!((u.kind, u.amount), (EntryKind::Expense, 2_000_000));
        assert_eq!(one("@@ 2026-11-14 12:00 | 토스 | (광고) 혜택\n최대 10,000원 받기"), Outcome::Skip("광고·인증 알림".into()));
        assert_eq!(one("@@ 2026-11-14 12:00 | 토스 | 로그인\n새 기기에서 로그인했어요"), Outcome::Skip("광고·인증 알림".into()));
        assert_eq!(one("@@ 2026-11-14 12:00 | 토스 | 토스\n이번 달 10,000원"), Outcome::Unreadable);
    }

    fn item(date: &str, kind: EntryKind, amount: i64, memo: &str, category: &str) -> FinanceItem {
        FinanceItem {
            path: "가계부/2026-11.md".into(),
            title: "2026-11".into(),
            line: 0,
            raw: String::new(),
            kind,
            amount,
            memo: memo.into(),
            category: Some(category.into()),
            date: Some(date.into()),
            explicit_date: true,
            parent_line: None,
        }
    }

    #[test]
    fn categories_follow_history() {
        let history = [
            item("2026-11-02", EntryKind::Expense, 1_040_000, "맘스터치 대전대점 [카카오페이]", "식비"),
            item("2026-11-12", EntryKind::Expense, 830_000, "맘스터치 대전대점 [토스]", "식비"),
        ];
        assert_eq!(guess_category("맘스터치대전대점 [토스]", EntryKind::Expense, &history), "식비");
        assert_eq!(guess_category("맘스터치", EntryKind::Expense, &history), "식비");
        assert_eq!(guess_category("GS25 대전점", EntryKind::Expense, &history), "편의점");
        assert_eq!(guess_category("어딘가", EntryKind::Expense, &history), "기타");
    }

    #[test]
    fn process_skips_seen_and_duplicates() {
        let file = "@@ 2026-11-14 13:22 | 토스 | 토스뱅크\n맘스터치 대전대점에서 8,300원 결제\n\
                    @@ 2026-11-14 13:22 | 토스 | 토스뱅크\n맘스터치 대전대점에서 8,300원 결제\n\
                    @@ 2026-11-14 13:23 | 토스 | 토스뱅크\n맘스터치 대전대점에서 8,300원 결제\n\
                    @@ 2026-11-14 14:00 | 토스 | 토스뱅크\nGS25에서 1,500원 결제\n\
                    @@ 2026-11-14 15:00 | 토스 | 로그인\n로그인 알림\n";
        let history = [item("2026-11-14", EntryKind::Expense, 150_000, "GS25 대전점 [토스]", "편의점")];
        let b = process(file, &HashSet::new(), &Rules::default(), &history, today());
        assert_eq!(b.records.len(), 1);
        assert_eq!(b.records[0].memo, "맘스터치 대전대점 [토스]");
        let status: Vec<Status> = b.log.iter().map(|l| l.status).collect();
        // Same header and text twice = one record; a minute later it is the same purchase.
        assert_eq!(status, [Status::Added, Status::Duplicate, Status::Duplicate, Status::Skipped]);
        assert_eq!(b.keys.len(), 4);
        let seen: HashSet<String> = b.keys.into_iter().collect();
        assert!(process(file, &seen, &Rules::default(), &history, today()).log.is_empty());
    }

    #[test]
    fn notes_by_month() {
        assert_eq!(note_for("", "2026-11-14"), "가계부/2026-11.md");
        assert_eq!(note_for("돈/YYYY년 MM월", "2026-01-02"), "돈/2026년 01월.md");
    }
}
