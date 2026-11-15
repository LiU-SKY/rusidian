//! Rusidian core: everything that does not depend on the UI shell.
//! Kept free of Tauri so it builds fast and its tests run anywhere.

pub mod calendar;
pub mod calendars;
pub mod daily;
pub mod date;
pub mod fileops;
pub mod assets;
pub mod attachments;
pub mod bookmarks;
pub mod finance;
pub mod fixed;
pub mod finance_import;
pub mod frontmatter;
pub mod gitsync;
pub mod graph;
pub mod history;
pub mod hotkeys;
pub mod ics;
pub mod index;
pub mod links;
pub mod mentions;
pub mod merge;
pub mod moment;
pub mod note_settings;
pub mod markdown;
pub mod obsidian_config;
pub mod phone;
pub mod recent;
pub mod recur;
pub mod statement;
pub mod sync;
pub mod tags;
pub mod templates;
pub mod tasks;
pub mod vault;
pub mod watch;
