use std::collections::BTreeMap;

const APP_CACHE_VERSION: &str = "v19";

#[derive(Clone)]
pub struct LaunchableApp {
    pub name: String,
    pub path: String,
    pub source: String,
    pub folder: String,
}

#[derive(Clone)]
pub struct AppGroup {
    pub name: String,
    pub source: String,
    pub entries: Vec<LaunchableApp>,
}

pub fn path_is_absolute_exe(path: &str) -> bool {
    if !path.to_lowercase().ends_with(".exe") {
        return false;
    }
    let b = path.as_bytes();
    if b.len() < 3 {
        return false;
    }
    let has_drive = b[1] == b':' && (b[2] == b'\\' || b[2] == b'/');
    let is_unc = path.starts_with("\\\\");
    has_drive || is_unc
}

#[cfg(windows)]
pub fn scan_apps_native() -> Vec<AppGroup> {
    let mut groups: BTreeMap<String, AppGroup> = BTreeMap::new();

    fn add_entry(
        groups: &mut BTreeMap<String, AppGroup>,
        path: String,
        exe_display_name: String,
        label: Option<String>,
        source: String,
        folder: String,
    ) {
        let key = folder.to_lowercase();

        let group_name = match &label {
            Some(l) => l.clone(),
            None => std::path::Path::new(&folder)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(&folder)
                .to_string(),
        };

        let group = groups.entry(key).or_insert_with(|| AppGroup {
            name: group_name.clone(),
            source: source.clone(),
            entries: Vec::new(),
        });

        if label.is_some() {
            let current_is_folder_derived = std::path::Path::new(&folder)
                .file_name()
                .and_then(|s| s.to_str())
                .map(|f| group.name.eq_ignore_ascii_case(f))
                .unwrap_or(false);
            if current_is_folder_derived {
                group.name = group_name;
            }
        }

        let lower = path.to_lowercase();
        if !group.entries.iter().any(|e| e.path.to_lowercase() == lower) {
            group.entries.push(LaunchableApp {
                name: exe_display_name,
                path,
                source,
                folder,
            });
        }
    }

    // Source 1: Uninstall keys.
    if let Ok(apps) = crate::core::registry::enumerate_uninstall_entries() {
        for (name, path) in apps {
            if !path_is_absolute_exe(&path) {
                continue;
            }
            let folder = std::path::Path::new(&path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            if folder.is_empty() {
                continue;
            }
            add_entry(
                &mut groups,
                path,
                name.clone(),
                Some(name),
                "Installed".into(),
                folder,
            );
        }
    }

    // Source 2: App Paths.
    if let Ok(apps) = crate::core::registry::enumerate_app_paths() {
        for (name, path) in apps {
            if !path_is_absolute_exe(&path) {
                continue;
            }
            let folder = std::path::Path::new(&path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            if folder.is_empty() {
                continue;
            }
            add_entry(
                &mut groups,
                path,
                name.clone(),
                Some(name),
                "Registered".into(),
                folder,
            );
        }
    }

    let mut out: Vec<AppGroup> = groups.into_values().collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

#[cfg(not(windows))]
pub fn scan_apps_native() -> Vec<AppGroup> {
    Vec::new()
}

pub fn app_cache_path() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("hook_injector_apps_cache_{}.txt", APP_CACHE_VERSION));
    p
}

pub fn load_cached_apps() -> Option<Vec<AppGroup>> {
    let path = app_cache_path();
    let meta = std::fs::metadata(&path).ok()?;
    let age = meta.modified().ok()?.elapsed().ok()?;
    if age.as_secs() > 86400 {
        return None;
    }

    let text = std::fs::read_to_string(&path).ok()?;
    let mut groups: BTreeMap<String, AppGroup> = BTreeMap::new();

    for line in text.lines() {
        let mut parts = line.splitn(4, '\t');
        let group_name = parts.next()?.to_string();
        let exe_name = parts.next()?.to_string();
        let path = parts.next()?.to_string();
        let source = parts.next().unwrap_or("").to_string();
        if group_name.is_empty() || exe_name.is_empty() || path.is_empty() {
            continue;
        }
        if !path_is_absolute_exe(&path) {
            continue;
        }
        let folder = std::path::Path::new(&path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let key = folder.to_lowercase();
        let group = groups.entry(key).or_insert_with(|| AppGroup {
            name: group_name,
            source: source.clone(),
            entries: Vec::new(),
        });
        group.entries.push(LaunchableApp {
            name: exe_name,
            path,
            source,
            folder,
        });
    }

    if groups.is_empty() {
        None
    } else {
        Some(groups.into_values().collect())
    }
}

pub fn save_apps_cache(groups: &[AppGroup]) {
    let path = app_cache_path();
    let mut text = String::new();
    for g in groups {
        for e in &g.entries {
            text.push_str(&g.name);
            text.push('\t');
            text.push_str(&e.name);
            text.push('\t');
            text.push_str(&e.path);
            text.push('\t');
            text.push_str(&g.source);
            text.push('\n');
        }
    }
    let _ = std::fs::write(path, text);
}