//! Static geometry by tile: the world's walls cut into 256 unit squares, so no client or request ever handles the
//! whole of a big world. The server indexes its walls once (`TileSet`), off the sim thread, and answers
//! `/play/tiles` from the index; a client keeps the tiles around the player (`TileStream`) and builds its collision
//! world from those. Compiled into the server and every Rust client, so the format cannot drift; the browser's copy
//! is `ui/src/play/walls.ts` (same cases tested on both sides).
//!
//! A `/play/tiles` reply, little-endian:
//!
//! ```text
//! u8 format (1) | u64 ver | u16 tiles | per tile: u16 cx | u16 cy | u32 n | n × entry
//! entry: u32 id | f32 x0 | f32 y0 | f32 x1 | f32 y1 | u8 class        (21 bytes)
//! ```
//!
//! A box that touches several tiles is in each, with the same id. Boxes are the server's f32s bit for bit, so
//! prediction collides with exactly what the server does.

use std::collections::HashMap;

use crate::wire::{put_u16, put_u32, put_u64, put_u8, Reader};
use crate::world::Aabb;

/// Edge of a tile, in world units.
pub const TILE: f32 = 256.0;
pub const FORMAT: u8 = 1;
/// Most tiles one request may ask for.
pub const MAX_PER_REQUEST: usize = 64;
/// Most boxes that may touch one tile: what keeps any one request, and any one client's world, bounded.
pub const MAX_PER_TILE: usize = 4096;
/// Tiles kept around the player in each direction (2 is a 5x5 window), and past which a tile is let go.
pub const NEAR: i32 = 2;
pub const KEEP: i32 = 3;
const ENTRY: usize = 21;

/// What a box is, for drawing. Collision treats every class the same.
pub mod class {
    /// A building, wall or anything else a client draws as a block.
    pub const WALL: u8 = 0;
    /// Solid scenery (a tree trunk, a rock): the world file draws it, a client should not add a block.
    pub const SCENERY: u8 = 1;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileBox {
    pub id: u32,
    pub b: Aabb,
    pub class: u8,
}

/// The tiles a box touches, clamped to a world `size` across.
pub fn tiles_of(b: &Aabb, size: f32) -> impl Iterator<Item = (u16, u16)> {
    let last = ((size - 1.0).max(0.0) / TILE) as u32;
    let t = |v: f32| ((v.max(0.0) / TILE) as u32).min(last) as u16;
    let (x0, x1, y0, y1) = (t(b.x0), t(b.x1), t(b.y0), t(b.y1));
    (y0..=y1).flat_map(move |y| (x0..=x1).map(move |x| (x, y)))
}

/// The walls of one world, indexed by tile.
#[derive(Default, Debug)]
pub struct TileSet {
    /// Changes whenever any box or class does.
    pub ver: u64,
    pub size: f32,
    pub boxes: Vec<Aabb>,
    pub class: Vec<u8>,
    tiles: HashMap<(u16, u16), Vec<u32>>,
}

impl TileSet {
    /// Index `boxes` (with a class each; missing classes are walls). Fails if a tile would hold more than
    /// `MAX_PER_TILE` boxes.
    pub fn build(size: f32, boxes: Vec<Aabb>, mut class: Vec<u8>) -> Result<TileSet, String> {
        class.resize(boxes.len(), class::WALL);
        // FNV-1a over the bit patterns: cheap, and any change to any box changes it.
        let mut h: u64 = 0xcbf29ce484222325;
        let mut eat = |byte: u8| h = (h ^ byte as u64).wrapping_mul(0x100000001b3);
        for (b, &c) in boxes.iter().zip(&class) {
            for v in [b.x0, b.y0, b.x1, b.y1] {
                v.to_bits().to_le_bytes().into_iter().for_each(&mut eat);
            }
            eat(c);
        }
        let mut tiles: HashMap<(u16, u16), Vec<u32>> = HashMap::new();
        for (i, b) in boxes.iter().enumerate() {
            for t in tiles_of(b, size) {
                let v = tiles.entry(t).or_default();
                v.push(i as u32);
                if v.len() > MAX_PER_TILE {
                    return Err(format!("more than {MAX_PER_TILE} solid objects touch the tile at ({}, {}) (tiles are {TILE} units)", t.0 as f32 * TILE, t.1 as f32 * TILE));
                }
            }
        }
        Ok(TileSet { ver: h ^ boxes.len() as u64, size, boxes, class, tiles })
    }

    /// Boxes in the fullest tile.
    pub fn densest(&self) -> usize {
        self.tiles.values().map(Vec::len).max().unwrap_or(0)
    }

    /// The reply for these tiles (unknown or empty ones come back empty), or None for too many.
    pub fn encode(&self, want: &[(u16, u16)]) -> Option<Vec<u8>> {
        if want.len() > MAX_PER_REQUEST {
            return None;
        }
        let n: usize = want.iter().map(|t| self.tiles.get(t).map_or(0, Vec::len)).sum();
        let mut out = Vec::with_capacity(11 + want.len() * 8 + n * ENTRY);
        put_u8(&mut out, FORMAT);
        put_u64(&mut out, self.ver);
        put_u16(&mut out, want.len() as u16);
        for &(cx, cy) in want {
            let ids = self.tiles.get(&(cx, cy)).map_or(&[][..], Vec::as_slice);
            put_u16(&mut out, cx);
            put_u16(&mut out, cy);
            put_u32(&mut out, ids.len() as u32);
            for &i in ids {
                let b = &self.boxes[i as usize];
                put_u32(&mut out, i);
                for v in [b.x0, b.y0, b.x1, b.y1] {
                    put_u32(&mut out, v.to_bits());
                }
                put_u8(&mut out, self.class[i as usize]);
            }
        }
        Some(out)
    }
}

/// Parse `cx,cy;cx,cy` (a `/play/tiles` query's `c`). None if any part is not a pair of numbers.
pub fn parse_list(list: &str) -> Option<Vec<(u16, u16)>> {
    list.split(';')
        .filter(|t| !t.is_empty())
        .map(|t| {
            let (a, b) = t.split_once(',')?;
            Some((a.parse().ok()?, b.parse().ok()?))
        })
        .collect()
}

/// A reply: its version and each tile's boxes. None if it is cut short or not this format.
pub fn decode(bytes: &[u8]) -> Option<(u64, Vec<((u16, u16), Vec<TileBox>)>)> {
    let mut r = Reader::new(bytes);
    if r.u8()? != FORMAT {
        return None;
    }
    let ver = r.u64()?;
    let n = r.u16()? as usize;
    let mut tiles = Vec::with_capacity(n);
    for _ in 0..n {
        let (cx, cy, k) = (r.u16()?, r.u16()?, r.u32()? as usize);
        if k > MAX_PER_TILE || r.remaining() < k * ENTRY {
            return None;
        }
        let mut v = Vec::with_capacity(k);
        for _ in 0..k {
            let id = r.u32()?;
            let mut f = || r.u32().map(f32::from_bits);
            let b = Aabb { x0: f()?, y0: f()?, x1: f()?, y1: f()? };
            v.push(TileBox { id, b, class: r.u8()? });
        }
        tiles.push(((cx, cy), v));
    }
    Some((ver, tiles))
}

/// A client's tiles: the window around the player, asked for as it moves and let go when far. Feed it replies with
/// `put`; `rev` bumps whenever the boxes held change, which is when to rebuild the collision world.
#[derive(Default)]
pub struct TileStream {
    pub ver: u64,
    pub rev: u64,
    size: f32,
    boxes: HashMap<u32, (Aabb, u8, u32)>,
    held: HashMap<(u16, u16), Vec<u32>>,
    pending: Vec<(u16, u16)>,
    at: Option<(i32, i32)>,
}

impl TileStream {
    pub fn new(size: f32, ver: u64) -> TileStream {
        TileStream { size, ver, ..Default::default() }
    }

    /// The server's walls changed: forget everything and start again.
    pub fn reset(&mut self, ver: u64) {
        *self = TileStream { size: self.size, ver, rev: self.rev + 1, ..Default::default() };
    }

    /// The player is at (x, y): let go of far tiles, and return the near ones still to fetch (each returned once,
    /// until `put` answers it or `retry` gives it back).
    pub fn wanted(&mut self, x: f32, y: f32) -> Vec<(u16, u16)> {
        let (cx, cy) = ((x.max(0.0) / TILE) as i32, (y.max(0.0) / TILE) as i32);
        if self.at == Some((cx, cy)) {
            return Vec::new();
        }
        self.at = Some((cx, cy));
        let far: Vec<(u16, u16)> = self.held.keys().filter(|t| (t.0 as i32 - cx).abs() > KEEP || (t.1 as i32 - cy).abs() > KEEP).copied().collect();
        for t in &far {
            self.drop_tile(*t);
        }
        if !far.is_empty() {
            self.rev += 1;
        }
        self.pending.retain(|t| (t.0 as i32 - cx).abs() <= KEEP && (t.1 as i32 - cy).abs() <= KEEP);
        let last = ((self.size - 1.0).max(0.0) / TILE) as i32;
        let mut want = Vec::new();
        for j in (cy - NEAR).max(0)..=(cy + NEAR).min(last) {
            for i in (cx - NEAR).max(0)..=(cx + NEAR).min(last) {
                let t = (i as u16, j as u16);
                if !self.held.contains_key(&t) && !self.pending.contains(&t) {
                    want.push(t);
                }
            }
        }
        self.pending.extend(&want);
        want
    }

    /// A fetch failed: ask for those tiles again on the next call to `wanted`.
    pub fn retry(&mut self) {
        self.pending.clear();
        self.at = None;
    }

    /// Take a reply. False if it is for another version of the walls (the caller should re-read `/play/info`).
    pub fn put(&mut self, bytes: &[u8]) -> bool {
        let Some((ver, tiles)) = decode(bytes) else { return false };
        if ver != self.ver {
            self.retry();
            return false;
        }
        for (t, boxes) in tiles {
            self.pending.retain(|p| *p != t);
            if self.held.contains_key(&t) {
                continue;
            }
            let mut ids = Vec::with_capacity(boxes.len());
            for tb in boxes {
                ids.push(tb.id);
                self.boxes.entry(tb.id).or_insert((tb.b, tb.class, 0)).2 += 1;
            }
            self.held.insert(t, ids);
        }
        self.rev += 1;
        true
    }

    fn drop_tile(&mut self, t: (u16, u16)) {
        for id in self.held.remove(&t).unwrap_or_default() {
            if let Some(e) = self.boxes.get_mut(&id) {
                e.2 -= 1;
                if e.2 == 0 {
                    self.boxes.remove(&id);
                }
            }
        }
    }

    /// Every box held, in id order (so a rebuilt world does not depend on hash order).
    pub fn boxes(&self) -> Vec<(u32, Aabb, u8)> {
        let mut v: Vec<(u32, Aabb, u8)> = self.boxes.iter().map(|(&id, &(b, c, _))| (id, b, c)).collect();
        v.sort_unstable_by_key(|e| e.0);
        v
    }

    /// True once every tile near the player is in, which is when prediction can be trusted.
    pub fn ready(&self) -> bool {
        self.pending.is_empty() && !self.held.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(x0: f32, y0: f32, x1: f32, y1: f32) -> Aabb {
        Aabb { x0, y0, x1, y1 }
    }

    #[test]
    fn a_box_is_in_every_tile_it_touches_and_round_trips_bit_for_bit() {
        let s = TileSet::build(1024.0, vec![bx(10.0, 10.0, 20.0, 20.0), bx(250.0, 0.1, 300.0, 5.0), bx(1000.0, 1000.0, 1024.0, 1024.0)], vec![0, 1]).unwrap();
        let (ver, tiles) = decode(&s.encode(&[(0, 0), (1, 0), (3, 3), (2, 2)]).unwrap()).unwrap();
        assert_eq!(ver, s.ver);
        let ids = |k: usize| tiles[k].1.iter().map(|b| b.id).collect::<Vec<_>>();
        assert_eq!((ids(0), ids(1), ids(2), ids(3)), (vec![0, 1], vec![1], vec![2], vec![]));
        assert_eq!(tiles[0].1[1], TileBox { id: 1, b: bx(250.0, 0.1, 300.0, 5.0), class: class::SCENERY });
        assert_eq!(tiles[2].1[0].class, class::WALL, "a missing class is a wall");
        assert!(s.encode(&vec![(0, 0); MAX_PER_REQUEST + 1]).is_none());
        assert!(decode(&s.encode(&[(0, 0)]).unwrap()[..20]).is_none(), "cut short");
        // The version follows the content, the class included.
        let other = TileSet::build(1024.0, s.boxes.clone(), vec![0, 0, 0]).unwrap();
        assert_ne!(other.ver, s.ver);
        assert_eq!(parse_list("0,0;3,2"), Some(vec![(0, 0), (3, 2)]));
        assert_eq!(parse_list("0,0;x"), None);
    }

    /// The same bytes are decoded in ui/src/play/walls.test.ts: change both together.
    pub const FIXTURE: &str = "015d68e04ca311b33f0300000000000200000000000000000028410000a2410000f04100002042000100000000007a43cdcccc3d000096430000a0400101000000010000000100000000007a43cdcccc3d000096430000a040010300030000000000";

    #[test]
    fn the_browser_fixture_is_what_the_server_sends() {
        let s = TileSet::build(1024.0, vec![bx(10.5, 20.25, 30.0, 40.0), bx(250.0, 0.1, 300.0, 5.0)], vec![0, 1]).unwrap();
        let hex: String = s.encode(&[(0, 0), (1, 0), (3, 3)]).unwrap().iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, FIXTURE);
    }

    #[test]
    fn too_many_boxes_in_one_tile_is_refused() {
        let many = vec![bx(1.0, 1.0, 2.0, 2.0); MAX_PER_TILE + 1];
        assert!(TileSet::build(512.0, many, vec![]).unwrap_err().contains("tile at (0, 0)"));
        assert!(TileSet::build(512.0, vec![bx(1.0, 1.0, 2.0, 2.0); MAX_PER_TILE], vec![]).is_ok());
    }

    #[test]
    fn a_stream_fetches_the_window_lets_far_tiles_go_and_keeps_shared_boxes() {
        let s = TileSet::build(4096.0, vec![bx(250.0, 250.0, 260.0, 260.0), bx(2000.0, 10.0, 2010.0, 20.0)], vec![]).unwrap();
        let mut c = TileStream::new(4096.0, s.ver);
        let want = c.wanted(10.0, 10.0);
        assert_eq!(want.len(), 9, "a 5x5 window clipped at the corner");
        assert!(c.wanted(12.0, 12.0).is_empty(), "same tile, nothing new");
        assert!(!c.ready());
        assert!(c.put(&s.encode(&want).unwrap()));
        assert!(c.ready());
        assert_eq!(c.boxes().iter().map(|b| b.0).collect::<Vec<_>>(), vec![0], "the box on four tiles is held once");
        // Walk east until the first tiles are more than KEEP away: the shared box goes only when its last tile does.
        let want = c.wanted(1800.0, 10.0);
        assert!(c.put(&s.encode(&want).unwrap()));
        assert_eq!(c.boxes().iter().map(|b| b.0).collect::<Vec<_>>(), vec![1]);
        // A reply for another version is refused, and the tiles asked for again.
        let newer = TileSet::build(4096.0, vec![bx(1.0, 1.0, 2.0, 2.0)], vec![]).unwrap();
        assert!(!c.put(&newer.encode(&[(7, 0)]).unwrap()));
        c.reset(newer.ver);
        assert_eq!(c.wanted(1800.0, 10.0).len(), 15);
    }
}
