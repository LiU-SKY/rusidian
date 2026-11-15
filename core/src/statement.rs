//! Statement files as rows of text, whatever the bank or card company sent:
//! CSV (any encoding, see `finance_import`), Excel xlsx / xls, and
//! password-protected Excel files (Kakao Pay's downloads are). The rows
//! then go through the same column guessing and import as CSV.

use crate::date::Date;
use crate::finance_import;
use calamine::{open_workbook_auto_from_rs, Data, Reader};
use regex::Regex;
use std::io::Cursor;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq)]
pub enum ReadError {
    /// An encrypted Office file and no password was given.
    NeedsPassword,
    /// The password did not open it.
    WrongPassword,
    Unreadable(String),
}

impl ReadError {
    /// What the UI gets: fixed codes for the password cases, else a message.
    pub fn message(&self) -> String {
        match self {
            ReadError::NeedsPassword => "PASSWORD_REQUIRED".into(),
            ReadError::WrongPassword => "WRONG_PASSWORD".into(),
            ReadError::Unreadable(m) => m.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub rows: Vec<Vec<String>>,
    /// "UTF-8", "CP949", "XLSX", "XLS", … (shown in the import dialog).
    pub format: String,
}

const OLE_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

fn is_zip(b: &[u8]) -> bool {
    b.starts_with(b"PK\x03\x04")
}

fn is_ole(b: &[u8]) -> bool {
    b.starts_with(&OLE_MAGIC)
}

/// An encrypted xlsx is an OLE file with an "EncryptionInfo" stream
/// (its directory names are UTF-16).
fn is_encrypted(b: &[u8]) -> bool {
    let name: Vec<u8> = "EncryptionInfo".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    is_ole(b) && b.windows(name.len()).any(|w| w == name.as_slice())
}

/// Read a statement file into rows.
pub fn read(bytes: &[u8], password: Option<&str>) -> Result<Table, ReadError> {
    if is_encrypted(bytes) {
        let password = password.filter(|p| !p.is_empty()).ok_or(ReadError::NeedsPassword)?;
        let plain = match msoffice_crypto::decrypt_ooxml(bytes, password) {
            Ok(p) => p,
            Err(msoffice_crypto::Error::WrongPassword) => return Err(ReadError::WrongPassword),
            Err(e) => return Err(ReadError::Unreadable(format!("암호 걸린 파일을 열 수 없습니다: {e}"))),
        };
        if !is_zip(&plain) {
            return Err(ReadError::WrongPassword);
        }
        return Ok(Table { rows: spreadsheet_rows(plain)?, format: "XLSX (암호)".into() });
    }
    if bytes.starts_with(b"%PDF") {
        return Ok(Table { rows: pdf_rows(bytes)?, format: "PDF (농협)".into() });
    }
    if is_zip(bytes) || is_ole(bytes) {
        let format = if is_zip(bytes) { "XLSX" } else { "XLS" };
        return Ok(Table { rows: spreadsheet_rows(bytes.to_vec())?, format: format.into() });
    }
    let (rows, encoding) = finance_import::csv_rows(bytes);
    Ok(Table { rows, format: encoding.to_string() })
}

/// The rows of the sheet with the most content (statements have one data sheet).
fn spreadsheet_rows(bytes: Vec<u8>) -> Result<Vec<Vec<String>>, ReadError> {
    let mut book = open_workbook_auto_from_rs(Cursor::new(bytes)).map_err(|e| ReadError::Unreadable(format!("엑셀 파일을 읽을 수 없습니다: {e}")))?;
    let best = book
        .worksheets()
        .into_iter()
        .map(|(_, range)| {
            range
                .rows()
                .map(|r| r.iter().map(cell_text).collect::<Vec<_>>())
                .filter(|r| r.iter().any(|c| !c.is_empty()))
                .collect::<Vec<_>>()
        })
        .max_by_key(Vec::len)
        .unwrap_or_default();
    // Statements often have an empty first column (a margin): drop columns that are empty everywhere.
    let width = best.iter().map(Vec::len).max().unwrap_or(0);
    let used: Vec<usize> = (0..width).filter(|&c| best.iter().any(|r| r.get(c).is_some_and(|v| !v.is_empty()))).collect();
    Ok(best.into_iter().map(|r| used.iter().map(|&c| r.get(c).cloned().unwrap_or_default()).collect()).collect())
}

// --------------------------------------------------------------------- PDF

/// Column names given to NH PDF rows (the names the column guessing knows).
const NH_HEADER: [&str; 8] = ["거래일시", "입출구분", "금액", "거래후잔액", "거래내용", "적요", "거래점", "이체메모"];

static NH_DATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{4}\.\d{2}\.\d{2}$").unwrap());
static NH_ENTRY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{2}:\d{2}:\d{2})\s+(입금|출금)\s+([\d,]+)원\s+(-?[\d,]+)원\s*(.*)$").unwrap());
static BRANCH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{6,7}$").unwrap());

fn pdf_rows(bytes: &[u8]) -> Result<Vec<Vec<String>>, ReadError> {
    let text = pdf_extract::extract_text_from_mem(bytes).map_err(|e| ReadError::Unreadable(format!("PDF를 읽을 수 없습니다: {e}")))?;
    nh_rows(&text).ok_or_else(|| {
        ReadError::Unreadable("이 PDF 형식은 아직 읽을 수 없습니다. 지금은 농협 '입출금 거래내역' 이메일 PDF를 읽습니다.".into())
    })
}

/// Page furniture between records: repeated headers, the print footer, the closing notice.
fn nh_furniture(line: &str) -> bool {
    line.starts_with("거래일시") || line.starts_with("file:") || line.contains("이메일 전송 서비스") || line.starts_with("- ")
}

/// Rows from the text of NH Bank's emailed statement ("입출금 거래 내역"):
/// each record is a date line, then "HH:MM:SS 입금|출금 금액원 잔액원
/// 거래내용 거래기록사항 거래점 [이체메모]", long cells wrapping onto the
/// next line in the middle of a word. `None` if the text is not that layout.
pub fn nh_rows(text: &str) -> Option<Vec<Vec<String>>> {
    if !text.contains("거래후잔액") {
        return None;
    }
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let mut rows = vec![NH_HEADER.iter().map(|s| s.to_string()).collect::<Vec<_>>()];
    let mut i = 0;
    while i + 1 < lines.len() {
        let Some(c) = NH_DATE.is_match(lines[i]).then(|| NH_ENTRY.captures(lines[i + 1])).flatten() else {
            i += 1;
            continue;
        };
        let mut rest = c[5].to_string();
        let mut j = i + 2;
        while j < lines.len() && !NH_DATE.is_match(lines[j]) && !nh_furniture(lines[j]) {
            // A wrapped cell continues the word; after the branch code a new line is the memo.
            if rest.split_whitespace().any(|t| BRANCH.is_match(t)) {
                rest.push(' ');
            }
            rest.push_str(lines[j]);
            j += 1;
        }
        let tokens: Vec<&str> = rest.split_whitespace().collect();
        let branch = tokens.iter().skip(1).position(|t| BRANCH.is_match(t)).map(|p| p + 1);
        let (record, branch_code, memo) = match branch {
            Some(b) => (tokens[1..b].join(" "), tokens[b].to_string(), tokens[b + 1..].join(" ")),
            None => (tokens.get(1..).map(|t| t.join(" ")).unwrap_or_default(), String::new(), String::new()),
        };
        rows.push(vec![
            format!("{} {}", lines[i], &c[1]),
            c[2].to_string(),
            c[3].to_string(),
            c[4].to_string(),
            tokens.first().map(|t| t.to_string()).unwrap_or_default(),
            record,
            branch_code,
            memo,
        ]);
        i = j;
    }
    (rows.len() > 1).then_some(rows)
}

fn number_text(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        format!("{f}")
    }
}

/// Excel's serial date (days since 1899-12-30, time as the fraction) as
/// "YYYY-MM-DD" or "YYYY-MM-DD HH:MM:SS".
fn excel_date_text(serial: f64) -> String {
    let days = serial.floor();
    let date = Date::from_days(days as i64 - 25_569); // 25569 = 1970-01-01
    let secs = ((serial - days) * 86_400.0).round() as i64;
    if secs == 0 {
        date.to_string()
    } else {
        format!("{date} {:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
    }
}

fn cell_text(d: &Data) -> String {
    match d {
        Data::String(s) => s.trim().to_string(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => number_text(*f),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(dt) if dt.is_datetime() => excel_date_text(dt.as_f64()),
        Data::DateTime(dt) => number_text(dt.as_f64()),
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Error(_) | Data::Empty => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A minimal xlsx with one sheet of inline strings and numbers.
    fn xlsx(rows: &[&[&str]]) -> Vec<u8> {
        let mut sheet = String::from(r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#);
        for (i, r) in rows.iter().enumerate() {
            sheet.push_str(&format!(r#"<row r="{}">"#, i + 1));
            for (j, v) in r.iter().enumerate() {
                let col = (b'A' + j as u8) as char;
                if v.is_empty() {
                    continue;
                }
                if let Ok(n) = v.parse::<f64>() {
                    sheet.push_str(&format!(r#"<c r="{col}{}"><v>{n}</v></c>"#, i + 1));
                } else {
                    sheet.push_str(&format!(r#"<c r="{col}{}" t="inlineStr"><is><t>{v}</t></is></c>"#, i + 1));
                }
            }
            sheet.push_str("</row>");
        }
        sheet.push_str("</sheetData></worksheet>");
        let files = [
            ("[Content_Types].xml", r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#.to_string()),
            ("_rels/.rels", r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.to_string()),
            ("xl/workbook.xml", r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="거래내역" sheetId="1" r:id="rId1"/></sheets></workbook>"#.to_string()),
            ("xl/_rels/workbook.xml.rels", r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#.to_string()),
            ("xl/worksheets/sheet1.xml", sheet),
        ];
        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default();
            for (name, body) in files {
                zip.start_file(name, opts).unwrap();
                zip.write_all(body.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn reads_xlsx_dropping_empty_columns() {
        let bytes = xlsx(&[
            &["", "토스뱅크 거래내역"],
            &["", "거래 일시", "적요", "거래 금액", "거래 후 잔액"],
            &["", "2026.11.13 16:58:42", "카카오페이", "-10000", "3183"],
            &["", "2026.11.12 20:04:50", "맘스터치", "-8300.5", ""],
        ]);
        let t = read(&bytes, None).unwrap();
        assert_eq!(t.format, "XLSX");
        assert_eq!(t.rows[1], ["거래 일시", "적요", "거래 금액", "거래 후 잔액"]);
        assert_eq!(t.rows[2], ["2026.11.13 16:58:42", "카카오페이", "-10000", "3183"]);
        assert_eq!(t.rows[3][2], "-8300.5");
    }

    #[test]
    fn excel_dates_and_numbers() {
        assert_eq!(excel_date_text(25_569.0), "1970-01-01");
        assert_eq!(excel_date_text(25_569.5), "1970-01-01 12:00:00");
        assert_eq!(excel_date_text(46_339.0), Date::from_days(46_339 - 25_569).to_string());
        assert_eq!(number_text(10000.0), "10000");
        assert_eq!(number_text(-8.25), "-8.25");
    }

    #[test]
    fn nh_pdf_text_becomes_rows() {
        let text = concat!(
            "고객명 : 홍길동님\n입출금 거래 내역\n총 4건\n",
            "거래일시 구분 금액 거래후잔액 거래내용 거래기록사항 거래점 이체메모\n",
            "2026.11.11\n05:30:38 출금 10,000원 0원 실시간이체 카카오페이 000242\n",
            "2026.10.28\n19:28:31 출금 30,600원 228,923원 NH체크 맘스터치대전대\n점 613093\n",
            "26. 11. 14. 오전 3:28 입출금거래내역 이메일 전송 서비스\nfile:///C :/Users/x/Message.html 1/6\n",
            "거래일시 구분 금액 거래후잔액 거래내용 거래기록사항 거래점 이체메모\n",
            "2026.10.26\n15:39:54 출금 55,000원 287,563원 금결원PG 진학사   613092\n",
            "2026.10.01\n14:22:10 입금 900,000원 1,000,000원 PC하나은행 이도윤 0816456\n연구실 비용\n",
            "- 본 메일은 발신전용입니다.\n발송하는 메일입니다 2026.01.01\n",
        );
        let rows = nh_rows(text).unwrap();
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0][1], "입출구분");
        assert_eq!(rows[1], ["2026.11.11 05:30:38", "출금", "10,000", "0", "실시간이체", "카카오페이", "000242", ""]);
        assert_eq!(rows[2][5], "맘스터치대전대점"); // the wrapped word is whole again
        assert_eq!((rows[3][5].as_str(), rows[3][6].as_str()), ("진학사", "613092")); // header and footer between pages skipped
        assert_eq!((rows[4][1].as_str(), rows[4][7].as_str()), ("입금", "연구실 비용")); // memo on its own line
        // The column guessing reads it as date + 입출구분 + 금액 + 적요.
        let m = finance_import::guess_mapping(&rows[0]);
        assert_eq!((m.date, m.direction, m.amount, m.memo), (Some(0), Some(1), Some(2), Some(5)));
        assert_eq!(nh_rows("아무 PDF\n2026.11.11\n텍스트"), None);
    }

    #[test]
    fn recognises_encrypted_files_and_csv() {
        // An OLE file whose directory names an EncryptionInfo stream.
        let mut fake = OLE_MAGIC.to_vec();
        fake.extend(vec![0u8; 100]);
        fake.extend("EncryptionInfo".encode_utf16().flat_map(|u| u.to_le_bytes()));
        assert_eq!(read(&fake, None), Err(ReadError::NeedsPassword));
        assert_eq!(read(&fake, Some("")), Err(ReadError::NeedsPassword));
        assert!(matches!(read(&fake, Some("1234")), Err(ReadError::Unreadable(_)))); // not a real encrypted file
        assert_eq!(ReadError::NeedsPassword.message(), "PASSWORD_REQUIRED");
        // Not a spreadsheet: CSV as before.
        let t = read("날짜,내용,금액\n2026-11-13,커피,-4500\n".as_bytes(), None).unwrap();
        assert_eq!((t.format.as_str(), t.rows.len()), ("UTF-8", 2));
    }
}
