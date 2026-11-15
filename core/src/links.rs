//! Link extraction from note text (Obsidian syntax).
//!
//! Recognised: `[[Note]]`, `[[Note|alias]]`, `[[Note#Heading]]`, `![[embed.png]]`
//! and markdown links to local files `[text](Folder/Note.md)`.
//! Links inside fenced code blocks and inline code spans are ignored.

use crate::markdown::{blank_inline_code, prose_lines};
use regex::Regex;
use std::ops::Range;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// Link target without alias / `#heading` / `^block` parts, e.g. `Folder/Note`.
    pub target: String,
    /// 0-based line number where the link appears.
    pub line: usize,
    /// `![[...]]` / `![](...)` embeds.
    pub embed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkStyle {
    /// `[[target|alias]]`
    Wiki,
    /// `[text](target)`, spaces written as `%20`
    Markdown,
    /// `[text](<target>)`, spaces written as-is
    MarkdownAngle,
}

/// A link plus where its target is written, for rewriting it in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkSpan {
    pub link: Link,
    pub style: LinkStyle,
    /// Byte range in the note text of the target as written: no alias,
    /// `#heading`, `^block`, `<>` or title, and still `%20`-encoded.
    pub range: Range<usize>,
}

static WIKILINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(!?)\[\[([^\[\]\n]+?)\]\]").unwrap());
static MD_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(!?)\[[^\]\n]*\]\(([^)\n]+)\)").unwrap());

pub fn extract_links(text: &str) -> Vec<Link> {
    link_spans(text).into_iter().map(|s| s.link).collect()
}

/// Every link in `text` with the byte range of its written target,
/// in reading order.
pub fn link_spans(text: &str) -> Vec<LinkSpan> {
    let mut spans = Vec::new();

    for (line_no, line) in prose_lines(text) {
        // `lines()` yields slices of `text`, so this is the line's byte offset.
        let base = line.as_ptr() as usize - text.as_ptr() as usize;
        // Same byte length as `line`: code spans become spaces.
        let line = blank_inline_code(line);

        for cap in WIKILINK.captures_iter(&line) {
            let m = cap.get(2).unwrap();
            let inner = m.as_str();
            // Target ends at the alias pipe (escaped as `\|` in tables) or `#` / `^`.
            let end = inner.find(['|', '#', '^']).unwrap_or(inner.len());
            let written = inner[..end].trim_end_matches('\\');
            let Some((start, len)) = trimmed_bounds(written) else { continue };
            if let Some(target) = clean_target(written) {
                spans.push(LinkSpan {
                    link: Link { target, line: line_no, embed: !cap[1].is_empty() },
                    style: LinkStyle::Wiki,
                    range: base + m.start() + start..base + m.start() + start + len,
                });
            }
        }
        for cap in MD_LINK.captures_iter(&line) {
            let m = cap.get(2).unwrap();
            let Some((mut start, len)) = trimmed_bounds(m.as_str()) else { continue };
            let mut raw = &m.as_str()[start..start + len];
            let angle = raw.len() >= 2 && raw.starts_with('<') && raw.ends_with('>');
            if angle {
                raw = &raw[1..raw.len() - 1];
                start += 1;
            } else {
                // Drop an optional title: [x](path "title")
                raw = raw.split(" \"").next().unwrap_or(raw);
            }
            if is_external(raw) {
                continue;
            }
            let path_len = raw.find('#').unwrap_or(raw.len());
            if let Some(target) = clean_target(&raw.replace("%20", " ")) {
                spans.push(LinkSpan {
                    link: Link { target, line: line_no, embed: !cap[1].is_empty() },
                    style: if angle { LinkStyle::MarkdownAngle } else { LinkStyle::Markdown },
                    range: base + m.start() + start..base + m.start() + start + path_len,
                });
            }
        }
    }
    spans.sort_by_key(|s| s.range.start);
    spans
}

/// Offset and length of `s` without surrounding whitespace; `None` if blank.
fn trimmed_bounds(s: &str) -> Option<(usize, usize)> {
    let t = s.trim();
    (!t.is_empty()).then(|| (s.len() - s.trim_start().len(), t.len()))
}

/// Strip `#heading` / `^block` and surrounding space. `None` for self-links.
fn clean_target(raw: &str) -> Option<String> {
    let t = raw.split('#').next().unwrap_or("");
    let t = t.split('^').next().unwrap_or("").trim();
    (!t.is_empty()).then(|| t.replace('\\', "/"))
}

fn is_external(target: &str) -> bool {
    target.starts_with('#')
        || target.starts_with("//")
        || target
            .split_once(':')
            .is_some_and(|(scheme, _)| !scheme.is_empty() && !scheme.contains('/') && scheme.len() > 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(text: &str) -> Vec<String> {
        extract_links(text).into_iter().map(|l| l.target).collect()
    }

    #[test]
    fn wikilink_forms() {
        let text = "[[Note]] [[Folder/Other|별칭]] [[With Heading#Section]] ![[pic.png]] [[Block^abc]]";
        assert_eq!(targets(text), ["Note", "Folder/Other", "With Heading", "pic.png", "Block"]);
        let links = extract_links(text);
        assert!(links[3].embed);
        assert!(!links[0].embed);
    }

    #[test]
    fn self_heading_links_are_skipped() {
        assert!(targets("[[#Heading]] [[^block]]").is_empty());
    }

    #[test]
    fn escaped_pipe_in_tables() {
        assert_eq!(targets("| [[Note\\|alias]] |"), ["Note"]);
    }

    #[test]
    fn markdown_links_local_only() {
        let text = "[a](Folder/My%20Note.md) [b](https://x.com) [c](mailto:a@b.c) [d](#top) ![e](img/pic.png) [f](<Other Note.md>)";
        assert_eq!(targets(text), ["Folder/My Note.md", "img/pic.png", "Other Note.md"]);
    }

    #[test]
    fn ignores_code() {
        let text = "real [[A]]\n```\n[[InFence]]\n```\n`[[Inline]]` and [[B]]\n~~~~\n[[Tilde]]\n~~~~\n[[C]]";
        assert_eq!(targets(text), ["A", "B", "C"]);
        let lines: Vec<_> = extract_links(text).into_iter().map(|l| l.line).collect();
        assert_eq!(lines, [0, 4, 8]);
    }

    #[test]
    fn spans_cover_only_the_written_target() {
        let text = "x [[Folder/Note#Head|alias]] y\n`[[code]]` ![[ pic.png ]] [a](My%20Note.md#sec \"t\") [b](<Other Note.md>)";
        let spans = link_spans(text);
        let written: Vec<_> = spans.iter().map(|s| &text[s.range.clone()]).collect();
        assert_eq!(written, ["Folder/Note", "pic.png", "My%20Note.md", "Other Note.md"]);
        let styles: Vec<_> = spans.iter().map(|s| s.style).collect();
        assert_eq!(styles, [LinkStyle::Wiki, LinkStyle::Wiki, LinkStyle::Markdown, LinkStyle::MarkdownAngle]);
        assert_eq!(spans[2].link.target, "My Note.md");
    }

    #[test]
    fn spans_handle_crlf_and_escaped_pipe() {
        let text = "a\r\n| [[Note\\|alias]] |\r\n[[Last]]";
        let written: Vec<_> = link_spans(text).iter().map(|s| &text[s.range.clone()]).collect();
        assert_eq!(written, ["Note", "Last"]);
    }

    #[test]
    fn unclosed_inline_code_keeps_rest() {
        assert_eq!(targets("a ` b [[X]]"), ["X"]);
    }
}
