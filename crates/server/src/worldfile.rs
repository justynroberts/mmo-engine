//! A world file (`--world`), as written by worldgen or anything else that follows WORLD.md.
//!
//! The server reads only what the simulation needs: the size, and the footprints of the solid objects, which
//! become its walls. Everything else in the file (terrain, water, scenery, what each object is) means nothing
//! here and is passed to clients byte for byte at `/play/world`, for a client theme to draw.

use proto::world::Aabb;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Most solid objects a world file may have.
pub const MAX_SOLIDS: usize = 20_000;

pub struct WorldFile {
    pub path: PathBuf,
    pub bytes: Arc<Vec<u8>>,
    /// Changes whenever the file's contents do; clients refetch when it does.
    pub ver: u64,
    pub size: f32,
    pub boxes: Vec<Aabb>,
}

pub fn load(path: &Path) -> Result<WorldFile, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: Value = serde_json::from_slice(&bytes).map_err(|e| format!("{}: not JSON: {e}", path.display()))?;
    if v["format"].as_u64() != Some(1) {
        return Err(format!("{}: not a format 1 world file", path.display()));
    }
    let size = v["size"].as_f64().ok_or("world file has no size")? as f32;
    if !(64.0..=proto::world::MAX_WORLD_SIZE as f32).contains(&size) {
        return Err(format!("world size {size} is outside 64..={}", proto::world::MAX_WORLD_SIZE));
    }
    let mut boxes = Vec::new();
    for (i, o) in v["objects"].as_array().ok_or("world file has no objects")?.iter().enumerate() {
        if o["solid"] != true {
            continue;
        }
        let b: Vec<f32> = o["b"].as_array().map(|a| a.iter().filter_map(|n| n.as_f64()).map(|n| n as f32).collect()).unwrap_or_default();
        if b.len() != 4 || !(b[0] < b[2] && b[1] < b[3]) || b[0] < 0.0 || b[1] < 0.0 || b[2] > size || b[3] > size {
            return Err(format!("object {i}: a solid object needs a footprint b = [x0, y0, x1, y1] inside the world"));
        }
        boxes.push(Aabb { x0: b[0], y0: b[1], x1: b[2], y1: b[3] });
    }
    if boxes.len() > MAX_SOLIDS {
        return Err(format!("{} solid objects; at most {MAX_SOLIDS}", boxes.len()));
    }
    let h = blake3::hash(&bytes);
    let ver = u64::from_le_bytes(h.as_bytes()[..8].try_into().unwrap());
    Ok(WorldFile { path: path.to_path_buf(), bytes: Arc::new(bytes), ver, size, boxes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(text: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("wf-{}-{}.json", std::process::id(), blake3::hash(text.as_bytes()).to_hex()));
        std::fs::write(&p, text).unwrap();
        p
    }

    #[test]
    fn solid_objects_become_walls_and_the_rest_is_ignored() {
        let p = write(r#"{"format":1,"size":512,"objects":[
            {"t":"house","b":[10,10,30,20],"solid":true},
            {"t":"tree","p":[50,50]},
            {"t":"square","b":[100,100,140,140]},
            {"t":"anything at all","b":[200,200,210,260],"solid":true,"whatever":{"a":1}}]}"#);
        let w = load(&p).unwrap();
        assert_eq!(w.size, 512.0);
        assert_eq!(w.boxes.len(), 2);
        assert_eq!((w.boxes[1].x0, w.boxes[1].y1), (200.0, 260.0));
        assert_eq!(w.bytes.len(), std::fs::read(&p).unwrap().len());
    }

    #[test]
    fn bad_files_are_refused() {
        for (text, why) in [
            (r#"{"format":2,"size":512,"objects":[]}"#, "format"),
            (r#"{"format":1,"objects":[]}"#, "size"),
            (r#"{"format":1,"size":512,"objects":[{"solid":true,"b":[10,10,5,20]}]}"#, "footprint"),
            (r#"{"format":1,"size":512,"objects":[{"solid":true,"b":[10,10,600,20]}]}"#, "footprint"),
            ("not json", "JSON"),
        ] {
            let e = load(&write(text)).err().unwrap_or_default();
            assert!(e.contains(why), "{text}: {e}");
        }
    }
}
