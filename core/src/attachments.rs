//! Pasted and dropped files saved as attachments the way Obsidian does it:
//! where they go (`attachmentFolderPath` in `.obsidian/app.json`), what they
//! are called ("Pasted image 20261114153012.png"), and the text that embeds
//! them in the note (`![[…]]`, or a markdown link with `useMarkdownLinks`).

use std::path::Path;

const APP_FILE: &str = "app.json";

/// Files larger than this are refused (a note is not a file store).
pub const MAX_BYTES: usize = 50 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkFormat {
    /// The file name when it is unique in the vault, else the vault path.
    Shortest,
    /// A path relative to the note.
    Relative,
    /// The vault path.
    Absolute,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// "/" (vault root), "./" (the note's folder), "./sub" (a folder next to
    /// the note) or a vault folder.
    pub folder: String,
    pub markdown_links: bool,
    pub format: LinkFormat,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { folder: "/".into(), markdown_links: false, format: LinkFormat::Shortest }
    }
}

/// Obsidian's attachment settings (its defaults when unset).
pub fn load(root: &Path) -> Settings {
    use crate::obsidian_config::read_key;
    let folder: Option<String> = read_key(root, APP_FILE, "attachmentFolderPath");
    let markdown: Option<bool> = read_key(root, APP_FILE, "useMarkdownLinks");
    let format: Option<String> = read_key(root, APP_FILE, "newLinkFormat");
    Settings {
        folder: folder.unwrap_or_else(|| "/".into()),
        markdown_links: markdown.unwrap_or(false),
        format: match format.as_deref() {
            Some("relative") => LinkFormat::Relative,
            Some("absolute") => LinkFormat::Absolute,
            _ => LinkFormat::Shortest,
        },
    }
}

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

fn join(dir: &str, name: &str) -> String {
    match (dir.trim_matches('/'), name.trim_matches('/')) {
        ("", n) => n.to_string(),
        (d, "") => d.to_string(),
        (d, n) => format!("{d}/{n}"),
    }
}

/// The vault folder ("" = root) a new attachment of `note` goes to.
pub fn folder_for(settings: &Settings, note: &str) -> String {
    let f = settings.folder.trim().replace('\\', "/");
    if f.is_empty() || f == "/" {
        String::new()
    } else if f == "." || f == "./" {
        dir_of(note).to_string()
    } else if let Some(sub) = f.strip_prefix("./") {
        join(dir_of(note), sub)
    } else {
        f.trim_matches('/').to_string()
    }
}

/// File extension for a pasted image's MIME type.
pub fn image_extension(mime: &str) -> Option<&'static str> {
    Some(match mime.trim().to_ascii_lowercase().as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/svg+xml" => "svg",
        "image/avif" => "avif",
        _ => return None,
    })
}

/// A safe file name from what the system gave: no folders, none of the
/// characters Windows or Obsidian links can't take, not hidden, not too long.
/// Returns (stem, ".ext").
pub fn clean_name(name: &str) -> Option<(String, String)> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .map(|c| if "<>:\"|?*#^[]".contains(c) || c.is_control() { ' ' } else { c })
        .collect();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned = cleaned.trim_start_matches('.').trim_end_matches(['.', ' ']);
    let (stem, ext) = match cleaned.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && !e.is_empty() && e.len() <= 10 && !e.contains(' ') => (s, format!(".{}", e.to_lowercase())),
        _ => (cleaned, String::new()),
    };
    let stem: String = stem.chars().take(120).collect();
    let stem = stem.trim().to_string();
    (!stem.is_empty()).then_some((stem, ext))
}

/// Shown inline with `![[…]]`; anything else gets a plain `[[…]]` link.
fn embeddable(path: &str) -> bool {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "avif"
            | "pdf" | "mp3" | "wav" | "m4a" | "ogg" | "flac" | "webm" | "mp4" | "mov" | "mkv"
    )
}

/// `to` (a vault path) as seen from folder `from`.
fn relative(from: &str, to: &str) -> String {
    let from: Vec<&str> = from.split('/').filter(|s| !s.is_empty()).collect();
    let to: Vec<&str> = to.split('/').filter(|s| !s.is_empty()).collect();
    let same = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<&str> = vec![".."; from.len() - same];
    parts.extend(&to[same..]);
    parts.join("/")
}

/// The text that embeds (or links) the saved file `path` in `note`.
/// `name_unique`: no other file in the vault has the same file name.
pub fn embed_text(settings: &Settings, path: &str, note: &str, name_unique: bool) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    let target = match settings.format {
        LinkFormat::Shortest if name_unique => name.to_string(),
        LinkFormat::Relative => relative(dir_of(note), path),
        _ => path.to_string(),
    };
    let bang = if embeddable(path) { "!" } else { "" };
    if settings.markdown_links {
        let encoded = target.replace('%', "%25").replace(' ', "%20").replace('(', "%28").replace(')', "%29");
        format!("{bang}[]({encoded})")
    } else {
        format!("{bang}[[{target}]]")
    }
}

/// Undo `encodeURIComponent` (headers carry the note path and file name).
pub fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(folder: &str, markdown: bool, format: LinkFormat) -> Settings {
        Settings { folder: folder.into(), markdown_links: markdown, format }
    }

    #[test]
    fn folder_follows_obsidian_setting() {
        let note = "업무/회의/1114.md";
        assert_eq!(folder_for(&settings("/", false, LinkFormat::Shortest), note), "");
        assert_eq!(folder_for(&settings("", false, LinkFormat::Shortest), note), "");
        assert_eq!(folder_for(&settings("./", false, LinkFormat::Shortest), note), "업무/회의");
        assert_eq!(folder_for(&settings("./attachments", false, LinkFormat::Shortest), note), "업무/회의/attachments");
        assert_eq!(folder_for(&settings("./attachments", false, LinkFormat::Shortest), "top.md"), "attachments");
        assert_eq!(folder_for(&settings("첨부/이미지/", false, LinkFormat::Shortest), note), "첨부/이미지");
    }

    #[test]
    fn names_are_safe() {
        assert_eq!(clean_name("C:\\Users\\me\\사진 #1 [최종].PNG"), Some(("사진 1 최종".into(), ".png".into())));
        assert_eq!(clean_name("../../etc/passwd"), Some(("passwd".into(), String::new())));
        assert_eq!(clean_name(".hidden.jpg"), Some(("hidden".into(), ".jpg".into())));
        assert_eq!(clean_name("a:b|c?.txt"), Some(("a b c".into(), ".txt".into())));
        assert_eq!(clean_name("..."), None);
        assert_eq!(clean_name(""), None);
        assert_eq!(image_extension("image/jpeg"), Some("jpg"));
        assert_eq!(image_extension("application/pdf"), None);
    }

    #[test]
    fn embed_text_matches_obsidian() {
        let wiki = settings("/", false, LinkFormat::Shortest);
        assert_eq!(embed_text(&wiki, "Pasted image 20261114153012.png", "a/b.md", true), "![[Pasted image 20261114153012.png]]");
        // Another file has the same name: the path keeps the link unambiguous.
        assert_eq!(embed_text(&wiki, "a/photo.png", "a/b.md", false), "![[a/photo.png]]");
        // Not embeddable: a plain link.
        assert_eq!(embed_text(&wiki, "보고서.docx", "a/b.md", true), "[[보고서.docx]]");
        let md = settings("/", true, LinkFormat::Relative);
        assert_eq!(embed_text(&md, "img/Pasted image 1.png", "notes/x/b.md", true), "![](../../img/Pasted%20image%201.png)");
        assert_eq!(embed_text(&settings("/", true, LinkFormat::Absolute), "img/a (1).png", "b.md", true), "![](img/a%20%281%29.png)");
        assert_eq!(embed_text(&settings("/", false, LinkFormat::Relative), "a/x.pdf", "a/b.md", true), "![[x.pdf]]");
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("%EA%B0%80%EA%B3%84%EB%B6%80%2F2026-11.md").as_deref(), Some("가계부/2026-11.md"));
        assert_eq!(percent_decode("plain").as_deref(), Some("plain"));
        assert_eq!(percent_decode("%ZZ"), None);
        assert_eq!(percent_decode("%E"), None);
    }

    #[test]
    fn settings_default_and_load() {
        let dir = std::env::temp_dir().join(format!("rusidian-attach-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".obsidian")).unwrap();
        assert_eq!(load(&dir), Settings::default());
        std::fs::write(
            dir.join(".obsidian/app.json"),
            r#"{"attachmentFolderPath":"./첨부","useMarkdownLinks":true,"newLinkFormat":"relative","other":1}"#,
        )
        .unwrap();
        assert_eq!(load(&dir), settings("./첨부", true, LinkFormat::Relative));
        std::fs::remove_dir_all(&dir).ok();
    }
}
