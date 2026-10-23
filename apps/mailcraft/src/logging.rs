//! A small logger: records go to standard error and to `mailcraft.log` in the data folder (the
//! previous run's log is kept as `mailcraft.1.log`). `info` for MailCraft, `warn` for the rest.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use log::{Level, LevelFilter, Log, Metadata, Record};

struct Logger {
    file: Mutex<Option<File>>,
}

impl Log for Logger {
    fn enabled(&self, m: &Metadata<'_>) -> bool {
        if m.target().starts_with("mailcraft") { m.level() <= Level::Info } else { m.level() <= Level::Warn }
    }

    fn log(&self, r: &Record<'_>) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let line = format!("{} {:<5} {}: {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), r.level(), r.target(), r.args());
        let _ = std::io::stderr().write_all(line.as_bytes());
        if let Ok(mut g) = self.file.lock()
            && let Some(f) = g.as_mut() {
                let _ = f.write_all(line.as_bytes());
            }
    }

    fn flush(&self) {}
}

pub fn init(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join("mailcraft.log");
    let _ = std::fs::rename(&path, dir.join("mailcraft.1.log"));
    let file = File::create(&path).ok();
    let logger = Box::leak(Box::new(Logger { file: Mutex::new(file) }));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(LevelFilter::Info);
    }
    std::panic::set_hook(Box::new(|info| {
        log::error!("panic: {info}");
    }));
}
