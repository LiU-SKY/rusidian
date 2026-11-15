//! YAML frontmatter ("Properties" in Obsidian).
//!
//! Obsidian's rule: the very first line of the file is `---`, and the YAML
//! block runs until the next line that is exactly `---`. Without a closing
//! fence there is no frontmatter. Key order is preserved for display.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use yaml_rust2::{Yaml, YamlLoader};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Property {
    pub key: String,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct Frontmatter {
    /// Whether the note has a (closed) frontmatter block at all.
    pub present: bool,
    /// Top-level properties in file order. Empty when `error` is set.
    pub properties: Vec<Property>,
    /// YAML error, if the block exists but cannot be parsed.
    pub error: Option<String>,
    /// Byte offset in the original text where the note body begins.
    pub body_start: usize,
}

impl Frontmatter {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.properties.iter().find(|p| p.key == key).map(|p| &p.value)
    }
}

const FENCE: &str = "---";
const BOM: char = '\u{feff}';

pub fn parse(text: &str) -> Frontmatter {
    let Some((yaml, body_start)) = split(text) else {
        return Frontmatter::default();
    };
    let mut fm = Frontmatter {
        present: true,
        body_start,
        ..Default::default()
    };
    match YamlLoader::load_from_str(yaml) {
        Err(e) => fm.error = Some(e.to_string()),
        Ok(docs) => match docs.into_iter().next() {
            None | Some(Yaml::Null) => {}
            Some(Yaml::Hash(hash)) => {
                fm.properties = hash
                    .into_iter()
                    .map(|(k, v)| Property {
                        key: key_to_string(&k),
                        value: to_json(v),
                    })
                    .collect();
            }
            Some(_) => fm.error = Some("frontmatter must be a list of `key: value` pairs".into()),
        },
    }
    fm
}

/// Returns the YAML source between the fences and the body's byte offset.
fn split(text: &str) -> Option<(&str, usize)> {
    let start = if text.starts_with(BOM) { BOM.len_utf8() } else { 0 };
    let rest = &text[start..];

    let (first, after_first) = next_line(rest)?;
    if first.trim_end() != FENCE {
        return None;
    }
    let yaml_start = start + after_first;

    let mut pos = yaml_start;
    while pos < text.len() {
        let (line, consumed) = next_line(&text[pos..])?;
        if line.trim_end() == FENCE {
            return Some((&text[yaml_start..pos], pos + consumed));
        }
        pos += consumed;
    }
    None
}

/// Next line without its terminator, plus bytes consumed (incl. `\n`).
fn next_line(s: &str) -> Option<(&str, usize)> {
    if s.is_empty() {
        return None;
    }
    Some(match s.find('\n') {
        Some(i) => (s[..i].strip_suffix('\r').unwrap_or(&s[..i]), i + 1),
        None => (s, s.len()),
    })
}

fn key_to_string(k: &Yaml) -> String {
    match k {
        Yaml::String(s) | Yaml::Real(s) => s.clone(),
        Yaml::Integer(i) => i.to_string(),
        Yaml::Boolean(b) => b.to_string(),
        Yaml::Null => "null".into(),
        other => format!("{other:?}"),
    }
}

fn to_json(y: Yaml) -> Value {
    match y {
        Yaml::String(s) => Value::String(s),
        Yaml::Integer(i) => Value::Number(i.into()),
        Yaml::Real(s) => s
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map(Value::Number)
            .unwrap_or(Value::String(s)),
        Yaml::Boolean(b) => Value::Bool(b),
        Yaml::Array(items) => Value::Array(items.into_iter().map(to_json).collect()),
        Yaml::Hash(hash) => Value::Object(
            hash.into_iter()
                .map(|(k, v)| (key_to_string(&k), to_json(v)))
                .collect::<Map<_, _>>(),
        ),
        Yaml::Null | Yaml::Alias(_) | Yaml::BadValue => Value::Null,
    }
}

// ------------------------------------------------------------------ writing

/// `text` with its frontmatter replaced by `props` (added at the top if the
/// note had none, removed if `props` is empty). The body is left byte for
/// byte; the block is written the way Obsidian writes properties, so YAML
/// comments and custom formatting inside it are not kept.
pub fn replace(text: &str, props: &[Property]) -> String {
    let bom = if text.starts_with(BOM) { BOM.to_string() } else { String::new() };
    let body_start = split(text).map(|(_, start)| start).unwrap_or(bom.len());
    let body = &text[body_start..];
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    if props.is_empty() {
        return format!("{bom}{body}");
    }
    format!("{bom}{}{body}", render(props, nl))
}

/// The `---` fenced block for `props`, ending with a newline.
pub fn render(props: &[Property], nl: &str) -> String {
    let mut out = format!("{FENCE}{nl}");
    for p in props {
        out.push_str(&scalar(&p.key));
        out.push(':');
        match &p.value {
            Value::Null => {}
            Value::Array(items) if items.is_empty() => out.push_str(" []"),
            Value::Array(items) => {
                for item in items {
                    out.push_str(nl);
                    out.push_str("  - ");
                    out.push_str(&inline(item));
                }
            }
            v => {
                out.push(' ');
                out.push_str(&inline(v));
            }
        }
        out.push_str(nl);
    }
    out.push_str(FENCE);
    out.push_str(nl);
    out
}

/// A value on one line: scalars plainly or quoted, nested values in flow
/// style (JSON is valid YAML flow syntax).
fn inline(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => scalar(s),
        other => other.to_string(),
    }
}

/// A string as a YAML scalar: plain when that reads back as the same
/// string, else double-quoted (JSON escaping is valid YAML).
fn scalar(s: &str) -> String {
    if plain_ok(s) { s.to_string() } else { serde_json::to_string(s).unwrap_or_default() }
}

fn plain_ok(s: &str) -> bool {
    const RESERVED: &[&str] = &["true", "false", "yes", "no", "on", "off", "null", "y", "n", "~", ".inf", ".nan"];
    let Some(first) = s.chars().next() else { return false };
    !(s.trim() != s
        || "-?:,[]{}#&*!|>'\"%@`".contains(first)
        || s.chars().any(|c| c.is_control())
        || s.contains(": ")
        || s.contains(" #")
        || s.ends_with(':')
        || RESERVED.contains(&s.to_ascii_lowercase().as_str())
        || looks_numeric(s))
}

/// Would a YAML reader take this as a number (`12`, `-3.5`, `1e3`, `0x1F`)?
fn looks_numeric(s: &str) -> bool {
    let t = s.trim_start_matches(['+', '-']);
    s.parse::<f64>().is_ok()
        || t.starts_with("0x")
        || t.starts_with("0o")
        || (t.starts_with('.') && t[1..].chars().next().is_some_and(|c| c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn prop(key: &str, value: Value) -> Property {
        Property { key: key.into(), value }
    }

    #[test]
    fn renders_obsidian_style() {
        let props = [
            prop("title", json!("회의록")),
            prop("tags", json!(["work", "#idea"])),
            prop("aliases", json!([])),
            prop("date", json!("2026-11-20")),
            prop("done", json!(false)),
            prop("count", json!(3)),
            prop("link", json!("[[Home]]")),
            prop("empty", Value::Null),
            prop("note", json!("a: b")),
            prop("zip", json!("01234")),
            prop("answer", json!("yes")),
            prop("obj", json!({ "a": 1 })),
        ];
        assert_eq!(
            render(&props, "\n"),
            "---\ntitle: 회의록\ntags:\n  - work\n  - \"#idea\"\naliases: []\ndate: 2026-11-20\ndone: false\n\
             count: 3\nlink: \"[[Home]]\"\nempty:\nnote: \"a: b\"\nzip: \"01234\"\nanswer: \"yes\"\nobj: {\"a\":1}\n---\n"
        );
    }

    #[test]
    fn render_round_trips() {
        let props = vec![
            prop("multi line", json!("one\ntwo")),
            prop("quote", json!("say \"hi\"")),
            prop("list", json!(["a b", 1, true, "-x"])),
            prop("float", json!(12.5)),
            prop("space", json!(" lead")),
        ];
        let text = replace("body", &props);
        let fm = parse(&text);
        assert_eq!(fm.error, None);
        assert_eq!(fm.properties, props);
        assert_eq!(&text[fm.body_start..], "body");
    }

    #[test]
    fn replace_keeps_body_bom_and_newlines() {
        let text = "\u{feff}---\r\nold: 1\r\n---\r\nbody\r\n";
        let out = replace(text, &[prop("new", json!("v"))]);
        assert_eq!(out, "\u{feff}---\r\nnew: v\r\n---\r\nbody\r\n");
        // Removing the last property removes the block.
        assert_eq!(replace(text, &[]), "\u{feff}body\r\n");
        // A note without frontmatter gets one at the top.
        assert_eq!(replace("# Title\n", &[prop("a", json!(1))]), "---\na: 1\n---\n# Title\n");
    }

    fn keys(fm: &Frontmatter) -> Vec<&str> {
        fm.properties.iter().map(|p| p.key.as_str()).collect()
    }

    #[test]
    fn parses_properties_in_file_order() {
        let text = "---\ntitle: 회의록\ntags:\n  - work\n  - meeting\ndate: 2026-11-20\ndone: false\nbudget: 12.5\ncount: 3\n---\n# Body\n";
        let fm = parse(text);
        assert!(fm.present);
        assert_eq!(fm.error, None);
        assert_eq!(keys(&fm), ["title", "tags", "date", "done", "budget", "count"]);
        assert_eq!(fm.get("title"), Some(&json!("회의록")));
        assert_eq!(fm.get("tags"), Some(&json!(["work", "meeting"])));
        // YAML 1.2 core schema: dates stay strings, which is what Obsidian shows.
        assert_eq!(fm.get("date"), Some(&json!("2026-11-20")));
        assert_eq!(fm.get("done"), Some(&json!(false)));
        assert_eq!(fm.get("budget"), Some(&json!(12.5)));
        assert_eq!(fm.get("count"), Some(&json!(3)));
        assert_eq!(&text[fm.body_start..], "# Body\n");
    }

    #[test]
    fn no_frontmatter_cases() {
        for text in [
            "",
            "# Just a note\n",
            "\n---\na: 1\n---\n",         // must be the very first line
            "---\na: 1\nno closing fence\n", // unclosed
            "----\na: 1\n----\n",
        ] {
            let fm = parse(text);
            assert!(!fm.present, "should not detect frontmatter in {text:?}");
            assert_eq!(fm.body_start, 0);
        }
    }

    #[test]
    fn handles_crlf_and_bom() {
        let text = "\u{feff}---\r\nstatus: active\r\n---\r\nbody";
        let fm = parse(text);
        assert_eq!(keys(&fm), ["status"]);
        assert_eq!(fm.get("status"), Some(&json!("active")));
        assert_eq!(&text[fm.body_start..], "body");
    }

    #[test]
    fn empty_block_and_fence_at_eof() {
        let fm = parse("---\n---\n");
        assert!(fm.present);
        assert!(fm.properties.is_empty());
        assert_eq!(fm.error, None);

        let fm = parse("---\na: 1\n---");
        assert_eq!(keys(&fm), ["a"]);
        assert_eq!(fm.body_start, "---\na: 1\n---".len());
    }

    #[test]
    fn invalid_yaml_reports_error() {
        let fm = parse("---\ntitle: [unclosed\n---\nbody");
        assert!(fm.present);
        assert!(fm.error.is_some());
        assert!(fm.properties.is_empty());

        let fm = parse("---\njust a scalar\n---\n");
        assert!(fm.error.is_some());
    }

    #[test]
    fn nested_objects_become_json_objects() {
        let fm = parse("---\nexpense:\n  amount: 15000\n  category: food\n---\n");
        assert_eq!(
            fm.get("expense"),
            Some(&json!({ "amount": 15000, "category": "food" }))
        );
    }
}
