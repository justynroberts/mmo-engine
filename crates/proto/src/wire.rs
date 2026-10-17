//! Packet framing. Everything is little-endian, byte aligned (bit packing is a
//! later optimisation: the schema already carries per-attribute bit widths).

pub const PROTO_VERSION: u16 = 4;
/// Conservative payload ceiling so a datagram never fragments.
pub const MAX_PACKET: usize = 1200;

pub mod kind {
    pub const HELLO: u8 = 0x01;
    pub const INPUT: u8 = 0x02;
    pub const CONNECT: u8 = 0x03;
    pub const DATA: u8 = 0x10;
    pub const WELCOME: u8 = 0x81;
    pub const SNAPSHOT: u8 = 0x82;
    pub const REJECT: u8 = 0x83;
    pub const CHALLENGE: u8 = 0x84;
    pub const ACCEPT: u8 = 0x85;
}

pub mod reject {
    pub const BAD_VERSION: u8 = 1;
    pub const CONTENT_MISMATCH: u8 = 2;
    pub const SERVER_FULL: u8 = 3;
}

#[inline]
pub fn put_u8(b: &mut Vec<u8>, v: u8) {
    b.push(v);
}
#[inline]
pub fn put_u16(b: &mut Vec<u8>, v: u16) {
    b.extend_from_slice(&v.to_le_bytes());
}
#[inline]
pub fn put_u32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_le_bytes());
}
#[inline]
pub fn put_u64(b: &mut Vec<u8>, v: u64) {
    b.extend_from_slice(&v.to_le_bytes());
}

/// Bounds-checked cursor over a received datagram.
pub struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Reader { b, p: 0 }
    }
    #[inline]
    pub fn remaining(&self) -> usize {
        self.b.len() - self.p
    }
    #[inline]
    pub fn u8(&mut self) -> Option<u8> {
        let v = *self.b.get(self.p)?;
        self.p += 1;
        Some(v)
    }
    #[inline]
    pub fn i8(&mut self) -> Option<i8> {
        self.u8().map(|v| v as i8)
    }
    #[inline]
    pub fn u16(&mut self) -> Option<u16> {
        let s = self.b.get(self.p..self.p + 2)?;
        self.p += 2;
        Some(u16::from_le_bytes([s[0], s[1]]))
    }
    #[inline]
    pub fn u32(&mut self) -> Option<u32> {
        let s = self.b.get(self.p..self.p + 4)?;
        self.p += 4;
        Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    #[inline]
    pub fn u64(&mut self) -> Option<u64> {
        let s = self.b.get(self.p..self.p + 8)?;
        self.p += 8;
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        Some(u64::from_le_bytes(a))
    }
}

// ---------------------------------------------------------------- client -> server

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientMsg {
    Input(Input),
}

/// One input frame. The client only ever sends *intent* (a direction and an
/// action request); the server owns position, speed, cooldowns and outcomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    pub seq: u32,
    /// Latest snapshot tick the client has fully applied (0 = none).
    pub ack_tick: u32,
    pub dir_x: i8,
    pub dir_y: i8,
    /// 0 = no action, otherwise an action id from the definitions.
    pub action: u8,
    pub target: u32,
    /// Newest chat message sequence number received from the server.
    pub chat_ack: u32,
}

pub fn encode_input(out: &mut Vec<u8>, i: &Input) {
    out.clear();
    put_u8(out, kind::INPUT);
    put_u32(out, i.seq);
    put_u32(out, i.ack_tick);
    put_u8(out, i.dir_x as u8);
    put_u8(out, i.dir_y as u8);
    put_u8(out, i.action);
    put_u32(out, i.target);
    put_u32(out, i.chat_ack);
}

pub fn decode_client(buf: &[u8]) -> Option<ClientMsg> {
    let mut r = Reader::new(buf);
    match r.u8()? {
        kind::INPUT => Some(ClientMsg::Input(Input {
            seq: r.u32()?,
            ack_tick: r.u32()?,
            dir_x: r.i8()?,
            dir_y: r.i8()?,
            action: r.u8()?,
            target: r.u32()?,
            chat_ack: r.u32()?,
        })),
        _ => None,
    }
}

// ---------------------------------------------------------------- server -> client

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ServerMsg {
    Welcome(Welcome),
    Reject { reason: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Welcome {
    pub entity: u32,
    pub tick: u32,
    pub tick_hz: u8,
    pub world_size: u16,
    pub world_seed: u64,
}

pub fn encode_welcome(out: &mut Vec<u8>, w: &Welcome) {
    out.clear();
    put_u8(out, kind::WELCOME);
    put_u32(out, w.entity);
    put_u32(out, w.tick);
    put_u8(out, w.tick_hz);
    put_u16(out, w.world_size);
    put_u64(out, w.world_seed);
}

pub fn encode_reject(out: &mut Vec<u8>, reason: u8) {
    out.clear();
    put_u8(out, kind::REJECT);
    put_u8(out, reason);
}

/// Decodes non-snapshot server messages. Snapshots go through
/// [`crate::snapshot::decode_snapshot`].
pub fn decode_server(buf: &[u8]) -> Option<ServerMsg> {
    let mut r = Reader::new(buf);
    match r.u8()? {
        kind::WELCOME => Some(ServerMsg::Welcome(Welcome {
            entity: r.u32()?,
            tick: r.u32()?,
            tick_hz: r.u8()?,
            world_size: r.u16()?,
            world_seed: r.u64()?,
        })),
        kind::REJECT => Some(ServerMsg::Reject { reason: r.u8()? }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_roundtrip() {
        let i = Input { seq: 7, ack_tick: 99, dir_x: -1, dir_y: 127, action: 1, target: 0x0100_0042, chat_ack: 5 };
        let mut b = Vec::new();
        encode_input(&mut b, &i);
        assert_eq!(decode_client(&b), Some(ClientMsg::Input(i)));
    }

    #[test]
    fn welcome_roundtrip() {
        let w = Welcome { entity: 5, tick: 10, tick_hz: 20, world_size: 2048, world_seed: 9 };
        let mut b = Vec::new();
        encode_welcome(&mut b, &w);
        assert_eq!(decode_server(&b), Some(ServerMsg::Welcome(w)));
    }

    #[test]
    fn truncated_packets_are_rejected_not_panicking() {
        let mut b = Vec::new();
        encode_input(&mut b, &Input { seq: 1, ack_tick: 0, dir_x: 0, dir_y: 0, action: 0, target: 0, chat_ack: 0 });
        for n in 0..b.len() {
            assert_eq!(decode_client(&b[..n]), None, "len {n}");
        }
        assert_eq!(decode_client(&[0xFF, 1, 2, 3]), None);
    }
}
