//! Player persistence: one SQLite row per player identity.
//!
//! Writes never touch the tick. The sim thread hands batches of records to a
//! writer thread over a channel; the writer coalesces whatever has queued up
//! into one transaction. Reads happen once per connection, on the sim thread,
//! through a separate read connection (WAL mode, so the writer does not block
//! it). A point query by primary key on a warm page is microseconds; the cost
//! is measured in `connect_load_is_fast` and a stall there would show up as a
//! slow tick rather than a lost packet.
//!
//! A record stores attributes by *name*, so adding, removing or reordering
//! attributes in the game file never scrambles a saved character: unknown
//! names are ignored on load and missing ones take the new default.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Record {
    pub x: f32,
    pub y: f32,
    pub attrs: BTreeMap<String, f32>,
    /// Carried items by name, so a changed item list never scrambles a bag.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inv: Vec<(String, u8)>,
    /// The display name the player claimed (`proto::names`), empty if they never did.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Set when the player left this zone for another (`--zone` worlds): that zone has their character now, and
    /// signing in here sends them there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone: Option<u8>,
}

pub type PlayerId = [u8; 32];

/// One saved bot. The archetype is stored by name, like attributes, so editing the game
/// file never scrambles a saved world: bots of archetypes that no longer exist are dropped.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct BotRec {
    pub arch: String,
    pub x: f32,
    pub y: f32,
    pub attrs: BTreeMap<String, f32>,
}

/// The saved population of the world (everything that is not a player).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct WorldSave {
    pub bots: Vec<BotRec>,
}

enum Msg {
    Save(Vec<(PlayerId, Record)>),
    /// `None` forgets the saved world.
    World(Option<String>),
    Flush(Sender<()>),
}

#[derive(Default)]
pub struct StoreStats {
    pub saved: AtomicU64,
    pub batches: AtomicU64,
    pub loaded: AtomicU64,
    pub errors: AtomicU64,
}

pub struct Store {
    path: std::path::PathBuf,
    tx: Sender<Msg>,
    read: Mutex<Connection>,
    /// Records queued for the writer but not yet committed, so a load right after a
    /// save (a quick reconnect, a session takeover) still sees the newest state.
    recent: Arc<Mutex<HashMap<PlayerId, Record>>>,
    pub stats: Arc<StoreStats>,
}

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS players (
    id      BLOB PRIMARY KEY,
    data    TEXT NOT NULL,
    updated INTEGER NOT NULL
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS world (
    key     TEXT PRIMARY KEY,
    data    TEXT NOT NULL,
    updated INTEGER NOT NULL
) WITHOUT ROWID;";

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

impl Store {
    pub fn open(path: &Path) -> Result<Store, String> {
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
        }
        let err = |e: rusqlite::Error| format!("{}: {e}", path.display());
        let write = Connection::open(path).map_err(err)?;
        // WAL lets the reader run while the writer commits; NORMAL sync is fine for a game
        // (a power cut may lose the last few seconds of progress, never corrupt the file).
        write.pragma_update(None, "journal_mode", "WAL").map_err(err)?;
        write.pragma_update(None, "synchronous", "NORMAL").map_err(err)?;
        write.execute_batch(SCHEMA).map_err(err)?;
        let read = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(err)?;

        let stats = Arc::new(StoreStats::default());
        let (tx, rx) = channel::<Msg>();
        let st = stats.clone();
        let recent: Arc<Mutex<HashMap<PlayerId, Record>>> = Arc::default();
        let recent_w = recent.clone();
        std::thread::Builder::new()
            .name("store-writer".into())
            .spawn(move || {
                let mut write = write;
                let mut warned = false;
                while let Ok(first) = rx.recv() {
                    let mut batch: Vec<(PlayerId, Record)> = Vec::new();
                    let mut acks: Vec<Sender<()>> = Vec::new();
                    // Only the newest world snapshot matters; `Some(None)` is a request to forget it.
                    let mut world: Option<Option<String>> = None;
                    let mut take = |m: Msg, batch: &mut Vec<(PlayerId, Record)>, acks: &mut Vec<Sender<()>>| match m {
                        Msg::Save(v) => batch.extend(v),
                        Msg::World(w) => world = Some(w),
                        Msg::Flush(a) => acks.push(a),
                    };
                    take(first, &mut batch, &mut acks);
                    // Coalesce whatever else is already waiting into the same transaction.
                    while let Ok(m) = rx.try_recv() {
                        take(m, &mut batch, &mut acks);
                    }
                    if let Some(w) = world {
                        let r = match w {
                            Some(text) => write.execute("INSERT OR REPLACE INTO world (key, data, updated) VALUES ('bots', ?1, ?2)", params![text, now_secs()]),
                            None => write.execute("DELETE FROM world WHERE key = 'bots'", []),
                        };
                        if r.is_err() {
                            st.errors.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    if !batch.is_empty() {
                        match write_batch(&mut write, &batch) {
                            Ok(()) => {
                                // Committed: forget the in-memory copy unless a newer save replaced it meanwhile.
                                if let Ok(mut r) = recent_w.lock() {
                                    for (id, rec) in &batch {
                                        if r.get(id) == Some(rec) {
                                            r.remove(id);
                                        }
                                    }
                                }
                                st.saved.fetch_add(batch.len() as u64, Ordering::Relaxed);
                                st.batches.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(e) => {
                                st.errors.fetch_add(1, Ordering::Relaxed);
                                if !warned {
                                    warned = true;
                                    eprintln!("store: write failed (further errors are only counted): {e}");
                                }
                            }
                        }
                    }
                    for a in acks {
                        let _ = a.send(());
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Store { path: path.to_path_buf(), tx, read: Mutex::new(read), recent, stats })
    }

    /// Queue records for the writer thread. Never blocks.
    pub fn save(&self, records: Vec<(PlayerId, Record)>) {
        if records.is_empty() {
            return;
        }
        if let Ok(mut r) = self.recent.lock() {
            for (id, rec) in &records {
                r.insert(*id, rec.clone());
            }
        }
        let _ = self.tx.send(Msg::Save(records));
    }

    /// Queue a snapshot of the bot population. Only the newest one is kept.
    pub fn save_world(&self, w: &WorldSave) {
        if let Ok(text) = serde_json::to_string(w) {
            let _ = self.tx.send(Msg::World(Some(text)));
        }
    }

    /// Forget the saved world (a deliberate restart under new definitions).
    #[allow(dead_code)] // kept for tests and tools; world edits now carry the bots over instead
    pub fn clear_world(&self) {
        let _ = self.tx.send(Msg::World(None));
    }

    /// The saved population, if there is a readable one. Call `flush` first if you just saved.
    pub fn load_world(&self) -> Option<WorldSave> {
        let conn = self.read.lock().ok()?;
        let text: Option<String> = conn.query_row("SELECT data FROM world WHERE key = 'bots'", [], |r| r.get(0)).optional().ok().flatten();
        text.and_then(|t| serde_json::from_str(&t).ok())
    }

    /// Wait until everything queued so far is on disk (shutdown, tests).
    pub fn flush(&self) {
        let (a, b) = channel();
        if self.tx.send(Msg::Flush(a)).is_ok() {
            let _ = b.recv_timeout(std::time::Duration::from_secs(10));
        }
    }

    pub fn load(&self, id: &PlayerId) -> Option<Record> {
        if let Some(r) = self.recent.lock().ok().and_then(|r| r.get(id).cloned()) {
            self.stats.loaded.fetch_add(1, Ordering::Relaxed);
            return Some(r);
        }
        let conn = self.read.lock().ok()?;
        let text: Option<String> = conn
            .query_row("SELECT data FROM players WHERE id = ?1", params![&id[..]], |r| r.get(0))
            .optional()
            .ok()
            .flatten();
        let rec = text.and_then(|t| serde_json::from_str::<Record>(&t).ok());
        if rec.is_some() {
            self.stats.loaded.fetch_add(1, Ordering::Relaxed);
        }
        rec
    }

    /// Every claimed name and whose it is, for keeping names unique. Runs before the tick loop starts.
    pub fn names(&self) -> Vec<(PlayerId, String)> {
        let Ok(conn) = self.read.lock() else { return Vec::new() };
        let Ok(mut q) = conn.prepare("SELECT id, data FROM players") else { return Vec::new() };
        let rows = q.query_map([], |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?)));
        let mut out = Vec::new();
        for (id, data) in rows.into_iter().flatten().flatten() {
            let (Ok(id), Ok(rec)) = (PlayerId::try_from(id.as_slice()), serde_json::from_str::<Record>(&data)) else { continue };
            if !rec.name.is_empty() {
                out.push((id, rec.name));
            }
        }
        out
    }

    /// Delete players not saved for `max_age_secs`. Returns how many went. Runs before
    /// the tick loop starts, so it may take its time.
    pub fn expire(&self, max_age_secs: i64) -> usize {
        let Ok(conn) = Connection::open(&self.path) else { return 0 };
        conn.execute("DELETE FROM players WHERE updated < ?1", params![now_secs() - max_age_secs]).unwrap_or(0)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn count(&self) -> usize {
        self.read
            .lock()
            .ok()
            .and_then(|c| c.query_row("SELECT COUNT(*) FROM players", [], |r| r.get::<_, i64>(0)).ok())
            .map_or(0, |n| n as usize)
    }
}

fn write_batch(conn: &mut Connection, batch: &[(PlayerId, Record)]) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare_cached("INSERT OR REPLACE INTO players (id, data, updated) VALUES (?1, ?2, ?3)")?;
        let now = now_secs();
        for (id, rec) in batch {
            let data = serde_json::to_string(rec).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            stmt.execute(params![&id[..], data, now])?;
        }
    }
    tx.commit()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("store-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("world.db")
    }

    fn rec(x: f32, hp: f32) -> Record {
        Record { x, y: x * 2.0, attrs: BTreeMap::from([("health".to_string(), hp), ("energy".to_string(), 50.0)]), inv: Vec::new(), name: String::new(), zone: None }
    }

    #[test]
    fn saved_players_come_back_after_a_reopen() {
        let path = tmp("reopen");
        {
            let s = Store::open(&path).unwrap();
            s.save(vec![([1; 32], rec(10.0, 70.0)), ([2; 32], rec(20.0, 30.0))]);
            s.flush();
            assert_eq!(s.count(), 2);
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.load(&[1; 32]), Some(rec(10.0, 70.0)));
        assert_eq!(s.load(&[2; 32]).unwrap().attrs["health"], 30.0);
        assert_eq!(s.load(&[3; 32]), None);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_later_save_replaces_the_earlier_one() {
        let path = tmp("replace");
        let s = Store::open(&path).unwrap();
        s.save(vec![([7; 32], rec(1.0, 10.0))]);
        s.save(vec![([7; 32], rec(5.0, 99.0))]);
        s.flush();
        assert_eq!(s.load(&[7; 32]), Some(rec(5.0, 99.0)));
        assert_eq!(s.count(), 1);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_writer_coalesces_queued_batches() {
        let path = tmp("coalesce");
        let s = Store::open(&path).unwrap();
        for i in 0..200u8 {
            s.save(vec![([i; 32], rec(i as f32, 1.0))]);
        }
        s.flush();
        assert_eq!(s.count(), 200);
        assert!(s.stats.batches.load(Ordering::Relaxed) <= 200);
        assert_eq!(s.stats.saved.load(Ordering::Relaxed), 200);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn garbage_in_the_table_is_ignored_not_fatal() {
        let path = tmp("garbage");
        let s = Store::open(&path).unwrap();
        s.save(vec![([1; 32], rec(1.0, 1.0))]);
        s.flush();
        Connection::open(&path).unwrap().execute("UPDATE players SET data = '{not json'", []).unwrap();
        assert_eq!(s.load(&[1; 32]), None);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn connect_load_is_fast() {
        let path = tmp("speed");
        let s = Store::open(&path).unwrap();
        s.save((0..5000u32).map(|i| ({ let mut id = [0u8; 32]; id[..4].copy_from_slice(&i.to_le_bytes()); id }, rec(i as f32, 50.0))).collect());
        s.flush();
        let t = std::time::Instant::now();
        let mut hit = 0;
        for i in 0..2000u32 {
            let mut id = [0u8; 32];
            id[..4].copy_from_slice(&(i * 2 % 5000).to_le_bytes());
            hit += s.load(&id).is_some() as u32;
        }
        let per = t.elapsed().as_secs_f64() * 1e6 / 2000.0;
        assert_eq!(hit, 2000);
        eprintln!("load: {per:.1} us per connection");
        assert!(per < 500.0, "{per} us per load is too slow for the sim thread");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn unwritable_location_is_a_clear_error() {
        let err = Store::open(Path::new("/proc/definitely/not/here/world.db")).err().expect("should fail");
        assert!(err.contains("world.db") || err.contains("/proc"), "{err}");
    }

    #[test]
    fn a_load_right_after_a_save_sees_the_new_state() {
        let path = tmp("ryw");
        let s = Store::open(&path).unwrap();
        for i in 0..300u32 {
            s.save(vec![([9; 32], rec(i as f32, 10.0))]);
            assert_eq!(s.load(&[9; 32]).map(|r| r.x), Some(i as f32), "save {i} not visible immediately");
        }
        s.flush();
        assert_eq!(s.load(&[9; 32]).map(|r| r.x), Some(299.0));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn old_players_expire_and_recent_ones_stay() {
        let path = tmp("expire");
        let s = Store::open(&path).unwrap();
        s.save(vec![([1; 32], rec(1.0, 1.0)), ([2; 32], rec(2.0, 2.0))]);
        s.flush();
        Connection::open(&path).unwrap().execute("UPDATE players SET updated = updated - 100 * 86400 WHERE id = ?1", params![&[1u8; 32][..]]).unwrap();
        assert_eq!(s.expire(90 * 86400), 1);
        assert_eq!(s.load(&[1; 32]), None);
        assert!(s.load(&[2; 32]).is_some());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_saved_world_round_trips_keeps_only_the_newest_and_can_be_forgotten() {
        let path = tmp("world");
        let st = Store::open(&path).unwrap();
        assert_eq!(st.load_world(), None);
        let bot = |arch: &str, x: f32| BotRec { arch: arch.into(), x, y: x + 1.0, attrs: BTreeMap::from([("health".to_string(), 12.0)]) };
        st.save_world(&WorldSave { bots: vec![bot("old", 1.0)] });
        st.save_world(&WorldSave { bots: vec![bot("drone", 5.0), bot("guard", 7.0)] });
        st.flush();
        let w = st.load_world().unwrap();
        assert_eq!(w.bots.len(), 2);
        assert_eq!(w.bots[0], bot("drone", 5.0));
        drop(st);
        let st = Store::open(&path).unwrap();
        assert_eq!(st.load_world().unwrap().bots.len(), 2, "survives a reopen");
        st.clear_world();
        st.flush();
        assert_eq!(st.load_world(), None);
        // Garbage in the table reads as "no saved world", not a crash.
        let c = Connection::open(&path).unwrap();
        c.execute("INSERT OR REPLACE INTO world (key, data, updated) VALUES ('bots', 'not json', 0)", []).unwrap();
        assert_eq!(st.load_world(), None);
    }
}
