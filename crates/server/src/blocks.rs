//! Blocks that players build and break: columns on a grid of `proto::world::BLOCK` units, each a stack of up to
//! `MAX_STACK` blocks of a material (0..BLOCK_KINDS). A column with anything in it is solid; the sim mirrors that
//! into its `World` so movement, and every client's prediction, treat it like a wall.
//!
//! The sim owns `Blocks`. Each tick's changes go to the web gateway's `Feed`, which serves them at
//! `/play/blocks?since=VER` (only what changed since a version the client has, or everything), and now and then
//! the whole set is written beside the world file so it survives a restart.

use std::collections::{HashMap, VecDeque};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use proto::world::{block_cell, block_key};

/// Most blocks in a world.
pub const MAX_BLOCKS: usize = 200_000;
/// Changes the feed remembers; a client further behind than this gets everything again.
const FEED_LOG: usize = 8192;

#[derive(Default)]
pub struct Blocks {
    /// Column key -> materials from the bottom up. Never holds an empty stack.
    pub cols: HashMap<u32, Vec<u8>>,
    pub total: usize,
    /// Columns changed since the last `take_changes`.
    changed: Vec<u32>,
    /// True when there is something not yet saved.
    pub unsaved: bool,
    /// Counts every change, so the console knows when to fetch them again.
    pub ver: u64,
}

impl Blocks {
    pub fn height(&self, key: u32) -> usize {
        self.cols.get(&key).map_or(0, |c| c.len())
    }

    pub fn push(&mut self, key: u32, kind: u8) {
        self.cols.entry(key).or_default().push(kind);
        self.total += 1;
        self.touch(key);
    }

    /// Take the top block off; true if the column is now empty.
    pub fn pop(&mut self, key: u32) -> bool {
        let Some(c) = self.cols.get_mut(&key) else { return false };
        c.pop();
        self.total -= 1;
        let empty = c.is_empty();
        if empty {
            self.cols.remove(&key);
        }
        self.touch(key);
        empty
    }

    /// Remove a whole column.
    pub fn clear_col(&mut self, key: u32) {
        if let Some(c) = self.cols.remove(&key) {
            self.total -= c.len();
            self.touch(key);
        }
    }

    /// Empty every column (each reported as changed); their keys.
    pub fn forget_all(&mut self) -> Vec<u32> {
        let keys: Vec<u32> = self.cols.drain().map(|(k, _)| k).collect();
        self.total = 0;
        for &k in &keys {
            self.touch(k);
        }
        keys
    }

    fn touch(&mut self, key: u32) {
        self.ver += 1;
        self.changed.push(key);
        self.unsaved = true;
    }

    /// The columns changed since last time, with what is in them now (empty: cleared).
    pub fn take_changes(&mut self) -> Vec<(u32, Vec<u8>)> {
        let mut keys = std::mem::take(&mut self.changed);
        keys.sort_unstable();
        keys.dedup();
        keys.into_iter().map(|k| (k, self.cols.get(&k).cloned().unwrap_or_default())).collect()
    }

    /// Every column, as written to disk and sent to a client that has nothing yet: `[[cx, cy, "0123"], ...]`.
    pub fn cols_json(&self) -> String {
        let mut keys: Vec<&u32> = self.cols.keys().collect();
        keys.sort_unstable();
        cols_json(keys.into_iter().map(|k| (*k, self.cols[k].as_slice())))
    }

    /// Read what `cols_json` wrote (wrapped in `{"cols": ...}` on disk); bad entries are skipped. An empty
    /// materials string is kept (it clears a column, where that means something).
    pub fn parse_with_clears(text: &str) -> Result<Vec<(u32, Vec<u8>)>, String> {
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let cols = v.get("cols").and_then(|c| c.as_array()).ok_or("no cols")?;
        Ok(cols
            .iter()
            .filter_map(|c| {
                let (cx, cy, s) = (c.get(0)?.as_u64()? as u32, c.get(1)?.as_u64()? as u32, c.get(2)?.as_str()?);
                let kinds: Vec<u8> = s.bytes().filter(|b| b.is_ascii_digit()).map(|b| b - b'0').filter(|&k| k < proto::game::BLOCK_KINDS).take(proto::game::MAX_STACK).collect();
                (cx < proto::world::BLOCK_GRID && cy < proto::world::BLOCK_GRID).then(|| (block_key(cx, cy), kinds))
            })
            .collect())
    }

    /// Read what `cols_json` wrote (wrapped in `{"cols": ...}` on disk); bad entries are skipped.
    pub fn parse(text: &str) -> Result<Vec<(u32, Vec<u8>)>, String> {
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let cols = v.get("cols").and_then(|c| c.as_array()).ok_or("no cols")?;
        Ok(cols
            .iter()
            .filter_map(|c| {
                let (cx, cy, s) = (c.get(0)?.as_u64()? as u32, c.get(1)?.as_u64()? as u32, c.get(2)?.as_str()?);
                let kinds: Vec<u8> = s.bytes().filter(|b| b.is_ascii_digit()).map(|b| b - b'0').filter(|&k| k < proto::game::BLOCK_KINDS).take(proto::game::MAX_STACK).collect();
                (cx < proto::world::BLOCK_GRID && cy < proto::world::BLOCK_GRID && !kinds.is_empty()).then(|| (block_key(cx, cy), kinds))
            })
            .collect())
    }
}

fn cols_json<'a>(cols: impl Iterator<Item = (u32, &'a [u8])>) -> String {
    let mut o = String::from("[");
    for (i, (k, kinds)) in cols.enumerate() {
        let (cx, cy) = block_cell(k);
        if i > 0 {
            o.push(',');
        }
        let _ = write!(o, "[{cx},{cy},\"");
        for &b in kinds {
            o.push((b'0' + b) as char);
        }
        o.push_str("\"]");
    }
    o.push(']');
    o
}

/// Where a world's blocks are kept: beside its world file.
pub fn path_for(world: &Path) -> PathBuf {
    world.with_extension("blocks.json")
}

/// Write the blocks off the sim thread (the caller hands over the text), replacing the file atomically.
pub fn save_in_background(path: PathBuf, text: String) {
    std::thread::spawn(move || {
        let tmp = path.with_extension("tmp");
        if let Err(e) = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &path)) {
            eprintln!("blocks: not saved to {}: {e}", path.display());
        }
    });
}

/// What the web gateway serves at `/play/blocks`: the columns as they are, and a log of recent changes so a
/// client can ask for only what is new.
#[derive(Default)]
pub struct Feed {
    pub ver: u64,
    cols: HashMap<u32, Vec<u8>>,
    log: VecDeque<(u64, u32)>,
}

impl Feed {
    /// Versions start from the clock, so a client still holding one from before a restart is behind every
    /// version of this run and gets everything.
    pub fn new() -> Feed {
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        Feed { ver: ms << 10, ..Feed::default() }
    }

    pub fn apply(&mut self, changes: Vec<(u32, Vec<u8>)>) {
        for (k, kinds) in changes {
            self.ver += 1;
            if kinds.is_empty() {
                self.cols.remove(&k);
            } else {
                self.cols.insert(k, kinds);
            }
            self.log.push_back((self.ver, k));
            if self.log.len() > FEED_LOG {
                self.log.pop_front();
            }
        }
    }

    /// `{"ver": V, "full": bool, "cols": [[cx, cy, "kinds"], ...]}`: what changed after `since` (an empty kinds
    /// string is a cleared column), or every column when the client is new or too far behind.
    pub fn since(&self, since: u64) -> String {
        let oldest = self.log.front().map_or(self.ver + 1, |e| e.0);
        let (full, body) = if since > 0 && since <= self.ver && since + 1 >= oldest {
            let mut keys: Vec<u32> = self.log.iter().filter(|e| e.0 > since).map(|e| e.1).collect();
            keys.sort_unstable();
            keys.dedup();
            (false, cols_json(keys.iter().map(|k| (*k, self.cols.get(k).map_or(&[][..], |c| c.as_slice())))))
        } else {
            let mut keys: Vec<&u32> = self.cols.keys().collect();
            keys.sort_unstable();
            (true, cols_json(keys.into_iter().map(|k| (*k, self.cols[k].as_slice()))))
        };
        format!(r#"{{"ver":{},"full":{full},"cols":{body}}}"#, self.ver)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stacks_change_and_round_trip() {
        let mut b = Blocks::default();
        let k = block_key(3, 4);
        b.push(k, 1);
        b.push(k, 2);
        b.push(block_key(9, 9), 0);
        assert_eq!((b.height(k), b.total), (2, 3));
        assert!(!b.pop(k));
        let text = format!(r#"{{"cols":{}}}"#, b.cols_json());
        assert_eq!(text, r#"{"cols":[[3,4,"1"],[9,9,"0"]]}"#);
        let back = Blocks::parse(&text).unwrap();
        assert_eq!(back, vec![(k, vec![1]), (block_key(9, 9), vec![0])]);
        // Changes come out once per column, as the column now is.
        assert_eq!(b.take_changes(), vec![(k, vec![1]), (block_key(9, 9), vec![0])]);
        assert!(b.pop(k));
        assert_eq!(b.take_changes(), vec![(k, vec![])]);
        assert!(b.take_changes().is_empty());
        // Junk on disk is skipped, not fatal.
        assert_eq!(Blocks::parse(r#"{"cols":[[1,2,"9"],[1,2],[1,2,"00000000000"]]}"#).unwrap(), vec![(block_key(1, 2), vec![0; 8])]);
    }

    #[test]
    fn the_feed_sends_what_changed_or_everything() {
        let mut f = Feed::new();
        assert!(f.since(1).contains(r#""full":true"#), "an old version gets everything");
        f.apply(vec![(block_key(1, 1), vec![0]), (block_key(2, 2), vec![1, 1])]);
        let v = f.ver;
        f.apply(vec![(block_key(1, 1), vec![]), (block_key(5, 5), vec![3])]);
        assert_eq!(f.since(v), format!(r#"{{"ver":{},"full":false,"cols":[[1,1,""],[5,5,"3"]]}}"#, f.ver));
        assert_eq!(f.since(0), format!(r#"{{"ver":{},"full":true,"cols":[[2,2,"11"],[5,5,"3"]]}}"#, f.ver));
        assert_eq!(f.since(f.ver), format!(r#"{{"ver":{},"full":false,"cols":[]}}"#, f.ver));
        // A version from the future (a restarted server) gets everything.
        assert!(f.since(f.ver + 50).contains(r#""full":true"#));
    }
}
