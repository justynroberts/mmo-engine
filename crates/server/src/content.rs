//! The game's content (items, spells, bot types, quests, places: the whole `GameFile`) kept in SQLite, so it can be
//! changed live, with any SQLite tool or through the admin console, and every version kept for rolling back.
//!
//! Each list in the game file is a table of its own, named as in the file (`items`, `actions`, `archetypes`,
//! `quests`, ...): one row per entry, `pos` its place in the list, `data` the entry as JSON, and `name` and `title`
//! read out of it for browsing. Everything else (`rules`, `levels`, `daylight`, `start_place`, ...) is a row of
//! `settings`. `sections` says which tables are lists, in the file's order.
//!
//! The server reads the content at start. Its own thread (`run`) then watches the file: a change made by another
//! program (a SQLite browser, a script) is put together into a game file, checked and applied live exactly as a
//! console edit is; one that does not check out is refused, said why in `changes`, and the world goes on unchanged.
//! Every version applied is kept whole (gzipped JSON) in `history`, numbered: `/api/content/history` lists them and
//! `/api/content/rollback` puts one back, as a new version, so a rollback can itself be undone.

use proto::game::GameFile;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

/// How many versions are kept (the oldest go first).
const KEEP: i64 = 2000;
/// How often the file is looked at for changes made by other programs.
const WATCH: Duration = Duration::from_secs(2);

/// One version of the content, for the history list.
#[derive(serde::Serialize, Clone, Debug)]
pub struct Version {
    pub rev: i64,
    /// Unix seconds.
    pub at: i64,
    pub note: String,
    pub bytes: usize,
}

/// A save for the content thread: the game file as applied, and what made the change.
pub struct Save {
    pub game: String,
    pub note: String,
}

pub struct Content {
    conn: Connection,
}

fn ident_ok(k: &str) -> bool {
    !k.is_empty() && k.len() <= 64 && k.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') && !k.starts_with(|c: char| c.is_ascii_digit())
}

/// A stable fingerprint of a game file's text.
fn fingerprint(text: &str) -> String {
    let h = blake3::hash(text.as_bytes());
    h.as_bytes().iter().take(12).map(|b| format!("{b:02x}")).collect()
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

impl Content {
    pub fn open(path: &Path) -> Result<Content, String> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let conn = Connection::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=5000;
             CREATE TABLE IF NOT EXISTS sections (key TEXT PRIMARY KEY, pos INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, data TEXT NOT NULL CHECK (json_valid(data)));
             CREATE TABLE IF NOT EXISTS history (rev INTEGER PRIMARY KEY, at INTEGER NOT NULL, note TEXT NOT NULL, fingerprint TEXT NOT NULL, game BLOB NOT NULL);
             CREATE TABLE IF NOT EXISTS changes (at INTEGER NOT NULL, rev INTEGER, note TEXT NOT NULL, result TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS translations (key TEXT NOT NULL, lang TEXT NOT NULL, text TEXT NOT NULL, PRIMARY KEY (key, lang));",
        )
        .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Content { conn })
    }

    /// Whether the content has never been filled.
    pub fn is_empty(&self) -> bool {
        self.conn.query_row("SELECT COUNT(*) FROM history", [], |r| r.get::<_, i64>(0)).map_or(true, |n| n == 0)
    }

    /// The game file as the tables hold it now, as JSON (not yet checked).
    pub fn assemble(&self) -> Result<String, String> {
        let mut obj = Map::new();
        let mut st = self.conn.prepare("SELECT key, data FROM settings").map_err(|e| e.to_string())?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(|e| e.to_string())?;
        for row in rows {
            let (k, d) = row.map_err(|e| e.to_string())?;
            obj.insert(k.clone(), serde_json::from_str(&d).map_err(|e| format!("settings '{k}': {e}"))?);
        }
        let mut st = self.conn.prepare("SELECT key FROM sections ORDER BY pos").map_err(|e| e.to_string())?;
        let keys: Vec<String> = st.query_map([], |r| r.get(0)).map_err(|e| e.to_string())?.filter_map(Result::ok).collect();
        for k in keys {
            if !ident_ok(&k) {
                return Err(format!("section '{k}' is not a plain name"));
            }
            let mut st = self.conn.prepare(&format!("SELECT pos, data FROM \"{k}\" ORDER BY pos")).map_err(|e| format!("{k}: {e}"))?;
            let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))).map_err(|e| format!("{k}: {e}"))?;
            let mut list = Vec::new();
            for row in rows {
                let (pos, d) = row.map_err(|e| e.to_string())?;
                list.push(serde_json::from_str::<Value>(&d).map_err(|e| format!("{k} row {pos}: {e}"))?);
            }
            obj.insert(k, Value::Array(list));
        }
        serde_json::to_string(&Value::Object(obj)).map_err(|e| e.to_string())
    }

    /// The content now, checked: the game file, or why it does not build.
    pub fn load(&self) -> Result<GameFile, String> {
        let text = self.assemble()?;
        let g = GameFile::from_json(&text)?;
        g.build()?;
        Ok(g)
    }

    /// The fingerprint of the version last applied.
    fn applied(&self) -> Option<String> {
        self.conn.query_row("SELECT fingerprint FROM history ORDER BY rev DESC LIMIT 1", [], |r| r.get(0)).optional().ok().flatten()
    }

    /// Write `game` (a checked game file's JSON) into the tables and keep it as a new version. Nothing is written
    /// when it is the version already there. Returns the version's number.
    pub fn save(&mut self, game: &str, note: &str) -> Result<i64, String> {
        let value: Value = serde_json::from_str(game).map_err(|e| e.to_string())?;
        let Value::Object(obj) = value else { return Err("a game file is an object".into()) };
        let canon = serde_json::to_string(&Value::Object(obj.clone())).map_err(|e| e.to_string())?;
        let fp = fingerprint(&canon);
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let last: Option<(i64, String)> = tx.query_row("SELECT rev, fingerprint FROM history ORDER BY rev DESC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(|e| e.to_string())?;
        if let Some((rev, f)) = &last {
            if *f == fp {
                return Ok(*rev);
            }
        }
        tx.execute("DELETE FROM settings", []).map_err(|e| e.to_string())?;
        let old: Vec<String> = {
            let mut st = tx.prepare("SELECT key FROM sections").map_err(|e| e.to_string())?;
            let v = st.query_map([], |r| r.get(0)).map_err(|e| e.to_string())?.filter_map(Result::ok).collect();
            v
        };
        tx.execute("DELETE FROM sections", []).map_err(|e| e.to_string())?;
        for (i, (k, v)) in obj.iter().enumerate() {
            match v {
                Value::Array(list) if ident_ok(k) => {
                    tx.execute_batch(&format!(
                        "CREATE TABLE IF NOT EXISTS \"{k}\" (pos INTEGER PRIMARY KEY, data TEXT NOT NULL CHECK (json_valid(data)),
                           name TEXT GENERATED ALWAYS AS (json_extract(data, '$.name')) VIRTUAL,
                           title TEXT GENERATED ALWAYS AS (json_extract(data, '$.title')) VIRTUAL);
                         DELETE FROM \"{k}\";"
                    ))
                    .map_err(|e| format!("{k}: {e}"))?;
                    let mut ins = tx.prepare(&format!("INSERT INTO \"{k}\" (pos, data) VALUES (?1, ?2)")).map_err(|e| e.to_string())?;
                    for (p, e) in list.iter().enumerate() {
                        ins.execute(params![p as i64, e.to_string()]).map_err(|e| format!("{k}: {e}"))?;
                    }
                    tx.execute("INSERT INTO sections (key, pos) VALUES (?1, ?2)", params![k, i as i64]).map_err(|e| e.to_string())?;
                }
                _ => {
                    tx.execute("INSERT INTO settings (key, data) VALUES (?1, ?2)", params![k, v.to_string()]).map_err(|e| e.to_string())?;
                }
            }
        }
        // A list the game file no longer has is emptied (kept as a table, so a tool's view of it does not break).
        for k in old.iter().filter(|k| !obj.get(k.as_str()).is_some_and(Value::is_array) && ident_ok(k)) {
            tx.execute_batch(&format!("DELETE FROM \"{k}\";")).map_err(|e| e.to_string())?;
        }
        let rev = last.map_or(1, |(r, _)| r + 1);
        tx.execute(
            "INSERT INTO history (rev, at, note, fingerprint, game) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![rev, now(), note, fp, crate::worldfile::gzip(canon.as_bytes(), 6)],
        )
        .map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM history WHERE rev <= ?1", params![rev - KEEP]).map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO changes (at, rev, note, result) VALUES (?1, ?2, ?3, 'applied')", params![now(), rev, note]).map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(rev)
    }

    /// Note something that happened to the content (a refused change) where a SQLite tool shows it.
    pub fn note(&self, note: &str, result: &str) {
        let _ = self.conn.execute("INSERT INTO changes (at, rev, note, result) VALUES (?1, NULL, ?2, ?3)", params![now(), note, result]);
        let _ = self.conn.execute("DELETE FROM changes WHERE rowid <= (SELECT MAX(rowid) FROM changes) - 5000", []);
    }

    /// The versions kept, newest first.
    pub fn history(&self, limit: usize) -> Result<Vec<Version>, String> {
        let mut st = self.conn.prepare("SELECT rev, at, note, length(game) FROM history ORDER BY rev DESC LIMIT ?1").map_err(|e| e.to_string())?;
        let rows = st
            .query_map(params![limit as i64], |r| Ok(Version { rev: r.get(0)?, at: r.get(1)?, note: r.get(2)?, bytes: r.get::<_, i64>(3)? as usize }))
            .map_err(|e| e.to_string())?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    /// One kept version's game file (JSON).
    pub fn version(&self, rev: i64) -> Result<String, String> {
        let gz: Vec<u8> = self
            .conn
            .query_row("SELECT game FROM history WHERE rev = ?1", params![rev], |r| r.get(0))
            .optional()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("there is no version {rev}"))?;
        let mut text = String::new();
        std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(&gz[..]), &mut text).map_err(|e| e.to_string())?;
        Ok(text)
    }

    /// The newest kept version that still builds (to start from when the tables were left broken).
    pub fn last_good(&self) -> Option<(i64, GameFile)> {
        for v in self.history(KEEP as usize).ok()? {
            if let Ok(g) = self.version(v.rev).and_then(|t| GameFile::from_json(&t)) {
                if g.build().is_ok() {
                    return Some((v.rev, g));
                }
            }
        }
        None
    }

    /// SQLite's count of commits made by other connections: it moves when another program changes the file.
    fn data_version(&self) -> i64 {
        self.conn.query_row("PRAGMA data_version", [], |r| r.get(0)).unwrap_or(0)
    }
}

/// The game file to start with: the content's, else (first run, or `import`) the game file at `game`, saved as a
/// new version. Content left broken by a tool starts from the newest version that builds.
pub fn start_with(path: &Path, game: &Path, import: bool, load_game: impl Fn(&Path) -> GameFile) -> GameFile {
    let mut c = Content::open(path).unwrap_or_else(|e| {
        eprintln!("content: {e}");
        std::process::exit(1);
    });
    if import || c.is_empty() {
        let g = load_game(game);
        let note = if import { format!("imported from {}", game.display()) } else { format!("first filled from {}", game.display()) };
        match c.save(&g.to_json(), &note) {
            Ok(rev) => eprintln!("content: {note} (version {rev})"),
            Err(e) => {
                eprintln!("content: could not save to {}: {e}", path.display());
                std::process::exit(1);
            }
        }
        return g;
    }
    match c.load() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("content: {}: {e}", path.display());
            match c.last_good() {
                Some((rev, g)) => {
                    eprintln!("content: starting from version {rev}, the newest that builds");
                    c.note("start", &format!("the tables did not build ({e}); started from version {rev}"));
                    if let Err(e) = c.save(&g.to_json(), &format!("restored version {rev} at start")) {
                        eprintln!("content: {e}");
                    }
                    g
                }
                None => {
                    eprintln!("content: no version builds; fix the tables or start with --import-game");
                    std::process::exit(1);
                }
            }
        }
    }
}

/// The content's own thread: writes each version the server applies (`saves`), and applies changes made to the
/// file by other programs through the admin queue, as a console edit would be.
pub fn run(path: PathBuf, saves: Receiver<Save>, admin: Option<Sender<crate::ctl::Request>>) {
    let mut c = match Content::open(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("content: {e}");
            return;
        }
    };
    let mut seen = c.data_version();
    // The last outside change refused, so it is said once, not every two seconds.
    let mut refused: Option<String> = None;
    loop {
        match saves.recv_timeout(WATCH) {
            Ok(s) => {
                if let Err(e) = c.save(&s.game, &s.note) {
                    eprintln!("content: not saved: {e}");
                }
                seen = c.data_version();
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
        let v = c.data_version();
        if v == seen {
            continue;
        }
        seen = v;
        let Some(admin) = &admin else { continue };
        // Changed by another program: put the tables together and, if they differ from what is applied, apply them.
        let text = match c.assemble().and_then(|t| GameFile::from_json(&t).map(|g| g.to_json())) {
            Ok(t) => t,
            Err(e) => {
                if refused.as_deref() != Some(&e) {
                    eprintln!("content: an outside change does not read: {e}");
                    c.note("outside change", &format!("refused: {e}"));
                    refused = Some(e);
                }
                continue;
            }
        };
        let canon = serde_json::from_str::<Value>(&text).ok().and_then(|v| serde_json::to_string(&v).ok()).unwrap_or_default();
        if c.applied().as_deref() == Some(fingerprint(&canon).as_str()) {
            continue;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        if admin.send(crate::ctl::Request { cmd: crate::ctl::Cmd::PutGame(text, "edited in the database".into()), reply: tx }).is_err() {
            return;
        }
        match rx.recv_timeout(Duration::from_secs(30)) {
            Ok((200, _)) => {
                refused = None;
                eprintln!("content: applied a change made in the database");
            }
            Ok((_, body)) => {
                let why = serde_json::from_str::<Value>(&body).ok().and_then(|v| v["error"].as_str().map(String::from)).unwrap_or(body);
                if refused.as_deref() != Some(&why) {
                    eprintln!("content: a change made in the database was refused: {why}");
                    c.note("edited in the database", &format!("refused: {why}"));
                    refused = Some(why);
                }
            }
            Err(_) => eprintln!("content: the server did not answer"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mmo-content-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("content.db")
    }

    #[test]
    fn a_game_file_goes_in_and_comes_out_the_same_and_each_version_is_kept() {
        let path = tmp("roundtrip");
        let mut c = Content::open(&path).unwrap();
        assert!(c.is_empty());
        let g = GameFile::default_game();
        assert_eq!(c.save(&g.to_json(), "first").unwrap(), 1);
        assert_eq!(c.load().unwrap(), g);
        // Saving the same again is no new version.
        assert_eq!(c.save(&g.to_json(), "again").unwrap(), 1);
        let mut g2 = g.clone();
        g2.items[0].title = "Renamed".into();
        assert_eq!(c.save(&g2.to_json(), "rename").unwrap(), 2);
        assert_eq!(c.load().unwrap(), g2);
        // Rows are browsable by name and title.
        let t: String = c.conn.query_row("SELECT title FROM items WHERE pos = 0", [], |r| r.get(0)).unwrap();
        assert_eq!(t, "Renamed");
        let h = c.history(10).unwrap();
        assert_eq!(h.iter().map(|v| v.rev).collect::<Vec<_>>(), vec![2, 1]);
        assert_eq!(GameFile::from_json(&c.version(1).unwrap()).unwrap(), g);
    }

    #[test]
    fn an_edit_from_another_program_is_seen_and_a_broken_one_falls_back_to_the_last_good_version() {
        let path = tmp("outside");
        let mut c = Content::open(&path).unwrap();
        let g = GameFile::default_game();
        c.save(&g.to_json(), "first").unwrap();
        let before = c.data_version();
        let other = Connection::open(&path).unwrap();
        other.execute("UPDATE items SET data = json_set(data, '$.title', 'From a tool') WHERE pos = 0", []).unwrap();
        assert_ne!(c.data_version(), before, "another program's change is seen");
        assert_eq!(c.load().unwrap().items[0].title, "From a tool");
        // A row that breaks the game (an item no slot can hold): it does not build, and the last good version is
        // there to start from.
        other.execute("UPDATE items SET data = json_set(data, '$.stack_max', 0) WHERE pos = 0", []).unwrap();
        assert!(c.load().is_err());
        let (rev, back) = c.last_good().unwrap();
        assert_eq!((rev, back), (1, g));
    }
}
