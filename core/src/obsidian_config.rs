//! Reading and writing one key of a JSON settings file in `<vault>/.obsidian/`,
//! keeping every other key as it is (the files are shared with Obsidian).

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

fn file_path(root: &Path, file: &str) -> PathBuf {
    root.join(".obsidian").join(file)
}

/// `key` of `.obsidian/<file>`, or the default when the file or key is
/// missing or doesn't parse.
pub fn read_key<T: DeserializeOwned + Default>(root: &Path, file: &str, key: &str) -> T {
    fs::read_to_string(file_path(root, file))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get(key).cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

/// The whole of `.obsidian/<file>` as a JSON object (empty when missing).
/// Errs on a file that exists but isn't a JSON object, so it is never
/// overwritten.
fn load_map(path: &Path) -> Result<Map<String, Value>, String> {
    match fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => Ok(Map::new()),
        Ok(s) => match serde_json::from_str::<Value>(&s) {
            Ok(Value::Object(m)) => Ok(m),
            _ => Err(format!("{}을(를) 읽을 수 없어 저장하지 않았습니다.", path.display())),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn save_map(path: &Path, map: Map<String, Value>) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&Value::Object(map)).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| format!("{}: {e}", path.display()))
}

/// Every key of `.obsidian/<file>` (empty when missing or unreadable).
pub fn read_all(root: &Path, file: &str) -> Map<String, Value> {
    load_map(&file_path(root, file)).unwrap_or_default()
}

/// Set `key` in `.obsidian/<file>` (created if missing). Refuses to touch a
/// file that exists but isn't a JSON object, rather than overwrite it.
pub fn write_key<T: Serialize>(root: &Path, file: &str, key: &str, value: &T) -> Result<(), String> {
    let path = file_path(root, file);
    let mut map = load_map(&path)?;
    map.insert(key.to_string(), serde_json::to_value(value).map_err(|e| e.to_string())?);
    save_map(&path, map)
}

/// Remove `key` from `.obsidian/<file>`; nothing happens if it isn't there.
pub fn remove_key(root: &Path, file: &str, key: &str) -> Result<(), String> {
    let path = file_path(root, file);
    let mut map = load_map(&path)?;
    if map.remove(key).is_some() {
        save_map(&path, map)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_other_keys() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(read_key::<Vec<i32>>(root, "x.json", "a"), Vec::<i32>::new());
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::write(root.join(".obsidian/x.json"), r#"{"other": true, "a": [1]}"#).unwrap();
        assert_eq!(read_key::<Vec<i32>>(root, "x.json", "a"), [1]);
        write_key(root, "x.json", "a", &vec![2, 3]).unwrap();
        let v: Value = serde_json::from_str(&fs::read_to_string(root.join(".obsidian/x.json")).unwrap()).unwrap();
        assert_eq!(v, serde_json::json!({"other": true, "a": [2, 3]}));

        fs::write(root.join(".obsidian/bad.json"), "[1, 2]").unwrap();
        assert!(write_key(root, "bad.json", "a", &1).is_err());
        assert_eq!(fs::read_to_string(root.join(".obsidian/bad.json")).unwrap(), "[1, 2]");
        assert!(remove_key(root, "bad.json", "a").is_err());

        remove_key(root, "x.json", "a").unwrap();
        remove_key(root, "none.json", "a").unwrap();
        assert!(!root.join(".obsidian/none.json").exists());
        assert_eq!(read_all(root, "x.json"), serde_json::json!({"other": true}).as_object().unwrap().clone());
    }
}
