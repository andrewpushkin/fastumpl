//! Where the app keeps its own files: recent files and recovery copies.
//!
//! Windows (portable): a `data` folder next to the executable, or
//! `%APPDATA%\fastumpl` if that folder cannot be written.
//! Elsewhere: `$XDG_CONFIG_HOME/fastumpl` (default `~/.config/fastumpl`) for
//! settings and `$XDG_CACHE_HOME/fastumpl` (default `~/.cache/fastumpl`) for
//! recovery copies.

use std::fs;
use std::path::{Path, PathBuf};

fn writable(dir: &Path) -> bool {
    if fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".write-test");
    let ok = fs::write(&probe, b"").is_ok();
    let _ = fs::remove_file(&probe);
    ok
}

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

#[cfg(windows)]
fn base_dirs() -> (PathBuf, PathBuf) {
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        let data = exe_dir.join("data");
        if writable(&data) {
            return (data.clone(), data);
        }
    }
    let app = env_dir("APPDATA").unwrap_or_else(std::env::temp_dir).join("fastumpl");
    (app.clone(), app)
}

#[cfg(not(windows))]
fn base_dirs() -> (PathBuf, PathBuf) {
    let home = env_dir("HOME").unwrap_or_else(std::env::temp_dir);
    let config = env_dir("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config")).join("fastumpl");
    let cache = env_dir("XDG_CACHE_HOME").unwrap_or_else(|| home.join(".cache")).join("fastumpl");
    (config, cache)
}

pub struct Storage {
    pub config: PathBuf,
    pub cache: PathBuf,
}

impl Storage {
    pub fn new() -> Storage {
        let (config, cache) = base_dirs();
        let _ = writable(&config);
        let _ = writable(&cache);
        Storage { config, cache }
    }

    pub fn recovery_dir(&self) -> PathBuf {
        self.cache.join("recovery")
    }

    pub fn load_recent(&self) -> Vec<PathBuf> {
        fs::read_to_string(self.config.join("recent.txt"))
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(PathBuf::from)
            .collect()
    }

    pub fn save_recent(&self, files: &[PathBuf]) {
        let text: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
        let _ = fs::create_dir_all(&self.config);
        let _ = fs::write(self.config.join("recent.txt"), text.join("\n"));
    }

    pub fn load_setting(&self, key: &str) -> Option<String> {
        fs::read_to_string(self.config.join("settings.txt"))
            .ok()?
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{key}=")).map(str::to_string))
    }

    pub fn save_setting(&self, key: &str, value: &str) {
        let path = self.config.join("settings.txt");
        let mut lines: Vec<String> = fs::read_to_string(&path)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.starts_with(&format!("{key}=")))
            .map(str::to_string)
            .collect();
        lines.push(format!("{key}={value}"));
        let _ = fs::create_dir_all(&self.config);
        let _ = fs::write(path, lines.join("\n"));
    }
}

/// Adds `path` to the front of the recent list (at most 10 entries).
pub fn push_recent(list: &mut Vec<PathBuf>, path: &Path) {
    let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    list.retain(|p| p != &canon);
    list.insert(0, canon);
    list.truncate(10);
}
