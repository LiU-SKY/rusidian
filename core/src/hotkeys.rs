//! Custom hotkeys, in Obsidian's own file `.obsidian/hotkeys.json`:
//! `{"command-id": [{"modifiers": ["Mod", "Shift"], "key": "P"}]}`.
//! A command listed there uses those keys instead of its defaults (an empty
//! list: no key at all); commands not listed keep their defaults. Entries
//! for commands Rusidian doesn't know are kept as they are.

use crate::obsidian_config;
use std::collections::BTreeMap;
use std::path::Path;

const FILE: &str = "hotkeys.json";

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct Hotkey {
    /// "Mod" (Ctrl, or Cmd on macOS), "Ctrl", "Meta", "Shift", "Alt".
    #[serde(default)]
    pub modifiers: Vec<String>,
    pub key: String,
}

const MODIFIERS: [&str; 5] = ["Mod", "Ctrl", "Meta", "Shift", "Alt"];

/// Every readable entry; ones in another shape are left out (and kept on disk).
pub fn read(root: &Path) -> BTreeMap<String, Vec<Hotkey>> {
    obsidian_config::read_all(root, FILE)
        .into_iter()
        .filter_map(|(id, v)| Some((id, serde_json::from_value(v).ok()?)))
        .collect()
}

/// Give `id` these keys (None: back to its defaults).
pub fn set(root: &Path, id: &str, keys: Option<&[Hotkey]>) -> Result<(), String> {
    let Some(keys) = keys else {
        return obsidian_config::remove_key(root, FILE, id);
    };
    for k in keys {
        if k.key.trim().is_empty() || k.modifiers.iter().any(|m| !MODIFIERS.contains(&m.as_str())) {
            return Err(format!("단축키가 올바르지 않습니다: {} {}", k.modifiers.join("+"), k.key));
        }
    }
    obsidian_config::write_key(root, FILE, id, &keys)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn key(mods: &[&str], key: &str) -> Hotkey {
        Hotkey { modifiers: mods.iter().map(|m| m.to_string()).collect(), key: key.into() }
    }

    #[test]
    fn reads_and_writes_obsidian_hotkeys() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert!(read(root).is_empty());
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::write(
            root.join(".obsidian/hotkeys.json"),
            r#"{"editor:toggle-bold": [{"modifiers": ["Mod"], "key": "B"}], "odd": "x", "graph:open": []}"#,
        )
        .unwrap();
        let all = read(root);
        assert_eq!(all["editor:toggle-bold"], [key(&["Mod"], "B")]);
        assert!(all["graph:open"].is_empty());
        assert!(!all.contains_key("odd"));

        set(root, "switcher:open", Some(&[key(&["Mod", "Shift"], "K")])).unwrap();
        set(root, "graph:open", None).unwrap();
        assert!(set(root, "x", Some(&[key(&["Hyper"], "K")])).is_err());
        assert!(set(root, "x", Some(&[key(&["Mod"], " ")])).is_err());
        let all = read(root);
        assert_eq!(all["switcher:open"], [key(&["Mod", "Shift"], "K")]);
        assert!(!all.contains_key("graph:open"));
        assert!(!all.contains_key("x"));
        // Obsidian's other entries are untouched.
        let raw = fs::read_to_string(root.join(".obsidian/hotkeys.json")).unwrap();
        assert!(raw.contains("editor:toggle-bold") && raw.contains("\"odd\""));
    }
}
