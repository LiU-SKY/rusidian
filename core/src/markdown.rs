//! Small markdown helpers shared by the link and task parsers.

/// Lines outside fenced code blocks, as `(0-based line number, line)`.
/// Fence lines themselves are skipped too.
pub fn prose_lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut fence: Option<(char, usize)> = None;
    text.lines().enumerate().filter(move |(_, line)| {
        if let Some(f) = fence_marker(line) {
            match fence {
                None => fence = Some(f),
                // A fence closes with the same char and at least the same length.
                Some((c, n)) if f.0 == c && f.1 >= n => fence = None,
                Some(_) => {}
            }
            return false;
        }
        fence.is_none()
    })
}

/// A code fence (3+ backticks or tildes) at the start of a line,
/// allowing up to 3 spaces of indent. Returns the fence char and length.
fn fence_marker(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let c = trimmed.chars().next()?;
    if c != '`' && c != '~' {
        return None;
    }
    let n = trimmed.chars().take_while(|&x| x == c).count();
    (n >= 3).then_some((c, n))
}

/// Lines with their terminators ("\n", "\r\n" or "" for the last line).
pub fn split_lines(text: &str) -> Vec<(&str, &str)> {
    text.split_inclusive('\n')
        .map(|piece| {
            if let Some(c) = piece.strip_suffix("\r\n") {
                (c, "\r\n")
            } else if let Some(c) = piece.strip_suffix('\n') {
                (c, "\n")
            } else {
                (piece, "")
            }
        })
        .collect()
}

/// Line `line_no` of `text`, if it still reads `expected` (the UI's copy).
pub fn checked_line<'a>(lines: &[(&'a str, &'a str)], line_no: usize, expected: &str) -> Result<&'a str, String> {
    let Some(&(current, _)) = lines.get(line_no) else {
        return Err("그 줄이 사라졌습니다. 목록을 새로고침하세요.".into());
    };
    if current != expected {
        return Err("그사이 파일이 바뀌었습니다. 목록을 새로고침한 뒤 다시 시도하세요.".into());
    }
    Ok(current)
}

/// `text` with line `line_no` replaced by `new` (or removed when `None`),
/// refused unless the line still reads `expected`. Line endings are kept.
pub fn replace_line(text: &str, line_no: usize, expected: &str, new: Option<&str>) -> Result<String, String> {
    let lines = split_lines(text);
    checked_line(&lines, line_no, expected)?;
    let mut out = String::with_capacity(text.len() + 64);
    for (i, (content, end)) in lines.iter().enumerate() {
        if i != line_no {
            out.push_str(content);
            out.push_str(end);
        } else if let Some(n) = new {
            out.push_str(n);
            out.push_str(end);
        }
    }
    Ok(out)
}

/// Replace inline code spans with spaces (same byte length), so links and
/// tags inside them are not matched.
pub fn blank_inline_code(line: &str) -> String {
    if !line.contains('`') {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(start) = rest.find('`') {
        out.push_str(&rest[..start]);
        let ticks = rest[start..].chars().take_while(|&c| c == '`').count();
        let after = &rest[start + ticks..];
        let closer = "`".repeat(ticks);
        match after.find(&closer) {
            Some(end) => {
                let span_len = ticks + end + ticks;
                out.extend(std::iter::repeat_n(' ', span_len));
                rest = &rest[start + span_len..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_fenced_blocks() {
        let text = "a\n```js\ncode\n```\nb\n~~~~\n```\nstill code\n~~~~\nc";
        let lines: Vec<_> = prose_lines(text).collect();
        assert_eq!(lines, [(0, "a"), (4, "b"), (9, "c")]);
    }
}
