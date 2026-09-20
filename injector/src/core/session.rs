//! Project system. Every "session" the user works in is a project,
//! stored as a folder under `%USERPROFILE%\Documents\Sigil\Projects\`.
//!
//! Folder layout:
//! ```text
//! <project>/
//!   project.json     ← metadata + appearance snapshot + active hooks
//!   changelog.json   ← append-only list of renames / patches / changes
//! ```
//!
//! Both files are written on every mutation when autosave is on.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::settings::Settings;
use crate::theme::{Accent, Density, ThemeMode};

// ============================================================
// DATA TYPES
// ============================================================

/// Snapshot of the appearance-related settings. Paths and behavior
/// settings stay global — a project should not silently rewrite where
/// your logs go.
#[derive(Clone, Serialize, Deserialize)]
pub struct AppearanceSnapshot {
    pub theme_mode: ThemeMode,
    pub accent: Accent,
    pub density: Density,
    pub corner_radius: f32,
    pub font_scale: f32,
}

impl From<&Settings> for AppearanceSnapshot {
    fn from(s: &Settings) -> Self {
        Self {
            theme_mode: s.theme_mode,
            accent: s.accent,
            density: s.density,
            corner_radius: s.corner_radius,
            font_scale: s.font_scale,
        }
    }
}

/// Info about the binary the project was captured against.
/// `sha256` is optional — we don't compute it yet, but the field is
/// here so files written by future versions are still readable.
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct TargetInfo {
    pub path: Option<String>,
    pub pid_at_capture: Option<u32>,
    pub sha256: Option<String>,
}

/// The `project.json` payload.
#[derive(Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub created: String,
    pub modified: String,

    pub target: TargetInfo,
    pub appearance: AppearanceSnapshot,
    pub active_hooks: Vec<String>,

    /// Free-form notes field. Shown in the Sessions view and saved
    /// to disk alongside everything else.
    #[serde(default)]
    pub notes: String,

    /// Not serialized — the folder on disk this project lives in.
    /// Populated on load and used by `save()`.
    #[serde(skip)]
    pub folder: PathBuf,
}

/// A single line in `changelog.json`. Append-only.
#[derive(Clone, Serialize, Deserialize)]
pub struct ChangelogEntry {
    pub timestamp: String,
    pub kind: ChangelogKind,
    pub detail: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangelogKind {
    Created,
    Opened,
    Renamed,
    SettingChanged,
    Patched,
    AddressNamed,
    HookAdded,
    HookRemoved,
    Note,
    Saved,
}

impl ChangelogKind {
    pub fn label(&self) -> &'static str {
        match self {
            ChangelogKind::Created => "created",
            ChangelogKind::Opened => "opened",
            ChangelogKind::Renamed => "renamed",
            ChangelogKind::SettingChanged => "setting",
            ChangelogKind::Patched => "patch",
            ChangelogKind::AddressNamed => "rename",
            ChangelogKind::HookAdded => "hook +",
            ChangelogKind::HookRemoved => "hook -",
            ChangelogKind::Note => "note",
            ChangelogKind::Saved => "saved",
        }
    }
}

// ============================================================
// PATHS
// ============================================================

/// Where projects live. Respects `settings.projects_dir` if set,
/// otherwise falls back to `%USERPROFILE%\Documents\Sigil\Projects`.
pub fn projects_root(settings: &Settings) -> PathBuf {
    if !settings.projects_dir.trim().is_empty() {
        PathBuf::from(&settings.projects_dir)
    } else {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
        PathBuf::from(home)
            .join("Documents")
            .join("Sigil")
            .join("Projects")
    }
}

pub fn project_json_path(folder: &Path) -> PathBuf {
    folder.join("project.json")
}

pub fn changelog_path(folder: &Path) -> PathBuf {
    folder.join("changelog.json")
}

// ============================================================
// TIMESTAMPS
// ============================================================

/// ISO-8601-ish timestamp without pulling in chrono. Reads local
/// wall-clock from `SystemTime` and formats as `YYYY-MM-DD HH:MM:SS`.
pub fn now_string() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    // We don't have a real calendar, so approximate. Good enough for
    // a changelog, and consistent across runs.
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Days since 1970-01-01, in a simple civil-time approximation.
    let days = secs / 86400;
    let secs_of_day = secs % 86400;
    let (h, m, s) = (secs_of_day / 3600, (secs_of_day % 3600) / 60, secs_of_day % 60);

    // Convert days-since-epoch to Y-M-D. Based on Howard Hinnant's
    // days_from_civil, inverted.
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m_ = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m_ <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        y, m_, d, h, m, s
    )
}

// ============================================================
// PROJECT LIFECYCLE
// ============================================================

/// Create a new project on disk from a name and the current settings.
pub fn create_project(
    settings: &Settings,
    name: &str,
) -> Result<Project, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("project name is empty".into());
    }

    let safe = sanitize_folder_name(trimmed);
    if safe.is_empty() {
        return Err("project name contains no valid characters".into());
    }

    let root = projects_root(settings);
    let folder = root.join(&safe);

    if folder.exists() {
        return Err(format!(
            "a project named '{}' already exists at {}",
            trimmed,
            folder.display()
        ));
    }

    fs::create_dir_all(&folder)
        .map_err(|e| format!("could not create project folder: {}", e))?;

    let now = now_string();
    let project = Project {
        name: trimmed.to_string(),
        created: now.clone(),
        modified: now.clone(),
        target: TargetInfo::default(),
        appearance: AppearanceSnapshot::from(settings),
        active_hooks: Vec::new(),
        notes: String::new(),
        folder: folder.clone(),
    };

    write_project_json(&project)?;

    // Start the changelog with a single "created" entry.
    let first = ChangelogEntry {
        timestamp: now,
        kind: ChangelogKind::Created,
        detail: format!("project '{}' created", trimmed),
    };
    write_changelog(&folder, &[first])?;

    Ok(project)
}

/// Load a project from a folder on disk.
pub fn load_project(folder: &Path) -> Result<Project, String> {
    let path = project_json_path(folder);
    let text = fs::read_to_string(&path)
        .map_err(|e| format!("could not read {}: {}", path.display(), e))?;

    let mut project: Project = serde_json::from_str(&text)
        .map_err(|e| format!("could not parse {}: {}", path.display(), e))?;

    project.folder = folder.to_path_buf();

    // Sanity-check the name against the folder if the JSON is missing
    // a name (corrupt file).
    if project.name.trim().is_empty() {
        project.name = folder
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed")
            .to_string();
    }

    Ok(project)
}

/// Write `project.json` to disk. Updates `modified` first.
pub fn save_project(project: &mut Project) -> Result<(), String> {
    project.modified = now_string();
    write_project_json(project)
}

fn write_project_json(project: &Project) -> Result<(), String> {
    let path = project_json_path(&project.folder);
    let text = serde_json::to_string_pretty(project)
        .map_err(|e| format!("serialize failed: {}", e))?;
    fs::write(&path, text)
        .map_err(|e| format!("write failed for {}: {}", path.display(), e))
}

/// Append one entry to `changelog.json`. Read-modify-write; the file
/// is small, so this is fine.
pub fn append_changelog(folder: &Path, entry: ChangelogEntry) -> Result<(), String> {
    let path = changelog_path(folder);
    let mut entries = if path.exists() {
        let text = fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str::<Vec<ChangelogEntry>>(&text).unwrap_or_default()
    } else {
        Vec::new()
    };

    entries.push(entry);
    write_changelog(folder, &entries)
}

fn write_changelog(folder: &Path, entries: &[ChangelogEntry]) -> Result<(), String> {
    let path = changelog_path(folder);
    let text = serde_json::to_string_pretty(entries)
        .map_err(|e| format!("serialize changelog failed: {}", e))?;
    fs::write(&path, text)
        .map_err(|e| format!("write changelog failed for {}: {}", path.display(), e))
}

/// Read the full changelog for a project. Returns empty on missing
/// or corrupt files instead of erroring — the changelog is a
/// nice-to-have, not critical.
pub fn read_changelog(folder: &Path) -> Vec<ChangelogEntry> {
    let path = changelog_path(folder);
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<ChangelogEntry>>(&text).unwrap_or_default()
}

/// List every project folder under the projects root.
pub fn list_projects(settings: &Settings) -> Vec<ProjectSummary> {
    let root = projects_root(settings);
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };

    let mut out: Vec<ProjectSummary> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let folder = e.path();
            let project = load_project(&folder).ok()?;
            Some(ProjectSummary {
                name: project.name.clone(),
                folder,
                modified: project.modified.clone(),
                created: project.created.clone(),
                target_path: project.target.path.clone(),
            })
        })
        .collect();

    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    out
}

#[derive(Clone)]
pub struct ProjectSummary {
    pub name: String,
    pub folder: PathBuf,
    pub modified: String,
    pub created: String,
    pub target_path: Option<String>,
}

/// Delete a project folder and everything in it.
pub fn delete_project(folder: &Path) -> Result<(), String> {
    fs::remove_dir_all(folder)
        .map_err(|e| format!("could not delete {}: {}", folder.display(), e))
}

/// Rename a project: updates `project.json` and appends a changelog
/// entry. Does NOT rename the folder on disk — keeps the path stable
/// so any open files or bookmarks keep working.
pub fn rename_project(project: &mut Project, new_name: &str) -> Result<(), String> {
    let trimmed = new_name.trim();
    if trimmed.is_empty() {
        return Err("new name is empty".into());
    }
    let old = project.name.clone();
    project.name = trimmed.to_string();
    save_project(project)?;
    append_changelog(
        &project.folder,
        ChangelogEntry {
            timestamp: now_string(),
            kind: ChangelogKind::Renamed,
            detail: format!("renamed from '{}' to '{}'", old, trimmed),
        },
    )?;
    Ok(())
}

// ============================================================
// HELPERS
// ============================================================

/// Strip characters that are illegal in Windows folder names.
fn sanitize_folder_name(name: &str) -> String {
    const BAD: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    let cleaned: String = name
        .chars()
        .map(|c| if BAD.contains(&c) || c.is_control() { '_' } else { c })
        .collect();
    cleaned.trim().trim_end_matches('.').to_string()
}
// ============================================================
// SCRIPTS
// ============================================================

/// One script in a project. The source is a full SigilScript program
/// (or a partial — the interpreter handles top-level statements).
///
/// A project's scripts live in `scripts.json` inside the project
/// folder. The list is read on open and written on every edit.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Script {
    /// Short, unique within the project. Used as the display name and
    /// the label in the sidebar.
    pub name: String,

    /// Optional author. Free text.
    #[serde(default)]
    pub author: String,

    /// One-line description shown in the library list.
    #[serde(default)]
    pub description: String,

    /// Free-form tags for filtering. Unused in v1, kept for future.
    #[serde(default)]
    pub tags: Vec<String>,

    /// The full source code of the script.
    #[serde(default)]
    pub source: String,

    /// ISO-8601-ish timestamp. Set on first write; updated on save.
    #[serde(default)]
    pub modified: String,

    /// Whether the script should run automatically when the project
    /// opens. Unused in v1.
    #[serde(default)]
    pub enabled: bool,
}

impl Script {
    /// A starter script with a header comment and a template loop.
    /// Handed out when the user creates a new script.
    pub fn template(name: &str) -> Self {
        let source = format!(
            r#"# @name        {name}
# @author      you
# @desc        Describe what this script does
# @version     1.0.0
#
# SigilScript runs on the memory of the attached process. Every
# `read`/`write`/`freeze` call goes through the Memory view's handle.
#
# Example: pin HP to 9999 while the script runs.
#
# addr hp = 0x7ff6a2c0 : int32
#
# loop every 500ms:
#     if read(hp) < 100:
#         write(hp, 9999)
#         log("HP reset to 9999")

log("hello from '{name}'")
"#,
            name = name
        );

        Self {
            name: name.to_string(),
            author: String::new(),
            description: String::new(),
            tags: Vec::new(),
            source,
            modified: now_string(),
            enabled: false,
        }
    }
}

/// `<project>/scripts.json`
pub fn scripts_path(folder: &Path) -> PathBuf {
    folder.join("scripts.json")
}

/// Read every script in a project. Returns empty on missing file.
pub fn read_scripts(folder: &Path) -> Vec<Script> {
    let path = scripts_path(folder);
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Write the full script list. Called on every edit when autosave is
/// on. Same autosave discipline as the project file itself.
pub fn write_scripts(folder: &Path, scripts: &[Script]) -> Result<(), String> {
    let path = scripts_path(folder);
    let text = serde_json::to_string_pretty(scripts)
        .map_err(|e| format!("serialize scripts failed: {}", e))?;
    fs::write(&path, text)
        .map_err(|e| format!("write scripts failed for {}: {}", path.display(), e))
}