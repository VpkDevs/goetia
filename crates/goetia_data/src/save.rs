//! Save substrate: versioned RON snapshot + keyed blob store, written
//! atomically (temp file + rename) so a crash mid-write can't corrupt an
//! existing save.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub type SaveResult<T> = Result<T, String>;

/// The on-disk container. `sections` hold game-defined serialized state under
/// stable string keys; `version` gates migration on load.
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct SaveFile {
    pub version: u32,
    pub sections: BTreeMap<String, String>,
}

impl SaveFile {
    pub const CURRENT_VERSION: u32 = 1;

    pub fn new() -> Self {
        SaveFile {
            version: Self::CURRENT_VERSION,
            sections: BTreeMap::new(),
        }
    }

    /// Serialize any serde value into a named section.
    pub fn put<T: Serialize>(&mut self, key: &str, value: &T) -> SaveResult<()> {
        let s = ron::to_string(value).map_err(|e| e.to_string())?;
        self.sections.insert(key.to_string(), s);
        Ok(())
    }

    pub fn take<T: serde::de::DeserializeOwned>(&self, key: &str) -> SaveResult<T> {
        let s = self
            .sections
            .get(key)
            .ok_or_else(|| format!("missing section '{key}'"))?;
        ron::from_str(s).map_err(|e| format!("section '{key}': {e}"))
    }

    pub fn has(&self, key: &str) -> bool {
        self.sections.contains_key(key)
    }

    /// Atomic write: serialize to `<path>.tmp`, then rename over the target.
    pub fn write(&self, path: impl AsRef<Path>) -> SaveResult<()> {
        let path = path.as_ref();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|e| e.to_string())?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, text.as_bytes()).map_err(|e| e.to_string())?;
        // On Windows, rename fails if target exists; remove first. The window
        // where neither file exists is why we keep `.tmp` on any failure.
        if path.exists() {
            std::fs::remove_file(path).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn read(path: impl AsRef<Path>) -> SaveResult<SaveFile> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let sf: SaveFile = ron::from_str(&text).map_err(|e| e.to_string())?;
        if sf.version > Self::CURRENT_VERSION {
            return Err(format!(
                "save version {} is newer than engine {}",
                sf.version,
                Self::CURRENT_VERSION
            ));
        }
        Ok(sf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Bank {
        souls: u64,
        relics: Vec<String>,
    }

    #[test]
    fn roundtrip_atomic() {
        let dir = std::env::temp_dir().join("goetia_save_test");
        let p = dir.join("run.save.ron");
        let mut sf = SaveFile::new();
        let bank = Bank {
            souls: 666,
            relics: vec!["ashen_idol".into()],
        };
        sf.put("bank", &bank).unwrap();
        sf.write(&p).unwrap();
        // Overwrite must also succeed (rename-over-existing path).
        sf.put(
            "bank",
            &Bank {
                souls: 667,
                relics: vec![],
            },
        )
        .unwrap();
        sf.write(&p).unwrap();
        let loaded = SaveFile::read(&p).unwrap();
        let got: Bank = loaded.take("bank").unwrap();
        assert_eq!(got.souls, 667);
        assert!(!p.with_extension("tmp").exists());
    }
}
