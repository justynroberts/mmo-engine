//! Stateless handshake and packet encryption.
//!
//! Flow (client C, server S, S has a long-term X25519 key whose public half
//! clients can pin):
//!
//! ```text
//! C -> S  HELLO      version, C ephemeral public key, padded to 96 bytes
//! S -> C  CHALLENGE  S public key, timestamp, cookie            (53 bytes, S keeps NO state)
//! C -> S  CONNECT    version, C pub, timestamp, cookie, AEAD{defs hash, C identity pub}
//! S -> C  ACCEPT     AEAD{S ephemeral pub, session id, welcome}  (S allocates state only now)
//! both    DATA       session id, counter, AEAD{inner message}
//! ```
//!
//! * The cookie is a keyed hash of the client's address, public key and a
//!   timestamp. A spoofed source address never sees the CHALLENGE, so it can
//!   never produce a valid cookie: no state, no CPU-heavy work and no
//!   amplification (the reply is smaller than the request) until the client
//!   has proved it can receive at its claimed address.
//! * CONNECT and ACCEPT are protected by a key from DH(C ephemeral, S static),
//!   so only the holder of S's private key can answer. Session keys also mix in
//!   DH(C ephemeral, S ephemeral), so recording traffic and later stealing S's
//!   long-term key does not reveal past sessions.
//! * A player's identity is a long-term X25519 key kept by the client. It is
//!   sent inside CONNECT's encrypted body (so passive observers cannot link
//!   sessions to a player) and proven by mixing DH(S ephemeral, C identity) into
//!   the session keys: only the holder of the identity secret can derive them,
//!   so a client that merely claims someone else's identity gets a session it
//!   cannot use. An all-zero identity means "guest": nothing is saved for it.
//! * DATA packets use ChaCha20-Poly1305 with one key per direction and an
//!   explicit 32-bit counter as the nonce, plus a 64-packet replay window.
//!   Header bytes are authenticated, not encrypted.
//!
//! Not covered: REJECT is unauthenticated (it can only make a handshake fail,
//! and clients retry), and there is no rekeying yet: a session ends before its
//! counter wraps (about 6 years at 20 packets/s).

use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce, Tag};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::wire::{kind, reject, PROTO_VERSION};

pub const HELLO_LEN: usize = 96;
pub const CHALLENGE_LEN: usize = 53;
pub const TAG_LEN: usize = 16;
/// kind + session id + counter
pub const DATA_HEADER: usize = 9;
/// Largest plaintext added to by encryption: header + tag.
pub const DATA_OVERHEAD: usize = DATA_HEADER + TAG_LEN;
pub const COOKIE_TTL_SECS: u32 = 30;

/// IPv4 address and port, as bytes (what the cookie binds to).
pub type Addr = [u8; 6];

pub fn addr_bytes(ip: [u8; 4], port: u16) -> Addr {
    let p = port.to_be_bytes();
    [ip[0], ip[1], ip[2], ip[3], p[0], p[1]]
}

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).expect("OS randomness unavailable");
    b
}

fn derive(context: &str, parts: &[&[u8]]) -> [u8; 32] {
    let mut input = Vec::with_capacity(parts.iter().map(|p| p.len()).sum());
    for p in parts {
        input.extend_from_slice(p);
    }
    blake3::derive_key(context, &input)
}

fn dh(secret: &StaticSecret, public: &[u8; 32]) -> Option<[u8; 32]> {
    let shared = secret.diffie_hellman(&PublicKey::from(*public));
    // An all-zero result means the peer sent a low-order point.
    shared.was_contributory().then(|| *shared.as_bytes())
}

fn nonce(ctr: u32) -> Nonce {
    let mut n = [0u8; 12];
    n[..4].copy_from_slice(&ctr.to_le_bytes());
    n.into()
}

pub fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn from_hex32(s: &str) -> Option<[u8; 32]> {
    let s = s.trim();
    if s.len() != 64 || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ------------------------------------------------------------------ session

/// Sliding window of the last 64 counters seen.
#[derive(Default, Clone)]
struct Replay {
    max: u32,
    bits: u64,
    any: bool,
}

impl Replay {
    fn check(&self, c: u32) -> bool {
        if !self.any || c > self.max {
            return true;
        }
        let d = self.max - c;
        d < 64 && (self.bits >> d) & 1 == 0
    }
    fn accept(&mut self, c: u32) {
        if !self.any {
            *self = Replay { max: c, bits: 1, any: true };
        } else if c > self.max {
            let shift = c - self.max;
            self.bits = if shift >= 64 { 0 } else { self.bits << shift };
            self.bits |= 1;
            self.max = c;
        } else {
            self.bits |= 1 << (self.max - c);
        }
    }
}

/// Keys and counters for one established connection (either side).
pub struct Session {
    pub sid: u32,
    tx: ChaCha20Poly1305,
    rx: ChaCha20Poly1305,
    tx_ctr: u32,
    replay: Replay,
}

impl Session {
    fn new(sid: u32, tx_key: [u8; 32], rx_key: [u8; 32]) -> Session {
        Session {
            sid,
            tx: ChaCha20Poly1305::new(Key::from_slice(&tx_key)),
            rx: ChaCha20Poly1305::new(Key::from_slice(&rx_key)),
            tx_ctr: 0,
            replay: Replay::default(),
        }
    }

    /// Encrypt `plain` into a DATA packet in `out` (cleared first). Returns
    /// false when the counter is exhausted and the session must be replaced.
    pub fn seal(&mut self, plain: &[u8], out: &mut Vec<u8>) -> bool {
        if self.tx_ctr == u32::MAX {
            return false;
        }
        let ctr = self.tx_ctr;
        self.tx_ctr += 1;
        out.clear();
        out.push(kind::DATA);
        out.extend_from_slice(&self.sid.to_le_bytes());
        out.extend_from_slice(&ctr.to_le_bytes());
        out.extend_from_slice(plain);
        let (head, body) = out.split_at_mut(DATA_HEADER);
        let tag = self
            .tx
            .encrypt_in_place_detached(&nonce(ctr), head, body)
            .expect("encrypt cannot fail for packet-sized input");
        out.extend_from_slice(&tag);
        true
    }

    /// Authenticate and decrypt a DATA packet. `scratch` receives the plaintext.
    /// Forged, corrupted and replayed packets return `None` and change nothing.
    pub fn open<'a>(&mut self, pkt: &[u8], scratch: &'a mut Vec<u8>) -> Option<&'a [u8]> {
        if pkt.len() < DATA_OVERHEAD || pkt[0] != kind::DATA {
            return None;
        }
        let ctr = u32::from_le_bytes([pkt[5], pkt[6], pkt[7], pkt[8]]);
        if !self.replay.check(ctr) {
            return None;
        }
        let split = pkt.len() - TAG_LEN;
        scratch.clear();
        scratch.extend_from_slice(&pkt[DATA_HEADER..split]);
        let tag = Tag::from_slice(&pkt[split..]);
        self.rx.decrypt_in_place_detached(&nonce(ctr), &pkt[..DATA_HEADER], scratch, tag).ok()?;
        // Only authenticated packets may move the replay window.
        self.replay.accept(ctr);
        Some(scratch)
    }
}

/// Session id of a DATA packet, before it has been authenticated.
#[inline]
pub fn data_sid(pkt: &[u8]) -> Option<u32> {
    if pkt.len() >= DATA_OVERHEAD && pkt[0] == kind::DATA {
        Some(u32::from_le_bytes([pkt[1], pkt[2], pkt[3], pkt[4]]))
    } else {
        None
    }
}

// ------------------------------------------------------------------- server

pub struct ServerKeys {
    secret: StaticSecret,
    pub public: [u8; 32],
    cookie_key: [u8; 32],
}

impl ServerKeys {
    pub fn from_secret(secret: [u8; 32]) -> ServerKeys {
        let secret = StaticSecret::from(secret);
        let public = PublicKey::from(&secret).to_bytes();
        ServerKeys { secret, public, cookie_key: random() }
    }

    pub fn generate() -> (ServerKeys, [u8; 32]) {
        let s: [u8; 32] = random();
        (ServerKeys::from_secret(s), s)
    }

    fn cookie(&self, addr: &Addr, client_pub: &[u8; 32], ts: u32) -> [u8; 16] {
        let mut h = blake3::Hasher::new_keyed(&self.cookie_key);
        h.update(addr);
        h.update(client_pub);
        h.update(&ts.to_le_bytes());
        let mut out = [0u8; 16];
        out.copy_from_slice(&h.finalize().as_bytes()[..16]);
        out
    }

    /// Answer a HELLO without remembering anything. Returns the packet to send
    /// back (a CHALLENGE, or a REJECT for a version mismatch), or `None` for junk.
    pub fn on_hello(&self, addr: &Addr, pkt: &[u8], now: u32) -> Option<Vec<u8>> {
        // Requiring a full-size HELLO is what keeps the reply smaller than the request.
        if pkt.len() < HELLO_LEN || pkt[0] != kind::HELLO {
            return None;
        }
        let version = u16::from_le_bytes([pkt[1], pkt[2]]);
        if version != PROTO_VERSION {
            return Some(vec![kind::REJECT, reject::BAD_VERSION]);
        }
        let mut cpub = [0u8; 32];
        cpub.copy_from_slice(&pkt[3..35]);
        let cookie = self.cookie(addr, &cpub, now);
        let mut out = Vec::with_capacity(CHALLENGE_LEN);
        out.push(kind::CHALLENGE);
        out.extend_from_slice(&self.public);
        out.extend_from_slice(&now.to_le_bytes());
        out.extend_from_slice(&cookie);
        debug_assert_eq!(out.len(), CHALLENGE_LEN);
        Some(out)
    }

    /// Cheap check of a CONNECT's cookie. Nothing expensive happens before this passes.
    pub fn check_connect(&self, addr: &Addr, pkt: &[u8], now: u32) -> Option<ConnectHdr> {
        if pkt.len() != CONNECT_LEN || pkt[0] != kind::CONNECT {
            return None;
        }
        let version = u16::from_le_bytes([pkt[1], pkt[2]]);
        let mut client_pub = [0u8; 32];
        client_pub.copy_from_slice(&pkt[3..35]);
        let ts = u32::from_le_bytes([pkt[35], pkt[36], pkt[37], pkt[38]]);
        if ts > now.saturating_add(2) || now.saturating_sub(ts) > COOKIE_TTL_SECS {
            return None;
        }
        if !ct_eq(&self.cookie(addr, &client_pub, ts), &pkt[39..55]) {
            return None;
        }
        Some(ConnectHdr { version, client_pub })
    }

    /// Do the key agreement and decrypt the CONNECT body (the content hash).
    pub fn open_connect(&self, hdr: &ConnectHdr, pkt: &[u8]) -> Option<ConnectOpen> {
        let dh1 = dh(&self.secret, &hdr.client_pub)?;
        let k_c2s = derive("mmo-engine 1 hs c2s", &[&dh1, &hdr.client_pub, &self.public]);
        let k_s2c = derive("mmo-engine 1 hs s2c", &[&dh1, &hdr.client_pub, &self.public]);
        let mut body = pkt[55..55 + 40].to_vec();
        let tag = Tag::from_slice(&pkt[55 + 40..]);
        ChaCha20Poly1305::new(Key::from_slice(&k_c2s))
            .decrypt_in_place_detached(&nonce(0), &pkt[..55], &mut body, tag)
            .ok()?;
        let mut h = [0u8; 8];
        h.copy_from_slice(&body[..8]);
        let mut id_pub = [0u8; 32];
        id_pub.copy_from_slice(&body[8..]);
        Some(ConnectOpen { client_pub: hdr.client_pub, defs_hash: u64::from_le_bytes(h), id_pub, dh1, k_s2c })
    }

    /// Finish the handshake: create the session and the ACCEPT packet that
    /// carries `welcome` (already encoded) to the client.
    pub fn accept(&self, open: &ConnectOpen, sid: u32, welcome: &[u8]) -> Option<(Vec<u8>, Session)> {
        let eph = StaticSecret::from(random::<32>());
        let eph_pub = PublicKey::from(&eph).to_bytes();
        let dh2 = dh(&eph, &open.client_pub)?;
        let dh3 = if open.id_pub == GUEST { [0u8; 32] } else { dh(&eph, &open.id_pub)? };
        let parts: [&[u8]; 7] = [&open.dh1, &dh2, &dh3, &open.client_pub, &self.public, &eph_pub, &open.id_pub];
        let k_c2s = derive("mmo-engine 1 session c2s", &parts);
        let k_s2c = derive("mmo-engine 1 session s2c", &parts);

        let mut body = Vec::with_capacity(36 + welcome.len() + TAG_LEN);
        body.extend_from_slice(&eph_pub);
        body.extend_from_slice(&sid.to_le_bytes());
        body.extend_from_slice(welcome);
        let aad = [kind::ACCEPT];
        let tag = ChaCha20Poly1305::new(Key::from_slice(&open.k_s2c))
            .encrypt_in_place_detached(&nonce(0), &aad, &mut body)
            .ok()?;
        let mut out = Vec::with_capacity(1 + body.len() + TAG_LEN);
        out.push(kind::ACCEPT);
        out.extend_from_slice(&body);
        out.extend_from_slice(&tag);
        // The server sends with the s2c key and receives with c2s.
        Some((out, Session::new(sid, k_s2c, k_c2s)))
    }
}

pub const CONNECT_LEN: usize = 55 + 8 + 32 + TAG_LEN;

/// The identity of a client that wants nothing remembered.
pub const GUEST: [u8; 32] = [0; 32];

/// Public half of an identity secret.
pub fn identity_public(secret: &[u8; 32]) -> [u8; 32] {
    PublicKey::from(&StaticSecret::from(*secret)).to_bytes()
}

/// A fresh random identity secret.
pub fn new_identity() -> [u8; 32] {
    random::<32>()
}

#[derive(Clone, Copy, Debug)]
pub struct ConnectHdr {
    pub version: u16,
    pub client_pub: [u8; 32],
}

pub struct ConnectOpen {
    pub client_pub: [u8; 32],
    pub defs_hash: u64,
    /// The client's claimed identity (`GUEST` for none). Only trustworthy once
    /// the session's first packet authenticates, because that is what proves it.
    pub id_pub: [u8; 32],
    dh1: [u8; 32],
    k_s2c: [u8; 32],
}

// ------------------------------------------------------------------- client

#[derive(Debug, PartialEq, Eq)]
pub enum HsError {
    /// The server's public key is not the one we pinned.
    KeyMismatch,
    Malformed,
}

pub struct ClientHs {
    secret: StaticSecret,
    public: [u8; 32],
    identity: Option<StaticSecret>,
    pinned: Option<[u8; 32]>,
    defs_hash: u64,
    server_pub: Option<[u8; 32]>,
    dh1: [u8; 32],
    k_s2c: [u8; 32],
    connect: Option<Vec<u8>>,
}

impl ClientHs {
    /// `pinned` is the server public key to insist on (None = trust whatever
    /// the first CHALLENGE says, which is fine for development only).
    pub fn new(pinned: Option<[u8; 32]>, defs_hash: u64) -> ClientHs {
        let secret = StaticSecret::from(random::<32>());
        let public = PublicKey::from(&secret).to_bytes();
        ClientHs { secret, public, identity: None, pinned, defs_hash, server_pub: None, dh1: [0; 32], k_s2c: [0; 32], connect: None }
    }

    /// Like `new`, but as a returning player: the server can recognise this
    /// identity across sessions and restore its character.
    pub fn with_identity(pinned: Option<[u8; 32]>, defs_hash: u64, identity_secret: [u8; 32]) -> ClientHs {
        let mut h = ClientHs::new(pinned, defs_hash);
        h.identity = Some(StaticSecret::from(identity_secret));
        h
    }

    fn id_pub(&self) -> [u8; 32] {
        self.identity.as_ref().map_or(GUEST, |i| PublicKey::from(i).to_bytes())
    }

    pub fn hello(&self) -> Vec<u8> {
        let mut out = vec![0u8; HELLO_LEN];
        out[0] = kind::HELLO;
        out[1..3].copy_from_slice(&PROTO_VERSION.to_le_bytes());
        out[3..35].copy_from_slice(&self.public);
        out
    }

    pub fn server_pub(&self) -> Option<[u8; 32]> {
        self.server_pub
    }

    /// The CONNECT to (re)send, once a CHALLENGE has been accepted.
    pub fn connect_packet(&self) -> Option<&[u8]> {
        self.connect.as_deref()
    }

    /// Handle a CHALLENGE. The first valid one wins; later ones are ignored.
    pub fn on_challenge(&mut self, pkt: &[u8]) -> Result<(), HsError> {
        if self.connect.is_some() {
            return Ok(());
        }
        if pkt.len() != CHALLENGE_LEN || pkt[0] != kind::CHALLENGE {
            return Err(HsError::Malformed);
        }
        let mut spub = [0u8; 32];
        spub.copy_from_slice(&pkt[1..33]);
        if let Some(p) = self.pinned {
            if !ct_eq(&p, &spub) {
                return Err(HsError::KeyMismatch);
            }
        }
        let dh1 = dh(&self.secret, &spub).ok_or(HsError::Malformed)?;
        let k_c2s = derive("mmo-engine 1 hs c2s", &[&dh1, &self.public, &spub]);
        self.k_s2c = derive("mmo-engine 1 hs s2c", &[&dh1, &self.public, &spub]);
        self.dh1 = dh1;
        self.server_pub = Some(spub);

        let mut out = Vec::with_capacity(CONNECT_LEN);
        out.push(kind::CONNECT);
        out.extend_from_slice(&PROTO_VERSION.to_le_bytes());
        out.extend_from_slice(&self.public);
        out.extend_from_slice(&pkt[33..37]); // timestamp
        out.extend_from_slice(&pkt[37..53]); // cookie
        let mut body = self.defs_hash.to_le_bytes().to_vec();
        body.extend_from_slice(&self.id_pub());
        let tag = ChaCha20Poly1305::new(Key::from_slice(&k_c2s))
            .encrypt_in_place_detached(&nonce(0), &out, &mut body)
            .map_err(|_| HsError::Malformed)?;
        out.extend_from_slice(&body);
        out.extend_from_slice(&tag);
        debug_assert_eq!(out.len(), CONNECT_LEN);
        self.connect = Some(out);
        Ok(())
    }

    /// Handle an ACCEPT. On success returns the session and the encoded welcome.
    pub fn on_accept(&self, pkt: &[u8]) -> Option<(Session, Vec<u8>)> {
        let spub = self.server_pub?;
        if pkt.len() < 1 + 36 + TAG_LEN || pkt[0] != kind::ACCEPT {
            return None;
        }
        let split = pkt.len() - TAG_LEN;
        let mut body = pkt[1..split].to_vec();
        let tag = Tag::from_slice(&pkt[split..]);
        ChaCha20Poly1305::new(Key::from_slice(&self.k_s2c))
            .decrypt_in_place_detached(&nonce(0), &pkt[..1], &mut body, tag)
            .ok()?;
        if body.len() < 36 {
            return None;
        }
        let mut eph_pub = [0u8; 32];
        eph_pub.copy_from_slice(&body[..32]);
        let sid = u32::from_le_bytes([body[32], body[33], body[34], body[35]]);
        let dh2 = dh(&self.secret, &eph_pub)?;
        let dh3 = match &self.identity {
            Some(i) => dh(i, &eph_pub)?,
            None => [0u8; 32],
        };
        let parts: [&[u8]; 7] = [&self.dh1, &dh2, &dh3, &self.public, &spub, &eph_pub, &self.id_pub()];
        let k_c2s = derive("mmo-engine 1 session c2s", &parts);
        let k_s2c = derive("mmo-engine 1 session s2c", &parts);
        Some((Session::new(sid, k_c2s, k_s2c), body[36..].to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{decode_server, encode_welcome, ServerMsg, Welcome};

    const A: Addr = [10, 0, 0, 7, 0x1E, 0x61];

    fn welcome_bytes() -> Vec<u8> {
        let mut b = Vec::new();
        encode_welcome(&mut b, &Welcome { entity: 9, tick: 5, tick_hz: 20, world_size: 512, world_seed: 3 });
        b
    }

    /// Run a full handshake, returning both sessions.
    fn handshake(server: &ServerKeys, pinned: Option<[u8; 32]>, hash: u64) -> (Session, Session) {
        let mut c = ClientHs::new(pinned, hash);
        let chal = server.on_hello(&A, &c.hello(), 1000).unwrap();
        c.on_challenge(&chal).unwrap();
        let cp = c.connect_packet().unwrap().to_vec();
        let hdr = server.check_connect(&A, &cp, 1001).expect("cookie");
        let open = server.open_connect(&hdr, &cp).expect("connect body");
        assert_eq!(open.defs_hash, hash);
        let (accept, ssess) = server.accept(&open, 42, &welcome_bytes()).unwrap();
        let (csess, w) = c.on_accept(&accept).expect("accept");
        assert!(matches!(decode_server(&w), Some(ServerMsg::Welcome(x)) if x.entity == 9));
        assert_eq!(csess.sid, 42);
        (csess, ssess)
    }

    #[test]
    fn full_handshake_and_both_directions() {
        let (server, _) = ServerKeys::generate();
        let (mut c, mut s) = handshake(&server, Some(server.public), 0xABCD);
        let (mut pkt, mut scratch) = (Vec::new(), Vec::new());
        assert!(c.seal(b"input please", &mut pkt));
        assert_eq!(data_sid(&pkt), Some(42));
        assert_eq!(s.open(&pkt, &mut scratch), Some(&b"input please"[..]));
        assert!(s.seal(b"snapshot", &mut pkt));
        assert_eq!(c.open(&pkt, &mut scratch), Some(&b"snapshot"[..]));
    }

    #[test]
    fn ciphertext_hides_plaintext_and_costs_25_bytes() {
        let (server, _) = ServerKeys::generate();
        let (mut c, _) = handshake(&server, None, 1);
        let mut pkt = Vec::new();
        let msg = b"the quick brown fox jumps over";
        c.seal(msg, &mut pkt);
        assert_eq!(pkt.len(), msg.len() + DATA_OVERHEAD);
        assert!(!pkt.windows(8).any(|w| w == &msg[..8]));
    }

    #[test]
    fn tampering_and_replay_are_rejected() {
        let (server, _) = ServerKeys::generate();
        let (mut c, mut s) = handshake(&server, None, 1);
        let (mut pkt, mut scratch) = (Vec::new(), Vec::new());
        c.seal(b"hello", &mut pkt);
        for i in 0..pkt.len() {
            let mut bad = pkt.clone();
            bad[i] ^= 0x01;
            assert!(s.open(&bad, &mut scratch).is_none(), "flip at byte {i} was accepted");
        }
        assert!(s.open(&pkt, &mut scratch).is_some());
        assert!(s.open(&pkt, &mut scratch).is_none(), "replay accepted");
        // Reordering inside the window is fine, old packets are not accepted twice.
        let mut p1 = Vec::new();
        let mut p2 = Vec::new();
        c.seal(b"one", &mut p1);
        c.seal(b"two", &mut p2);
        assert!(s.open(&p2, &mut scratch).is_some());
        assert!(s.open(&p1, &mut scratch).is_some());
        assert!(s.open(&p1, &mut scratch).is_none());
    }

    #[test]
    fn packets_older_than_the_window_are_dropped() {
        let (server, _) = ServerKeys::generate();
        let (mut c, mut s) = handshake(&server, None, 1);
        let (mut first, mut p, mut scratch) = (Vec::new(), Vec::new(), Vec::new());
        c.seal(b"old", &mut first);
        for _ in 0..70 {
            c.seal(b"x", &mut p);
            assert!(s.open(&p, &mut scratch).is_some());
        }
        assert!(s.open(&first, &mut scratch).is_none());
    }

    #[test]
    fn each_direction_has_its_own_key() {
        let (server, _) = ServerKeys::generate();
        let (mut c, mut s) = handshake(&server, None, 1);
        let (mut pkt, mut scratch) = (Vec::new(), Vec::new());
        // A packet reflected back at its sender must not verify.
        c.seal(b"reflect", &mut pkt);
        assert!(c.open(&pkt, &mut scratch).is_none());
        s.seal(b"reflect", &mut pkt);
        assert!(s.open(&pkt, &mut scratch).is_none());
    }

    #[test]
    fn hello_gets_a_smaller_reply_and_no_state() {
        let (server, _) = ServerKeys::generate();
        let c = ClientHs::new(None, 1);
        let hello = c.hello();
        let reply = server.on_hello(&A, &hello, 5).unwrap();
        assert!(reply.len() < hello.len(), "amplification: {} -> {}", hello.len(), reply.len());
        // Short hellos are ignored outright.
        assert!(server.on_hello(&A, &hello[..HELLO_LEN - 1], 5).is_none());
        assert!(server.on_hello(&A, &[], 5).is_none());
    }

    #[test]
    fn cookie_is_bound_to_address_and_expires() {
        let (server, _) = ServerKeys::generate();
        let mut c = ClientHs::new(None, 1);
        let chal = server.on_hello(&A, &c.hello(), 1000).unwrap();
        c.on_challenge(&chal).unwrap();
        let cp = c.connect_packet().unwrap().to_vec();
        let other: Addr = [10, 0, 0, 8, 0x1E, 0x61];
        assert!(server.check_connect(&other, &cp, 1000).is_none(), "spoofed source accepted");
        assert!(server.check_connect(&A, &cp, 1000 + COOKIE_TTL_SECS + 1).is_none(), "stale cookie accepted");
        assert!(server.check_connect(&A, &cp, 900).is_none(), "cookie from the future accepted");
        assert!(server.check_connect(&A, &cp, 1000 + COOKIE_TTL_SECS).is_some());
        // A different server instance (new cookie key) does not honour it.
        let (other_server, _) = ServerKeys::generate();
        assert!(other_server.check_connect(&A, &cp, 1000).is_none());
    }

    #[test]
    fn tampered_connect_body_fails_authentication() {
        let (server, _) = ServerKeys::generate();
        let mut c = ClientHs::new(None, 7);
        let chal = server.on_hello(&A, &c.hello(), 1000).unwrap();
        c.on_challenge(&chal).unwrap();
        let cp = c.connect_packet().unwrap().to_vec();
        let hdr = server.check_connect(&A, &cp, 1000).unwrap();
        for i in 55..cp.len() {
            let mut bad = cp.clone();
            bad[i] ^= 0x80;
            assert!(server.open_connect(&hdr, &bad).is_none(), "byte {i}");
        }
        // Swapping the public key invalidates the cookie.
        let mut forged = cp.clone();
        forged[3] ^= 1;
        assert!(server.check_connect(&A, &forged, 1000).is_none());
    }

    #[test]
    fn pinned_key_mismatch_is_detected() {
        let (server, _) = ServerKeys::generate();
        let (impostor, _) = ServerKeys::generate();
        let mut c = ClientHs::new(Some(server.public), 1);
        let chal = impostor.on_hello(&A, &c.hello(), 1000).unwrap();
        assert_eq!(c.on_challenge(&chal), Err(HsError::KeyMismatch));
        assert!(c.connect_packet().is_none());
    }

    #[test]
    fn impostor_cannot_complete_a_handshake() {
        // Client trusts nothing in particular, but an attacker without the
        // server's private key cannot read the CONNECT or forge a valid ACCEPT.
        let (server, _) = ServerKeys::generate();
        let (attacker, _) = ServerKeys::generate();
        let mut c = ClientHs::new(Some(server.public), 1);
        let chal = server.on_hello(&A, &c.hello(), 1000).unwrap();
        c.on_challenge(&chal).unwrap();
        let cp = c.connect_packet().unwrap().to_vec();
        let hdr = ConnectHdr { version: PROTO_VERSION, client_pub: cp[3..35].try_into().unwrap() };
        assert!(attacker.open_connect(&hdr, &cp).is_none());
    }

    #[test]
    fn duplicate_challenge_does_not_change_the_connect() {
        let (server, _) = ServerKeys::generate();
        let mut c = ClientHs::new(None, 1);
        let c1 = server.on_hello(&A, &c.hello(), 1000).unwrap();
        let c2 = server.on_hello(&A, &c.hello(), 1001).unwrap();
        c.on_challenge(&c1).unwrap();
        let first = c.connect_packet().unwrap().to_vec();
        c.on_challenge(&c2).unwrap();
        assert_eq!(c.connect_packet().unwrap(), &first[..]);
    }

    #[test]
    fn bad_version_is_rejected_statelessly() {
        let (server, _) = ServerKeys::generate();
        let mut hello = ClientHs::new(None, 1).hello();
        hello[1] = 99;
        assert_eq!(server.on_hello(&A, &hello, 1), Some(vec![kind::REJECT, reject::BAD_VERSION]));
    }

    #[test]
    fn hex_roundtrip() {
        let k: [u8; 32] = std::array::from_fn(|i| i as u8 * 7);
        assert_eq!(from_hex32(&to_hex(&k)), Some(k));
        assert_eq!(from_hex32("zz"), None);
        assert_eq!(from_hex32(&"g".repeat(64)), None);
    }

    #[test]
    fn replay_window_basics() {
        let mut r = Replay::default();
        assert!(r.check(0));
        r.accept(0);
        assert!(!r.check(0));
        r.accept(5);
        assert!(r.check(3) && !r.check(5));
        r.accept(200);
        assert!(!r.check(5), "fell out of the window");
        assert!(r.check(199) && !r.check(200));
    }

    #[test]
    fn counter_exhaustion_is_reported() {
        let (server, _) = ServerKeys::generate();
        let (mut c, _) = handshake(&server, None, 1);
        c.tx_ctr = u32::MAX;
        assert!(!c.seal(b"x", &mut Vec::new()));
    }

    /// Handshake as a client with the given identity (None = guest), returning what the
    /// server learned, both sessions and the CONNECT packet.
    fn handshake_as(server: &ServerKeys, secret: Option<[u8; 32]>) -> ([u8; 32], Session, Session, Vec<u8>) {
        let mut c = match secret {
            Some(s) => ClientHs::with_identity(None, 7, s),
            None => ClientHs::new(None, 7),
        };
        let chal = server.on_hello(&A, &c.hello(), 1000).unwrap();
        c.on_challenge(&chal).unwrap();
        let cp = c.connect_packet().unwrap().to_vec();
        let hdr = server.check_connect(&A, &cp, 1001).expect("cookie");
        let open = server.open_connect(&hdr, &cp).expect("body");
        let (accept, ssess) = server.accept(&open, 3, &welcome_bytes()).unwrap();
        let (csess, _) = c.on_accept(&accept).expect("accept");
        (open.id_pub, csess, ssess, cp)
    }

    #[test]
    fn a_returning_player_is_recognised_and_the_session_works() {
        let (server, _) = ServerKeys::generate();
        let secret = new_identity();
        let (id, mut c, mut s, cp) = handshake_as(&server, Some(secret));
        assert_eq!(id, identity_public(&secret));
        assert_eq!(cp.len(), CONNECT_LEN);
        assert!(!cp.windows(32).any(|w| w == id), "identity must not travel in the clear");
        let (mut pkt, mut sc) = (Vec::new(), Vec::new());
        assert!(c.seal(b"hi", &mut pkt));
        assert_eq!(s.open(&pkt, &mut sc), Some(&b"hi"[..]));
        // Same identity later: same id, different session keys (ephemeral keys differ).
        let (id2, mut c2, _, _) = handshake_as(&server, Some(secret));
        assert_eq!(id2, id);
        let mut pkt2 = Vec::new();
        assert!(c2.seal(b"hi", &mut pkt2));
        assert_ne!(pkt[9..], pkt2[9..]);
    }

    #[test]
    fn guests_have_no_identity() {
        let (server, _) = ServerKeys::generate();
        let (id, mut c, mut s, _) = handshake_as(&server, None);
        assert_eq!(id, GUEST);
        let (mut pkt, mut sc) = (Vec::new(), Vec::new());
        assert!(c.seal(b"x", &mut pkt));
        assert_eq!(s.open(&pkt, &mut sc), Some(&b"x"[..]));
    }

    /// Claiming someone's identity without their secret gives a session that cannot talk.
    #[test]
    fn claiming_another_identity_without_its_secret_fails_to_authenticate() {
        let (server, _) = ServerKeys::generate();
        let victim = new_identity();
        let victim_pub = identity_public(&victim);
        // Build a CONNECT body that names the victim's public key but is sealed by someone who lacks the secret.
        let mut c = ClientHs::with_identity(None, 7, [5u8; 32]);
        c.identity = Some(StaticSecret::from([5u8; 32]));
        let chal = server.on_hello(&A, &c.hello(), 1000).unwrap();
        // Derive the same CONNECT but with the victim's pub in the body.
        c.on_challenge(&chal).unwrap();
        let spub = c.server_pub().unwrap();
        let dh1 = dh(&c.secret, &spub).unwrap();
        let k_c2s = derive("mmo-engine 1 hs c2s", &[&dh1, &c.public, &spub]);
        let mut cp = c.connect_packet().unwrap().to_vec();
        let mut body = 7u64.to_le_bytes().to_vec();
        body.extend_from_slice(&victim_pub);
        let tag = ChaCha20Poly1305::new(Key::from_slice(&k_c2s)).encrypt_in_place_detached(&nonce(0), &cp[..55], &mut body).unwrap();
        cp.truncate(55);
        cp.extend_from_slice(&body);
        cp.extend_from_slice(&tag);
        let hdr = server.check_connect(&A, &cp, 1001).expect("cookie");
        let open = server.open_connect(&hdr, &cp).expect("body");
        assert_eq!(open.id_pub, victim_pub, "the server hears the claim");
        let (accept, mut ssess) = server.accept(&open, 3, &welcome_bytes()).unwrap();
        let (mut csess, _) = c.on_accept(&accept).expect("the accept itself decrypts, the identity is only proven by the data keys");
        let (mut pkt, mut sc) = (Vec::new(), Vec::new());
        assert!(csess.seal(b"let me be the victim", &mut pkt));
        assert_eq!(ssess.open(&pkt, &mut sc), None, "no proof of the identity secret, so no valid packets");
    }
}
