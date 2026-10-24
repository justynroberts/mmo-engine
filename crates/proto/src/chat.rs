//! Chat: a small reliable, ordered channel on top of unreliable datagrams.
//!
//! Snapshots can afford to lose packets because the next one supersedes them.
//! A chat line cannot, so each direction gets its own tiny reliability scheme:
//!
//! * client to server: one message in flight, resent every tick until the
//!   server's batch acknowledges its `cseq`. A repeated message is recognised by
//!   `cseq` and acknowledged again without being routed twice.
//! * server to client: every message gets a per-recipient sequence number and
//!   stays queued, and the whole unacked queue is resent in each tick's batch,
//!   until the client's input packets acknowledge it (`chat_ack`).
//!
//! Text is validated and sanitised on the server; clients are never trusted
//! to send well-formed text.

use std::collections::VecDeque;

pub const MAX_TEXT: usize = 120;
/// Unacked messages kept per recipient. Past this the oldest are dropped.
pub const MAX_QUEUE: usize = 64;
/// Keeps a batch inside one datagram together with the snapshot's overhead.
const BATCH_BUDGET: usize = 1000;

pub mod channel {
    /// Heard by players within a radius of the speaker.
    pub const SAY: u8 = 0;
    /// Heard by everyone.
    pub const SHOUT: u8 = 1;
    /// Client to server only: the text is the display name the player claims (`crate::names`). Never routed.
    pub const NAME: u8 = 2;
    /// Server to client only, from the server (0): a message to put across the screen as well as in chat (an
    /// event's banner). Older clients just show it as a chat line.
    pub const BANNER: u8 = 3;
    /// Server to client: a whisper to you, from the speaker (`from`).
    pub const WHISPER: u8 = 4;
    /// Server to client: your own whisper, echoed back; `from` is who it went to.
    pub const WHISPER_SENT: u8 = 5;
    /// Server to client: said to your party (`/p`).
    pub const PARTY: u8 = 6;
    /// Server to client: a bot passing the time of day (`Act::Chat`), heard close by. Clients show it over the
    /// speaker's head rather than in the chat; older ones show it as a line said nearby.
    pub const NEARBY: u8 = 7;
}

pub mod kind {
    /// Client to server (inner message).
    pub const SEND: u8 = 0x04;
    /// Server to client (inner message).
    pub const BATCH: u8 = 0x86;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatMsg {
    pub seq: u32,
    /// Entity id of the speaker.
    pub from: u32,
    pub channel: u8,
    pub text: String,
}

/// Clean up text a client wants to say: control characters become spaces,
/// whitespace is trimmed and collapsed, and the result is cut to `MAX_TEXT`
/// bytes on a character boundary. `None` if nothing is left.
pub fn sanitize(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len().min(MAX_TEXT));
    let mut last_space = true;
    for c in text.chars() {
        let c = if c.is_control() || c == '\u{200b}' || c == '\u{202e}' || c == '\u{2028}' || c == '\u{2029}' { ' ' } else { c };
        if c == ' ' {
            if last_space {
                continue;
            }
            last_space = true;
        } else {
            last_space = false;
        }
        if out.len() + c.len_utf8() > MAX_TEXT {
            break;
        }
        out.push(c);
    }
    let out = out.trim_end().to_string();
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

// ------------------------------------------------------------------- wire

pub fn encode_send(out: &mut Vec<u8>, cseq: u32, channel: u8, text: &str) {
    out.clear();
    out.push(kind::SEND);
    out.extend_from_slice(&cseq.to_le_bytes());
    out.push(channel);
    let t = text.as_bytes();
    let n = t.len().min(MAX_TEXT);
    out.push(n as u8);
    out.extend_from_slice(&t[..n]);
}

/// Parse a client's chat message. Invalid UTF-8, bad lengths and unknown
/// channels are rejected; sanitising is the caller's job.
pub fn decode_send(buf: &[u8]) -> Option<(u32, u8, String)> {
    if buf.len() < 7 || buf[0] != kind::SEND {
        return None;
    }
    let cseq = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
    let ch = buf[5];
    let n = buf[6] as usize;
    if ch > channel::NAME || n > MAX_TEXT || buf.len() != 7 + n {
        return None;
    }
    let text = std::str::from_utf8(&buf[7..]).ok()?.to_string();
    Some((cseq, ch, text))
}

pub fn encode_batch(out: &mut Vec<u8>, ack_cseq: u32, msgs: &[&ChatMsg]) {
    out.clear();
    out.push(kind::BATCH);
    out.extend_from_slice(&ack_cseq.to_le_bytes());
    out.push(msgs.len() as u8);
    for m in msgs {
        out.extend_from_slice(&m.seq.to_le_bytes());
        out.extend_from_slice(&m.from.to_le_bytes());
        out.push(m.channel);
        out.push(m.text.len() as u8);
        out.extend_from_slice(m.text.as_bytes());
    }
}

pub fn decode_batch(buf: &[u8]) -> Option<(u32, Vec<ChatMsg>)> {
    if buf.len() < 6 || buf[0] != kind::BATCH {
        return None;
    }
    let ack = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
    let n = buf[5] as usize;
    let mut p = 6;
    let mut msgs = Vec::with_capacity(n);
    for _ in 0..n {
        let h = buf.get(p..p + 10)?;
        let seq = u32::from_le_bytes([h[0], h[1], h[2], h[3]]);
        let from = u32::from_le_bytes([h[4], h[5], h[6], h[7]]);
        let channel = h[8];
        let len = h[9] as usize;
        p += 10;
        let text = std::str::from_utf8(buf.get(p..p + len)?).ok()?.to_string();
        p += len;
        msgs.push(ChatMsg { seq, from, channel, text });
    }
    (p == buf.len()).then_some((ack, msgs))
}

// ----------------------------------------------------------------- server

/// Per-client chat state kept by the server.
pub struct ChatServer {
    next_seq: u32,
    queue: VecDeque<ChatMsg>,
    /// Highest client message sequence number accepted so far.
    last_cseq: u32,
    ack_dirty: bool,
    tokens: f32,
    pub shout_ready_tick: u32,
}

/// What the server decided about an incoming client message.
#[derive(Debug, PartialEq, Eq)]
pub enum Accept {
    /// New and allowed: route it.
    Route { channel: u8, text: String },
    /// New, but dropped (rate limit or empty after sanitising). Still acknowledged.
    Dropped,
    /// Already seen: acknowledge again, do not route.
    Duplicate,
}

const SAY_BURST: f32 = 3.0;
const SAY_REFILL_PER_SEC: f32 = 1.0;

impl Default for ChatServer {
    fn default() -> Self {
        ChatServer { next_seq: 1, queue: VecDeque::new(), last_cseq: 0, ack_dirty: false, tokens: SAY_BURST, shout_ready_tick: 0 }
    }
}

impl ChatServer {
    /// Refill the rate-limit bucket. Call once per second of game time (cheap).
    pub fn refill(&mut self, secs: f32) {
        self.tokens = (self.tokens + secs * SAY_REFILL_PER_SEC).min(SAY_BURST);
    }

    /// Judge an incoming message. `tick`/`hz` gate shouting (one per 10 s).
    pub fn accept(&mut self, cseq: u32, channel: u8, raw: &str, tick: u32, hz: u32) -> Accept {
        self.ack_dirty = true;
        if cseq <= self.last_cseq {
            return Accept::Duplicate;
        }
        // Only the next message in order is taken; anything further ahead cannot happen
        // with a client that keeps one in flight, so treat it as a skip rather than stall.
        self.last_cseq = cseq;
        let Some(text) = sanitize(raw) else { return Accept::Dropped };
        if self.tokens < 1.0 {
            return Accept::Dropped;
        }
        if channel == channel::SHOUT {
            if tick < self.shout_ready_tick {
                return Accept::Dropped;
            }
            self.shout_ready_tick = tick + hz * 10;
        }
        self.tokens -= 1.0;
        Accept::Route { channel, text }
    }

    /// Queue a message for this client to receive.
    pub fn deliver(&mut self, from: u32, channel: u8, text: &str) {
        if self.queue.len() >= MAX_QUEUE {
            self.queue.pop_front();
        }
        self.queue.push_back(ChatMsg { seq: self.next_seq, from, channel, text: text.to_string() });
        self.next_seq = self.next_seq.wrapping_add(1);
    }

    /// The client says it has everything up to `ack`.
    pub fn on_client_ack(&mut self, ack: u32) {
        while let Some(m) = self.queue.front() {
            if (ack.wrapping_sub(m.seq) as i32) >= 0 {
                self.queue.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn due(&self) -> bool {
        !self.queue.is_empty() || self.ack_dirty
    }

    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// Build this tick's batch (unacked messages that fit, oldest first).
    pub fn build(&mut self, out: &mut Vec<u8>) -> bool {
        if !self.due() {
            return false;
        }
        let mut size = 6;
        let mut take: Vec<&ChatMsg> = Vec::new();
        for m in &self.queue {
            size += 10 + m.text.len();
            if size > BATCH_BUDGET {
                break;
            }
            take.push(m);
        }
        encode_batch(out, self.last_cseq, &take);
        self.ack_dirty = false;
        true
    }
}

// ----------------------------------------------------------------- client

/// Client side: outgoing queue and in-order receive.
pub struct ChatClient {
    next_cseq: u32,
    outbox: VecDeque<(u32, u8, String)>,
    next_expected: u32,
}

impl Default for ChatClient {
    fn default() -> Self {
        ChatClient { next_cseq: 1, outbox: VecDeque::new(), next_expected: 1 }
    }
}

impl ChatClient {
    pub fn say(&mut self, channel: u8, text: &str) {
        self.outbox.push_back((self.next_cseq, channel, text.to_string()));
        self.next_cseq += 1;
    }

    /// The datagram to (re)send this tick, if a message is waiting for its ack.
    pub fn outgoing(&self, out: &mut Vec<u8>) -> bool {
        match self.outbox.front() {
            Some((cseq, ch, text)) => {
                encode_send(out, *cseq, *ch, text);
                true
            }
            None => false,
        }
    }

    /// Value for the `chat_ack` field of input packets.
    pub fn ack(&self) -> u32 {
        self.next_expected - 1
    }

    /// Apply a server batch. Returns the messages that are new, in order.
    pub fn on_batch(&mut self, ack_cseq: u32, msgs: Vec<ChatMsg>) -> Vec<ChatMsg> {
        while let Some((cseq, _, _)) = self.outbox.front() {
            if *cseq <= ack_cseq {
                self.outbox.pop_front();
            } else {
                break;
            }
        }
        let mut fresh = Vec::new();
        for m in msgs {
            if (m.seq.wrapping_sub(self.next_expected) as i32) >= 0 {
                self.next_expected = m.seq.wrapping_add(1);
                fresh.push(m);
            }
        }
        fresh
    }

    pub fn unsent(&self) -> usize {
        self.outbox.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;

    #[test]
    fn sanitize_cleans_and_limits() {
        assert_eq!(sanitize("  hello   world \n").as_deref(), Some("hello world"));
        assert_eq!(sanitize("a\u{0007}b\tc\0d").as_deref(), Some("a b c d"));
        assert_eq!(sanitize("   \n\t "), None);
        assert_eq!(sanitize(""), None);
        let long = "é".repeat(200);
        let s = sanitize(&long).unwrap();
        assert!(s.len() <= MAX_TEXT && s.chars().all(|c| c == 'é'), "cut on a char boundary");
        // Bidi override and zero-width characters are neutralised.
        assert_eq!(sanitize("a\u{202e}b\u{200b}c").as_deref(), Some("a b c"));
    }

    #[test]
    fn wire_roundtrips_and_rejects_garbage() {
        let mut b = Vec::new();
        encode_send(&mut b, 9, channel::SHOUT, "héllo");
        assert_eq!(decode_send(&b), Some((9, channel::SHOUT, "héllo".to_string())));
        for n in 0..b.len() {
            assert_eq!(decode_send(&b[..n]), None, "len {n}");
        }
        let mut bad = b.clone();
        bad[5] = 7; // unknown channel
        assert_eq!(decode_send(&bad), None);
        let mut bad = b.clone();
        let last = bad.len() - 1;
        bad[last] = 0xFF; // invalid UTF-8
        assert_eq!(decode_send(&bad), None);
        bad = b.clone();
        bad.push(0); // trailing byte
        assert_eq!(decode_send(&bad), None);

        let msgs = [
            ChatMsg { seq: 1, from: 5, channel: 0, text: "one".into() },
            ChatMsg { seq: 2, from: 6, channel: 1, text: "twö".into() },
        ];
        encode_batch(&mut b, 77, &msgs.iter().collect::<Vec<_>>());
        assert_eq!(decode_batch(&b), Some((77, msgs.to_vec())));
        for n in 0..b.len() {
            assert_eq!(decode_batch(&b[..n]), None, "len {n}");
        }
    }

    #[test]
    fn rate_limit_duplicates_and_shout_cooldown() {
        let mut s = ChatServer::default();
        let hz = 20;
        // Burst of 3, then dropped.
        for (i, expect_route) in [true, true, true, false].iter().enumerate() {
            let r = s.accept(i as u32 + 1, channel::SAY, "hi", 100, hz);
            assert_eq!(matches!(r, Accept::Route { .. }), *expect_route, "message {i}");
        }
        assert_eq!(s.accept(2, channel::SAY, "hi", 100, hz), Accept::Duplicate);
        s.refill(5.0);
        assert!(matches!(s.accept(5, channel::SHOUT, "loud", 100, hz), Accept::Route { .. }));
        assert_eq!(s.accept(6, channel::SHOUT, "again", 101, hz), Accept::Dropped, "shout cooldown");
        assert!(matches!(s.accept(7, channel::SHOUT, "later", 100 + 200, hz), Accept::Route { .. }));
        assert_eq!(s.accept(8, channel::SAY, "   ", 400, hz), Accept::Dropped, "empty after sanitising");
    }

    /// Both directions over a lossy, reordering link: every message arrives
    /// exactly once and in order, and every acknowledged send is delivered.
    #[test]
    fn reliable_in_order_over_a_lossy_link() {
        for seed in 1..=8u64 {
            let mut rng = Rng::new(seed);
            let mut server = ChatServer::default();
            let mut client = ChatClient::default();
            let mut got: Vec<String> = Vec::new();
            let mut routed: Vec<String> = Vec::new();
            let mut to_server: Vec<Vec<u8>> = Vec::new();
            let mut to_client: Vec<Vec<u8>> = Vec::new();
            let mut sent = 0;
            let mut buf = Vec::new();
            for tick in 0..1500u32 {
                if tick % 40 == 0 && sent < 30 {
                    client.say(channel::SAY, &format!("c{sent}"));
                    sent += 1;
                }
                if tick % 20 == 0 {
                    server.refill(1.0);
                }
                if tick % 25 == 3 && tick < 1000 {
                    server.deliver(9, channel::SAY, &format!("s{tick}"));
                }
                // Client tick: resend outstanding message; ack goes with the "input".
                if client.outgoing(&mut buf) && rng.below(100) >= 30 {
                    to_server.push(buf.clone());
                }
                server.on_client_ack(client.ack());
                // Server tick: receive (possibly reordered), then batch.
                while let Some(p) = pop_random(&mut to_server, &mut rng) {
                    if let Some((c, ch, t)) = decode_send(&p) {
                        if let Accept::Route { text, .. } = server.accept(c, ch, &t, tick, 20) {
                            routed.push(text);
                        }
                    }
                }
                if server.build(&mut buf) && rng.below(100) >= 30 {
                    to_client.push(buf.clone());
                }
                while let Some(p) = pop_random(&mut to_client, &mut rng) {
                    if let Some((ack, msgs)) = decode_batch(&p) {
                        for m in client.on_batch(ack, msgs) {
                            got.push(m.text);
                        }
                    }
                }
            }
            // Drain: no more loss for a while.
            for _ in 0..50 {
                if client.outgoing(&mut buf) {
                    let (c, ch, t) = decode_send(&buf).unwrap();
                    if let Accept::Route { text, .. } = server.accept(c, ch, &t, 5000, 20) {
                        routed.push(text);
                    }
                }
                server.on_client_ack(client.ack());
                if server.build(&mut buf) {
                    let (ack, msgs) = decode_batch(&buf).unwrap();
                    for m in client.on_batch(ack, msgs) {
                        got.push(m.text);
                    }
                }
            }
            let expect: Vec<String> = (0..1000u32).filter(|t| t % 25 == 3).map(|t| format!("s{t}")).collect();
            assert_eq!(got, expect, "seed {seed}: server to client must be exact and ordered");
            assert_eq!(routed.len(), 30, "seed {seed}: each of the client's messages routed exactly once");
            assert_eq!(routed, (0..30).map(|i| format!("c{i}")).collect::<Vec<_>>(), "seed {seed}: in order");
            assert_eq!(client.unsent(), 0);
            assert_eq!(server.pending(), 0);
        }
    }

    fn pop_random(v: &mut Vec<Vec<u8>>, rng: &mut Rng) -> Option<Vec<u8>> {
        if v.is_empty() {
            None
        } else {
            let i = rng.below(v.len() as u32) as usize;
            Some(v.swap_remove(i))
        }
    }

    #[test]
    fn queue_is_bounded_and_clients_tolerate_gaps() {
        let mut s = ChatServer::default();
        let mut c = ChatClient::default();
        for i in 0..(MAX_QUEUE + 10) {
            s.deliver(1, 0, &format!("m{i}"));
        }
        assert_eq!(s.pending(), MAX_QUEUE);
        let mut buf = Vec::new();
        s.build(&mut buf);
        let (ack, msgs) = decode_batch(&buf).unwrap();
        let fresh = c.on_batch(ack, msgs);
        // Oldest ten were dropped; the client takes what is there and moves on.
        assert!(!fresh.is_empty());
        assert_eq!(fresh.last().unwrap().text, format!("m{}", MAX_QUEUE + 9));
        s.on_client_ack(c.ack());
        assert_eq!(s.pending(), 0);
    }

    #[test]
    fn batches_fit_in_one_datagram() {
        let mut s = ChatServer::default();
        for _ in 0..MAX_QUEUE {
            s.deliver(1, 0, &"x".repeat(MAX_TEXT));
        }
        let mut buf = Vec::new();
        s.build(&mut buf);
        assert!(buf.len() <= BATCH_BUDGET, "{}", buf.len());
    }
}
