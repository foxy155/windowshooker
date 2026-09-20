use std::collections::HashSet;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use log::Level;

use crate::core::code_editor::{CodeLanguage, EditorState};
use crate::core::filter::{parse_filter, Expr};
use crate::core::hooks::{self, Hook, HookSummary};
use crate::core::injection::{inject, InjectionMethod};
use crate::core::launch::{enable_debug_privilege, launch_process, wait_for_connection};
use crate::core::logging::{install_logger, LogEntry};
use crate::core::packets::{Direction, PacketStore};
use crate::core::processes::{
    find_process_by_pid, find_processes_by_name, AccessStatus, ProcessEntry,
};
use crate::core::scanning::{
    app_cache_path, load_cached_apps, save_apps_cache, scan_apps_native, AppGroup,
};
use crate::core::session::{
    self, now_string, ChangelogEntry, ChangelogKind, Project,
};
use crate::core::settings::Settings;

pub const CONTROL_PIPE_NAME: &str = r"\\.\pipe\hook_control";

#[cfg(windows)]
pub mod win_pipe {
    pub type Handle = *mut core::ffi::c_void;
    pub type Bool = i32;
    pub type Dword = u32;

    pub const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
    pub const GENERIC_WRITE: Dword = 0x40000000;
    pub const OPEN_EXISTING: Dword = 3;
    pub const FILE_ATTRIBUTE_NORMAL: Dword = 0x00000080;

    #[link(name = "kernel32")]
    extern "system" {
        pub fn CreateFileW(
            lpFileName: *const u16,
            dwDesiredAccess: Dword,
            dwShareMode: Dword,
            lpSecurityAttributes: *mut core::ffi::c_void,
            dwCreationDisposition: Dword,
            dwFlagsAndAttributes: Dword,
            hTemplateFile: Handle,
        ) -> Handle;

        pub fn WriteFile(
            hFile: Handle,
            lpBuffer: *const u8,
            nNumberOfBytesToWrite: Dword,
            lpNumberOfBytesWritten: *mut Dword,
            lpOverlapped: *mut core::ffi::c_void,
        ) -> Bool;

        pub fn CloseHandle(hObject: Handle) -> Bool;
    }
}

#[derive(Clone, PartialEq)]
pub enum ScanStatus {
    Idle,
    Scanning,
    Ready { count: usize },
    Failed(String),
}

#[derive(PartialEq, Clone, Copy)]
pub enum TargetMode {
    Pid,
    Name,
}

impl TargetMode {
    pub fn label(&self) -> &'static str {
        match self {
            TargetMode::Pid => "By PID",
            TargetMode::Name => "By Process Name",
        }
    }
}

#[derive(PartialEq, Clone, Copy)]
pub enum View {
    Dashboard,
    Injector,
    Packets,
    Hooks,
    Memory,
    Analyzer,
    Sessions,
    Settings,
}

impl View {
    pub fn title(&self) -> &'static str {
        match self {
            View::Dashboard => "Dashboard",
            View::Injector => "Injector",
            View::Packets => "Packets",
            View::Hooks => "Hooks",
            View::Memory => "Memory",
            View::Analyzer => "Analyzer",
            View::Sessions => "Sessions",
            View::Settings => "Settings",
        }
    }
}

#[derive(PartialEq, Clone, Copy)]
pub enum InterceptState {
    Idle,
    Listening,
    Receiving,
}

#[derive(PartialEq, Clone, Copy)]
pub enum LogLevelFilter {
    All,
    Info,
    Warn,
    Error,
}

impl LogLevelFilter {
    pub fn label(&self) -> &'static str {
        match self {
            LogLevelFilter::All => "All",
            LogLevelFilter::Info => "Info+",
            LogLevelFilter::Warn => "Warn+",
            LogLevelFilter::Error => "Error",
        }
    }

    pub fn accepts(&self, level: Level) -> bool {
        match self {
            LogLevelFilter::All => true,
            LogLevelFilter::Info => matches!(level, Level::Info | Level::Warn | Level::Error),
            LogLevelFilter::Warn => matches!(level, Level::Warn | Level::Error),
            LogLevelFilter::Error => matches!(level, Level::Error),
        }
    }
}

#[derive(PartialEq, Clone, Copy)]
pub enum SortColumn {
    Id,
    Pid,
    Direction,
    Time,
    Opcode,
    Size,
}

#[derive(PartialEq, Clone, Copy)]
pub enum SortOrder {
    Asc,
    Desc,
}

#[derive(PartialEq, Clone, Copy)]
pub enum SessionsTab {
    OpenProject,
    AllProjects,
    NewProject,
}

impl SessionsTab {
    pub fn label(&self) -> &'static str {
        match self {
            SessionsTab::OpenProject => "Open Project",
            SessionsTab::AllProjects => "All Projects",
            SessionsTab::NewProject => "New Project",
        }
    }
}

#[derive(PartialEq, Clone, Copy)]
pub enum HooksTab {
    Library,
    EditHook,
    NewHook,
}

impl HooksTab {
    pub fn label(&self) -> &'static str {
        match self {
            HooksTab::Library => "Library",
            HooksTab::EditHook => "Edit Hook",
            HooksTab::NewHook => "New Hook",
        }
    }
}

type ScanSlot = Arc<Mutex<Option<Result<Vec<AppGroup>, String>>>>;

pub struct App {
    pub view: View,

    pub settings: Settings,
    pub settings_snapshot: Settings,

    pub is_elevated: Arc<AtomicBool>,
    pub pipe_connected: Arc<AtomicBool>,

    pub launch_path: String,
    pub auto_inject_after_launch: bool,
    pub remote_ip: String,
    pub installed_apps: Vec<AppGroup>,
    pub app_search: String,
    pub expanded: HashSet<String>,
    pub pending_app_scan: Option<ScanSlot>,
    pub scan_status: ScanStatus,
    pub launched_pid: Option<u32>,

    pub mode: TargetMode,
    pub target_pid: String,
    pub target_name: String,
    pub dll_path: String,
    pub injection_method: InjectionMethod,

    pub preflight_results: Vec<ProcessEntry>,
    pub preflight_message: Option<String>,

    pub log_entries: Arc<Mutex<Vec<LogEntry>>>,
    pub log_level_filter: LogLevelFilter,
    pub log_search: String,
    pub log_autoscroll: bool,

    pub store: Arc<Mutex<PacketStore>>,
    pub selected_packet: Option<usize>,
    pub filter_text: String,
    pub filter_error: Option<String>,
    pub cached_filter: Option<Expr>,
    pub cached_filter_text: String,
    pub sort_column: SortColumn,
    pub sort_order: SortOrder,

    pub presets: Vec<(String, String)>,

    pub intercept_active: Arc<AtomicBool>,
    pub intercept_state: InterceptState,
    pub intercept_started_at: Option<Instant>,
    pub intercept_packet_count_at_start: usize,
    pub reader_thread_started: bool,

    pub shutdown_done: bool,

    // ---- Project state ----
    pub current_project: Option<Project>,
    pub project_dirty: bool,
    pub project_list: Vec<session::ProjectSummary>,
    pub project_list_loaded: bool,
    pub new_project_name: String,
    pub rename_buffer: String,
    pub show_delete_confirm: Option<PathBuf>,

    // ---- Sessions view ----
    pub sessions_tab: SessionsTab,

    // ---- Hooks view ----
    pub hooks_tab: HooksTab,
    pub hooks: Vec<HookSummary>,
    pub hooks_loaded: bool,
    pub hooks_search: String,
    pub selected_hook: Option<Hook>,
    pub new_hook_name: String,
    pub new_hook_language: CodeLanguage,
    pub editor: Arc<Mutex<EditorState>>,
    pub pending_close_tab: Option<usize>,
}

impl App {
    pub fn new() -> Self {
        let log_entries = Arc::new(Mutex::new(Vec::new()));
        install_logger(Arc::clone(&log_entries));

        let settings = Settings::load();
        crate::theme::install(settings.to_theme());
        let snapshot = settings.clone();

        log::info!(target: "injector", "application started");
        log::info!(
            target: "settings",
            "loaded config from {}",
            Settings::config_path().display()
        );

        let is_elevated = Arc::new(AtomicBool::new(false));
        match enable_debug_privilege() {
            Ok(()) => {
                is_elevated.store(true, Ordering::Relaxed);
                log::info!(target: "privilege", "SeDebugPrivilege enabled");
            }
            Err(e) => log::warn!(
                target: "privilege",
                "could not enable SeDebugPrivilege: {}",
                e
            ),
        }

        let pipe_connected = Arc::new(AtomicBool::new(false));

        let mut app = Self {
            view: View::Dashboard,
            settings,
            settings_snapshot: snapshot,
            is_elevated,
            pipe_connected,
            launch_path: String::new(),
            auto_inject_after_launch: true,
            remote_ip: "172.65.204.220".into(),
            installed_apps: Vec::new(),
            app_search: String::new(),
            expanded: HashSet::new(),
            pending_app_scan: None,
            scan_status: ScanStatus::Idle,
            launched_pid: None,
            mode: TargetMode::Pid,
            target_pid: String::new(),
            target_name: String::new(),
            dll_path: String::new(),
            injection_method: InjectionMethod::CreateRemoteThread,
            preflight_results: Vec::new(),
            preflight_message: None,
            log_entries,
            log_level_filter: LogLevelFilter::All,
            log_search: String::new(),
            log_autoscroll: true,
            store: Arc::new(Mutex::new(PacketStore::new())),
            selected_packet: None,
            filter_text: String::new(),
            filter_error: None,
            cached_filter: None,
            cached_filter_text: String::new(),
            sort_column: SortColumn::Id,
            sort_order: SortOrder::Asc,
            presets: vec![
                ("All".into(), "".into()),
                ("Sent only".into(), "dir == sent".into()),
                ("Received only".into(), "dir == received".into()),
                ("Large packets".into(), "len > 50".into()),
                ("Small packets".into(), "len < 10".into()),
            ],
            intercept_active: Arc::new(AtomicBool::new(true)),
            intercept_state: InterceptState::Idle,
            intercept_started_at: None,
            intercept_packet_count_at_start: 0,
            reader_thread_started: false,
            shutdown_done: false,
            current_project: None,
            project_dirty: false,
            project_list: Vec::new(),
            project_list_loaded: false,
            new_project_name: String::new(),
            rename_buffer: String::new(),
            show_delete_confirm: None,
            sessions_tab: SessionsTab::AllProjects,
            hooks_tab: HooksTab::Library,
            hooks: Vec::new(),
            hooks_loaded: false,
            hooks_search: String::new(),
            selected_hook: None,
            new_hook_name: String::new(),
            new_hook_language: CodeLanguage::Cpp,
            editor: Arc::new(Mutex::new(EditorState::new())),
            pending_close_tab: None,
        };

        if let Some(cached) = load_cached_apps() {
            let count = cached.len();
            log::info!(target: "launch", "loaded {} groups from cache", count);
            app.installed_apps = cached;
            app.scan_status = ScanStatus::Ready { count };
        } else {
            log::info!(target: "launch", "scanning for installed apps in background...");
            app.scan_status = ScanStatus::Scanning;
            app.spawn_scan();
        }

        app
    }

    // ---------- Editor helpers ----------

    /// Convenience: lock the editor state. Panics if poisoned.
    pub fn editor(&self) -> std::sync::MutexGuard<'_, EditorState> {
        self.editor.lock().expect("editor mutex poisoned")
    }

    // ---------- Settings helpers ----------

    pub fn apply_settings_to_theme(&self) {
        crate::theme::install(self.settings.to_theme());
    }

    pub fn revert_settings(&mut self) {
        self.settings = self.settings_snapshot.clone();
        self.apply_settings_to_theme();
    }

    pub fn save_settings(&mut self) {
        self.settings.save();
        self.settings_snapshot = self.settings.clone();
        log::info!(target: "settings", "settings saved");
    }

    // ---------- Project helpers ----------

    pub fn refresh_project_list(&mut self) {
        self.project_list = session::list_projects(&self.settings);
        self.project_list_loaded = true;
    }

    pub fn new_project(&mut self, name: &str) {
        match session::create_project(&self.settings, name) {
            Ok(p) => {
                log::info!(target: "project", "created '{}' at {}", p.name, p.folder.display());
                self.current_project = Some(p);
                self.project_dirty = false;
                self.rename_buffer = self
                    .current_project
                    .as_ref()
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                self.refresh_project_list();
                self.log_change(ChangelogKind::Created, format!("created '{}'", name));
                self.sessions_tab = SessionsTab::OpenProject;
            }
            Err(e) => {
                log::error!(target: "project", "{}", e);
            }
        }
    }

    pub fn open_project(&mut self, folder: &std::path::Path) {
        match session::load_project(folder) {
            Ok(p) => {
                log::info!(target: "project", "opened '{}'", p.name);

                let a = p.appearance.clone();
                self.settings.theme_mode = a.theme_mode;
                self.settings.accent = a.accent;
                self.settings.density = a.density;
                self.settings.corner_radius = a.corner_radius;
                self.settings.font_scale = a.font_scale;
                self.apply_settings_to_theme();

                self.current_project = Some(p);
                self.project_dirty = false;
                self.rename_buffer = self
                    .current_project
                    .as_ref()
                    .map(|p| p.name.clone())
                    .unwrap_or_default();

                let folder_buf = folder.to_path_buf();
                let _ = session::append_changelog(
                    &folder_buf,
                    ChangelogEntry {
                        timestamp: now_string(),
                        kind: ChangelogKind::Opened,
                        detail: "project opened".into(),
                    },
                );

                self.sessions_tab = SessionsTab::OpenProject;
            }
            Err(e) => {
                log::error!(target: "project", "{}", e);
            }
        }
    }

    pub fn close_project(&mut self) {
        if let Some(p) = &self.current_project {
            log::info!(target: "project", "closing '{}'", p.name);
        }
        self.current_project = None;
        self.project_dirty = false;
        self.rename_buffer.clear();
        self.sessions_tab = SessionsTab::AllProjects;
    }

    pub fn save_project(&mut self) {
        let Some(p) = self.current_project.as_mut() else { return; };
        match session::save_project(p) {
            Ok(()) => {
                self.project_dirty = false;
                let folder = p.folder.clone();
                let _ = session::append_changelog(
                    &folder,
                    ChangelogEntry {
                        timestamp: now_string(),
                        kind: ChangelogKind::Saved,
                        detail: "manual save".into(),
                    },
                );
                log::info!(target: "project", "saved '{}'", p.name);
            }
            Err(e) => log::error!(target: "project", "{}", e),
        }
    }

    pub fn rename_current_project(&mut self, new_name: &str) {
        let Some(p) = self.current_project.as_mut() else { return; };
        match session::rename_project(p, new_name) {
            Ok(()) => {
                self.project_dirty = false;
                log::info!(target: "project", "renamed to '{}'", new_name);
                self.refresh_project_list();
            }
            Err(e) => log::error!(target: "project", "{}", e),
        }
    }

    pub fn delete_project(&mut self, folder: &std::path::Path) {
        match session::delete_project(folder) {
            Ok(()) => {
                log::info!(target: "project", "deleted {}", folder.display());
                if let Some(p) = &self.current_project {
                    if p.folder == folder {
                        self.current_project = None;
                        self.project_dirty = false;
                    }
                }
                self.refresh_project_list();
            }
            Err(e) => log::error!(target: "project", "{}", e),
        }
    }

    pub fn touch_project(&mut self) {
        let Some(p) = self.current_project.as_mut() else { return; };
        self.project_dirty = true;

        if self.settings.project_autosave {
            match session::save_project(p) {
                Ok(()) => {
                    self.project_dirty = false;
                }
                Err(e) => {
                    log::error!(target: "project", "autosave failed: {}", e);
                }
            }
        }
    }

    pub fn log_change(&mut self, kind: ChangelogKind, detail: String) {
        let Some(p) = &self.current_project else { return; };
        let folder = p.folder.clone();
        let entry = ChangelogEntry {
            timestamp: now_string(),
            kind,
            detail,
        };
        if let Err(e) = session::append_changelog(&folder, entry) {
            log::error!(target: "project", "changelog: {}", e);
        }
    }

    pub fn sync_project_appearance(&mut self) {
        let snapshot = session::AppearanceSnapshot::from(&self.settings);
        if let Some(p) = self.current_project.as_mut() {
            p.appearance = snapshot;
        }
        self.touch_project();
    }

    // ---------- Hook helpers ----------

    pub fn refresh_hook_list(&mut self) {
        self.hooks = hooks::list_hooks();
        self.hooks_loaded = true;
    }

    pub fn new_hook(&mut self, name: &str) {
        match hooks::create_hook(name) {
            Ok(h) => {
                log::info!(target: "hooks", "created hook '{}'", h.name);
                self.selected_hook = Some(h);
                self.refresh_hook_list();
                self.hooks_tab = HooksTab::EditHook;
                self.log_change(
                    ChangelogKind::HookAdded,
                    format!("hook '{}' created", name),
                );
            }
            Err(e) => log::error!(target: "hooks", "{}", e),
        }
    }

    pub fn import_hook(&mut self, source: &std::path::Path) {
        match hooks::import_hook(source) {
            Ok(h) => {
                log::info!(target: "hooks", "imported hook '{}'", h.name);
                self.selected_hook = Some(h);
                self.refresh_hook_list();
                self.hooks_tab = HooksTab::EditHook;
            }
            Err(e) => log::error!(target: "hooks", "{}", e),
        }
    }

    pub fn open_hook(&mut self, folder: &std::path::Path) {
        match hooks::load_hook(folder) {
            Ok(h) => {
                log::info!(target: "hooks", "selected hook '{}'", h.name);
                self.selected_hook = Some(h);
                self.hooks_tab = HooksTab::EditHook;
            }
            Err(e) => log::error!(target: "hooks", "{}", e),
        }
    }

    pub fn save_selected_hook(&mut self) {
        let Some(h) = self.selected_hook.as_mut() else { return; };
        match hooks::save_hook(h) {
            Ok(()) => {
                log::info!(target: "hooks", "saved hook '{}'", h.name);
                self.refresh_hook_list();
            }
            Err(e) => log::error!(target: "hooks", "{}", e),
        }
    }

    pub fn deselect_hook_with_autosave(&mut self) {
        if let Some(h) = self.selected_hook.as_mut() {
            match hooks::save_hook(h) {
                Ok(()) => {
                    log::info!(
                        target: "hooks",
                        "autosaved '{}' before deselecting",
                        h.name
                    );
                }
                Err(e) => {
                    log::error!(target: "hooks", "autosave failed: {}", e);
                }
            }
        }
        self.selected_hook = None;
        self.refresh_hook_list();
    }

    pub fn delete_selected_hook(&mut self) {
        let Some(h) = self.selected_hook.take() else { return; };
        let folder = h.folder.clone();
        let name = h.name.clone();

        {
            let mut e = self.editor();
            e.close_tabs_for_hook(&name);
        }

        match hooks::delete_hook(&folder) {
            Ok(()) => {
                log::info!(target: "hooks", "deleted hook '{}'", name);
                self.refresh_hook_list();
                self.hooks_tab = HooksTab::Library;
            }
            Err(e) => {
                log::error!(target: "hooks", "{}", e);
                self.selected_hook = Some(h);
            }
        }
    }

    pub fn open_selected_hook_source(&mut self, language: CodeLanguage) {
        let Some(h) = self.selected_hook.as_mut() else {
            log::warn!(target: "hooks", "no hook selected");
            return;
        };

        let hook_name = h.name.clone();

        let (path, _content) = match hooks::ensure_source_file(h, language.key()) {
            Ok(v) => v,
            Err(e) => {
                log::error!(target: "hooks", "{}", e);
                return;
            }
        };

        if let Err(e) = hooks::save_hook(h) {
            log::error!(target: "hooks", "{}", e);
        }

        let mut e = self.editor();
        match e.open_or_focus(hook_name.clone(), path.clone(), language) {
            Ok(i) => {
                e.active = Some(i);
                log::info!(
                    target: "hooks",
                    "opened source for '{}' at {}",
                    hook_name,
                    path.display()
                );
            }
            Err(e) => log::error!(target: "hooks", "{}", e),
        }
    }

    pub fn save_active_editor(&mut self) {
        let mut e = self.editor();
        let Some(tab) = e.active_buffer_mut() else { return; };
        match tab.save() {
            Ok(()) => log::info!(target: "editor", "saved {}", tab.path.display()),
            Err(e) => log::error!(target: "editor", "{}", e),
        }
    }

    pub fn open_active_in_external_editor(&mut self) {
        let path = {
            let e = self.editor();
            e.active_buffer().map(|t| t.path.clone())
        };
        let Some(path) = path else { return; };
        let cmd = self.settings.external_editor_command.clone();
        open_in_external_editor(&path, &cmd);
    }

    pub fn open_active_with_picker(&mut self) {
        let path = {
            let e = self.editor();
            e.active_buffer().map(|t| t.path.clone())
        };
        let Some(path) = path else { return; };
        open_with_picker(&path);
    }

    pub fn open_path_with_picker(&self, path: &std::path::Path) {
        open_with_picker(path);
    }

    pub fn open_in_external_editor_path(&self, path: &std::path::Path) {
        open_in_external_editor(path, &self.settings.external_editor_command);
    }

    pub fn inject_hook_by_name(&mut self, hook_name: &str) {
        let folder = self
            .hooks
            .iter()
            .find(|h| h.name == hook_name)
            .map(|h| h.folder.clone());

        let Some(folder) = folder else {
            log::error!(target: "hooks", "hook '{}' not found", hook_name);
            return;
        };

        let hook = match hooks::load_hook(&folder) {
            Ok(h) => h,
            Err(e) => {
                log::error!(target: "hooks", "{}", e);
                return;
            }
        };

        let dll = hooks::hook_dll_path(&hook);
        if !dll.exists() {
            log::error!(
                target: "hooks",
                "'{}' has no compiled DLL at {}",
                hook.name,
                dll.display()
            );
            return;
        }

        let pid = match self.resolve_single_target_pid() {
            Ok(p) => p,
            Err(e) => {
                log::error!(target: "hooks", "{}", e);
                return;
            }
        };

        let method: InjectionMethod = hook.injection_method.into();
        let dll_str = dll.to_string_lossy().to_string();

        match inject(pid, &dll_str, method) {
            Ok(()) => {
                log::info!(
                    target: "hooks",
                    "injected '{}' into PID {}",
                    hook.name,
                    pid
                );
                thread::sleep(Duration::from_millis(250));
                self.send_hook_config(&hook);
            }
            Err(e) => {
                log::warn!(target: "hooks", "'{}': {}", hook.name, e);
            }
        }
    }

    pub fn inject_all_enabled_hooks(&mut self) {
        let names: Vec<String> = self
            .hooks
            .iter()
            .filter(|h| h.enabled)
            .map(|h| h.name.clone())
            .collect();

        if names.is_empty() {
            log::info!(target: "hooks", "no hooks are enabled");
            return;
        }

        log::info!(target: "hooks", "injecting {} enabled hook(s)", names.len());
        for name in names {
            self.inject_hook_by_name(&name);
        }
    }

    fn send_hook_config(&mut self, hook: &Hook) {
        let payload = match hooks::params_to_control_json(hook) {
            Ok(p) => p,
            Err(e) => {
                log::warn!(target: "hooks", "could not serialize config: {}", e);
                return;
            }
        };

        if self.send_control_message(&payload) {
            log::info!(
                target: "hooks",
                "config delivered to DLL for '{}'",
                hook.name
            );
        } else {
            log::warn!(
                target: "hooks",
                "no DLL listening on control pipe for '{}'",
                hook.name
            );
        }
    }

    pub fn send_control_message(&self, bytes: &[u8]) -> bool {
        #[cfg(windows)]
        unsafe {
            let name: Vec<u16> = CONTROL_PIPE_NAME
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();

            let handle = win_pipe::CreateFileW(
                name.as_ptr(),
                win_pipe::GENERIC_WRITE,
                0,
                std::ptr::null_mut(),
                win_pipe::OPEN_EXISTING,
                win_pipe::FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            );

            if handle == win_pipe::INVALID_HANDLE_VALUE {
                return false;
            }

            let mut written: win_pipe::Dword = 0;
            let ok = win_pipe::WriteFile(
                handle,
                bytes.as_ptr(),
                bytes.len() as win_pipe::Dword,
                &mut written,
                std::ptr::null_mut(),
            );
            let _ = win_pipe::CloseHandle(handle);
            ok != 0
        }

        #[cfg(not(windows))]
        {
            let _ = bytes;
            false
        }
    }

    fn resolve_single_target_pid(&mut self) -> Result<u32, String> {
        match self.mode {
            TargetMode::Pid => {
                let pid: u32 = self
                    .target_pid
                    .trim()
                    .parse()
                    .map_err(|_| "target PID is not a number".to_string())?;
                match find_process_by_pid(pid) {
                    Some(_) => Ok(pid),
                    None => Err(format!("no process with PID {}", pid)),
                }
            }
            TargetMode::Name => {
                let name = self.target_name.trim().to_string();
                if name.is_empty() {
                    return Err("target process name is empty".into());
                }
                let matches = find_processes_by_name(&name);
                match matches.first() {
                    Some(p) => Ok(p.pid),
                    None => Err(format!("no processes match '{}'", name)),
                }
            }
        }
    }

    // ---------- Scanning ----------

    pub fn spawn_scan(&mut self) {
        let slot: ScanSlot = Arc::new(Mutex::new(None));
        let slot_clone = Arc::clone(&slot);

        thread::spawn(move || {
            let result = std::panic::catch_unwind(|| scan_apps_native())
                .map_err(|_| "scan thread panicked".to_string());

            let to_store = match result {
                Ok(groups) => {
                    save_apps_cache(&groups);
                    Ok(groups)
                }
                Err(e) => Err(e),
            };

            if let Ok(mut guard) = slot_clone.lock() {
                *guard = Some(to_store);
            }
        });

        self.pending_app_scan = Some(slot);
    }

    pub fn pick_up_background_scan(&mut self) {
        if let Some(slot) = &self.pending_app_scan {
            let taken: Option<Result<Vec<AppGroup>, String>> = {
                match slot.lock() {
                    Ok(mut guard) => guard.take(),
                    Err(_) => None,
                }
            };

            if let Some(result) = taken {
                match result {
                    Ok(groups) => {
                        let count = groups.len();
                        log::info!(target: "launch", "background scan found {} groups", count);
                        self.installed_apps = groups;
                        self.scan_status = ScanStatus::Ready { count };
                    }
                    Err(e) => {
                        log::error!(target: "launch", "background scan failed: {}", e);
                        self.scan_status = ScanStatus::Failed(e);
                    }
                }
                self.pending_app_scan = None;
            }
        }
    }

    pub fn start_rescan(&mut self) {
        log::info!(target: "launch", "rescan requested");
        self.scan_status = ScanStatus::Scanning;
        self.installed_apps.clear();
        self.expanded.clear();
        self.launched_pid = None;

        let _ = std::fs::remove_file(app_cache_path());

        self.spawn_scan();
    }

    // ---------- Packet pipe reader ----------

    pub fn ensure_reader_thread(&mut self) {
        if self.reader_thread_started {
            return;
        }
        self.reader_thread_started = true;

        let store = Arc::clone(&self.store);
        let active = Arc::clone(&self.intercept_active);
        let connected = Arc::clone(&self.pipe_connected);

        thread::spawn(move || {
            const PIPE_PATH: &str = r"\\.\pipe\hook_packets";
            let mut buf = vec![0u8; 65536];

            loop {
                match std::fs::OpenOptions::new().read(true).open(PIPE_PATH) {
                    Ok(mut pipe) => {
                        connected.store(true, Ordering::Relaxed);
                        log::info!(target: "pipe", "connected to packet pipe");

                        loop {
                            let mut header = [0u8; 5];
                            if pipe.read_exact(&mut header).is_err() {
                                log::warn!(target: "pipe", "pipe closed; will reconnect");
                                break;
                            }

                            let tag = header[0];
                            let len = u32::from_le_bytes([
                                header[1], header[2], header[3], header[4],
                            ]) as usize;

                            if len == 0 || len > buf.len() {
                                log::warn!(target: "pipe", "invalid frame length {}", len);
                                break;
                            }

                            if pipe.read_exact(&mut buf[..len]).is_err() {
                                log::warn!(target: "pipe", "short read; will reconnect");
                                break;
                            }

                            match tag {
                                0x01 | 0x02 => {
                                    let direction = if tag == 0x01 {
                                        Direction::Sent
                                    } else {
                                        Direction::Received
                                    };
                                    if active.load(Ordering::Relaxed) {
                                        if let Ok(mut s) = store.lock() {
                                            s.push(0, direction, buf[..len].to_vec());
                                        }
                                    }
                                }
                                0x03 => {
                                    let text =
                                        String::from_utf8_lossy(&buf[..len]).to_string();
                                    log::info!(target: "dll", "{}", text);
                                }
                                other => {
                                    log::warn!(
                                        target: "pipe",
                                        "unknown direction byte 0x{:02x}",
                                        other
                                    );
                                    break;
                                }
                            }
                        }

                        connected.store(false, Ordering::Relaxed);
                    }
                    Err(_) => {
                        thread::sleep(Duration::from_millis(500));
                    }
                }
            }
        });
    }

    pub fn start_intercepting(&mut self) {
        self.ensure_reader_thread();
        self.intercept_active.store(true, Ordering::Relaxed);
        self.intercept_state = InterceptState::Listening;
        self.intercept_started_at = Some(Instant::now());
        self.intercept_packet_count_at_start =
            self.store.lock().map(|s| s.packets.len()).unwrap_or(0);
        log::info!(target: "intercept", "interception started");
    }

    pub fn stop_intercepting(&mut self) {
        self.intercept_active.store(false, Ordering::Relaxed);
        self.intercept_state = InterceptState::Idle;
        self.intercept_started_at = None;
        log::info!(target: "intercept", "interception stopped");
    }

    // ---------- Shutdown ----------

    pub fn shutdown_injected_dlls(&mut self) {
        if self.shutdown_done {
            return;
        }
        self.shutdown_done = true;

        log::info!(target: "shutdown", "starting injector shutdown");

        if self.project_dirty && self.current_project.is_some() {
            log::info!(target: "shutdown", "autosaving open project before exit");
            self.save_project();
        }

        {
            let mut e = self.editor();
            if e.has_dirty() {
                let saved = e.save_all();
                log::info!(target: "shutdown", "autosaved {} editor tab(s)", saved);
            }
        }

        let ok = self.send_control_message(b"shutdown");
        if ok {
            log::info!(target: "shutdown", "shutdown command delivered to DLL");
        } else {
            log::info!(target: "shutdown", "no DLL listening on control pipe");
        }

        thread::sleep(Duration::from_millis(400));

        let mut pids_to_kill: Vec<u32> = Vec::new();
        if !self.target_pid.trim().is_empty() {
            if let Ok(pid) = self.target_pid.trim().parse::<u32>() {
                pids_to_kill.push(pid);
            }
        }
        if let Some(launched) = self.launched_pid {
            if !pids_to_kill.contains(&launched) {
                pids_to_kill.push(launched);
            }
        }

        for pid in pids_to_kill {
            log::info!(target: "shutdown", "killing PID {}", pid);
            kill_process_tree(pid);
        }

        #[cfg(windows)]
        {
            kill_by_image_name("Artix Game Launcher.exe");
        }

        thread::sleep(Duration::from_millis(200));
        log::info!(target: "shutdown", "shutdown complete");
    }

    // ---------- Preflight ----------

    pub fn run_preflight(&mut self) {
        self.preflight_results.clear();
        self.preflight_message = None;

        match self.mode {
            TargetMode::Pid => {
                let pid: u32 = match self.target_pid.trim().parse() {
                    Ok(v) => v,
                    Err(_) => {
                        self.preflight_message = Some("PID is not a number".into());
                        return;
                    }
                };
                match find_process_by_pid(pid) {
                    Some(entry) => {
                        self.preflight_message =
                            Some(format!("found 1 process with PID {}", pid));
                        self.preflight_results.push(entry);
                    }
                    None => {
                        self.preflight_message = Some(format!("no process with PID {}", pid));
                    }
                }
            }
            TargetMode::Name => {
                let name = self.target_name.trim().to_string();
                if name.is_empty() {
                    self.preflight_message = Some("process name is empty".into());
                    return;
                }
                let matches = find_processes_by_name(&name);
                if matches.is_empty() {
                    self.preflight_message = Some(format!("no processes match '{}'", name));
                } else {
                    let injectable = matches
                        .iter()
                        .filter(|p| p.status == AccessStatus::Injectable)
                        .count();
                    self.preflight_message = Some(format!(
                        "found {} process(es), {} injectable",
                        matches.len(),
                        injectable
                    ));
                }
                self.preflight_results = matches;
            }
        }

        if let Some(msg) = &self.preflight_message {
            log::info!(target: "preflight", "{}", msg);
        }
    }

    // ---------- Launch and inject ----------

    pub fn do_launch_and_inject(&mut self) {
        if self.launch_path.trim().is_empty() {
            log::error!(target: "launch", "launch path is empty — select an app first");
            return;
        }
        if self.dll_path.trim().is_empty() {
            log::error!(
                target: "launch",
                "DLL path is empty — fill in the DLL PATH field before launching"
            );
            return;
        }
        if !std::path::Path::new(&self.dll_path).exists() {
            log::error!(
                target: "launch",
                "DLL path does not exist: {}",
                self.dll_path
            );
            return;
        }

        log::info!(target: "launch", "launching {}", self.launch_path);

        match launch_process(&self.launch_path) {
            Ok(child) => {
                let pid = child.id();
                log::info!(target: "launch", "spawned with PID {}", pid);
                self.launched_pid = Some(pid);
            }
            Err(e) => {
                log::error!(target: "launch", "{}", e);
                return;
            }
        }

        if !self.auto_inject_after_launch {
            log::info!(target: "launch", "auto-inject disabled; enter PID manually");
            return;
        }

        let ip = self.remote_ip.trim().to_string();
        if ip.is_empty() {
            log::error!(target: "launch", "watch IP is empty");
            return;
        }

        log::info!(
            target: "launch",
            "waiting up to 30s for a connection to {}...",
            ip
        );

        let timeout = Duration::from_secs(30);
        match wait_for_connection(&ip, timeout) {
            Some(pid) => {
                log::info!(target: "launch", "found PID {} with live connection", pid);
                self.mode = TargetMode::Pid;
                self.target_pid = pid.to_string();

                thread::sleep(Duration::from_millis(800));

                self.do_inject();
            }
            None => {
                log::warn!(
                    target: "launch",
                    "no process connected to {} within {}s",
                    ip,
                    timeout.as_secs()
                );
            }
        }
    }

    pub fn do_inject(&mut self) {
        if self.dll_path.trim().is_empty() {
            log::error!(target: "inject", "DLL path is empty");
            return;
        }

        self.ensure_reader_thread();

        let targets: Vec<u32> = match self.mode {
            TargetMode::Pid => {
                let pid: u32 = match self.target_pid.trim().parse() {
                    Ok(v) => v,
                    Err(_) => {
                        log::error!(target: "inject", "PID is not a valid number");
                        return;
                    }
                };
                match find_process_by_pid(pid) {
                    Some(entry) => {
                        log::info!(
                            target: "inject",
                            "PID {} resolved to '{}' (status: {})",
                            pid,
                            entry.name,
                            entry.status.label()
                        );
                        vec![pid]
                    }
                    None => {
                        log::error!(target: "inject", "no process found with PID {}", pid);
                        return;
                    }
                }
            }
            TargetMode::Name => {
                let name = self.target_name.trim().to_string();
                if name.is_empty() {
                    log::error!(target: "inject", "process name is empty");
                    return;
                }
                let matches = find_processes_by_name(&name);
                if matches.is_empty() {
                    log::error!(target: "inject", "no processes match '{}'", name);
                    return;
                }
                let mut pids = Vec::new();
                for p in &matches {
                    log::info!(
                        target: "inject",
                        "will target PID {} ({}, {})",
                        p.pid,
                        p.name,
                        p.status.label()
                    );
                    pids.push(p.pid);
                }
                pids
            }
        };

        let method = self.injection_method;
        log::info!(
            target: "inject",
            "injecting via {} into {} process(es)...",
            method.label(),
            targets.len()
        );

        for pid in targets {
            match inject(pid, &self.dll_path, method) {
                Ok(()) => {
                    log::info!(target: "inject", "PID {}: injection succeeded", pid);
                }
                Err(e) => {
                    log::warn!(target: "inject", "PID {}: {}", pid, e);
                }
            }
        }

        log::info!(target: "inject", "injection finished");
    }

    // ---------- Packet filtering ----------

    pub fn ensure_filter_parsed(&mut self) {
        if self.cached_filter_text == self.filter_text {
            return;
        }
        self.cached_filter_text = self.filter_text.clone();
        match parse_filter(&self.filter_text) {
            Ok(expr) => {
                self.cached_filter = expr;
                self.filter_error = None;
            }
            Err(e) => {
                self.cached_filter = None;
                self.filter_error = Some(e.message);
            }
        }
    }

    pub fn toggle_sort(&mut self, col: SortColumn) {
        if self.sort_column == col {
            self.sort_order = match self.sort_order {
                SortOrder::Asc => SortOrder::Desc,
                SortOrder::Desc => SortOrder::Asc,
            };
        } else {
            self.sort_column = col;
            self.sort_order = SortOrder::Asc;
        }
    }
}

// ============================================================
// FREE FUNCTIONS
// ============================================================

fn open_in_external_editor(path: &std::path::Path, command: &str) {
    use std::process::Command;

    let cmd = command.trim();
    let cmd = if cmd.is_empty() { "notepad" } else { cmd };

    match Command::new(cmd).arg(path).spawn() {
        Ok(_) => {
            log::info!(target: "editor", "opened {} in {}", path.display(), cmd);
        }
        Err(e) => {
            log::warn!(
                target: "editor",
                "could not launch '{}' ({}); falling back to notepad",
                cmd,
                e
            );
            let _ = Command::new("notepad").arg(path).spawn();
        }
    }
}

fn open_with_picker(path: &std::path::Path) {
    #[cfg(windows)]
    {
        use std::process::Command;

        let status = Command::new("rundll32.exe")
            .args(["shell32.dll,OpenAs_RunDLL"])
            .arg(path)
            .spawn();

        match status {
            Ok(_) => {
                log::info!(
                    target: "editor",
                    "opened picker for {}",
                    path.display()
                );
            }
            Err(e) => {
                log::warn!(
                    target: "editor",
                    "picker failed ({}); falling back to default handler",
                    e
                );
                let _ = Command::new("cmd")
                    .args(["/C", "start", ""])
                    .arg(path)
                    .spawn();
            }
        }
    }

    #[cfg(not(windows))]
    {
        use std::process::Command;
        let _ = Command::new("xdg-open").arg(path).spawn();
    }
}

#[cfg(windows)]
fn kill_process_tree(root_pid: u32) {
    use std::process::Command;

    let _ = Command::new("taskkill")
        .args(["/PID", &root_pid.to_string(), "/T", "/F"])
        .output();
}

#[cfg(not(windows))]
fn kill_process_tree(_root_pid: u32) {}

#[cfg(windows)]
fn kill_by_image_name(image_name: &str) {
    use std::process::Command;

    let _ = Command::new("taskkill")
        .args(["/IM", image_name, "/T", "/F"])
        .output();
}

#[cfg(not(windows))]
fn kill_by_image_name(_image_name: &str) {}