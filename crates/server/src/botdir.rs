//! Drop-in bot types: put a JSON file in the bots directory and the bot appears in the running world.
//!
//! Each `*.json` file holds one bot type (an object) or several (an array), in exactly the shape the console's
//! Builder writes into the game file. The directory is checked every couple of seconds; a new or changed file
//! is merged into the game definitions by bot name (a file wins over an earlier bot of the same name), checked
//! with the same validation as the console, and applied live through the same path as an edit in the Builder.
//! That means no restart, no recompile and no dropped players, and the result is saved into the game file.
//!
//! Files whose names start with `_` or `.` are ignored, which is how the shipped example stays inert. Deleting
//! a file does not remove the bot (the game file owns it now); remove it in the console.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use proto::game::{Archetype, GameFile};

const EVERY: Duration = Duration::from_secs(2);

#[derive(Clone, Default, Debug)]
pub struct BotDirStatus {
    pub dir: String,
    /// Files read on the last successful pass.
    pub files: Vec<String>,
    /// Bot types those files define.
    pub bots: Vec<String>,
    /// Why the newest set of files was not applied, if it was not. Cleared by the next good pass.
    pub error: Option<String>,
    /// Times a change was applied.
    pub applied: u64,
}

pub struct BotDir {
    dir: PathBuf,
    next: Instant,
    seen: Option<Vec<(String, u64, u128)>>,
    pub status: BotDirStatus,
}

const EXAMPLE: &str = r##"{
  "name": "warden",
  "color": "#ff3df0",
  "count": 4,
  "behaviour": "wander",
  "speed_mult": 0.8,
  "sight": 60,
  "attack": "zap",
  "rules": [
    { "when": { "kind": "sees", "who": "player", "within": 40 }, "then": { "kind": "attack" } }
  ]
}
"##;

impl BotDir {
    /// Watch `dir`, creating it with an inert example if it does not exist.
    pub fn open(dir: &Path) -> BotDir {
        if !dir.exists() && std::fs::create_dir_all(dir).is_ok() {
            let _ = std::fs::write(dir.join("_example.json"), EXAMPLE);
        }
        BotDir {
            dir: dir.to_path_buf(),
            next: Instant::now(),
            seen: None,
            status: BotDirStatus { dir: dir.display().to_string(), ..Default::default() },
        }
    }

    fn scan(&self) -> Vec<(String, u64, u128)> {
        let mut v = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if !name.ends_with(".json") || name.starts_with('_') || name.starts_with('.') {
                    continue;
                }
                let Ok(m) = e.metadata() else { continue };
                if !m.is_file() {
                    continue;
                }
                let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
                v.push((name, m.len(), t));
            }
        }
        v.sort();
        v
    }

    /// Check the directory (at most every couple of seconds). Returns the merged game definitions when there is
    /// something new to apply.
    pub fn poll(&mut self, base: &GameFile) -> Option<GameFile> {
        let now = Instant::now();
        if now < self.next {
            return None;
        }
        self.next = now + EVERY;
        let seen = self.scan();
        if self.seen.as_ref() == Some(&seen) {
            return None;
        }
        self.seen = Some(seen.clone());
        match self.merge(base, &seen) {
            Ok((merged, files, bots)) => {
                self.status.error = None;
                self.status.files = files;
                self.status.bots = bots;
                if merged == *base {
                    return None;
                }
                self.status.applied += 1;
                Some(merged)
            }
            Err(e) => {
                eprintln!("bots dir {}: {e}", self.dir.display());
                self.status.error = Some(e);
                None
            }
        }
    }

    fn merge(&self, base: &GameFile, seen: &[(String, u64, u128)]) -> Result<(GameFile, Vec<String>, Vec<String>), String> {
        let mut merged = base.clone();
        let mut files = Vec::new();
        let mut bots = Vec::new();
        for (name, _, _) in seen {
            let text = std::fs::read_to_string(self.dir.join(name)).map_err(|e| format!("{name}: {e}"))?;
            for a in parse_file(&text).map_err(|e| format!("{name}: {e}"))? {
                bots.push(a.name.clone());
                match merged.archetypes.iter_mut().find(|x| x.name == a.name) {
                    Some(slot) => *slot = a,
                    None => merged.archetypes.push(a),
                }
            }
            files.push(name.clone());
        }
        merged.build().map_err(|e| format!("not applied: {e}"))?;
        Ok((merged, files, bots))
    }
}

/// One bot type or an array of them. Errors name the field, since "did not match any variant" helps nobody.
pub fn parse_file(text: &str) -> Result<Vec<Archetype>, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
    let one = |v: serde_json::Value| serde_json::from_value::<Archetype>(v).map_err(|e| e.to_string());
    match v {
        serde_json::Value::Array(items) => items.into_iter().map(one).collect(),
        other => Ok(vec![one(other)?]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("botdir-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn the_shipped_example_is_valid_but_ignored() {
        let base = GameFile::default_game();
        let a = parse_file(EXAMPLE).unwrap();
        assert_eq!(a[0].name, "warden");
        let mut g = base.clone();
        g.archetypes.push(a[0].clone());
        g.build().expect("the example must build against the default game");
        let d = tmp("example");
        let mut b = BotDir::open(&d);
        assert!(d.join("_example.json").exists());
        assert!(b.poll(&base).is_none(), "an underscore file is not loaded");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_dropped_file_adds_a_bot_and_a_changed_file_replaces_it() {
        let base = GameFile::default_game();
        let d = tmp("drop");
        let mut b = BotDir::open(&d);
        std::fs::write(d.join("warden.json"), EXAMPLE).unwrap();
        let g = b.poll(&base).expect("new file applies");
        assert_eq!(g.archetypes.len(), base.archetypes.len() + 1);
        assert_eq!(b.status.bots, vec!["warden"]);
        // Nothing changed: nothing to do.
        b.next = Instant::now();
        assert!(b.poll(&g).is_none());
        // Edit the file (different length, so the signature changes even on a coarse clock).
        std::fs::write(d.join("warden.json"), EXAMPLE.replace("\"count\": 4", "\"count\": 12")).unwrap();
        b.next = Instant::now();
        let g2 = b.poll(&g).expect("changed file applies");
        assert_eq!(g2.archetypes.iter().find(|a| a.name == "warden").unwrap().count, 12);
        assert_eq!(g2.archetypes.len(), g.archetypes.len(), "replaced by name, not duplicated");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_bad_file_is_reported_and_changes_nothing() {
        let base = GameFile::default_game();
        let d = tmp("bad");
        let mut b = BotDir::open(&d);
        std::fs::write(d.join("broken.json"), r##"{"name":"x","colour":"#fff"}"##).unwrap();
        assert!(b.poll(&base).is_none());
        let e = b.status.error.clone().expect("reported");
        assert!(e.contains("broken.json") && e.contains("colour"), "{e}");
        // Unknown action: parses, fails validation.
        std::fs::write(d.join("broken.json"), r#"{"name":"x","attack":"no-such-action"}"#).unwrap();
        b.next = Instant::now();
        assert!(b.poll(&base).is_none());
        assert!(b.status.error.is_some());
        // Fixing it clears the error.
        std::fs::write(d.join("broken.json"), r#"[{"name":"x"},{"name":"y","count":2}]"#).unwrap();
        b.next = Instant::now();
        let g = b.poll(&base).expect("fixed");
        assert!(b.status.error.is_none());
        assert_eq!(g.archetypes.len(), base.archetypes.len() + 2);
        let _ = std::fs::remove_dir_all(&d);
    }
}
