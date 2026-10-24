//! Packet framing. Everything is little-endian, byte aligned (bit packing is a
//! later optimisation: the schema already carries per-attribute bit widths).

pub const PROTO_VERSION: u16 = 10;
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
    /// Inner (inside DATA): this zone handed you over, reconnect to the address given.
    pub const REDIRECT: u8 = 0x87;
    /// Inner: your bag, `n u8 | n x (item+1 u8, count u8)` (item+1 of 0 is an empty slot).
    pub const INVENTORY: u8 = 0x88;
    /// Inner: items lying near you, `n u8 | n x (id u32, x u32, y u32, item u8, count u8)`.
    pub const GROUND: u8 = 0x89;
    /// Inner: spells and blows used near you, `n u8 | n x (seq u32, caster u32, target u32, action u8)`. Each is
    /// sent for a few ticks running, since UDP may drop one; `seq` tells a repeat from a new one.
    pub const CASTS: u8 = 0x8A;
    /// Inner: your own progress, `level u8 | xp u32 | this level's xp u32 | next level's xp u32` (next 0 at the
    /// cap). Sent when it changes (a few times over) and now and then regardless.
    pub const PROGRESS: u8 = 0x8B;
    /// Inner: your quests, `offer u16 | n u8 | n x (quest u16, step u8, count u16, state u8) | m u8 | m bytes`:
    /// `offer` is a quest a giver has just put to you (0xFFFF for none); the n are the quests in hand (state 0) or
    /// ready to hand in (1); the m bytes are a bitmap of the quests done (bit k%8 of byte k/8 for quest k). Sent
    /// on a change.
    pub const QUESTS: u8 = 0x8C;
    /// Inner: your party, `n u8 | n x id u32` (you among them; n 0 when you are in none). Sent on a change.
    pub const PARTY: u8 = 0x8D;
    /// Inner: what you wear, `weapon u8 | armour u8 | trinket u8` (item index + 1, 0 for nothing), and whether you
    /// ride (`riding u8`). Sent on a change.
    pub const GEAR: u8 = 0x8E;
    /// Inner: who is in a fight with you, `n u8 | n x id u32` (n 0 when nobody is): bots locked on you, then
    /// players. A bot locks on when either of you hurts the other and lets go when one of you falls or you get
    /// away; a player is in it from a blow either way until one of you falls or neither lands one for 8 s. Sent
    /// on a change.
    pub const ENCOUNTER: u8 = 0x8F;
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
    /// More for the action: for a build or break, the block's layer (an i8, low byte; see `world::BLOCK`) and, for a
    /// build, its material (high byte). See `game::build_arg`.
    pub arg: u16,
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
    put_u16(out, i.arg);
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
            arg: r.u16()?,
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
    pub world_size: u32,
    pub world_seed: u64,
}

pub fn encode_welcome(out: &mut Vec<u8>, w: &Welcome) {
    out.clear();
    put_u8(out, kind::WELCOME);
    put_u32(out, w.entity);
    put_u32(out, w.tick);
    put_u8(out, w.tick_hz);
    put_u32(out, w.world_size);
    put_u64(out, w.world_seed);
}

/// Where a handed-over player should reconnect: the zone's index and its public UDP address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Redirect {
    pub zone: u8,
    pub ip: [u8; 4],
    pub port: u16,
}

pub fn encode_redirect(out: &mut Vec<u8>, r: &Redirect) {
    out.clear();
    put_u8(out, kind::REDIRECT);
    put_u8(out, r.zone);
    out.extend_from_slice(&r.ip);
    put_u16(out, r.port);
}

pub fn decode_redirect(buf: &[u8]) -> Option<Redirect> {
    if buf.len() != 8 || buf[0] != kind::REDIRECT {
        return None;
    }
    Some(Redirect { zone: buf[1], ip: buf[2..6].try_into().ok()?, port: u16::from_le_bytes([buf[6], buf[7]]) })
}

/// One thing lying on the ground, as a client is told about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroundEntry {
    pub id: u32,
    pub x: u32,
    pub y: u32,
    pub item: u8,
    pub count: u8,
}

pub const MAX_GROUND_SENT: usize = 40;

pub fn encode_inventory(out: &mut Vec<u8>, bag: &[(u8, u8)]) {
    out.clear();
    put_u8(out, kind::INVENTORY);
    put_u8(out, bag.len().min(255) as u8);
    for &(i, c) in bag.iter().take(255) {
        put_u8(out, i);
        put_u8(out, c);
    }
}

pub fn decode_inventory(buf: &[u8]) -> Option<Vec<(u8, u8)>> {
    if buf.len() < 2 || buf[0] != kind::INVENTORY || buf.len() != 2 + buf[1] as usize * 2 {
        return None;
    }
    Some(buf[2..].chunks_exact(2).map(|c| (c[0], c[1])).collect())
}

pub fn encode_ground(out: &mut Vec<u8>, items: &[GroundEntry]) {
    out.clear();
    put_u8(out, kind::GROUND);
    put_u8(out, items.len().min(MAX_GROUND_SENT) as u8);
    for g in items.iter().take(MAX_GROUND_SENT) {
        put_u32(out, g.id);
        put_u32(out, g.x);
        put_u32(out, g.y);
        put_u8(out, g.item);
        put_u8(out, g.count);
    }
}

pub fn decode_ground(buf: &[u8]) -> Option<Vec<GroundEntry>> {
    if buf.len() < 2 || buf[0] != kind::GROUND || buf.len() != 2 + buf[1] as usize * 14 {
        return None;
    }
    Some(
        buf[2..]
            .chunks_exact(14)
            .map(|c| GroundEntry {
                id: u32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                x: u32::from_le_bytes([c[4], c[5], c[6], c[7]]),
                y: u32::from_le_bytes([c[8], c[9], c[10], c[11]]),
                item: c[12],
                count: c[13],
            })
            .collect(),
    )
}

/// One use of an action (`kind::CASTS`): `target` is 0 when it was aimed at the caster.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cast {
    pub seq: u32,
    pub caster: u32,
    pub target: u32,
    pub action: u8,
}

pub const MAX_CASTS_SENT: usize = 32;

pub fn encode_casts(out: &mut Vec<u8>, casts: &[Cast]) {
    out.clear();
    put_u8(out, kind::CASTS);
    put_u8(out, casts.len().min(MAX_CASTS_SENT) as u8);
    for c in casts.iter().take(MAX_CASTS_SENT) {
        put_u32(out, c.seq);
        put_u32(out, c.caster);
        put_u32(out, c.target);
        put_u8(out, c.action);
    }
}

pub fn decode_casts(buf: &[u8]) -> Option<Vec<Cast>> {
    if buf.len() < 2 || buf[0] != kind::CASTS || buf.len() != 2 + buf[1] as usize * 13 {
        return None;
    }
    let u = |c: &[u8], i: usize| u32::from_le_bytes([c[i], c[i + 1], c[i + 2], c[i + 3]]);
    Some(buf[2..].chunks_exact(13).map(|c| Cast { seq: u(c, 0), caster: u(c, 4), target: u(c, 8), action: c[12] }).collect())
}

/// Your level and experience (`kind::PROGRESS`): `at` is what this level needed, `next` what the next needs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub level: u8,
    pub xp: u32,
    pub at: u32,
    pub next: u32,
}

pub fn encode_progress(out: &mut Vec<u8>, p: &Progress) {
    out.clear();
    put_u8(out, kind::PROGRESS);
    put_u8(out, p.level);
    put_u32(out, p.xp);
    put_u32(out, p.at);
    put_u32(out, p.next);
}

pub fn decode_progress(buf: &[u8]) -> Option<Progress> {
    if buf.len() != 14 || buf[0] != kind::PROGRESS {
        return None;
    }
    let u = |i: usize| u32::from_le_bytes([buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]);
    Some(Progress { level: buf[1], xp: u(2), at: u(6), next: u(10) })
}

/// One quest of yours (`kind::QUESTS`): which (its index), which step it is on and how far, and its state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QuestState {
    pub quest: u16,
    pub step: u8,
    pub count: u16,
    pub state: u8,
}

pub const QUEST_ACTIVE: u8 = 0;
pub const QUEST_READY: u8 = 1;
pub const QUEST_DONE: u8 = 2;
pub const NO_OFFER: u16 = u16::MAX;

/// Quests done go as a bitmap, so a player who has done hundreds still fits one packet; the quests in hand
/// (`game::MAX_HELD_QUESTS` at most) go one by one. A done quest comes back with step and count 0.
pub fn encode_quests(out: &mut Vec<u8>, offer: u16, list: &[QuestState]) {
    out.clear();
    put_u8(out, kind::QUESTS);
    put_u16(out, offer);
    let open: Vec<&QuestState> = list.iter().filter(|q| q.state != QUEST_DONE).take(255).collect();
    put_u8(out, open.len() as u8);
    for q in open {
        put_u16(out, q.quest);
        put_u8(out, q.step);
        put_u16(out, q.count);
        put_u8(out, q.state);
    }
    let mut done = Vec::<u8>::new();
    for q in list.iter().filter(|q| q.state == QUEST_DONE && (q.quest as usize) < 255 * 8) {
        let (byte, bit) = (q.quest as usize / 8, q.quest % 8);
        if done.len() <= byte {
            done.resize(byte + 1, 0);
        }
        done[byte] |= 1 << bit;
    }
    put_u8(out, done.len() as u8);
    out.extend_from_slice(&done);
}

pub fn decode_quests(buf: &[u8]) -> Option<(u16, Vec<QuestState>)> {
    if buf.len() < 4 || buf[0] != kind::QUESTS {
        return None;
    }
    let offer = u16::from_le_bytes([buf[1], buf[2]]);
    let n = buf[3] as usize;
    let at = 4 + n * 6;
    let m = *buf.get(at)? as usize;
    if buf.len() != at + 1 + m {
        return None;
    }
    let mut list: Vec<QuestState> = buf[4..at]
        .chunks_exact(6)
        .map(|c| QuestState { quest: u16::from_le_bytes([c[0], c[1]]), step: c[2], count: u16::from_le_bytes([c[3], c[4]]), state: c[5] })
        .collect();
    for (i, &b) in buf[at + 1..].iter().enumerate() {
        for bit in 0..8 {
            if b & (1 << bit) != 0 {
                list.push(QuestState { quest: (i * 8 + bit) as u16, step: 0, count: 0, state: QUEST_DONE });
            }
        }
    }
    Some((offer, list))
}

/// The most bots (and, apart, players) one player may be in a fight with at once.
pub const MAX_PURSUERS: usize = 16;

pub fn encode_encounter(out: &mut Vec<u8>, ids: &[u32]) {
    out.clear();
    put_u8(out, kind::ENCOUNTER);
    put_u8(out, ids.len().min(MAX_PURSUERS) as u8);
    for &id in ids.iter().take(MAX_PURSUERS) {
        put_u32(out, id);
    }
}

pub fn decode_encounter(buf: &[u8]) -> Option<Vec<u32>> {
    if buf.len() < 2 || buf[0] != kind::ENCOUNTER || buf.len() != 2 + buf[1] as usize * 4 {
        return None;
    }
    Some(buf[2..].chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
}

pub fn encode_party(out: &mut Vec<u8>, ids: &[u32]) {
    out.clear();
    put_u8(out, kind::PARTY);
    put_u8(out, ids.len().min(16) as u8);
    for &id in ids.iter().take(16) {
        put_u32(out, id);
    }
}

pub fn decode_party(buf: &[u8]) -> Option<Vec<u32>> {
    if buf.len() < 2 || buf[0] != kind::PARTY || buf.len() != 2 + buf[1] as usize * 4 {
        return None;
    }
    Some(buf[2..].chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
}

pub fn encode_gear(out: &mut Vec<u8>, gear: [u8; 3], riding: bool) {
    out.clear();
    put_u8(out, kind::GEAR);
    out.extend_from_slice(&gear);
    put_u8(out, riding as u8);
}

pub fn decode_gear(buf: &[u8]) -> Option<([u8; 3], bool)> {
    (buf.len() == 5 && buf[0] == kind::GEAR).then(|| ([buf[1], buf[2], buf[3]], buf[4] != 0))
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
            world_size: r.u32()?,
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
    fn quests_round_trip() {
        let mut b = Vec::new();
        let l = [
            QuestState { quest: 300, step: 1, count: 4, state: QUEST_ACTIVE },
            QuestState { quest: 0, step: 0, count: 0, state: QUEST_DONE },
            QuestState { quest: 511, step: 0, count: 0, state: QUEST_DONE },
        ];
        encode_quests(&mut b, 405, &l);
        assert_eq!(decode_quests(&b), Some((405, l.to_vec())));
        assert_eq!(decode_quests(&b[..b.len() - 1]), None);
        // Every quest of the most a game may have done, and a full hand: still one packet.
        let mut all: Vec<QuestState> = (0..crate::game::MAX_QUESTS as u16).map(|k| QuestState { quest: k, state: QUEST_DONE, ..Default::default() }).collect();
        all.extend((0..crate::game::MAX_HELD_QUESTS as u16).map(|k| QuestState { quest: k, step: 7, count: 65_000, state: QUEST_READY }));
        encode_quests(&mut b, NO_OFFER, &all);
        assert!(b.len() + 64 < MAX_PACKET, "{} bytes", b.len());
        assert_eq!(decode_quests(&b).map(|(o, l)| (o, l.len())), Some((NO_OFFER, all.len())));
    }

    #[test]
    fn encounter_round_trips() {
        let mut b = Vec::new();
        encode_encounter(&mut b, &[7, 0x0100_0042]);
        assert_eq!(decode_encounter(&b), Some(vec![7, 0x0100_0042]));
        assert_eq!(decode_encounter(&b[..b.len() - 1]), None);
        encode_encounter(&mut b, &[]);
        assert_eq!(decode_encounter(&b), Some(vec![]));
    }

    #[test]
    fn progress_round_trips() {
        let mut b = Vec::new();
        let p = Progress { level: 7, xp: 2_500, at: 1_900, next: 2_900 };
        encode_progress(&mut b, &p);
        assert_eq!(decode_progress(&b), Some(p));
        assert_eq!(decode_progress(&b[..13]), None);
    }

    #[test]
    fn casts_round_trip_and_reject_bad_lengths() {
        let mut b = Vec::new();
        let c = [Cast { seq: 9, caster: 0x0100_0005, target: 0, action: 3 }, Cast { seq: 10, caster: 7, target: 8, action: 12 }];
        encode_casts(&mut b, &c);
        assert_eq!(b.len(), 2 + 2 * 13);
        assert_eq!(decode_casts(&b), Some(c.to_vec()));
        assert_eq!(decode_casts(&b[..b.len() - 1]), None);
    }

    #[test]
    fn inventory_and_ground_round_trip_and_reject_bad_lengths() {
        let mut b = Vec::new();
        encode_inventory(&mut b, &[(1, 3), (0, 0), (2, 99)]);
        assert_eq!(decode_inventory(&b), Some(vec![(1, 3), (0, 0), (2, 99)]));
        assert_eq!(decode_inventory(&b[..b.len() - 1]), None);
        let g = [GroundEntry { id: 70000, x: 123, y: 4_000_000_000, item: 2, count: 5 }, GroundEntry { id: 1, x: 0, y: 1, item: 0, count: 1 }];
        encode_ground(&mut b, &g);
        assert_eq!(decode_ground(&b), Some(g.to_vec()));
        encode_ground(&mut b, &[]);
        assert_eq!(decode_ground(&b), Some(vec![]));
        b[0] = kind::INVENTORY;
        assert_eq!(decode_ground(&b), None);
    }

    #[test]
    fn redirect_round_trips_and_rejects_bad_lengths() {
        let r = Redirect { zone: 2, ip: [10, 0, 0, 7], port: 7779 };
        let mut b = Vec::new();
        encode_redirect(&mut b, &r);
        assert_eq!(b.len(), 8);
        assert_eq!(decode_redirect(&b), Some(r));
        assert_eq!(decode_redirect(&b[..7]), None);
        b[0] = 0x86;
        assert_eq!(decode_redirect(&b), None);
    }

    #[test]
    fn input_roundtrip() {
        let i = Input { seq: 7, ack_tick: 99, dir_x: -1, dir_y: 127, action: 1, target: 0x0100_0042, arg: 0x07fe, chat_ack: 5 };
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
        encode_input(&mut b, &Input { seq: 1, ack_tick: 0, dir_x: 0, dir_y: 0, action: 0, target: 0, arg: 0, chat_ack: 0 });
        for n in 0..b.len() {
            assert_eq!(decode_client(&b[..n]), None, "len {n}");
        }
        assert_eq!(decode_client(&[0xFF, 1, 2, 3]), None);
    }
}
