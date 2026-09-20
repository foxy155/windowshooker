//! Hook library. Each hook is a folder under `%APPDATA%\Sigil\hooks\`
//! containing a `hook.json`, an optional `hook.dll`, and an optional
//! source file (`hook.cpp`, `hook.rs`, etc.).
//!
//! The library is a *template* store — the user copies hooks out of it
//! to use them, and copies working hooks into it to keep them. Nothing
//! in here is destructive to the original: importing always copies.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::injection::InjectionMethod;

// ============================================================
// DATA TYPES
// ============================================================

/// What kind of value a hook parameter holds. Used by the UI to pick
/// the right editor widget and by the injector when serializing to
/// the control pipe.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamKind {
    Float,
    Int,
    Bool,
    String,
}

impl ParamKind {
    pub fn label(&self) -> &'static str {
        match self {
            ParamKind::Float => "float",
            ParamKind::Int => "int",
            ParamKind::Bool => "bool",
            ParamKind::String => "string",
        }
    }

    pub fn all() -> [ParamKind; 4] {
        [ParamKind::Float, ParamKind::Int, ParamKind::Bool, ParamKind::String]
    }
}

/// One tunable value that ships with the hook and can be edited from
/// the UI. On inject, all params are serialized to JSON and written
/// to `\\.\pipe\hook_control`.
#[derive(Clone, Serialize, Deserialize)]
pub struct HookParam {
    /// Machine name — the key the DLL expects in the JSON blob.
    pub key: String,

    /// Human-readable label shown in the UI.
    pub label: String,

    pub kind: ParamKind,

    /// Default value when the hook is first added.
    pub default: ParamValue,

    /// Current value. Saved with the hook when the user tweaks it.
    pub value: ParamValue,

    /// Optional range for numeric kinds.
    #[serde(default)]
    pub min: Option<f64>,

    #[serde(default)]
    pub max: Option<f64>,
}

/// A parameter value. Serialized to JSON as a tagged union so the DLL
/// can tell types apart.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "v")]
pub enum ParamValue {
    Float(f64),
    Int(i64),
    Bool(bool),
    String(String),
}

impl ParamValue {
    pub fn kind(&self) -> ParamKind {
        match self {
            ParamValue::Float(_) => ParamKind::Float,
            ParamValue::Int(_) => ParamKind::Int,
            ParamValue::Bool(_) => ParamKind::Bool,
            ParamValue::String(_) => ParamKind::String,
        }
    }

    pub fn default_for(kind: ParamKind) -> Self {
        match kind {
            ParamKind::Float => ParamValue::Float(0.0),
            ParamKind::Int => ParamValue::Int(0),
            ParamKind::Bool => ParamValue::Bool(false),
            ParamKind::String => ParamValue::String(String::new()),
        }
    }
}

/// The full `hook.json` payload for one hook.
#[derive(Clone, Serialize, Deserialize)]
pub struct Hook {
    pub name: String,

    #[serde(default)]
    pub author: String,

    #[serde(default = "default_version")]
    pub version: String,

    #[serde(default)]
    pub description: String,

    /// Module (exe name) the hook is meant to run inside, e.g.
    /// `"AdventureQuest Worlds.exe"`. Free-form; used by the UI as a
    /// hint and by "inject all enabled" filtering.
    #[serde(default)]
    pub target_module: String,

    /// Filename of the DLL inside the hook folder. Usually `hook.dll`.
    #[serde(default = "default_dll")]
    pub dll: String,

    /// Filename of the optional source file inside the hook folder.
    #[serde(default)]
    pub source: Option<String>,

    /// Language of the source file, for syntax highlighting.
    #[serde(default)]
    pub language: Option<String>,

    #[serde(default)]
    pub injection_method: SerializedInjectionMethod,

    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub params: Vec<HookParam>,

    /// Not serialized — the folder on disk this hook lives in.
    #[serde(skip)]
    pub folder: PathBuf,
}

fn default_version() -> String {
    "1.0.0".into()
}

fn default_dll() -> String {
    "hook.dll".into()
}

/// Serde-friendly mirror of `InjectionMethod` so we don't have to
/// touch the injection module's enum just to make it serializable.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SerializedInjectionMethod {
    CreateRemoteThread,
    QueueUserApc,
}

impl Default for SerializedInjectionMethod {
    fn default() -> Self {
        Self::CreateRemoteThread
    }
}

impl From<InjectionMethod> for SerializedInjectionMethod {
    fn from(m: InjectionMethod) -> Self {
        match m {
            InjectionMethod::CreateRemoteThread => Self::CreateRemoteThread,
            InjectionMethod::QueueUserAPC => Self::QueueUserApc,
        }
    }
}

impl From<SerializedInjectionMethod> for InjectionMethod {
    fn from(m: SerializedInjectionMethod) -> Self {
        match m {
            SerializedInjectionMethod::CreateRemoteThread => {
                InjectionMethod::CreateRemoteThread
            }
            SerializedInjectionMethod::QueueUserApc => InjectionMethod::QueueUserAPC,
        }
    }
}

impl SerializedInjectionMethod {
    pub fn label(&self) -> &'static str {
        match self {
            Self::CreateRemoteThread => "CreateRemoteThread",
            Self::QueueUserApc => "QueueUserAPC",
        }
    }
}

/// Summary used by the library list. Doesn't include the full param
/// list — just enough for a row.
#[derive(Clone)]
pub struct HookSummary {
    pub name: String,
    pub folder: PathBuf,
    pub author: String,
    pub version: String,
    pub enabled: bool,
    pub dll_present: bool,
    pub source_present: bool,
}

// ============================================================
// PATHS
// ============================================================

/// Default hooks library folder. `%APPDATA%\Sigil\hooks\`.
pub fn hooks_root() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("Sigil").join("hooks")
}

pub fn hook_json_path(folder: &Path) -> PathBuf {
    folder.join("hook.json")
}

pub fn hook_dll_path(hook: &Hook) -> PathBuf {
    hook.folder.join(&hook.dll)
}

pub fn hook_source_path(hook: &Hook) -> Option<PathBuf> {
    hook.source.as_ref().map(|s| hook.folder.join(s))
}

// ============================================================
// LIST / LOAD / SAVE
// ============================================================

/// List every hook folder under the library root.
pub fn list_hooks() -> Vec<HookSummary> {
    let root = hooks_root();
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };

    let mut out: Vec<HookSummary> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let folder = e.path();
            let hook = load_hook(&folder).ok()?;
            let dll_present = hook_dll_path(&hook).exists();
            let source_present = hook_source_path(&hook)
                .map(|p| p.exists())
                .unwrap_or(false);

            Some(HookSummary {
                name: hook.name,
                folder,
                author: hook.author,
                version: hook.version,
                enabled: hook.enabled,
                dll_present,
                source_present,
            })
        })
        .collect();

    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

/// Load one hook from a folder.
pub fn load_hook(folder: &Path) -> Result<Hook, String> {
    let path = hook_json_path(folder);
    let text = fs::read_to_string(&path)
        .map_err(|e| format!("read {} failed: {}", path.display(), e))?;
    let mut hook: Hook = serde_json::from_str(&text)
        .map_err(|e| format!("parse {} failed: {}", path.display(), e))?;

    hook.folder = folder.to_path_buf();

    // Fallback name from folder if the JSON didn't have one.
    if hook.name.trim().is_empty() {
        hook.name = folder
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed")
            .to_string();
    }

    Ok(hook)
}

/// Save `hook.json`. Does not touch the DLL or source.
pub fn save_hook(hook: &Hook) -> Result<(), String> {
    let path = hook_json_path(&hook.folder);
    let text = serde_json::to_string_pretty(hook)
        .map_err(|e| format!("serialize failed: {}", e))?;
    fs::write(&path, text)
        .map_err(|e| format!("write {} failed: {}", path.display(), e))
}

// ============================================================
// CREATE / IMPORT / DELETE
// ============================================================

/// Scaffold a new empty hook folder with a template `hook.json`.
pub fn create_hook(name: &str) -> Result<Hook, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("hook name is empty".into());
    }

    let safe = sanitize_folder_name(trimmed);
    if safe.is_empty() {
        return Err("hook name contains no valid characters".into());
    }

    let folder = hooks_root().join(&safe);
    if folder.exists() {
        return Err(format!(
            "a hook named '{}' already exists at {}",
            trimmed,
            folder.display()
        ));
    }

    fs::create_dir_all(&folder)
        .map_err(|e| format!("could not create hook folder: {}", e))?;

    let hook = Hook {
        name: trimmed.to_string(),
        author: String::new(),
        version: default_version(),
        description: String::new(),
        target_module: String::new(),
        dll: default_dll(),
        source: None,
        language: None,
        injection_method: SerializedInjectionMethod::default(),
        enabled: false,
        params: Vec::new(),
        folder: folder.clone(),
    };

    save_hook(&hook)?;
    Ok(hook)
}

/// Copy an external folder into the library.
pub fn import_hook(source_folder: &Path) -> Result<Hook, String> {
    if !source_folder.is_dir() {
        return Err(format!(
            "{} is not a folder",
            source_folder.display()
        ));
    }

    let hook_json = hook_json_path(source_folder);
    if !hook_json.exists() {
        return Err(format!(
            "{} does not contain a hook.json",
            source_folder.display()
        ));
    }

    // Peek at the source hook to get its name.
    let peek = load_hook(source_folder)?;
    let safe = sanitize_folder_name(&peek.name);
    if safe.is_empty() {
        return Err("source hook has no usable name".into());
    }

    let dest = hooks_root().join(&safe);
    if dest.exists() {
        return Err(format!(
            "a hook named '{}' already exists in the library",
            peek.name
        ));
    }

    // Ensure the library root exists.
    fs::create_dir_all(hooks_root())
        .map_err(|e| format!("could not create library root: {}", e))?;

    // Recursively copy everything.
    copy_dir_recursive(source_folder, &dest)?;

    // Reload from the destination so `folder` points to the right place.
    let imported = load_hook(&dest)?;
    Ok(imported)
}

/// Delete a hook folder and everything inside it.
pub fn delete_hook(folder: &Path) -> Result<(), String> {
    fs::remove_dir_all(folder)
        .map_err(|e| format!("could not delete {}: {}", folder.display(), e))
}

// ============================================================
// SOURCE FILE MANAGEMENT
// ============================================================

/// Ensure a source file exists for the hook, creating a template if
/// missing. Returns the path and the content.
pub fn ensure_source_file(
    hook: &mut Hook,
    language: &str,
) -> Result<(PathBuf, String), String> {
    let ext = extension_for_language(language);
    let filename = format!("hook.{}", ext);
    let path = hook.folder.join(&filename);

    if !path.exists() {
        let template = template_for_language(language, &hook.name);
        fs::write(&path, &template)
            .map_err(|e| format!("could not write template: {}", e))?;
    }

    hook.source = Some(filename.clone());
    hook.language = Some(language.to_string());
    save_hook(hook)?;

    let content = fs::read_to_string(&path)
        .map_err(|e| format!("could not read source: {}", e))?;

    Ok((path, content))
}

/// Return the file extension for a given language key.
pub fn extension_for_language(lang: &str) -> &'static str {
    match lang.to_lowercase().as_str() {
        "cpp" | "c++" => "cpp",
        "c" => "c",
        "rust" | "rs" => "rs",
        "python" | "py" => "py",
        "javascript" | "js" => "js",
        "typescript" | "ts" => "ts",
        "lua" => "lua",
        "asm" => "asm",
        "json" => "json",
        "toml" => "toml",
        "plain" | "txt" | "" => "txt",
        _ => "txt",
    }
}

/// Return a starter source file for the given language. The hook name
/// is embedded in the comment header so users can tell them apart.
pub fn template_for_language(lang: &str, hook_name: &str) -> String {
    match lang.to_lowercase().as_str() {
        "cpp" | "c++" => format!(
            r#"// {name}
//
// Build with: cl /LD /EHsc /O2 hook.cpp /link /OUT:hook.dll
//
// The control pipe sends:
//   - literal "shutdown"       → unload
//   - JSON {{"cmd":"config",...}} → params update

#include <windows.h>

extern "C" __declspec(dllexport)
void Configure(const char* json) {{
    // Parse `json` and apply your hook parameters here.
    (void)json;
}}

BOOL WINAPI DllMain(HINSTANCE hinst, DWORD reason, LPVOID reserved) {{
    (void)hinst; (void)reserved;
    switch (reason) {{
        case DLL_PROCESS_ATTACH: break;
        case DLL_PROCESS_DETACH: break;
    }}
    return TRUE;
}}
"#,
            name = hook_name
        ),
        "rust" | "rs" => format!(
            r#"// {name}
//
// Build with: cargo build --release
// and copy target/release/*.dll to hook.dll

use std::ffi::c_void;

#[no_mangle]
pub extern "C" fn Configure(_json: *const c_void) {{
    // Parse the config JSON and apply your hook parameters.
}}

#[no_mangle]
pub extern "system" fn DllMain(
    _hinst: *mut c_void,
    _reason: u32,
    _reserved: *mut c_void,
) -> i32 {{
    1
}}
"#,
            name = hook_name
        ),
        "python" | "py" => format!(
            r#"# {name}
#
# Python bytecode hooks are loaded by a host DLL.
# This file is a reference implementation, not compiled.

def configure(json_str):
    """Called when the injector sends a config update."""
    import json
    cfg = json.loads(json_str)
    print("config:", cfg)
"#,
            name = hook_name
        ),
        _ => format!("// {}\n// (empty template)\n", hook_name),
    }
}

// ============================================================
// CONFIG SERIALIZATION
// ============================================================

/// Serialize the hook's params into the JSON payload the DLL expects.
/// Shape:
/// ```json
/// {"cmd":"config","params":{"multiplier":{"kind":"float","v":10.0}}}
/// ```
pub fn params_to_control_json(hook: &Hook) -> Result<Vec<u8>, String> {
    use serde_json::{json, Map, Value};

    let mut params = Map::new();
    for p in &hook.params {
        let v = serde_json::to_value(&p.value)
            .map_err(|e| format!("serialize param '{}' failed: {}", p.key, e))?;
        params.insert(p.key.clone(), v);
    }

    let payload = json!({
        "cmd": "config",
        "params": Value::Object(params),
    });

    let text = serde_json::to_string(&payload)
        .map_err(|e| format!("serialize config failed: {}", e))?;
    Ok(text.into_bytes())
}

// ============================================================
// HELPERS
// ============================================================

fn sanitize_folder_name(name: &str) -> String {
    const BAD: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    let cleaned: String = name
        .chars()
        .map(|c| if BAD.contains(&c) || c.is_control() { '_' } else { c })
        .collect();
    cleaned.trim().trim_end_matches('.').to_string()
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("create {} failed: {}", dst.display(), e))?;

    let entries = fs::read_dir(src)
        .map_err(|e| format!("read {} failed: {}", src.display(), e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("read entry failed: {}", e))?;
        let path = entry.path();
        let name = entry.file_name();
        let dest = dst.join(&name);

        if path.is_dir() {
            copy_dir_recursive(&path, &dest)?;
        } else {
            fs::copy(&path, &dest).map_err(|e| {
                format!(
                    "copy {} -> {} failed: {}",
                    path.display(),
                    dest.display(),
                    e
                )
            })?;
        }
    }

    Ok(())
}