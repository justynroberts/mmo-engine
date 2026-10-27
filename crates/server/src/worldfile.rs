//! A world file (`--world`), as written by worldgen or anything else that follows WORLD.md.
//!
//! The server reads only what the simulation needs: the size, the footprints of the solid objects, which
//! become its walls, and the ground's heights, which say which layer each built block column starts in.
//! Everything else in the file (water, scenery, what each object is) means nothing here and is passed to
//! clients for a client theme to draw: whole at `/play/world`, or, so that no client handles a big world's million
//! trees at once, as a head (`/play/world/head`: the file with only its footprint objects, those without a point
//! `p`) and its point objects by tile (`/play/world/tiles`). Both are slices of the file's own bytes. A solid object
//! with a point `p` (or marked `scenery`) is still a wall; its class only tells clients the file draws it (a trunk,
//! a rock), so they do not add a block for it.
//!
//! Loading is slow for a big world (parse, index, compress), so it never runs on the sim thread: at start-up it runs
//! before the tick does, and a live reload runs on the admin connection's thread (`crate::statics::prepare`).

use proto::tiles::{class, TileSet};
use proto::world::Aabb;
use serde_json::value::RawValue;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Most solid objects a world file may have in all. What really bounds a world is `proto::tiles::MAX_PER_TILE`
/// (what any one client or request handles); this only caps the server's memory (about 50 bytes a wall).
pub const MAX_SOLIDS: usize = 4_000_000;

pub struct WorldFile {
    pub path: PathBuf,
    pub bytes: Arc<Vec<u8>>,
    /// Changes whenever the file's contents do; clients refetch when it does.
    pub ver: u64,
    pub size: f32,
    /// The file gzipped, made the first time a client asks for it whole.
    gz: std::sync::OnceLock<Arc<Vec<u8>>>,
    /// The file with only its footprint objects, plain and gzipped.
    pub head: Arc<Vec<u8>>,
    pub head_gz: Arc<Vec<u8>>,
    /// The point objects by tile (`proto::tiles::TILE`, counted from (0, 0); scenery past the edge is in tiles -1 and
    /// beyond): where each one's text is in `bytes`.
    points: HashMap<(i32, i32), Vec<(u32, u32)>>,
    pub point_count: usize,
    /// The solid objects, indexed by tile, with their classes.
    pub tiles: Arc<TileSet>,
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
    /// The footprints of the file's houses (objects of class `house`), where townsfolk live: they come out of
    /// one in the morning and go back into it at night (`Sim::door_of`).
    pub houses: Arc<Vec<Aabb>>,
}

impl WorldFile {
    /// The collision world this file makes: its walls and its roads.
    pub fn world(&self, seed: u64) -> proto::World {
        let mut w = proto::World::from_boxes(self.size, seed, self.tiles.boxes.clone());
        w.set_roads(self.tiles.road_cells());
        w
    }
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
        Ok(Terrain { x0: num("x0")?, y0: num("y0")?, cell, w, h: hh, heights, houses: Default::default() })
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

/// The parts of a world file the server reads. Parsed straight from the bytes with nothing else kept: everything a
/// theme wants (scenery, water) is skipped as it streams past, and objects are reduced to their solid footprints
/// one at a time, so a world of a million objects costs its footprints, not a tree of JSON values ten times the file.
#[derive(serde::Deserialize)]
struct FileIn<'a> {
    #[serde(default)]
    format: Value,
    #[serde(default)]
    size: Value,
    #[serde(default)]
    heights: Value,
    #[serde(default)]
    roads: Value,
    #[serde(borrow)]
    objects: Option<&'a RawValue>,
}

/// The objects, one at a time: the solid ones' footprints (unchecked yet: the size may come after the objects) with
/// their index in the file, and where each object's text is, split into footprints and points.
#[derive(Default)]
struct Objects {
    found: Vec<(usize, Option<[f32; 4]>, u8)>,
    over: bool,
    /// Address ranges of the text: footprint objects, and point objects with their position.
    feet: Vec<(usize, usize)>,
    points: Vec<(usize, usize, f32, f32)>,
    houses: Vec<[f32; 4]>,
}

#[derive(serde::Deserialize)]
struct ObjIn {
    #[serde(default)]
    solid: Value,
    #[serde(default)]
    scenery: Value,
    #[serde(default)]
    b: Value,
    #[serde(default)]
    p: Value,
    #[serde(default)]
    t: Value,
}

impl<'de> serde::Deserialize<'de> for Objects {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Objects, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Objects;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an array of objects")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Objects, A::Error> {
                use serde::de::Error;
                let mut out = Objects::default();
                let mut i = 0;
                while let Some(raw) = seq.next_element::<&'de RawValue>()? {
                    let text = raw.get();
                    let o: ObjIn = serde_json::from_str(text).map_err(A::Error::custom)?;
                    let point = o.p[0].is_number() && o.p[1].is_number();
                    if o.solid == true && !out.over {
                        let b: Vec<f32> = o.b.as_array().map(|a| a.iter().filter_map(|n| n.as_f64()).map(|n| n as f32).collect()).unwrap_or_default();
                        let b = (b.len() == 4).then(|| [b[0], b[1], b[2], b[3]]);
                        // A solid object at a point is drawn there by the theme (a trunk, a rock), so it is scenery
                        // whether or not it says so: files made before the mark get it right too.
                        if let (Some(b), true) = (b, o.t == "house") {
                            out.houses.push(b);
                        }
                        out.found.push((i, b, if o.scenery == true || point { class::SCENERY } else { class::WALL }));
                        out.over = out.found.len() > MAX_SOLIDS;
                    }
                    let at = (text.as_ptr() as usize, text.as_ptr() as usize + text.len());
                    match (o.p[0].as_f64(), o.p[1].as_f64()) {
                        (Some(x), Some(y)) => out.points.push((at.0, at.1, x as f32, y as f32)),
                        _ => out.feet.push(at),
                    }
                    i += 1;
                }
                Ok(out)
            }
        }
        d.deserialize_seq(V)
    }
}

/// Most points in all the roads of one file: enough for a road network across the largest world.
const MAX_ROAD_POINTS: usize = 2_000_000;

/// The file's `roads`: `[{"half_width": w, "pts": [[x, y], ...]}]`. None at all is no roads.
fn parse_roads(v: &Value) -> Result<Vec<proto::tiles::Road>, String> {
    let Some(list) = v.as_array() else { return Ok(Vec::new()) };
    let mut out = Vec::with_capacity(list.len());
    let mut total = 0;
    for (i, r) in list.iter().enumerate() {
        let hw = r["half_width"].as_f64().filter(|w| (0.5..=32.0).contains(w)).ok_or(format!("road {i}: half_width must be 0.5 to 32"))? as f32;
        let pts: Vec<(f32, f32)> = r["pts"]
            .as_array()
            .ok_or(format!("road {i}: no pts"))?
            .iter()
            .map(|p| Some((p[0].as_f64()? as f32, p[1].as_f64()? as f32)))
            .collect::<Option<_>>()
            .filter(|p: &Vec<(f32, f32)>| p.iter().all(|q| q.0.is_finite() && q.1.is_finite()))
            .ok_or(format!("road {i}: each point is [x, y]"))?;
        total += pts.len();
        if total > MAX_ROAD_POINTS {
            return Err(format!("more than {MAX_ROAD_POINTS} road points"));
        }
        out.push(proto::tiles::Road { half_width: hw, pts });
    }
    Ok(out)
}

pub fn load(path: &Path) -> Result<WorldFile, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: FileIn = serde_json::from_slice(&bytes).map_err(|e| format!("{}: not JSON: {e}", path.display()))?;
    let base = bytes.as_ptr() as usize;
    if v.format.as_u64() != Some(1) {
        return Err(format!("{}: not a format 1 world file", path.display()));
    }
    let size = v.size.as_f64().ok_or("world file has no size")? as f32;
    if !(64.0..=proto::world::MAX_WORLD_SIZE as f32).contains(&size) {
        return Err(format!("world size {size} is outside 64..={}", proto::world::MAX_WORLD_SIZE));
    }
    let list = v.objects.ok_or("world file has no objects")?;
    let solids: Objects = serde_json::from_str(list.get()).map_err(|e| format!("{}: objects: {e}", path.display()))?;
    if solids.over {
        return Err(format!("more than {MAX_SOLIDS} solid objects"));
    }
    let (mut boxes, mut classes) = (Vec::with_capacity(solids.found.len()), Vec::with_capacity(solids.found.len()));
    for (i, b, c) in solids.found {
        match b {
            Some(b) if b[0] < b[2] && b[1] < b[3] && b[0] >= 0.0 && b[1] >= 0.0 && b[2] <= size && b[3] <= size => {
                boxes.push(Aabb { x0: b[0], y0: b[1], x1: b[2], y1: b[3] });
                classes.push(c);
            }
            _ => return Err(format!("object {i}: a solid object needs a footprint b = [x0, y0, x1, y1] inside the world")),
        }
    }
    let roads = parse_roads(&v.roads)?;
    let tiles = Arc::new(TileSet::build(size, boxes, classes)?.with_roads(&proto::tiles::road_cells(size, &roads)));
    // Heights only place block columns, so a world whose heights cannot be read still runs, on flat ground.
    let mut terrain = Terrain::parse(&v.heights).unwrap_or_else(|e| {
        eprintln!("{}: {e}; built blocks will sit as if the ground were flat", path.display());
        Terrain::default()
    });
    terrain.houses = Arc::new(solids.houses.iter().map(|b| Aabb { x0: b[0], y0: b[1], x1: b[2], y1: b[3] }).collect());
    let h = blake3::hash(&bytes);
    let ver = u64::from_le_bytes(h.as_bytes()[..8].try_into().unwrap());
    // The head: the file as it is, with the object list cut down to the footprints.
    let (lo, hi) = (list.get().as_ptr() as usize - base, list.get().as_ptr() as usize - base + list.get().len());
    let mut head = Vec::with_capacity(lo + bytes.len() - hi + solids.feet.len() * 128);
    head.extend_from_slice(&bytes[..lo]);
    head.push(b'[');
    for (n, &(a, b)) in solids.feet.iter().enumerate() {
        if n > 0 {
            head.push(b',');
        }
        head.extend_from_slice(&bytes[a - base..b - base]);
    }
    head.push(b']');
    head.extend_from_slice(&bytes[hi..]);
    let mut points: HashMap<(i32, i32), Vec<(u32, u32)>> = HashMap::new();
    for &(a, b, x, y) in &solids.points {
        let t = ((x / proto::tiles::TILE).floor() as i32, (y / proto::tiles::TILE).floor() as i32);
        points.entry(t).or_default().push(((a - base) as u32, (b - base) as u32));
    }
    let head_gz = Arc::new(gzip(&head, 6));
    Ok(WorldFile {
        path: path.to_path_buf(),
        bytes: Arc::new(bytes),
        ver,
        size,
        gz: Default::default(),
        head: Arc::new(head),
        head_gz,
        point_count: solids.points.len(),
        points,
        tiles,
        terrain,
    })
}

impl WorldFile {
    /// The whole file gzipped (made once, on the caller's thread).
    pub fn gz(&self) -> Arc<Vec<u8>> {
        self.gz.get_or_init(|| Arc::new(gzip(&self.bytes, 6))).clone()
    }

    /// `/play/world/tiles`: each wanted tile's point objects, as the file has them, or None for too many tiles.
    pub fn points_json(&self, want: &[(i32, i32)]) -> Option<Vec<u8>> {
        if want.len() > proto::tiles::MAX_PER_REQUEST {
            return None;
        }
        let mut o = format!("{{\"ver\":\"{:016x}\",\"tile\":{},\"tiles\":{{", self.ver, proto::tiles::TILE).into_bytes();
        for (n, &(cx, cy)) in want.iter().enumerate() {
            if n > 0 {
                o.push(b',');
            }
            o.extend_from_slice(format!("\"{cx},{cy}\":[").as_bytes());
            for (k, &(a, b)) in self.points.get(&(cx, cy)).map_or(&[][..], Vec::as_slice).iter().enumerate() {
                if k > 0 {
                    o.push(b',');
                }
                o.extend_from_slice(&self.bytes[a as usize..b as usize]);
            }
            o.push(b']');
        }
        o.extend_from_slice(b"}}");
        Some(o)
    }
}

/// Parse `cx,cy_cx,cy` where a tile may be negative (scenery past the world's edge). None if any part is not a pair.
pub fn parse_tiles(list: &str) -> Option<Vec<(i32, i32)>> {
    proto::tiles::list_parts(list)
        .filter(|t| !t.is_empty())
        .map(|t| {
            let (a, b) = t.split_once(',')?;
            Some((a.parse().ok()?, b.parse().ok()?))
        })
        .collect()
}

/// `bytes` gzipped at `level` (1 fast .. 9 small).
pub fn gzip(bytes: &[u8], level: u32) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::GzEncoder::new(Vec::with_capacity(bytes.len() / 4), flate2::Compression::new(level));
    let _ = e.write_all(bytes);
    e.finish().unwrap_or_default()
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
            {"t":"anything at all","b":[200,200,210,260],"solid":true,"whatever":{"a":1}},
            {"t":"tree","p":[300,300],"b":[299.5,299.5,300.5,300.5],"solid":true,"scenery":true},
            {"t":"rock","p":[400,400],"b":[399.5,399.5,400.5,400.5],"solid":true}]}"#);
        let w = load(&p).unwrap();
        assert_eq!(w.size, 512.0);
        assert_eq!(w.tiles.boxes.len(), 4);
        assert_eq!((w.tiles.boxes[1].x0, w.tiles.boxes[1].y1), (200.0, 260.0));
        assert_eq!(w.tiles.class, vec![class::WALL, class::WALL, class::SCENERY, class::SCENERY], "a solid point is scenery, marked or not");
        assert_eq!(w.bytes.len(), std::fs::read(&p).unwrap().len());
        let mut back = Vec::new();
        std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(&w.gz()[..]), &mut back).unwrap();
        assert_eq!(back, *w.bytes, "the gzipped copy is the file");
    }

    #[test]
    fn the_file_splits_into_a_head_of_footprints_and_point_objects_by_tile() {
        let p = write(r#"{"format":1,"size":1024, "water":{"rivers":[]},"objects":[
            {"t":"house","b":[10,10,30,20],"solid":true},
            {"t":"tree","p":[50.5,60]},
            {"t":"rock", "p":[300,10],"b":[299,9,301,11],"solid":true,"scenery":true},
            {"t":"spire","p":[-40,700]}],"heights":null}"#);
        let w = load(&p).unwrap();
        let head: Value = serde_json::from_slice(&w.head).unwrap();
        assert_eq!(head["objects"], serde_json::json!([{"t":"house","b":[10,10,30,20],"solid":true}]), "only footprints");
        assert_eq!((head["size"].clone(), head["water"].clone()), (1024.into(), serde_json::json!({"rivers": []})), "the rest as it was");
        assert_eq!(w.point_count, 3);
        let t: Value = serde_json::from_slice(&w.points_json(&[(0, 0), (1, 0), (-1, 2), (3, 3)]).unwrap()).unwrap();
        assert_eq!(t["ver"], format!("{:016x}", w.ver));
        assert_eq!(t["tiles"]["0,0"], serde_json::json!([{"t":"tree","p":[50.5,60]}]));
        assert_eq!(t["tiles"]["1,0"][0]["t"], "rock");
        assert_eq!(t["tiles"]["-1,2"][0]["t"], "spire", "scenery past the edge is in a negative tile");
        assert_eq!(t["tiles"]["3,3"], serde_json::json!([]));
        assert!(w.points_json(&vec![(0, 0); proto::tiles::MAX_PER_REQUEST + 1]).is_none());
        assert_eq!(parse_tiles("-1,2;0,0"), Some(vec![(-1, 2), (0, 0)]));
    }

    #[test]
    fn bad_files_are_refused() {
        for (text, why) in [
            (r#"{"format":2,"size":512,"objects":[]}"#, "format"),
            (r#"{"format":1,"objects":[]}"#, "size"),
            (r#"{"format":1,"size":512,"objects":[{"solid":true,"b":[10,10,5,20]}]}"#, "footprint"),
            (r#"{"format":1,"size":512,"objects":[{"solid":true,"b":[10,10,600,20]}]}"#, "footprint"),
            ("not json", "JSON"),
            (format!(r#"{{"format":1,"size":512,"objects":[{}]}}"#, vec![r#"{"solid":true,"b":[1,1,2,2]}"#; proto::tiles::MAX_PER_TILE + 1].join(",")).as_str(), "tile at (0, 0)"),
        ] {
            let e = load(&write(text)).err().unwrap_or_default();
            assert!(e.contains(why), "{text}: {e}");
        }
    }

    #[test]
    fn roads_become_road_cells_in_the_tiles_and_the_collision_world() {
        let p = write(r#"{"format": 1, "size": 512, "objects": [], "roads": [{"half_width": 2, "pts": [[100, 102], [140, 102]]}]}"#);
        let w = load(&p).unwrap();
        assert!(w.tiles.has_roads());
        let world = w.world(1);
        assert!(world.on_road(120.0, 102.0) && !world.on_road(120.0, 110.0));
        let p = write(r#"{"format": 1, "size": 512, "objects": [], "roads": [{"half_width": 99, "pts": [[1, 1], [2, 2]]}]}"#);
        assert!(load(&p).err().unwrap().contains("half_width"));
        let p = write(r#"{"format": 1, "size": 512, "objects": []}"#);
        assert!(!load(&p).unwrap().tiles.has_roads(), "no roads is fine");
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
