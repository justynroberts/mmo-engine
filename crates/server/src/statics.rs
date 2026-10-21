//! The world's static parts as everything off the sim thread sees them: the walls indexed by tile and the world
//! file. The admin API (`/api/world`, `/api/worldfile`) and the web gateway (`/play/tiles`, `/play/world`) read them
//! on their own threads; the sim swaps in new ones when the world changes. Reading costs an `Arc` clone, so neither
//! side ever waits on the other for long, however big the world.

use std::path::Path;
use std::sync::{Arc, RwLock};

use proto::tiles::TileSet;
use proto::World;

use crate::worldfile::WorldFile;

#[derive(Default)]
pub struct Statics {
    tiles: RwLock<Arc<TileSet>>,
    file: RwLock<Option<Arc<WorldFile>>>,
}

impl Statics {
    pub fn new(tiles: Arc<TileSet>, file: Option<Arc<WorldFile>>) -> Arc<Statics> {
        Arc::new(Statics { tiles: RwLock::new(tiles), file: RwLock::new(file) })
    }

    pub fn tiles(&self) -> Arc<TileSet> {
        self.tiles.read().map(|t| t.clone()).unwrap_or_default()
    }

    pub fn file(&self) -> Option<Arc<WorldFile>> {
        self.file.read().ok().and_then(|f| f.clone())
    }

    /// Swap in new walls and file; the old ones come back, for the caller to drop where it is cheap to.
    pub fn set(&self, tiles: Arc<TileSet>, file: Option<Arc<WorldFile>>) -> (Option<Arc<TileSet>>, Option<Arc<WorldFile>>) {
        let t = self.tiles.write().ok().map(|mut t| std::mem::replace(&mut *t, tiles));
        let f = self.file.write().ok().and_then(|mut f| std::mem::replace(&mut *f, file));
        (t, f)
    }

    /// The walls as `/api/world` lists them: every box and its class. Big for a big world, so it is built here, on
    /// the caller's thread.
    pub fn world_json(&self, size: f32) -> String {
        let t = self.tiles();
        let mut o = String::with_capacity(32 + t.boxes.len() * 40);
        o.push_str(&format!("{{\"size\":{},\"boxes\":[", serde_json::json!(size)));
        for (i, b) in t.boxes.iter().enumerate() {
            if i > 0 {
                o.push(',');
            }
            o.push_str(&serde_json::to_string(&[b.x0, b.y0, b.x1, b.y1]).unwrap_or_default());
        }
        o.push_str("],\"class\":");
        o.push_str(&serde_json::to_string(&t.class).unwrap_or_else(|_| "[]".into()));
        o.push('}');
        o
    }
}

/// A world file made ready to swap in: read, indexed, compressed and its collision world built, all on the calling
/// thread, so the sim only has to take it.
pub struct Prepared {
    pub file: WorldFile,
    pub world: World,
}

pub fn prepare(path: &Path, seed: u64) -> Result<Prepared, String> {
    let file = crate::worldfile::load(path)?;
    let world = World::from_boxes(file.size, seed, file.tiles.boxes.clone());
    Ok(Prepared { file, world })
}
