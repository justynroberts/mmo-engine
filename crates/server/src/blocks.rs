//! Blocks that players build and break, on one grid for the whole world: a column per `proto::world::BLOCK`
//! cell, starting at the layer its ground is in (its base) and spanning `COLUMN_LAYERS` layers above it. Any
//! layer may hold a block or not, so blocks bridge gaps and leave windows. Each block has a material
//! (0..BLOCK_KINDS) and an owner (the account that built it, or nobody); only its owner can break it. The sim
//! mirrors the columns into its `World`, so movement and every client's prediction climb and stop on them.
//!
//! The sim owns `Blocks`. Each tick's changes go to the web gateway's `Feed`, which serves them by region at
//! `/play/blocks/r` (`proto::blockfeed`: only the regions near a client, only those that changed since the version
//! it holds), and still at `/play/blocks?since=VER` for older clients. Blocks are kept by region too
//! (`proto::blockfeed::REGION` cells square, 256 world units): every few seconds the regions that changed are
//! written, one small file each, beside the world file (`<world>.blocks.d/`), by a writer thread of their own.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use proto::blockfeed::{region_key, region_of};
use proto::world::{block_cell, block_key, Column, COLUMN_LAYERS};

/// Most blocks in a world.
pub const MAX_BLOCKS: usize = 2_000_000;

/// The region a column key is in, as `proto::blockfeed::region_key`.
pub fn region_of_key(key: u32) -> u32 {
    let (cx, cy) = block_cell(key);
    let (rx, ry) = region_of(cx, cy);
    region_key(rx, ry)
}
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
    /// Region key -> the columns in it, so a region can be written or served without looking at the others.
    pub regions: HashMap<u32, BTreeSet<u32>>,
    /// Blocks per region and per owner (for the build limits in `GameFile::building`).
    pub per_region: HashMap<u32, u32>,
    pub per_owner: HashMap<u16, u32>,
    /// Regions changed since the last `take_dirty`.
    dirty: HashSet<u32>,
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
        let (was_kind, was_owner) = (col.kinds[k], col.owners[k]);
        col.kinds[k] = kind + 1;
        col.owners[k] = owner;
        if was_kind != 0 {
            self.count(key, was_owner, -1);
        }
        self.count(key, owner, 1);
        self.regions.entry(region_of_key(key)).or_default().insert(key);
        self.touch(key);
    }

    /// Keep the totals (all, per region, per owner) as a block comes (+1) or goes (-1).
    fn count(&mut self, key: u32, owner: u16, d: i32) {
        self.total = (self.total as i64 + d as i64) as usize;
        let r = self.per_region.entry(region_of_key(key)).or_default();
        *r = (*r as i32 + d).max(0) as u32;
        if owner != 0 {
            let o = self.per_owner.entry(owner).or_default();
            *o = (*o as i32 + d).max(0) as u32;
        }
    }

    fn unindex(&mut self, key: u32) {
        let r = region_of_key(key);
        if let Some(set) = self.regions.get_mut(&r) {
            set.remove(&key);
            if set.is_empty() {
                self.regions.remove(&r);
            }
        }
    }

    /// How many blocks are in the region this column is in, and how many this owner has built.
    pub fn in_region(&self, key: u32) -> u32 {
        self.per_region.get(&region_of_key(key)).copied().unwrap_or(0)
    }
    pub fn by_owner(&self, owner: u16) -> u32 {
        self.per_owner.get(&owner).copied().unwrap_or(0)
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
        let empty = col.count() == 0;
        self.count(key, got.1, -1);
        if empty {
            self.cols.remove(&key);
            self.unindex(key);
        }
        self.touch(key);
        Some(got)
    }

    /// Replace a whole column (None removes it).
    pub fn set_col(&mut self, key: u32, col: Option<Col>) {
        if let Some(old) = self.cols.remove(&key) {
            for k in 0..LAYERS {
                if old.kinds[k] != 0 {
                    self.count(key, old.owners[k], -1);
                }
            }
            self.unindex(key);
        }
        if let Some(c) = col.filter(|c| c.count() > 0) {
            for k in 0..LAYERS {
                if c.kinds[k] != 0 {
                    self.count(key, c.owners[k], 1);
                }
            }
            self.cols.insert(key, c);
            self.regions.entry(region_of_key(key)).or_default().insert(key);
        }
        self.touch(key);
    }

    /// Empty every column (each reported as changed); their keys.
    pub fn forget_all(&mut self) -> Vec<u32> {
        let keys: Vec<u32> = self.cols.drain().map(|(k, _)| k).collect();
        self.total = 0;
        self.regions.clear();
        self.per_region.clear();
        self.per_owner.clear();
        for &k in &keys {
            self.touch(k);
        }
        keys
    }

    fn touch(&mut self, key: u32) {
        self.ver += 1;
        self.changed.push(key);
        self.dirty.insert(region_of_key(key));
        self.unsaved = true;
    }

    /// The regions changed since last time, each as the text its file holds (as `to_json`, for that region only),
    /// or None for a region that is now empty (its file goes).
    pub fn take_dirty(&mut self) -> Vec<(u32, Option<String>)> {
        let mut out: Vec<(u32, Option<String>)> = Vec::with_capacity(self.dirty.len());
        for r in std::mem::take(&mut self.dirty) {
            out.push((r, self.regions.get(&r).map(|keys| self.json_of(keys.iter().copied()))));
        }
        out.sort_unstable_by_key(|e| e.0);
        out
    }

    /// Mark every region as changed, so the next save writes them all (moving from the one-file store).
    pub fn all_dirty(&mut self) {
        self.dirty.extend(self.regions.keys().copied());
        self.unsaved = true;
    }

    /// Some columns as `to_json` writes them, with an owners table of their own.
    fn json_of(&self, keys: impl Iterator<Item = u32>) -> String {
        let mut owners: Vec<String> = Vec::new();
        let mut local: HashMap<u16, u16> = HashMap::new();
        let mut cols: Vec<(u32, Col)> = Vec::new();
        for k in keys {
            let Some(c) = self.cols.get(&k) else { continue };
            let mut c = *c;
            for o in c.owners.iter_mut().filter(|o| **o != 0) {
                *o = *local.entry(*o).or_insert_with(|| {
                    owners.push(self.owners.get(*o as usize - 1).cloned().unwrap_or_default());
                    owners.len() as u16
                });
            }
            cols.push((k, c));
        }
        let body = cols_json(cols.iter().map(|(k, c)| (*k, Some(c))));
        format!(r#"{{"owners":{},"cols":{body}}}"#, serde_json::to_string(&owners).unwrap_or_else(|_| "[]".into()))
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

/// Where a world's blocks were kept before they were kept by region: one file beside the world file. Still read
/// (once, to move them into regions) and left in place.
pub fn path_for(world: &Path) -> PathBuf {
    world.with_extension("blocks.json")
}

/// Where a world's blocks are kept: a directory beside the world file, a file per region (`<rx>_<ry>.json`).
pub fn dir_for(world: &Path) -> PathBuf {
    world.with_extension("blocks.d")
}

/// Every region file in a block directory, read as `Blocks::parse` reads one file. None if there is no directory.
pub fn load_dir(dir: &Path) -> Option<Vec<ColText>> {
    let rd = std::fs::read_dir(dir).ok()?;
    let mut out = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| Blocks::parse(&t)) {
            Ok(cols) => out.extend(cols),
            Err(e) => eprintln!("blocks: {} skipped: {e}", p.display()),
        }
    }
    Some(out)
}

/// Writes region files on a thread of its own, in the order they were handed over (so an older save of a region
/// never lands after a newer one), each replaced atomically.
pub struct RegionWriter {
    tx: Option<std::sync::mpsc::Sender<Vec<(u32, Option<String>)>>>,
    done: Option<std::thread::JoinHandle<()>>,
}

impl RegionWriter {
    pub fn new(dir: PathBuf) -> RegionWriter {
        let (tx, rx) = std::sync::mpsc::channel::<Vec<(u32, Option<String>)>>();
        let done = std::thread::Builder::new()
            .name("blocks-writer".into())
            .spawn(move || {
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    eprintln!("blocks: cannot make {}: {e}", dir.display());
                }
                for batch in rx {
                    for (r, text) in batch {
                        let path = dir.join(format!("{}_{}.json", r & 0xffff, r >> 16));
                        let res = match text {
                            Some(t) => {
                                let tmp = path.with_extension("tmp");
                                std::fs::write(&tmp, t).and_then(|_| std::fs::rename(&tmp, &path))
                            }
                            None => std::fs::remove_file(&path).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) }),
                        };
                        if let Err(e) = res {
                            eprintln!("blocks: {} not saved: {e}", path.display());
                        }
                    }
                }
            })
            .ok();
        RegionWriter { tx: Some(tx), done }
    }

    pub fn save(&self, batch: Vec<(u32, Option<String>)>) {
        if !batch.is_empty() {
            if let Some(tx) = &self.tx {
                let _ = tx.send(batch);
            }
        }
    }

    /// Write whatever is queued and stop (at shutdown).
    pub fn finish(mut self) {
        self.tx = None;
        if let Some(h) = self.done.take() {
            let _ = h.join();
        }
    }
}

/// What the web gateway serves at `/play/blocks`: the columns as they are, and a log of recent changes so a
/// client can ask for only what is new.
#[derive(Default)]
pub struct Feed {
    pub ver: u64,
    cols: HashMap<u32, Col>,
    owners: Vec<String>,
    log: VecDeque<(u64, u32)>,
    /// Region key -> its version and columns, and its encoded record once someone has asked for it.
    regions: HashMap<u32, RegionFeed>,
    /// Region versions count up from here: different on every run, so a client's versions from before a
    /// restart never match.
    next_region_ver: u32,
}

#[derive(Default)]
struct RegionFeed {
    ver: u32,
    keys: BTreeSet<u32>,
    enc: Mutex<Option<Arc<Vec<u8>>>>,
}

impl Feed {
    /// Versions start from the clock, so a client still holding one from before a restart is behind every
    /// version of this run and gets everything.
    pub fn new() -> Feed {
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        // Spread the run's start over the u32 range (and never 0, which a client sends for "nothing yet").
        let start = ((ms as u32) ^ (ms >> 32) as u32).wrapping_mul(2_654_435_761) | 1;
        Feed { ver: ms << 10, next_region_ver: start, ..Feed::default() }
    }

    /// This tick's changes, and the owners table as it now is (it only ever grows).
    pub fn apply(&mut self, changes: Vec<(u32, Option<Col>)>, owners: &[String]) {
        if owners.len() != self.owners.len() {
            self.owners = owners.to_vec();
        }
        let mut touched: HashSet<u32> = HashSet::new();
        for (k, col) in changes {
            self.ver += 1;
            let r = self.regions.entry(region_of_key(k)).or_default();
            match col {
                Some(c) => {
                    r.keys.insert(k);
                    self.cols.insert(k, c)
                }
                None => {
                    r.keys.remove(&k);
                    self.cols.remove(&k)
                }
            };
            touched.insert(region_of_key(k));
            self.log.push_back((self.ver, k));
            if self.log.len() > FEED_LOG {
                self.log.pop_front();
            }
        }
        for r in touched {
            self.next_region_ver = self.next_region_ver.wrapping_add(1).max(1);
            if let Some(rf) = self.regions.get_mut(&r) {
                rf.ver = self.next_region_ver;
                *rf.enc.get_mut().unwrap_or_else(|e| e.into_inner()) = None;
            }
        }
    }

    /// The reply to `/play/blocks/r`: each asked-for region whose version is not the one the client holds (a
    /// region it has never had and that holds nothing is left out).
    pub fn regions_reply(&self, ask: &[(u32, u32, u32)]) -> Vec<u8> {
        let mut recs: Vec<Arc<Vec<u8>>> = Vec::new();
        for &(rx, ry, have) in ask {
            match self.regions.get(&region_key(rx, ry)) {
                Some(rf) if rf.ver != have && !(have == 0 && rf.keys.is_empty()) => recs.push(self.region_bytes(rx, ry, rf)),
                None if have != 0 => recs.push(Arc::new(proto::blockfeed::encode_region(rx, ry, 0, std::iter::empty()))),
                _ => {}
            }
        }
        proto::blockfeed::encode_reply(&recs.iter().map(|r| r.as_slice()).collect::<Vec<_>>())
    }

    /// A region's record, encoded once per version however many clients ask.
    fn region_bytes(&self, rx: u32, ry: u32, rf: &RegionFeed) -> Arc<Vec<u8>> {
        let mut enc = rf.enc.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(b) = enc.as_ref() {
            return b.clone();
        }
        let tag = |o: u16| (o > 0).then(|| self.owners.get(o as usize - 1).map(|s| s.as_str())).flatten();
        let cols = rf.keys.iter().filter_map(|k| self.cols.get(k).map(|c| (*k, c))).map(|(key, c)| {
            let mut layers = [None; LAYERS];
            for k in 0..LAYERS {
                if c.kinds[k] != 0 {
                    layers[k] = Some((c.kinds[k] - 1, tag(c.owners[k])));
                }
            }
            proto::blockfeed::ColOut { key, base: c.base, layers }
        });
        let b = Arc::new(proto::blockfeed::encode_region(rx, ry, rf.ver, cols));
        *enc = Some(b.clone());
        b
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

    fn one(kind: u8) -> Col {
        let mut c = Col::new(0);
        c.kinds[0] = kind + 1;
        c
    }

    #[test]
    fn counts_by_region_and_owner_follow_every_change() {
        let mut b = Blocks::default();
        let me = b.owner(Some("00aa00aa00aa00aa"));
        let (k, far) = (block_key(3, 4), block_key(300, 4));
        b.put(k, 0, 0, 1, me);
        b.put(k, 0, 1, 1, me);
        b.put(k, 0, 1, 2, 0); // over the top of our own: now nobody's
        b.put(far, 0, 0, 1, me);
        assert_eq!((b.total, b.in_region(k), b.in_region(far), b.by_owner(me)), (3, 2, 1, 2));
        b.take(k, 0);
        b.set_col(far, None);
        assert_eq!((b.total, b.in_region(k), b.in_region(far), b.by_owner(me)), (1, 1, 0, 0));
        assert_eq!(b.regions.len(), 1, "an emptied region leaves the index");
        b.forget_all();
        assert_eq!((b.total, b.in_region(k), b.regions.len()), (0, 0, 0));
    }

    #[test]
    fn only_changed_regions_are_saved_and_they_read_back() {
        let mut b = Blocks::default();
        let me = b.owner(Some("00aa00aa00aa00aa"));
        b.put(block_key(3, 4), -1, -1, 1, me);
        b.put(block_key(130, 4), 2, 2, 5, 0);
        let d = b.take_dirty();
        assert_eq!(d.len(), 2);
        assert!(b.take_dirty().is_empty(), "nothing new");
        b.put(block_key(131, 4), 2, 2, 5, 0);
        let d2 = b.take_dirty();
        assert_eq!(d2.len(), 1, "one region changed");
        let back = Blocks::parse(d2[0].1.as_deref().unwrap()).unwrap();
        assert_eq!(back.len(), 2, "the whole of that region");
        // Each region file has its own owners table.
        let first = Blocks::parse(d[0].1.as_deref().unwrap()).unwrap();
        assert_eq!(first[0].layers[0], Some((1, Some("00aa00aa00aa00aa".into()))));
        b.take(block_key(3, 4), -1);
        assert_eq!(b.take_dirty(), vec![(region_of_key(block_key(3, 4)), None)], "an empty region's file goes");
    }

    #[test]
    fn the_writer_keeps_regions_in_files_that_load_back() {
        let dir = std::env::temp_dir().join(format!("blocks-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut b = Blocks::default();
        for x in 0..300 {
            b.put(block_key(x, 7), 0, 0, (x % 5) as u8, 0);
        }
        b.all_dirty();
        let w = RegionWriter::new(dir.clone());
        w.save(b.take_dirty());
        b.take(block_key(299, 7), 0);
        w.save(b.take_dirty());
        w.finish();
        let files = std::fs::read_dir(&dir).unwrap().count();
        let cols = load_dir(&dir).unwrap();
        assert_eq!((files, cols.len()), (3, 299));
        assert!(load_dir(&dir.join("nope")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_feed_serves_only_the_regions_a_client_lacks() {
        use proto::blockfeed::decode_reply;
        let mut f = Feed::new();
        f.apply(vec![(block_key(1, 1), Some(one(0))), (block_key(200, 1), Some(one(3)))], &[]);
        let all = decode_reply(&f.regions_reply(&[(0, 0, 0), (1, 0, 0), (2, 0, 0)])).unwrap();
        assert_eq!(all.len(), 2, "region (2, 0) never had anything");
        let (v0, v1) = (all[0].ver, all[1].ver);
        assert!(decode_reply(&f.regions_reply(&[(0, 0, v0), (1, 0, v1)])).unwrap().is_empty(), "nothing changed");
        f.apply(vec![(block_key(1, 1), None)], &[]);
        let r = decode_reply(&f.regions_reply(&[(0, 0, v0), (1, 0, v1)])).unwrap();
        assert_eq!((r.len(), r[0].rx, r[0].cols.len()), (1, 0, 0), "the change, and only it: the region now empty");
        assert_ne!(r[0].ver, v0);
        // A version this server never gave (one from before a restart) gets the region as it is.
        assert_eq!(decode_reply(&f.regions_reply(&[(1, 0, v1 ^ 0x5555)])).unwrap().len(), 1);
    }

    /// Fill a world with 200,000 blocks, then measure what a client near the middle downloads, what a save
    /// costs, and the same for the one-file store: `cargo test -p server --release blocks_at_scale -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn blocks_at_scale() {
        use std::time::Instant;
        let mut b = Blocks::default();
        let owners: Vec<u16> = (0..200).map(|i| b.owner(Some(&format!("{i:016x}")))).collect();
        let mut rng = 1u32;
        let mut next = || {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            rng
        };
        // Towns of walls and towers scattered over a 4096-unit world (2048 cells), four-high on average.
        while b.total < 200_000 {
            let (cx, cy) = (next() % 2040, next() % 2040);
            let h = 1 + next() % 8;
            let o = owners[(next() % 200) as usize];
            for z in 0..h as i16 {
                b.put(block_key(cx, cy), 0, z, (next() % 14) as u8, o);
            }
        }
        let t = Instant::now();
        let legacy = b.to_json();
        let legacy_ms = t.elapsed().as_secs_f64() * 1e3;
        let legacy_gz = crate::worldfile::gzip(legacy.as_bytes(), 1).len();
        b.all_dirty();
        let t = Instant::now();
        let dirty = b.take_dirty();
        let all_ms = t.elapsed().as_secs_f64() * 1e3;
        b.put(block_key(1000, 1000), 0, 12, 1, 0);
        let t = Instant::now();
        let one_save = b.take_dirty();
        let one_ms = t.elapsed().as_secs_f64() * 1e3;
        let mut f = Feed::new();
        f.apply(b.cols.iter().map(|(k, c)| (*k, Some(*c))).collect(), &b.owners);
        let (px, py) = (1000 / proto::blockfeed::REGION, 1000 / proto::blockfeed::REGION);
        let ask: Vec<(u32, u32, u32)> = (0..5).flat_map(|dy| (0..5).map(move |dx| (px + dx - 2, py + dy - 2, 0))).collect();
        let t = Instant::now();
        let near = f.regions_reply(&ask);
        let near_ms = t.elapsed().as_secs_f64() * 1e3;
        let t = Instant::now();
        let again = f.regions_reply(&ask);
        let cached_ms = t.elapsed().as_secs_f64() * 1e3;
        assert_eq!(near, again);
        let near_gz = crate::worldfile::gzip(&near, 1).len();
        let full = f.since(0).len();
        println!("{} blocks in {} columns, {} regions", b.total, b.cols.len(), b.regions.len());
        println!("one file:   {:.1} MB JSON ({:.1} MB gzipped), built in {legacy_ms:.0} ms on the sim thread at every save", legacy.len() as f64 / 1e6, legacy_gz as f64 / 1e6);
        println!("by region:  all {} regions in {all_ms:.0} ms; one changed region in {one_ms:.2} ms ({} file)", dirty.len(), one_save.len());
        println!("joining, old feed: {:.1} MB JSON for the whole world", full as f64 / 1e6);
        println!("joining, 5x5 regions near the middle: {:.0} KB binary, {:.0} KB gzipped, encoded in {near_ms:.1} ms, {cached_ms:.2} ms the next time", near.len() as f64 / 1e3, near_gz as f64 / 1e3);
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
