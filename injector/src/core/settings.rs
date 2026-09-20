use std::fs;
use std::path::PathBuf;

use crate::theme::{Accent, Density, Theme, ThemeMode};

#[derive(Clone)]
pub struct Settings {
    // Appearance
    pub theme_mode: ThemeMode,
    pub accent: Accent,
    pub density: Density,
    pub corner_radius: f32,
    pub font_scale: f32,

    // Paths
    pub projects_dir: String,
    pub dumps_dir: String,
    pub logs_dir: String,
    pub dll_search_dir: String,
    pub temp_dir: String,

    // Behavior
    pub confirm_inject: bool,
    pub confirm_kill: bool,
    pub restore_last_view: bool,
    pub autosave_minutes: u32,
    pub startup_log_level: String,
    pub project_autosave: bool,

    // Advanced
    pub scan_depth: String,
    pub pipe_buffer_kb: u32,
    pub inject_timeout_sec: u32,
    pub hooks_json_path: String,
    pub external_editor_command: String,
}

impl Default for Settings {
    fn default() -> Self {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
        let sigil_root = format!("{}\\Documents\\Sigil", home);
        let temp_default = format!(
            "{}\\Sigil",
            std::env::var("TEMP").unwrap_or_else(|_| "C:\\Temp".into())
        );

        Self {
            theme_mode: ThemeMode::Dark,
            accent: Accent::Violet,
            density: Density::Comfortable,
            corner_radius: 12.0,
            font_scale: 1.0,

            projects_dir: format!("{}\\Projects", sigil_root),
            dumps_dir: format!("{}\\Dumps", sigil_root),
            logs_dir: format!("{}\\Logs", sigil_root),
            dll_search_dir: String::new(),
            temp_dir: temp_default,

            confirm_inject: true,
            confirm_kill: true,
            restore_last_view: true,
            autosave_minutes: 5,
            startup_log_level: "info".into(),
            project_autosave: true,

            scan_depth: "registry".into(),
            pipe_buffer_kb: 64,
            inject_timeout_sec: 30,
            hooks_json_path: String::new(),
            external_editor_command: "notepad".into(),
        }
    }
}

impl Settings {
    pub fn config_dir() -> PathBuf {
        let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
        PathBuf::from(base).join("Sigil")
    }

    pub fn config_path() -> PathBuf {
        Self::config_dir().join("settings.txt")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        let mut s = Self::default();

        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => return s,
        };

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some(eq) = line.find('=') else { continue };
            let key = line[..eq].trim();
            let value = line[eq + 1..].trim();
            s.apply(key, value);
        }

        s
    }

    fn apply(&mut self, key: &str, value: &str) {
        match key {
            "theme_mode" => {
                self.theme_mode = match value.to_lowercase().as_str() {
                    "darker" => ThemeMode::Darker,
                    "black" => ThemeMode::Black,
                    "midnight" => ThemeMode::Midnight,
                    "slate" => ThemeMode::Slate,
                    "ash" => ThemeMode::Ash,
                    "warm" => ThemeMode::Warm,
                    _ => ThemeMode::Dark,
                };
            }
            "accent" => {
                self.accent = match value.to_lowercase().as_str() {
                    "cyan" => Accent::Cyan,
                    "magenta" => Accent::Magenta,
                    "green" => Accent::Green,
                    "amber" => Accent::Amber,
                    "red" => Accent::Red,
                    _ => Accent::Violet,
                };
            }
            "density" => {
                self.density = match value.to_lowercase().as_str() {
                    "compact" => Density::Compact,
                    "spacious" => Density::Spacious,
                    _ => Density::Comfortable,
                };
            }
            "corner_radius" => {
                if let Ok(v) = value.parse::<f32>() {
                    self.corner_radius = v.clamp(0.0, 24.0);
                }
            }
            "font_scale" => {
                if let Ok(v) = value.parse::<f32>() {
                    self.font_scale = v.clamp(0.85, 1.30);
                }
            }
            "projects_dir" => self.projects_dir = value.to_string(),
            "dumps_dir" => self.dumps_dir = value.to_string(),
            "logs_dir" => self.logs_dir = value.to_string(),
            "dll_search_dir" => self.dll_search_dir = value.to_string(),
            "temp_dir" => self.temp_dir = value.to_string(),
            "confirm_inject" => self.confirm_inject = parse_bool(value),
            "confirm_kill" => self.confirm_kill = parse_bool(value),
            "restore_last_view" => self.restore_last_view = parse_bool(value),
            "autosave_minutes" => {
                if let Ok(v) = value.parse::<u32>() {
                    self.autosave_minutes = v;
                }
            }
            "startup_log_level" => self.startup_log_level = value.to_lowercase(),
            "project_autosave" => self.project_autosave = parse_bool(value),
            "scan_depth" => self.scan_depth = value.to_lowercase(),
            "pipe_buffer_kb" => {
                if let Ok(v) = value.parse::<u32>() {
                    self.pipe_buffer_kb = v.clamp(16, 4096);
                }
            }
            "inject_timeout_sec" => {
                if let Ok(v) = value.parse::<u32>() {
                    self.inject_timeout_sec = v.clamp(5, 600);
                }
            }
            "hooks_json_path" => self.hooks_json_path = value.to_string(),
            "external_editor_command" => {
                self.external_editor_command = value.to_string()
            }
            _ => {}
        }
    }

    pub fn save(&self) {
        let dir = Self::config_dir();
        let _ = fs::create_dir_all(&dir);

        let path = Self::config_path();
        let text = self.to_text();
        let _ = fs::write(path, text);
    }

    fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("# Sigil settings\n");
        out.push_str("# Edit while the app is closed, or use the Settings view.\n\n");

        out.push_str("# ---- Appearance ----\n");
        out.push_str(&format!("theme_mode = {}\n", theme_mode_key(self.theme_mode)));
        out.push_str(&format!("accent = {}\n", accent_key(self.accent)));
        out.push_str(&format!("density = {}\n", density_key(self.density)));
        out.push_str(&format!("corner_radius = {}\n", self.corner_radius));
        out.push_str(&format!("font_scale = {}\n\n", self.font_scale));

        out.push_str("# ---- Paths ----\n");
        out.push_str(&format!("projects_dir = {}\n", self.projects_dir));
        out.push_str(&format!("dumps_dir = {}\n", self.dumps_dir));
        out.push_str(&format!("logs_dir = {}\n", self.logs_dir));
        out.push_str(&format!("dll_search_dir = {}\n", self.dll_search_dir));
        out.push_str(&format!("temp_dir = {}\n\n", self.temp_dir));

        out.push_str("# ---- Behavior ----\n");
        out.push_str(&format!("confirm_inject = {}\n", self.confirm_inject));
        out.push_str(&format!("confirm_kill = {}\n", self.confirm_kill));
        out.push_str(&format!("restore_last_view = {}\n", self.restore_last_view));
        out.push_str(&format!("autosave_minutes = {}\n", self.autosave_minutes));
        out.push_str(&format!("startup_log_level = {}\n", self.startup_log_level));
        out.push_str(&format!("project_autosave = {}\n\n", self.project_autosave));

        out.push_str("# ---- Advanced ----\n");
        out.push_str(&format!("scan_depth = {}\n", self.scan_depth));
        out.push_str(&format!("pipe_buffer_kb = {}\n", self.pipe_buffer_kb));
        out.push_str(&format!("inject_timeout_sec = {}\n", self.inject_timeout_sec));
        out.push_str(&format!("hooks_json_path = {}\n", self.hooks_json_path));
        out.push_str(&format!(
            "external_editor_command = {}\n",
            self.external_editor_command
        ));

        out
    }

    pub fn to_theme(&self) -> Theme {
        Theme::from_parts(
            self.theme_mode,
            self.accent,
            self.density,
            self.corner_radius,
            self.font_scale,
        )
    }

    pub fn expand_path(input: &str) -> String {
        let mut out = String::new();
        let bytes = input.as_bytes();
        let mut i = 0;

        while i < bytes.len() {
            if bytes[i] == b'%' {
                if let Some(end) = input[i + 1..].find('%') {
                    let name = &input[i + 1..i + 1 + end];
                    if !name.is_empty() {
                        if let Ok(val) = std::env::var(name) {
                            out.push_str(&val);
                            i += end + 2;
                            continue;
                        }
                    }
                }
            }
            out.push(bytes[i] as char);
            i += 1;
        }

        out
    }
}

fn parse_bool(s: &str) -> bool {
    matches!(s.to_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

fn theme_mode_key(m: ThemeMode) -> &'static str {
    match m {
        ThemeMode::Dark => "dark",
        ThemeMode::Darker => "darker",
        ThemeMode::Black => "black",
        ThemeMode::Midnight => "midnight",
        ThemeMode::Slate => "slate",
        ThemeMode::Ash => "ash",
        ThemeMode::Warm => "warm",
    }
}

fn accent_key(a: Accent) -> &'static str {
    match a {
        Accent::Violet => "violet",
        Accent::Cyan => "cyan",
        Accent::Magenta => "magenta",
        Accent::Green => "green",
        Accent::Amber => "amber",
        Accent::Red => "red",
    }
}

fn density_key(d: Density) -> &'static str {
    match d {
        Density::Comfortable => "comfortable",
        Density::Compact => "compact",
        Density::Spacious => "spacious",
    }
}