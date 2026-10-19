//! A world file (`--world`), as written by worldgen or anything else that follows WORLD.md.
//!
//! The server reads only what the simulation needs: the size, the footprints of the solid objects, which
//! become its walls, and the ground's heights, which say which layer each built block column starts in.
//! Everything else in the file (water, scenery, what each object is) means nothing here and is passed to
//! clients byte for byte at `/play/world`, for a client theme to draw.

use proto::world::Aabb;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Most solid objects a world file may have (trees and stones are solid too, so a big forest has many).
pub const MAX_SOLIDS: usize = 200_000;

pub struct WorldFile {
    pub path: PathBuf,
    pub bytes: Arc<Vec<u8>>,
    /// Changes whenever the file's contents do; clients refetch when it does.
    pub ver: u64,
    pub size: f32,
    pub boxes: Vec<Aabb>,
    pub terrain: Terrain,
}

/// The ground's heights (WORLD.md `heights`), sampled the way clients draw them. Flat at 0 without any.
#[derive(Clone, Default)]
pub struct Terrain {
    x0: f64,
    y0: f64,
    cell: f64,
    w: usize,
    h: usize,
    heights: Vec<f32>,
}

impl Terrain {
    fn parse(h: &Value) -> Result<Terrain, String> {
        if h.is_null() {
            return Ok(Terrain::default());
        }
        let num = |k: &str| h[k].as_f64().ok_or(format!("heights has no {k}"));
        let (w, hh) = (num("w")? as usize, num("h")? as usize);
        let (lo, hi, cell) = (num("lo")?, num("hi")?, num("cell")?);
        let raw = base64_decode(h["u16le"].as_str().ok_or("heights has no u16le")?).ok_or("heights: u16le is not base64")?;
        if w < 2 || hh < 2 || raw.len() != w * hh * 2 || cell <= 0.0 {
            return Err(format!("heights: {w} x {hh} samples do not match {} bytes", raw.len()));
        }
        let span = hi - lo;
        let heights = raw.chunks_exact(2).map(|b| (lo + span * u16::from_le_bytes([b[0], b[1]]) as f64 / 65535.0) as f32).collect();
        Ok(Terrain { x0: num("x0")?, y0: num("y0")?, cell, w, h: hh, heights })
    }

    /// Ground height at a point, interpolated between samples as godot/scripts/world_data.gd does.
    pub fn height(&self, x: f32, y: f32) -> f32 {
        if self.heights.is_empty() {
            return 0.0;
        }
        let fx = ((x as f64 - self.x0) / self.cell).clamp(0.0, (self.w - 1) as f64);
        let fy = ((y as f64 - self.y0) / self.cell).clamp(0.0, (self.h - 1) as f64);
        let (i, j) = ((fx as usize).min(self.w - 2), (fy as usize).min(self.h - 2));
        let (tx, ty) = (fx - i as f64, fy - j as f64);
        let at = |i: usize, j: usize| self.heights[j * self.w + i] as f64;
        let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
        lerp(lerp(at(i, j), at(i + 1, j), tx), lerp(at(i, j + 1), at(i + 1, j + 1), tx), ty) as f32
    }

    /// The layer a block column at cell `(cx, cy)` starts in: the one the lowest corner of its ground is in.
    pub fn base(&self, cx: u32, cy: u32) -> i8 {
        let b = proto::world::BLOCK;
        let (x0, y0) = (cx as f32 * b, cy as f32 * b);
        let lowest = self.height(x0, y0).min(self.height(x0 + b, y0)).min(self.height(x0, y0 + b)).min(self.height(x0 + b, y0 + b));
        proto::world::base_layer(lowest)
    }
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let s = s.trim_end_matches('=').as_bytes();
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for chunk in s.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= (val(c)? as u32) << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
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
    // Heights only place block columns, so a world whose heights cannot be read still runs, on flat ground.
    let terrain = Terrain::parse(&v["heights"]).unwrap_or_else(|e| {
        eprintln!("{}: {e}; built blocks will sit as if the ground were flat", path.display());
        Terrain::default()
    });
    let h = blake3::hash(&bytes);
    let ver = u64::from_le_bytes(h.as_bytes()[..8].try_into().unwrap());
    Ok(WorldFile { path: path.to_path_buf(), bytes: Arc::new(bytes), ver, size, boxes, terrain })
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

    #[test]
    fn heights_are_read_and_sampled_like_the_client_and_give_column_bases() {
        assert_eq!(base64_decode("TWFu").unwrap(), b"Man");
        assert_eq!(base64_decode("TWE=").unwrap(), b"Ma");
        // 2 x 2 samples 4 apart from (0, 0): heights 0, 4 along x, and 8, 12 a row up (lo 0, hi 12).
        let px = |h: f64| ((h / 12.0 * 65535.0).round() as u16).to_le_bytes();
        let raw: Vec<u8> = [0.0, 4.0, 8.0, 12.0].iter().flat_map(|&h| px(h)).collect();
        let b64 = |b: &[u8]| {
            const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            b.chunks(3)
                .flat_map(|c| {
                    let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
                    (0..4).map(move |i| if i <= c.len() { A[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' })
                })
                .collect::<String>()
        };
        let h = serde_json::json!({"x0": 0, "y0": 0, "cell": 4, "w": 2, "h": 2, "lo": 0, "hi": 12, "u16le": b64(&raw)});
        let t = Terrain::parse(&h).unwrap();
        assert!((t.height(2.0, 2.0) - 6.0).abs() < 1e-3 && (t.height(4.0, 0.0) - 4.0).abs() < 1e-3);
        // Cell (1, 0) covers x 2..4, y 0..2: lowest corner (2, 0) is at 2, which is layer 1 (heights 2..4).
        assert_eq!(t.base(1, 0), 1);
        assert_eq!(t.base(0, 0), 0);
        assert_eq!(Terrain::default().base(5, 5), 0, "flat without heights");
    }
}
