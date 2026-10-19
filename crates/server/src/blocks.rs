//! Blocks that players build and break, on one grid for the whole world: a column per `proto::world::BLOCK`
//! cell, starting at the layer its ground is in (its base) and spanning `COLUMN_LAYERS` layers above it. Any
//! layer may hold a block or not, so blocks bridge gaps and leave windows. Each block has a material
//! (0..BLOCK_KINDS) and an owner (the account that built it, or nobody); only its owner can break it. The sim
//! mirrors the columns into its `World`, so movement and every client's prediction climb and stop on them.
//!
//! The sim owns `Blocks`. Each tick's changes go to the web gateway's `Feed`, which serves them at
//! `/play/blocks?since=VER` (only what changed since a version the client has, or everything), and now and then
//! the whole set is written beside the world file so it survives a restart.

use std::collections::{HashMap, VecDeque};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use proto::world::{block_cell, block_key, Column, COLUMN_LAYERS};

/// Most blocks in a world.
pub const MAX_BLOCKS: usize = 200_000;
/// Changes the feed remembers; a client further behind than this gets everything again.
const FEED_LOG: usize = 8192;
const LAYERS: usize = COLUMN_LAYERS as usize;

/// One column: its base layer, and for each layer above it the material (0 = empty, else material + 1) and the
/// owner (0 = nobody, else an index into `Blocks::owners` plus one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Col {
    pub base: i8,
    pub kinds: [u8; LAYERS],
    pub owners: [u16; LAYERS],
}

impl Col {
    pub fn new(base: i8) -> Col {
        Col { base, kinds: [0; LAYERS], owners: [0; LAYERS] }
    }

    /// The layer's index in this column, if it is one of its layers.
    pub fn index(&self, z: i16) -> Option<usize> {
        let k = z - self.base as i16;
        (0..COLUMN_LAYERS).contains(&k).then_some(k as usize)
    }

    pub fn solid(&self, z: i16) -> bool {
        self.index(z).is_some_and(|k| self.kinds[k] != 0)
    }

    pub fn count(&self) -> usize {
        self.kinds.iter().filter(|&&k| k != 0).count()
    }

    pub fn column(&self) -> Column {
        Column { base: self.base, mask: self.kinds.iter().enumerate().fold(0, |m, (k, &v)| if v != 0 { m | 1 << k } else { m }) }
    }
}

/// A column as read from text, before it is checked against the world: the base if the text gave one (the
/// ground's otherwise), and each layer's material and owner's tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColText {
    pub key: u32,
    pub base: Option<i8>,
    pub layers: Vec<Option<(u8, Option<String>)>>,
}

#[derive(Default)]
pub struct Blocks {
    /// Column key -> column. Never holds an empty column.
    pub cols: HashMap<u32, Col>,
    /// Owners' account tags (16 hex digits of the identity), as `Col::owners` indexes them less one.
    pub owners: Vec<String>,
    owner_ix: HashMap<String, u16>,
    pub total: usize,
    /// Columns changed since the last `take_changes`.
    changed: Vec<u32>,
    /// True when there is something not yet saved.
    pub unsaved: bool,
    /// Counts every change, so the console knows when to fetch them again.
    pub ver: u64,
}

impl Blocks {
    /// The owner number for an account tag (registered on first use); 0 for none.
    pub fn owner(&mut self, tag: Option<&str>) -> u16 {
        let Some(tag) = tag else { return 0 };
        if let Some(&i) = self.owner_ix.get(tag) {
            return i;
        }
        if self.owners.len() >= u16::MAX as usize - 1 {
            return 0;
        }
        self.owners.push(tag.to_string());
        let i = self.owners.len() as u16;
        self.owner_ix.insert(tag.to_string(), i);
        i
    }

    pub fn get(&self, key: u32) -> Option<&Col> {
        self.cols.get(&key)
    }

    /// Put a block in layer `z` of a column (made with `base` if it is new). The caller has checked the layer is
    /// one of the column's and is empty.
    pub fn put(&mut self, key: u32, base: i8, z: i16, kind: u8, owner: u16) {
        let col = self.cols.entry(key).or_insert_with(|| Col::new(base));
        let Some(k) = col.index(z) else { return };
        if col.kinds[k] == 0 {
            self.total += 1;
        }
        col.kinds[k] = kind + 1;
        col.owners[k] = owner;
        self.touch(key);
    }

    /// Take the block out of layer `z`: its material and owner, if there was one.
    pub fn take(&mut self, key: u32, z: i16) -> Option<(u8, u16)> {
        let col = self.cols.get_mut(&key)?;
        let k = col.index(z)?;
        if col.kinds[k] == 0 {
            return None;
        }
        let got = (col.kinds[k] - 1, col.owners[k]);
        col.kinds[k] = 0;
        col.owners[k] = 0;
        self.total -= 1;
        if col.count() == 0 {
            self.cols.remove(&key);
        }
        self.touch(key);
        Some(got)
    }

    /// Replace a whole column (None removes it).
    pub fn set_col(&mut self, key: u32, col: Option<Col>) {
        if let Some(old) = self.cols.remove(&key) {
            self.total -= old.count();
        }
        if let Some(c) = col.filter(|c| c.count() > 0) {
            self.total += c.count();
            self.cols.insert(key, c);
        }
        self.touch(key);
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

    /// The columns changed since last time, with what is in them now (None: cleared).
    pub fn take_changes(&mut self) -> Vec<(u32, Option<Col>)> {
        let mut keys = std::mem::take(&mut self.changed);
        keys.sort_unstable();
        keys.dedup();
        keys.into_iter().map(|k| (k, self.cols.get(&k).copied())).collect()
    }

    /// Everything, as written to disk: `{"owners": [...], "cols": [[cx, cy, base, "..01", [0, 0, 1, 2]], ...]}`.
    pub fn to_json(&self) -> String {
        let mut keys: Vec<&u32> = self.cols.keys().collect();
        keys.sort_unstable();
        let cols = cols_json(keys.into_iter().map(|k| (*k, Some(&self.cols[k]))));
        format!(r#"{{"owners":{},"cols":{cols}}}"#, serde_json::to_string(&self.owners).unwrap_or_else(|_| "[]".into()))
    }

    /// Read columns from text: `to_json`'s, the console's `{"cols": [[cx, cy, "..01"], ...]}` (layers counted from
    /// the ground's base) and the older files where each column was a stack from its base. Bad entries are
    /// skipped; an empty layers string is kept (it clears a column, where that means something).
    pub fn parse(text: &str) -> Result<Vec<ColText>, String> {
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let cols = v.get("cols").and_then(|c| c.as_array()).ok_or("no cols")?;
        let owners: Vec<&str> = v.get("owners").and_then(|o| o.as_array()).map(|a| a.iter().map(|o| o.as_str().unwrap_or("")).collect()).unwrap_or_default();
        Ok(cols
            .iter()
            .filter_map(|c| {
                let (cx, cy) = (c.get(0)?.as_u64()? as u32, c.get(1)?.as_u64()? as u32);
                if cx >= proto::world::BLOCK_GRID || cy >= proto::world::BLOCK_GRID {
                    return None;
                }
                // [cx, cy, "layers"] counts from the ground; [cx, cy, base, "layers", owners] says where it starts.
                let (base, s, own) = match c.get(2)? {
                    serde_json::Value::String(s) => (None, s.as_str(), None),
                    b => (Some(b.as_i64()?.clamp(-120, i8::MAX as i64 - COLUMN_LAYERS as i64) as i8), c.get(3)?.as_str()?, c.get(4).and_then(|o| o.as_array())),
                };
                let layers = s
                    .bytes()
                    .take(LAYERS)
                    .enumerate()
                    .map(|(k, b)| {
                        proto::game::block_kind(b).map(|kind| {
                            let o = own.and_then(|o| o.get(k)).and_then(|o| o.as_u64()).unwrap_or(0) as usize;
                            (kind, (o > 0).then(|| owners.get(o - 1).map(|t| t.to_string())).flatten().filter(|t| !t.is_empty()))
                        })
                    })
                    .collect();
                Some(ColText { key: block_key(cx, cy), base, layers })
            })
            .collect())
    }
}

/// `[[cx, cy, base, "..01", [owners]], ...]`: layers from the base, `.` for an empty one, up to the highest block;
/// an owner per layer (0 = nobody). A cleared column is `[cx, cy, 0, "", []]`.
fn cols_json<'a>(cols: impl Iterator<Item = (u32, Option<&'a Col>)>) -> String {
    let mut o = String::from("[");
    for (i, (k, col)) in cols.enumerate() {
        let (cx, cy) = block_cell(k);
        if i > 0 {
            o.push(',');
        }
        let Some(c) = col else {
            let _ = write!(o, r#"[{cx},{cy},0,"",[]]"#);
            continue;
        };
        let top = c.kinds.iter().rposition(|&v| v != 0).map_or(0, |t| t + 1);
        let _ = write!(o, "[{cx},{cy},{},\"", c.base);
        for &v in &c.kinds[..top] {
            o.push(if v == 0 { '.' } else { proto::game::block_char(v - 1) });
        }
        o.push_str("\",[");
        for (n, &w) in c.owners[..top].iter().enumerate() {
            if n > 0 {
                o.push(',');
            }
            let _ = write!(o, "{w}");
        }
        o.push_str("]]");
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
    cols: HashMap<u32, Col>,
    owners: Vec<String>,
    log: VecDeque<(u64, u32)>,
}

impl Feed {
    /// Versions start from the clock, so a client still holding one from before a restart is behind every
    /// version of this run and gets everything.
    pub fn new() -> Feed {
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        Feed { ver: ms << 10, ..Feed::default() }
    }

    /// This tick's changes, and the owners table as it now is (it only ever grows).
    pub fn apply(&mut self, changes: Vec<(u32, Option<Col>)>, owners: &[String]) {
        if owners.len() != self.owners.len() {
            self.owners = owners.to_vec();
        }
        for (k, col) in changes {
            self.ver += 1;
            match col {
                Some(c) => self.cols.insert(k, c),
                None => self.cols.remove(&k),
            };
            self.log.push_back((self.ver, k));
            if self.log.len() > FEED_LOG {
                self.log.pop_front();
            }
        }
    }

    /// `{"ver": V, "full": bool, "owners": [...], "cols": [...]}` (as `cols_json`): what changed after `since`, or
    /// every column when the client is new or too far behind.
    pub fn since(&self, since: u64) -> String {
        let oldest = self.log.front().map_or(self.ver + 1, |e| e.0);
        let (full, body) = if since > 0 && since <= self.ver && since + 1 >= oldest {
            let mut keys: Vec<u32> = self.log.iter().filter(|e| e.0 > since).map(|e| e.1).collect();
            keys.sort_unstable();
            keys.dedup();
            (false, cols_json(keys.iter().map(|k| (*k, self.cols.get(k)))))
        } else {
            let mut keys: Vec<&u32> = self.cols.keys().collect();
            keys.sort_unstable();
            (true, cols_json(keys.into_iter().map(|k| (*k, Some(&self.cols[k])))))
        };
        let owners = serde_json::to_string(&self.owners).unwrap_or_else(|_| "[]".into());
        format!(r#"{{"ver":{},"full":{full},"owners":{owners},"cols":{body}}}"#, self.ver)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_go_in_any_layer_round_trip_and_remember_their_owner() {
        let mut b = Blocks::default();
        let (k, j) = (block_key(3, 4), block_key(9, 9));
        let me = b.owner(Some("00aa00aa00aa00aa"));
        assert_eq!(b.owner(Some("00aa00aa00aa00aa")), me);
        b.put(k, -2, -2, 1, me);
        b.put(k, -2, 0, 2, 0); // a gap at -1
        b.put(j, 5, 5, 0, me);
        assert_eq!(b.total, 3);
        assert!(b.get(k).unwrap().solid(0) && !b.get(k).unwrap().solid(-1));
        let text = b.to_json();
        assert_eq!(text, r#"{"owners":["00aa00aa00aa00aa"],"cols":[[3,4,-2,"1.2",[1,0,0]],[9,9,5,"0",[1]]]}"#);
        let back = Blocks::parse(&text).unwrap();
        let me_tag = Some("00aa00aa00aa00aa".to_string());
        assert_eq!(back[0], ColText { key: k, base: Some(-2), layers: vec![Some((1, me_tag.clone())), None, Some((2, None))] });
        // Changes come out once per column, as the column now is.
        assert_eq!(b.take_changes().len(), 2);
        assert_eq!(b.take(k, -2), Some((1, me)));
        assert_eq!(b.take(k, -2), None, "already gone");
        assert_eq!(b.take(k, 0), Some((2, 0)));
        assert!(b.get(k).is_none(), "an empty column goes");
        assert_eq!(b.take_changes(), vec![(k, None)]);
        // The console's and older files' stacks count from the ground; junk is skipped, not fatal.
        let old = Blocks::parse(r#"{"cols":[[1,2,"0b9z"],[1,2],[1,2,"00000000000000000000"]]}"#).unwrap();
        assert_eq!(old[0].layers, vec![Some((0, None)), Some((11, None)), Some((9, None)), None], "materials 0-9 then a-b; z is not one");
        assert_eq!((old.len(), old[1].layers.len()), (2, LAYERS));
    }

    #[test]
    fn the_feed_sends_what_changed_or_everything() {
        let mut f = Feed::new();
        assert!(f.since(1).contains(r#""full":true"#), "an old version gets everything");
        let col = |base: i8, kind: u8| {
            let mut c = Col::new(base);
            c.kinds[0] = kind + 1;
            Some(c)
        };
        f.apply(vec![(block_key(1, 1), col(0, 0)), (block_key(2, 2), col(1, 1))], &["a".to_string()]);
        let v = f.ver;
        f.apply(vec![(block_key(1, 1), None), (block_key(5, 5), col(-1, 3))], &["a".to_string()]);
        assert_eq!(f.since(v), format!(r#"{{"ver":{},"full":false,"owners":["a"],"cols":[[1,1,0,"",[]],[5,5,-1,"3",[0]]]}}"#, f.ver));
        assert_eq!(f.since(0), format!(r#"{{"ver":{},"full":true,"owners":["a"],"cols":[[2,2,1,"1",[0]],[5,5,-1,"3",[0]]]}}"#, f.ver));
        assert_eq!(f.since(f.ver), format!(r#"{{"ver":{},"full":false,"owners":["a"],"cols":[]}}"#, f.ver));
        // A version from the future (a restarted server) gets everything.
        assert!(f.since(f.ver + 50).contains(r#""full":true"#));
    }
}
