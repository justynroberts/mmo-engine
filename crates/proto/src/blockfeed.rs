//! Built blocks by region, for clients to stream the blocks near them (`/play/blocks/r`).
//!
//! The world's block grid (`world::BLOCK` cells) is cut into regions of `REGION` x `REGION` cells, the same
//! 256-unit squares the walls stream in (`tiles::TILE`). Each region carries a version that changes whenever a
//! column in it does; a client says which regions it wants and which version of each it holds, and gets back only
//! those that differ. A reply is little endian binary:
//!
//! ```text
//! u8 format (1) | u16 n | n x region
//! region: u16 rx | u16 ry | u32 ver | u16 n_owners | n_owners x 8 bytes (an owner's tag) | u16 n_cols | n_cols x col
//! col:    u16 cell (local y * REGION + local x) | i8 base | u16 mask | per set bit, lowest first: u8 kind | u8 owner
//! ```
//!
//! `mask` bit k is a block in layer base + k (`world::Column`); `owner` 0 is nobody, otherwise the region's owner
//! table plus one (a region with more than 255 builders reports the rest as nobody; the server still knows).
//! A region with no columns clears what the client held there.

use crate::world::{block_cell, block_key, COLUMN_LAYERS};

/// Cells along a region's side: 256 world units, as a wall tile.
pub const REGION: u32 = 128;
/// Most regions in one request.
pub const MAX_ASK: usize = 64;
pub const FORMAT: u8 = 1;

/// The region a block cell is in.
pub fn region_of(cx: u32, cy: u32) -> (u32, u32) {
    (cx / REGION, cy / REGION)
}

/// A region's key: `ry << 16 | rx`.
pub fn region_key(rx: u32, ry: u32) -> u32 {
    ry << 16 | rx
}

/// One column as it goes on the wire: its global key, base, and per layer (0..COLUMN_LAYERS) a material and an
/// owner's tag, or None for an empty layer.
pub struct ColOut<'a> {
    pub key: u32,
    pub base: i8,
    pub layers: [Option<(u8, Option<&'a str>)>; COLUMN_LAYERS as usize],
}

/// An owner's tag (16 hex digits) as 8 bytes; anything else is nobody.
fn tag_bytes(tag: &str) -> Option<[u8; 8]> {
    if tag.len() != 16 {
        return None;
    }
    let mut b = [0u8; 8];
    for (i, out) in b.iter_mut().enumerate() {
        *out = u8::from_str_radix(tag.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(b)
}

/// One region's record, as `encode_reply` joins them.
pub fn encode_region<'a>(rx: u32, ry: u32, ver: u32, cols: impl Iterator<Item = ColOut<'a>>) -> Vec<u8> {
    let mut owners: Vec<[u8; 8]> = Vec::new();
    let mut body: Vec<u8> = Vec::new();
    let mut n = 0u16;
    for c in cols {
        let (cx, cy) = block_cell(c.key);
        let local = ((cy % REGION) * REGION + cx % REGION) as u16;
        let mask = c.layers.iter().enumerate().fold(0u16, |m, (k, l)| if l.is_some() { m | 1 << k } else { m });
        if mask == 0 {
            continue;
        }
        body.extend_from_slice(&local.to_le_bytes());
        body.push(c.base as u8);
        body.extend_from_slice(&mask.to_le_bytes());
        for (kind, owner) in c.layers.iter().flatten() {
            let o = owner.and_then(tag_bytes).map_or(0, |t| match owners.iter().position(|x| *x == t) {
                Some(i) => i + 1,
                None if owners.len() < 255 => {
                    owners.push(t);
                    owners.len()
                }
                None => 0,
            });
            body.push(*kind);
            body.push(o as u8);
        }
        n += 1;
    }
    let mut out = Vec::with_capacity(12 + owners.len() * 8 + body.len());
    out.extend_from_slice(&(rx as u16).to_le_bytes());
    out.extend_from_slice(&(ry as u16).to_le_bytes());
    out.extend_from_slice(&ver.to_le_bytes());
    out.extend_from_slice(&(owners.len() as u16).to_le_bytes());
    for t in &owners {
        out.extend_from_slice(t);
    }
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&body);
    out
}

/// A reply: the format byte, the count, then the regions' records.
pub fn encode_reply(regions: &[&[u8]]) -> Vec<u8> {
    let mut out = vec![FORMAT];
    out.extend_from_slice(&(regions.len() as u16).to_le_bytes());
    for r in regions {
        out.extend_from_slice(r);
    }
    out
}

/// A region as a client reads it: where, its version, and its columns as (global key, base, [(layer, kind,
/// owner tag as hex or "")]).
#[derive(Debug, PartialEq)]
pub struct RegionIn {
    pub rx: u32,
    pub ry: u32,
    pub ver: u32,
    pub cols: Vec<(u32, i8, Vec<(u8, u8, String)>)>,
}

/// Read a reply (for tests and Rust clients); None if it is cut short or not this format.
pub fn decode_reply(b: &[u8]) -> Option<Vec<RegionIn>> {
    let mut p = 0usize;
    let mut take = |n: usize| -> Option<&[u8]> {
        let s = b.get(p..p + n)?;
        p += n;
        Some(s)
    };
    if take(1)?[0] != FORMAT {
        return None;
    }
    let u16_ = |s: &[u8]| u16::from_le_bytes([s[0], s[1]]);
    let n = u16_(take(2)?);
    let mut out = Vec::new();
    for _ in 0..n {
        let rx = u16_(take(2)?) as u32;
        let ry = u16_(take(2)?) as u32;
        let v = take(4)?;
        let ver = u32::from_le_bytes([v[0], v[1], v[2], v[3]]);
        let no = u16_(take(2)?) as usize;
        let mut owners = Vec::with_capacity(no);
        for _ in 0..no {
            owners.push(take(8)?.iter().map(|x| format!("{x:02x}")).collect::<String>());
        }
        let nc = u16_(take(2)?);
        let mut cols = Vec::with_capacity(nc as usize);
        for _ in 0..nc {
            let local = u16_(take(2)?) as u32;
            let base = take(1)?[0] as i8;
            let mask = u16_(take(2)?);
            let mut layers = Vec::new();
            for k in 0..COLUMN_LAYERS as u8 {
                if mask & 1 << k != 0 {
                    let kb = take(2)?;
                    let tag = if kb[1] == 0 { String::new() } else { owners.get(kb[1] as usize - 1)?.clone() };
                    layers.push((k, kb[0], tag));
                }
            }
            let key = block_key(rx * REGION + local % REGION, ry * REGION + local / REGION);
            cols.push((key, base, layers));
        }
        out.push(RegionIn { rx, ry, ver, cols });
    }
    Some(out)
}

/// Read a request's `c=rx,ry,ver_rx,ry,ver_...` (at most MAX_ASK; `tiles::list_parts`).
pub fn parse_ask(s: &str) -> Option<Vec<(u32, u32, u32)>> {
    let mut out = Vec::new();
    for part in crate::tiles::list_parts(s).filter(|p| !p.is_empty()) {
        let mut it = part.split(',');
        let rx = it.next()?.parse().ok()?;
        let ry = it.next()?.parse().ok()?;
        let ver = it.next().map_or(Some(0), |v| v.parse().ok())?;
        out.push((rx, ry, ver));
        if out.len() > MAX_ASK {
            return None;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The browser's and Godot's decoders are tested against these same bytes.
    pub const FIXTURE: &str = "0102000100020007000000010000aa00bb00cc00dd02008100ff050002010a01ff3f0500800000020000000200000000000000";

    fn cols() -> Vec<ColOut<'static>> {
        let mut a = ColOut { key: block_key(129, 257), base: -1, layers: [None; COLUMN_LAYERS as usize] };
        a.layers[0] = Some((2, Some("00aa00bb00cc00dd")));
        a.layers[2] = Some((10, Some("00aa00bb00cc00dd")));
        let mut b = ColOut { key: block_key(128 + 127, 256 + 127), base: 5, layers: [None; COLUMN_LAYERS as usize] };
        b.layers[15] = Some((0, None));
        vec![a, b]
    }

    #[test]
    fn regions_round_trip_with_their_owners() {
        let r1 = encode_region(1, 2, 7, cols().into_iter());
        let r2 = encode_region(2, 0, 2, std::iter::empty());
        let reply = encode_reply(&[&r1, &r2]);
        let hex: String = reply.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, FIXTURE, "{hex}");
        let back = decode_reply(&reply).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!((back[0].rx, back[0].ry, back[0].ver), (1, 2, 7));
        assert_eq!(back[0].cols[0], (block_key(129, 257), -1, vec![(0, 2, "00aa00bb00cc00dd".into()), (2, 10, "00aa00bb00cc00dd".into())]));
        assert_eq!(back[0].cols[1], (block_key(255, 383), 5, vec![(15, 0, String::new())]));
        assert!(back[1].cols.is_empty(), "an empty region clears");
        assert!(decode_reply(&reply[..reply.len() - 3]).is_none(), "cut short");
    }

    #[test]
    fn asks_parse_and_are_capped() {
        assert_eq!(parse_ask("1,2,7;3,4"), Some(vec![(1, 2, 7), (3, 4, 0)]));
        assert_eq!(parse_ask(""), Some(vec![]));
        assert!(parse_ask("1,x,2").is_none());
        let many = (0..=MAX_ASK).map(|i| format!("{i},0,0")).collect::<Vec<_>>().join(";");
        assert!(parse_ask(&many).is_none());
    }
}
