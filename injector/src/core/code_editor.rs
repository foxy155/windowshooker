//! In-memory editor state for hook source files.
//!
//! One buffer per open file. The UI draws tabs across the top and
//! renders the active buffer in either a docked or floating panel.
//!
//! Nothing here talks to egui. That keeps the data model testable and
//! lets us swap the rendering backend later without touching this file.

use std::fs;
use std::path::{Path, PathBuf};

// ============================================================
// LANGUAGE
// ============================================================

/// Which syntax highlighter to hand to `egui_code_editor`. Kept as a
/// string because egui_code_editor uses its own enum and we don't want
/// to tie our data model to a specific version of it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CodeLanguage {
    Cpp,
    C,
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Lua,
    Assembly,
    Json,
    Toml,
    Plain,
}

impl CodeLanguage {
    /// The key we store in `hook.json` and match against
    /// `extension_for_language`.
    pub fn key(&self) -> &'static str {
        match self {
            CodeLanguage::Cpp => "cpp",
            CodeLanguage::C => "c",
            CodeLanguage::Rust => "rust",
            CodeLanguage::Python => "python",
            CodeLanguage::JavaScript => "javascript",
            CodeLanguage::TypeScript => "typescript",
            CodeLanguage::Lua => "lua",
            CodeLanguage::Assembly => "asm",
            CodeLanguage::Json => "json",
            CodeLanguage::Toml => "toml",
            CodeLanguage::Plain => "plain",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            CodeLanguage::Cpp => "C++",
            CodeLanguage::C => "C",
            CodeLanguage::Rust => "Rust",
            CodeLanguage::Python => "Python",
            CodeLanguage::JavaScript => "JavaScript",
            CodeLanguage::TypeScript => "TypeScript",
            CodeLanguage::Lua => "Lua",
            CodeLanguage::Assembly => "Assembly",
            CodeLanguage::Json => "JSON",
            CodeLanguage::Toml => "TOML",
            CodeLanguage::Plain => "Plain Text",
        }
    }

    pub fn all() -> [CodeLanguage; 11] {
        [
            CodeLanguage::Cpp,
            CodeLanguage::C,
            CodeLanguage::Rust,
            CodeLanguage::Python,
            CodeLanguage::JavaScript,
            CodeLanguage::TypeScript,
            CodeLanguage::Lua,
            CodeLanguage::Assembly,
            CodeLanguage::Json,
            CodeLanguage::Toml,
            CodeLanguage::Plain,
        ]
    }

    /// Parse from the stored key, defaulting to Plain.
    pub fn from_key(key: &str) -> Self {
        match key.to_lowercase().as_str() {
            "cpp" | "c++" => Self::Cpp,
            "c" => Self::C,
            "rust" | "rs" => Self::Rust,
            "python" | "py" => Self::Python,
            "javascript" | "js" => Self::JavaScript,
            "typescript" | "ts" => Self::TypeScript,
            "lua" => Self::Lua,
            "asm" | "assembly" => Self::Assembly,
            "json" => Self::Json,
            "toml" => Self::Toml,
            _ => Self::Plain,
        }
    }

    /// Guess from a file extension when we don't have a stored key.
    pub fn from_extension(path: &Path) -> Self {
        path.extension()
            .and_then(|e| e.to_str())
            .map(Self::from_key)
            .unwrap_or(Self::Plain)
    }
}

// ============================================================
// OPEN BUFFER
// ============================================================

/// One open file in the editor. The UI shows these as tabs.
pub struct OpenEditor {
    /// Which hook this buffer belongs to (by name, not path).
    pub hook_name: String,

    /// Full path on disk.
    pub path: PathBuf,

    /// What the user currently has in the buffer.
    pub content: String,

    /// What was last saved to disk. Used for dirty tracking.
    pub original: String,

    /// Language for the highlighter.
    pub language: CodeLanguage,

    /// Saved scroll position so switching tabs feels stable.
    pub scroll_top: f32,
}

impl OpenEditor {
    pub fn new(
        hook_name: String,
        path: PathBuf,
        content: String,
        language: CodeLanguage,
    ) -> Self {
        Self {
            hook_name,
            path,
            original: content.clone(),
            content,
            language,
            scroll_top: 0.0,
        }
    }

    /// Are there unsaved changes?
    pub fn is_dirty(&self) -> bool {
        self.content != self.original
    }

    /// Filename without directory, for the tab label.
    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("untitled")
            .to_string()
    }

    /// Save the buffer to disk. On success, `original` is updated so
    /// the tab is no longer dirty.
    pub fn save(&mut self) -> Result<(), String> {
        fs::write(&self.path, &self.content)
            .map_err(|e| format!("could not write {}: {}", self.path.display(), e))?;
        self.original = self.content.clone();
        Ok(())
    }

    /// Reload from disk. Discards unsaved changes.
    pub fn reload(&mut self) -> Result<(), String> {
        let text = fs::read_to_string(&self.path)
            .map_err(|e| format!("could not read {}: {}", self.path.display(), e))?;
        self.content = text.clone();
        self.original = text;
        Ok(())
    }
}

// ============================================================
// EDITOR STATE
// ============================================================

/// All editor tabs across all hooks, plus the floating/docked mode.
pub struct EditorState {
    pub tabs: Vec<OpenEditor>,
    pub active: Option<usize>,

    /// Whether the editor is popped out into a floating window.
    pub floating: bool,

    /// Remembered window position when floating.
    pub float_pos: [f32; 2],

    /// Remembered window size when floating.
    pub float_size: [f32; 2],

    /// Set to true when the user clicks "Pop out" this frame. Cleared
    /// after the frame is drawn so we only act on it once.
    pub pop_out_requested: bool,

    /// Set to true when the user clicks "Dock" this frame.
    pub dock_requested: bool,
}

impl EditorState {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active: None,
            floating: false,
            float_pos: [120.0, 120.0],
            float_size: [800.0, 500.0],
            pop_out_requested: false,
            dock_requested: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn active_buffer(&self) -> Option<&OpenEditor> {
        self.active.and_then(|i| self.tabs.get(i))
    }

    pub fn active_buffer_mut(&mut self) -> Option<&mut OpenEditor> {
        self.active.and_then(|i| self.tabs.get_mut(i))
    }

    /// Is there a tab open for this file path?
    pub fn find_by_path(&self, path: &Path) -> Option<usize> {
        self.tabs.iter().position(|t| t.path == path)
    }

    /// Open or focus a file. Returns the tab index.
    pub fn open_or_focus(
        &mut self,
        hook_name: String,
        path: PathBuf,
        language: CodeLanguage,
    ) -> Result<usize, String> {
        if let Some(i) = self.find_by_path(&path) {
            self.active = Some(i);
            return Ok(i);
        }

        let content = fs::read_to_string(&path)
            .map_err(|e| format!("could not read {}: {}", path.display(), e))?;

        let tab = OpenEditor::new(hook_name, path, content, language);
        self.tabs.push(tab);
        let i = self.tabs.len() - 1;
        self.active = Some(i);
        Ok(i)
    }

    /// Close a tab. Returns true if the tab was dirty and the caller
    /// should confirm before actually closing.
    ///
    /// The UI is expected to call `close_at` only after confirmation.
    pub fn would_lose_changes(&self, index: usize) -> bool {
        self.tabs.get(index).map(|t| t.is_dirty()).unwrap_or(false)
    }

    /// Close a tab unconditionally. Adjusts `active` so it still points
    /// at a valid tab afterwards.
    pub fn close_at(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(index);

        if self.tabs.is_empty() {
            self.active = None;
            return;
        }

        // Keep the active index sensible.
        self.active = match self.active {
            Some(a) if a == index => {
                // Closed the active tab — activate the one that slid in,
                // or the last one.
                Some(index.min(self.tabs.len() - 1))
            }
            Some(a) if a > index => Some(a - 1),
            other => other,
        };
    }

    pub fn close_all(&mut self) {
        self.tabs.clear();
        self.active = None;
    }

    pub fn close_others(&mut self, keep: usize) {
        if keep >= self.tabs.len() {
            return;
        }
        let kept = self.tabs.remove(keep);
        self.tabs.clear();
        self.tabs.push(kept);
        self.active = Some(0);
    }

    /// Save every dirty tab. Returns the number saved.
    pub fn save_all(&mut self) -> usize {
        let mut saved = 0;
        for tab in &mut self.tabs {
            if tab.is_dirty() && tab.save().is_ok() {
                saved += 1;
            }
        }
        saved
    }

    /// Any tab with unsaved changes?
    pub fn has_dirty(&self) -> bool {
        self.tabs.iter().any(|t| t.is_dirty())
    }

    /// Close any tabs that belong to a hook we just deleted or renamed
    /// away from. Prevents editing a file that no longer exists.
    pub fn close_tabs_for_hook(&mut self, hook_name: &str) {
        self.tabs.retain(|t| t.hook_name != hook_name);
        if self.tabs.is_empty() {
            self.active = None;
        } else if let Some(a) = self.active {
            if a >= self.tabs.len() {
                self.active = Some(self.tabs.len() - 1);
            }
        }
    }
}

impl Default for EditorState {
    fn default() -> Self {
        Self::new()
    }
}