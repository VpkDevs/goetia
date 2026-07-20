//! Typed RON data registry with mtime-based hot reload (no file-watcher
//! dependency; the app polls once per second in debug builds).
//!
//! One registry per data type: `DataRegistry<StatusDef>`, `DataRegistry<RoomTemplate>`, …
//! Files load as RON; a reload swaps the value in place and bumps `version`
//! so systems can notice.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub struct DataHandle(usize);

struct Entry<T> {
    path: PathBuf,
    mtime: Option<SystemTime>,
    pub value: T,
}

pub struct DataRegistry<T> {
    entries: Vec<Entry<T>>,
    /// Bumped on every successful (re)load; cheap change detection.
    pub version: u64,
    /// When false, `poll_reload` is a no-op (release builds).
    pub hot_reload: bool,
}

impl<T: serde::de::DeserializeOwned> DataRegistry<T> {
    pub fn new() -> Self {
        DataRegistry {
            entries: Vec::new(),
            version: 0,
            hot_reload: cfg!(debug_assertions),
        }
    }

    /// Load a RON file. Returns a stable handle.
    pub fn load(&mut self, path: impl AsRef<Path>) -> Result<DataHandle, String> {
        let path = path.as_ref().to_path_buf();
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let value: T = ron::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        self.entries.push(Entry { path, mtime, value });
        self.version += 1;
        Ok(DataHandle(self.entries.len() - 1))
    }

    /// Register an in-memory value (tests, embedded defaults).
    pub fn insert(&mut self, value: T) -> DataHandle {
        self.entries.push(Entry {
            path: PathBuf::new(),
            mtime: None,
            value,
        });
        self.version += 1;
        DataHandle(self.entries.len() - 1)
    }

    pub fn get(&self, h: &DataHandle) -> &T {
        &self.entries[h.0].value
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.entries.iter().map(|e| &e.value)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Re-read any file whose mtime changed. Parse errors keep the old value
    /// (bad hot edits must never crash a run). Returns number reloaded.
    pub fn poll_reload(&mut self) -> usize {
        if !self.hot_reload {
            return 0;
        }
        let mut n = 0;
        for e in &mut self.entries {
            if e.path.as_os_str().is_empty() {
                continue;
            }
            let mtime = std::fs::metadata(&e.path).and_then(|m| m.modified()).ok();
            if mtime.is_some() && mtime != e.mtime {
                e.mtime = mtime;
                match std::fs::read_to_string(&e.path)
                    .map_err(|e| e.to_string())
                    .and_then(|t| ron::from_str::<T>(&t).map_err(|e| e.to_string()))
                {
                    Ok(v) => {
                        e.value = v;
                        n += 1;
                        log::info!("hot-reloaded {}", e.path.display());
                    }
                    Err(err) => log::warn!("hot-reload failed for {}: {err}", e.path.display()),
                }
            }
        }
        if n > 0 {
            self.version += 1;
        }
        n
    }
}

impl<T: serde::de::DeserializeOwned> Default for DataRegistry<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize, PartialEq, Debug)]
    struct Cfg {
        speed: f32,
        name: String,
    }

    #[test]
    fn load_and_reload() {
        // Unique per process: leftover files from a previous run can otherwise
        // collide with mtime granularity and flake the reload assertions.
        let dir = std::env::temp_dir().join(format!("goetia_data_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("cfg.ron");
        std::fs::write(&p, r#"(speed: 1.5, name: "imp")"#).unwrap();
        let mut reg: DataRegistry<Cfg> = DataRegistry::new();
        reg.hot_reload = true;
        let h = reg.load(&p).unwrap();
        assert_eq!(reg.get(&h).speed, 1.5);
        // Bad edit: keeps old value.
        std::fs::write(&p, "(speed: broken").unwrap();
        force_mtime_change(&p);
        reg.poll_reload();
        assert_eq!(reg.get(&h).speed, 1.5);
        // Good edit: picked up.
        std::fs::write(&p, r#"(speed: 3.0, name: "imp")"#).unwrap();
        force_mtime_change(&p);
        let v0 = reg.version;
        assert_eq!(reg.poll_reload(), 1);
        assert_eq!(reg.get(&h).speed, 3.0);
        assert!(reg.version > v0);
    }

    fn force_mtime_change(p: &Path) {
        // mtime granularity can swallow same-instant writes; nudge via touch.
        let meta = std::fs::metadata(p).unwrap();
        let new = meta.modified().unwrap() + std::time::Duration::from_secs(2);
        let f = std::fs::OpenOptions::new().write(true).open(p).unwrap();
        f.set_modified(new).unwrap();
    }
}
