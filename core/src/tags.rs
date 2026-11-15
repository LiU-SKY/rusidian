//! Tags, Obsidian style: `#tag` in the text (not in code) and the `tags`
//! (or `tag`) property. Nested tags use `/`: `#project/rusidian`.

use crate::frontmatter::{self, Frontmatter};
use crate::markdown::{blank_inline_code, prose_lines};
use regex::Regex;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::LazyLock;

/// `#` at the start or after whitespace, then letters, digits, `_`, `-`, `/`.
static INLINE_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\s)#([\p{L}\p{N}_/-]+)").unwrap());

/// A tag body Obsidian accepts: allowed characters and not only digits.
fn valid(tag: &str) -> bool {
    !tag.is_empty()
        && tag.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '/'))
        && !tag.chars().all(|c| c.is_ascii_digit() || c == '/')
}

/// Tags of a note without `#`, in first-seen order, deduplicated
/// case-insensitively (Obsidian treats `#Tag` and `#tag` as one).
pub fn note_tags(text: &str) -> Vec<String> {
    let fm = if text.trim_start_matches('\u{feff}').starts_with("---") {
        frontmatter::parse(text)
    } else {
        Frontmatter::default()
    };
    note_tags_with(text, &fm)
}

/// [`note_tags`] with the note's frontmatter already parsed.
pub fn note_tags_with(text: &str, fm: &Frontmatter) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut push = |t: &str| {
        let t = t.trim().trim_start_matches('#').trim_end_matches('/');
        if valid(t) && seen.insert(t.to_lowercase()) {
            out.push(t.to_string());
        }
    };

    for key in ["tags", "tag"] {
        match fm.get(key) {
            Some(Value::Array(items)) => {
                for item in items {
                    if let Some(s) = item.as_str() {
                        push(s);
                    }
                }
            }
            Some(Value::String(s)) => s.split([',', ' ']).for_each(&mut push),
            _ => {}
        }
    }

    let body = &text[fm.body_start..];
    for (_, line) in prose_lines(body) {
        let line = blank_inline_code(line);
        for cap in INLINE_TAG.captures_iter(&line) {
            push(&cap[1]);
        }
    }
    out
}

/// `tag` (no `#`, any case) is `query` or nested below it.
pub fn matches(tag: &str, query: &str) -> bool {
    let (tag, query) = (tag.to_lowercase(), query.trim_start_matches('#').to_lowercase());
    tag == query || tag.strip_prefix(&query).is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_tags_outside_code() {
        let text = "# Heading\n#일 and #work/meeting, not a#b or #123\n`#code` ```\n```\n#fenced\n```\nend #Work/Meeting";
        assert_eq!(note_tags(text), ["일", "work/meeting"]);
    }

    #[test]
    fn property_tags_list_and_string() {
        let text = "---\ntags:\n  - project\n  - \"#idea\"\ntag: a, b\n---\nbody #project #new\n";
        assert_eq!(note_tags(text), ["project", "idea", "a", "b", "new"]);
    }

    #[test]
    fn yaml_comments_are_not_tags() {
        let text = "---\ncolor: \"#ff0000\" # note\n---\ntext";
        assert!(note_tags(text).is_empty());
    }

    #[test]
    fn nested_match() {
        assert!(matches("work/meeting", "#work"));
        assert!(matches("Work", "work"));
        assert!(!matches("workshop", "work"));
        assert!(!matches("work", "work/meeting"));
    }
}
