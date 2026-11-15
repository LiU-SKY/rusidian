//! "Unlinked mentions": a note's title (or alias) written as plain text in
//! another note, and turning one into a `[[link]]`.

use regex::Regex;
use std::sync::LazyLock;

/// Parts of a line that are already links, code or URLs: a mention there doesn't count.
static PROTECTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"!?\[\[[^\]\n]*\]\]|!?\[[^\]\n]*\]\([^)\n]*\)|`[^`\n]*`|https?://\S+").unwrap());

fn same_letter(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// Byte range of the first plain-text occurrence of `term` in `line`
/// (case-insensitive). A term starting or ending with an ASCII letter or
/// digit must not run into one ("Rust" is not in "Rusty"); Korean and
/// other text matches anywhere, like Obsidian.
pub fn find_mention(line: &str, term: &str) -> Option<(usize, usize)> {
    let term: Vec<char> = term.chars().collect();
    if term.is_empty() {
        return None;
    }
    let blocked: Vec<(usize, usize)> = PROTECTED.find_iter(line).map(|m| (m.start(), m.end())).collect();
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let word = |c: char| c.is_ascii_alphanumeric();
    for i in 0..chars.len() {
        if i + term.len() > chars.len() {
            break;
        }
        if !term.iter().enumerate().all(|(k, &t)| same_letter(chars[i + k].1, t)) {
            continue;
        }
        let start = chars[i].0;
        let end = chars.get(i + term.len()).map_or(line.len(), |c| c.0);
        if word(term[0]) && i > 0 && word(chars[i - 1].1) {
            continue;
        }
        if word(*term.last().unwrap()) && chars.get(i + term.len()).is_some_and(|c| word(c.1)) {
            continue;
        }
        if blocked.iter().any(|&(a, b)| start < b && end > a) {
            continue;
        }
        return Some((start, end));
    }
    None
}

/// `line` with its first plain mention of `term` linked to `target`:
/// `[[target]]` when the text is exactly the target, else `[[target|text]]`.
pub fn link_mention(line: &str, term: &str, target: &str) -> Option<String> {
    let (a, b) = find_mention(line, term)?;
    let text = &line[a..b];
    let link = if text == target { format!("[[{target}]]") } else { format!("[[{target}|{text}]]") };
    Some(format!("{}{link}{}", &line[..a], &line[b..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_plain_mentions_only() {
        assert_eq!(find_mention("오늘 회의록 정리", "회의록"), Some((7, 16)));
        assert_eq!(find_mention("[[회의록]] 보고 회의록 다시", "회의록"), Some((21, 30)));
        assert_eq!(find_mention("[[회의록|기록]] 과 `회의록`", "회의록"), None);
        assert_eq!(find_mention("see [회의록](회의록.md)", "회의록"), None);
        assert_eq!(find_mention("https://x.com/회의록", "회의록"), None);
        // ASCII words need word edges; case is ignored.
        assert_eq!(find_mention("Rusty and rust", "Rust"), Some((10, 14)));
        assert_eq!(find_mention("Rustacean", "Rust"), None);
        assert_eq!(find_mention("C++ 공부", "C++"), Some((0, 3)));
        // Korean matches inside longer words, as in Obsidian.
        assert_eq!(find_mention("회의록을 보다", "회의록").map(|(a, _)| a), Some(0));
        assert_eq!(find_mention("", "x"), None);
        assert_eq!(find_mention("abc", ""), None);
    }

    #[test]
    fn links_a_mention() {
        assert_eq!(link_mention("오늘 회의록 정리", "회의록", "회의록").as_deref(), Some("오늘 [[회의록]] 정리"));
        assert_eq!(link_mention("about rust here", "Rust", "Rust").as_deref(), Some("about [[Rust|rust]] here"));
        assert_eq!(link_mention("별칭 미팅 메모", "미팅", "회의록").as_deref(), Some("별칭 [[회의록|미팅]] 메모"));
        assert_eq!(link_mention("[[회의록]]만", "회의록", "회의록"), None);
    }
}
