//! The link graph: notes as nodes, resolved `[[links]]` as edges.
//! Links to attachments and to notes that don't exist yet become their own
//! node kinds so the UI can show or hide them (like Obsidian's graph filters).

use crate::index::{VaultIndex, title_of};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Note,
    Attachment,
    /// Linked to, but no such file exists (Obsidian's "unresolved" link).
    Missing,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GraphNode {
    /// Vault path, or `missing:<target>` for unresolved links.
    pub id: String,
    pub title: String,
    pub kind: NodeKind,
    /// Number of distinct notes linked to or from this node.
    pub degree: usize,
    /// The note's tags without `#` (for color groups like `tag:#work`).
    pub tags: Vec<String>,
}

/// A color group of the graph, in Obsidian's `.obsidian/graph.json`:
/// nodes matching `query` are drawn in `color`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColorGroup {
    pub query: String,
    /// `#rrggbb`
    pub color: String,
}

/// Obsidian writes colors as `{ "a": 1, "rgb": 16711680 }`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ObsidianColor {
    #[serde(default = "opaque")]
    a: f64,
    rgb: u32,
}

fn opaque() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ObsidianGroup {
    query: String,
    color: ObsidianColor,
}

const GRAPH_FILE: &str = "graph.json";
const GROUPS_KEY: &str = "colorGroups";

pub fn load_groups(root: &std::path::Path) -> Vec<ColorGroup> {
    crate::obsidian_config::read_key::<Vec<ObsidianGroup>>(root, GRAPH_FILE, GROUPS_KEY)
        .into_iter()
        .map(|g| ColorGroup { query: g.query, color: format!("#{:06x}", g.color.rgb & 0xff_ffff) })
        .collect()
}

pub fn save_groups(root: &std::path::Path, groups: &[ColorGroup]) -> Result<(), String> {
    let out: Vec<ObsidianGroup> = groups
        .iter()
        .map(|g| {
            let rgb = u32::from_str_radix(g.color.trim_start_matches('#'), 16)
                .map_err(|_| format!("잘못된 색: {}", g.color))?;
            Ok(ObsidianGroup { query: g.query.clone(), color: ObsidianColor { a: 1.0, rgb } })
        })
        .collect::<Result<_, String>>()?;
    crate::obsidian_config::write_key(root, GRAPH_FILE, GROUPS_KEY, &out)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct Graph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

const MISSING_PREFIX: &str = "missing:";

pub fn build(index: &VaultIndex) -> Graph {
    let mut nodes: BTreeMap<String, (String, NodeKind)> = BTreeMap::new();
    let mut edges: BTreeSet<GraphEdge> = BTreeSet::new();

    for (source, links) in index.note_links() {
        nodes.entry(source.to_string()).or_insert_with(|| (title_of(source), NodeKind::Note));
        for link in links {
            let (target, title, kind) = match index.resolve(source, &link.target) {
                Some(path) if path.to_ascii_lowercase().ends_with(".md") => {
                    let title = title_of(&path);
                    (path, title, NodeKind::Note)
                }
                Some(path) => {
                    let title = title_of(&path);
                    (path, title, NodeKind::Attachment)
                }
                None => {
                    // One node per distinct missing target, case-insensitively.
                    let name = link.target.rsplit('/').next().unwrap_or(&link.target).trim_end_matches(".md");
                    (format!("{MISSING_PREFIX}{}", link.target.to_lowercase()), name.to_string(), NodeKind::Missing)
                }
            };
            if target == source {
                continue;
            }
            nodes.entry(target.clone()).or_insert((title, kind));
            edges.insert(GraphEdge { source: source.to_string(), target });
        }
    }

    // Degree = distinct neighbours, regardless of direction.
    let mut neighbours: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for e in &edges {
        neighbours.entry(&e.source).or_default().insert(&e.target);
        neighbours.entry(&e.target).or_default().insert(&e.source);
    }
    let nodes = nodes
        .iter()
        .map(|(id, (title, kind))| GraphNode {
            id: id.clone(),
            title: title.clone(),
            kind: *kind,
            degree: neighbours.get(id.as_str()).map_or(0, BTreeSet::len),
            tags: index.note_tags(id).to_vec(),
        })
        .collect();

    Graph { nodes, edges: edges.into_iter().collect() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn graph_of(files: &[(&str, &str)]) -> Graph {
        let dir = tempfile::tempdir().unwrap();
        for (path, content) in files {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, content).unwrap();
        }
        build(&VaultIndex::build(dir.path()).unwrap())
    }

    fn node<'a>(g: &'a Graph, id: &str) -> &'a GraphNode {
        g.nodes.iter().find(|n| n.id == id).unwrap_or_else(|| panic!("no node {id}"))
    }

    #[test]
    fn notes_edges_and_degrees() {
        let g = graph_of(&[
            ("A.md", "[[B]] [[B|again]] [[Sub/C]] [[A]] [[#heading]]"),
            ("B.md", "back to [[A]]"),
            ("Sub/C.md", ""),
            ("Orphan.md", "no links"),
        ]);
        let edges: Vec<_> = g.edges.iter().map(|e| (e.source.as_str(), e.target.as_str())).collect();
        // duplicates collapsed, self-links and heading-only links dropped
        assert_eq!(edges, [("A.md", "B.md"), ("A.md", "Sub/C.md"), ("B.md", "A.md")]);
        assert_eq!(node(&g, "A.md").degree, 2); // B (both ways counts once) + C
        assert_eq!(node(&g, "B.md").degree, 1);
        assert_eq!(node(&g, "Sub/C.md").title, "C");
        assert_eq!(node(&g, "Orphan.md").degree, 0);
        assert!(g.nodes.iter().all(|n| n.kind == NodeKind::Note));
    }

    #[test]
    fn attachments_and_missing_targets() {
        let g = graph_of(&[
            ("A.md", "![[pic.png]] [[Not Yet]] [[not yet]] [[Folder/Later.md]]"),
            ("B.md", "[[Not Yet]]"),
            ("img/pic.png", "png"),
        ]);
        let pic = node(&g, "img/pic.png");
        assert_eq!((pic.kind, pic.title.as_str()), (NodeKind::Attachment, "pic.png"));
        // the same missing note from two notes / two spellings is one node
        let missing = node(&g, "missing:not yet");
        assert_eq!((missing.kind, missing.title.as_str(), missing.degree), (NodeKind::Missing, "Not Yet", 2));
        assert_eq!(node(&g, "missing:folder/later.md").title, "Later");
        // unlinked attachments don't appear
        assert_eq!(g.nodes.len(), 5);
    }

    #[test]
    fn nodes_carry_tags() {
        let g = graph_of(&[("A.md", "#work [[B]]"), ("B.md", "---\ntags: [idea]\n---\n")]);
        assert_eq!(node(&g, "A.md").tags, ["work"]);
        assert_eq!(node(&g, "B.md").tags, ["idea"]);
    }

    #[test]
    fn color_groups_round_trip_obsidian_format() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::write(
            root.join(".obsidian/graph.json"),
            r#"{"showTags": false, "colorGroups": [{"query": "path:Daily", "color": {"a": 1, "rgb": 14701138}}]}"#,
        )
        .unwrap();
        let groups = load_groups(root);
        assert_eq!(groups, [ColorGroup { query: "path:Daily".into(), color: "#e05252".into() }]);

        save_groups(root, &[ColorGroup { query: "tag:#work".into(), color: "#2a78d6".into() }]).unwrap();
        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join(".obsidian/graph.json")).unwrap()).unwrap();
        assert_eq!(json["showTags"], false);
        assert_eq!(json["colorGroups"][0]["color"]["rgb"], 0x2a78d6);
        assert!(save_groups(root, &[ColorGroup { query: "x".into(), color: "blue".into() }]).is_err());
    }
}
