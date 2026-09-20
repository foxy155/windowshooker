use crate::theme;
use log::{Level, LevelFilter, Log, Metadata, Record};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct LogEntry {
    pub level: Level,
    pub target: String,
    pub message: String,
    pub timestamp: String,
}

impl LogEntry {
    pub fn color(&self) -> egui::Color32 {
        match self.level {
            Level::Error => theme::error(),
            Level::Warn => theme::warn(),
            Level::Info => theme::info(),
            Level::Debug => theme::debug(),
            Level::Trace => theme::trace(),
        }
    }
}

pub struct GuiLogger {
    entries: Arc<Mutex<Vec<LogEntry>>>,
    max_entries: usize,
}

impl Log for GuiLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        let target = record.target();
        if target.starts_with("egui")
            || target.starts_with("winit")
            || target.starts_with("epaint")
            || target.starts_with("eframe")
            || target.starts_with("glutin")
            || target.starts_with("naga")
            || target.starts_with("wgpu")
        {
            return;
        }

        let entry = LogEntry {
            level: record.level(),
            target: target.to_string(),
            message: format!("{}", record.args()),
            timestamp: format_now(),
        };

        if let Ok(mut guard) = self.entries.lock() {
            guard.push(entry);
            if guard.len() > self.max_entries {
                let excess = guard.len() - self.max_entries;
                guard.drain(0..excess);
            }
        }
    }

    fn flush(&self) {}
}

pub fn install_logger(entries: Arc<Mutex<Vec<LogEntry>>>) {
    let logger = GuiLogger {
        entries,
        max_entries: 5000,
    };
    log::set_boxed_logger(Box::new(logger)).expect("logger already set");
    log::set_max_level(LevelFilter::Trace);
}

pub fn format_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();
    let secs_of_day = secs % 86400;
    let h = secs_of_day / 3600;
    let m = (secs_of_day % 3600) / 60;
    let s = secs_of_day % 60;
    format!("{:02}:{:02}:{:02}.{:03}", h, m, s, millis)
}