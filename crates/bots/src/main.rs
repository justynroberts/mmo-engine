//! Bot load tester: N real UDP clients speaking the real protocol.
//!
//! Each bot does the encrypted handshake, sends one input per tick (random walk plus the odd
//! zap), applies delta snapshots with the same `ClientView` a game client would
//! use, acks them, and measures input-to-ack latency. Because it decodes every
//! snapshot, it also catches protocol corruption, not just throughput problems.

use std::net::UdpSocket;
use std::time::{Duration, Instant};

use proto::game::GameFile;
use proto::secure::{from_hex32, ClientHs, HsError, Session};
use proto::wire::{self, kind, Input, ServerMsg};
use proto::world::dpos;
use proto::world::World;
use proto::{decode_snapshot, ClientView, DecodeError, Predictor, Rng};
use std::rc::Rc;

struct Config {
    server: String,
    bots: usize,
    duration: u64,
    hz: u32,
    ramp: usize,
    seed: u64,
    quiet: bool,
    game: String,
    server_key: Option<[u8; 32]>,
    chat: f64,
    persist: bool,
    /// Fixed identity for bot 0 (from --account/--passphrase or --recovery-code).
    identity: Option<[u8; 32]>,
    print_recovery: bool,
}

fn usage(msg: &str) -> ! {
    if !msg.is_empty() {
        eprintln!("error: {msg}\n");
    }
    eprintln!(
        "bot load tester\n\n  --server ADDR   host:port (127.0.0.1:7777)\n  --bots N        number of bots (100)\n  --duration S    seconds to run (30)\n  --hz N          input rate, match the server (20)\n  --ramp N        bots connected per second (500)\n  --seed N        behaviour seed (1)\n  --game FILE     game definitions, must match the server's (game.json, else built-in default)\n  --server-key H  pin the server's public key (64 hex chars, printed by the server); without it the first key seen is trusted\n  --chat N        chat lines per bot per minute (0)\n  --account NAME --passphrase P   bot 0 logs in as that account: same pair on any machine is the same character\n  --recovery-code CODE            bot 0 uses the identity from a recovery code\n  --print-recovery                print bot 0's recovery code and exit\n  --persist       bots keep a stable identity (from --seed and their number) so the server restores their character\n  --quiet         only print the final summary"
    );
    std::process::exit(if msg.is_empty() { 0 } else { 2 });
}

fn parse_args() -> Config {
    let mut c = Config {
        server: "127.0.0.1:7777".into(),
        bots: 100,
        duration: 30,
        hz: 20,
        ramp: 500,
        seed: 1,
        quiet: false,
        game: "game.json".into(),
        server_key: None,
        chat: 0.0,
        persist: false,
        identity: None,
        print_recovery: false,
    };
    let (mut account, mut pass): (Option<String>, Option<String>) = (None, None);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let mut val = |n: &str| -> String {
            i += 1;
            args.get(i).cloned().unwrap_or_else(|| usage(&format!("{n} needs a value")))
        };
        match a {
            "--server" => c.server = val(a),
            "--bots" => c.bots = val(a).parse().unwrap_or_else(|_| usage("bad --bots")),
            "--duration" => c.duration = val(a).parse().unwrap_or_else(|_| usage("bad --duration")),
            "--hz" => c.hz = val(a).parse().unwrap_or_else(|_| usage("bad --hz")),
            "--ramp" => c.ramp = val(a).parse().unwrap_or_else(|_| usage("bad --ramp")),
            "--seed" => c.seed = val(a).parse().unwrap_or_else(|_| usage("bad --seed")),
            "--game" => c.game = val(a),
            "--server-key" => {
                c.server_key = Some(from_hex32(&val(a)).unwrap_or_else(|| usage("--server-key must be 64 hex characters")))
            }
            "--chat" => c.chat = val(a).parse().unwrap_or_else(|_| usage("bad --chat")),
            "--persist" => c.persist = true,
            "--account" => account = Some(val(a)),
            "--passphrase" => pass = Some(val(a)),
            "--recovery-code" => {
                c.identity = Some(proto::secure::parse_recovery_code(&val(a)).unwrap_or_else(|| usage("that recovery code has a typo in it")))
            }
            "--print-recovery" => c.print_recovery = true,
            "--quiet" => c.quiet = true,
            "-h" | "--help" => usage(""),
            other => usage(&format!("unknown argument {other}")),
        }
        i += 1;
    }
    match (account, pass) {
        (Some(a), Some(p)) => c.identity = Some(proto::secure::identity_from_passphrase(&a, &p)),
        (None, None) => {}
        _ => usage("--account and --passphrase go together"),
    }
    c
}

#[derive(PartialEq)]
enum State {
    Idle,
    Connecting,
    Playing,
    Rejected,
    /// Pinned key did not match: refuse to talk to this server.
    Untrusted,
}

struct Bot {
    sock: UdpSocket,
    hs: ClientHs,
    session: Option<Session>,
    last_rx: Instant,
    state: State,
    last_hello: Instant,
    entity: u32,
    view: ClientView,
    pred: Option<Predictor>,
    seq: u32,
    dir: (i8, i8),
    dir_left: u32,
    rng: Rng,
    sent_at: [(u32, Instant); 128],
    rtt_seq: u32,
    chat: proto::chat::ChatClient,
    chat_n: u32,
    chat_buf: Vec<u8>,
}

#[derive(Default)]
struct Totals {
    snaps: u64,
    snap_bytes: u64,
    visible: u64,
    stale: u64,
    missing_baseline: u64,
    decode_err: u64,
    self_missing: u64,
    welcomes: u64,
    rejects: u64,
    reconnects: u64,
    teleports: u64,
    untrusted: u64,
    auth_fail: u64,
    inputs_sent: u64,
    zaps_sent: u64,
    send_errors: u64,
    chat_sent: u64,
    chat_rx: u64,
    chat_bad: u64,
    inventories: u64,
    grounds: u64,
}

/// Handshake state for bot `i`. With `--persist` its identity is fixed by the seed, so a
/// reconnect (or a whole new run with the same seed) is the same player to the server.
fn new_hs(cfg: &Config, i: usize, hash: u64) -> ClientHs {
    if i == 0 {
        if let Some(id) = cfg.identity {
            return ClientHs::with_identity(cfg.server_key, hash, id);
        }
    }
    if !cfg.persist {
        return ClientHs::new(cfg.server_key, hash);
    }
    let mut r = Rng::new(cfg.seed.wrapping_mul(0x9E37_79B9).wrapping_add(i as u64).wrapping_add(0x1D));
    let mut secret = [0u8; 32];
    for chunk in secret.chunks_mut(8) {
        chunk.copy_from_slice(&r.next_u64().to_le_bytes());
    }
    ClientHs::with_identity(cfg.server_key, hash, secret)
}

fn main() {
    let cfg = parse_args();
    if cfg.print_recovery {
        let secret = match (cfg.identity, cfg.persist) {
            (Some(id), _) => id,
            (None, true) => {
                // Same derivation `new_hs` uses for bot 0.
                let mut r = Rng::new(cfg.seed.wrapping_mul(0x9E37_79B9).wrapping_add(0x1D));
                let mut secret = [0u8; 32];
                for chunk in secret.chunks_mut(8) {
                    chunk.copy_from_slice(&r.next_u64().to_le_bytes());
                }
                secret
            }
            _ => usage("--print-recovery needs --account/--passphrase, --recovery-code or --persist"),
        };
        println!("{}", proto::secure::recovery_code(&secret));
        return;
    }
    let game = match std::fs::read_to_string(&cfg.game) {
        Ok(t) => GameFile::from_json(&t).unwrap_or_else(|e| usage(&format!("{}: {e}", cfg.game))),
        Err(_) => GameFile::default_game(),
    };
    let defs = game.build().unwrap_or_else(|e| usage(&format!("{}: {e}", cfg.game))).defs;
    let hash = defs.content_hash();
    let attack_action = defs.actions.first().map_or(0, |a| a.id);
    let attack_range = defs.actions.first().map_or(0.0, |a| a.range);
    let server: std::net::SocketAddr = std::net::ToSocketAddrs::to_socket_addrs(&cfg.server)
        .ok()
        .and_then(|mut a| a.next())
        .unwrap_or_else(|| usage("cannot resolve --server"));

    let now = Instant::now();
    let mut bots: Vec<Bot> = Vec::with_capacity(cfg.bots);
    for i in 0..cfg.bots {
        let sock = UdpSocket::bind("0.0.0.0:0").unwrap_or_else(|e| {
            eprintln!("cannot open socket {i}: {e} (raise `ulimit -n`?)");
            std::process::exit(1);
        });
        sock.connect(server).expect("connect");
        sock.set_nonblocking(true).expect("nonblocking");
        bots.push(Bot {
            sock,
            hs: new_hs(&cfg, i, hash),
            session: None,
            last_rx: now,
            state: State::Idle,
            last_hello: now,
            entity: 0,
            view: ClientView::new(),
            pred: None,
            seq: 0,
            dir: (0, 0),
            dir_left: 0,
            rng: Rng::new(cfg.seed.wrapping_mul(1_000_003).wrapping_add(i as u64)),
            sent_at: [(u32::MAX, now); 128],
            rtt_seq: 0,
            chat: Default::default(),
            chat_n: 0,
            chat_buf: Vec::new(),
        });
    }

    let dt = Duration::from_secs_f64(1.0 / cfg.hz as f64);
    let start = Instant::now();
    let mut next = start + dt;
    let mut buf = [0u8; 2048];
    let mut out = Vec::with_capacity(64);
    let mut sealed = Vec::with_capacity(96);
    let mut plain = Vec::with_capacity(2048);
    let mut tot = Totals::default();
    let mut rtts: Vec<u32> = Vec::new();
    let mut window_rtts: Vec<u32> = Vec::new();
    let mut last_report = start;
    let mut w_snaps = 0u64;
    let mut w_bytes = 0u64;
    let mut activated = 0usize;
    let mut late_ticks = 0u64;
    let mut tick = 0u64;
    let mut world: Option<Rc<World>> = None;
    let mut pred_err: Vec<u32> = Vec::new(); // corrections in 1/1000 unit

    eprintln!(
        "bots: {} -> {} ({} Hz inputs, ramp {}/s, {}s), content hash {:016x}",
        cfg.bots, cfg.server, cfg.hz, cfg.ramp, cfg.duration, hash
    );

    loop {
        let now = Instant::now();
        if next > now + Duration::from_micros(300) {
            std::thread::sleep(next - now - Duration::from_micros(250));
        }
        while Instant::now() < next {
            std::hint::spin_loop();
        }
        if Instant::now() > next + Duration::from_millis(5) {
            late_ticks += 1;
        }
        next += dt;
        tick += 1;

        // Ramp connections up gradually.
        let should = (((start.elapsed().as_secs_f64()) * cfg.ramp as f64) as usize).min(cfg.bots);
        while activated < should {
            bots[activated].state = State::Connecting;
            bots[activated].last_hello = Instant::now() - Duration::from_secs(10);
            activated += 1;
        }

        for (bi, b) in bots.iter_mut().enumerate() {
            if matches!(b.state, State::Idle | State::Rejected | State::Untrusted) {
                continue;
            }
            // Receive everything waiting.
            loop {
                match b.sock.recv(&mut buf) {
                    Ok(n) if n > 0 => {
                        let t = Instant::now();
                        b.last_rx = t;
                        match buf[0] {
                            kind::CHALLENGE => match b.hs.on_challenge(&buf[..n]) {
                                Ok(()) => {}
                                Err(HsError::KeyMismatch) => {
                                    b.state = State::Untrusted;
                                    tot.untrusted += 1;
                                }
                                Err(HsError::Malformed) => tot.decode_err += 1,
                            },
                            kind::ACCEPT => {
                                if b.state != State::Playing {
                                    if let Some((sess, w)) = b.hs.on_accept(&buf[..n]) {
                                        if let Some(ServerMsg::Welcome(w)) = wire::decode_server(&w) {
                                            b.session = Some(sess);
                                            b.state = State::Playing;
                                            b.entity = w.entity;
                                            b.view = ClientView::new();
                                            b.pred = None;
                                            if world.is_none() {
                                                world = Some(Rc::new(game.world(w.world_size, w.world_seed).unwrap_or_else(|_| World::generate(w.world_size, w.world_seed))));
                                            }
                                            tot.welcomes += 1;
                                        }
                                    } else {
                                        tot.auth_fail += 1;
                                    }
                                }
                            }
                            kind::REJECT => {
                                if b.state == State::Connecting {
                                    b.state = State::Rejected;
                                    tot.rejects += 1;
                                    eprintln!("rejected by server, reason {}", buf.get(1).copied().unwrap_or(0));
                                }
                            }
                            kind::DATA => {
                                let Some(sess) = b.session.as_mut() else { continue };
                                let Some(inner) = sess.open(&buf[..n], &mut plain) else {
                                    tot.auth_fail += 1;
                                    continue;
                                };
                                if inner.first() == Some(&proto::chat::kind::BATCH) {
                                    match proto::chat::decode_batch(inner) {
                                        Some((ack, msgs)) => {
                                            for m in b.chat.on_batch(ack, msgs) {
                                                if m.text.is_empty() || m.from == 0 && m.channel > 1 {
                                                    tot.chat_bad += 1;
                                                }
                                                tot.chat_rx += 1;
                                            }
                                        }
                                        None => tot.chat_bad += 1,
                                    }
                                    continue;
                                }
                                // Bag and nearby-item updates: bots never pick anything up, so only check they decode.
                                match inner.first() {
                                    Some(&kind::INVENTORY) => {
                                        match wire::decode_inventory(inner) {
                                            Some(_) => tot.inventories += 1,
                                            None => tot.decode_err += 1,
                                        }
                                        continue;
                                    }
                                    Some(&kind::GROUND) => {
                                        match wire::decode_ground(inner) {
                                            Some(_) => tot.grounds += 1,
                                            None => tot.decode_err += 1,
                                        }
                                        continue;
                                    }
                                    Some(&kind::SNAPSHOT) => {}
                                    _ => {
                                        tot.decode_err += 1;
                                        continue;
                                    }
                                }
                                match decode_snapshot(inner, &defs, &mut b.view) {
                                    Ok(info) if !info.stale => {
                                        tot.snaps += 1;
                                        tot.snap_bytes += n as u64;
                                        tot.visible += b.view.entries().len() as u64;
                                        w_snaps += 1;
                                        w_bytes += n as u64;
                                        match (b.view.get(b.entity), world.as_ref()) {
                                            (None, _) => tot.self_missing += 1,
                                            (Some(me), Some(wd)) => {
                                                let (mx, my) = (dpos(me.x), dpos(me.y));
                                                match b.pred.as_mut() {
                                                    None => {
                                                        b.pred = Some(Predictor::new(mx, my, defs.move_speed, cfg.hz, defs.entity_radius));
                                                    }
                                                    Some(pr) => {
                                                        let err = pr.reconcile(wd, mx, my, info.last_input_seq);
                                                        if err > 5.0 {
                                                            tot.teleports += 1; // respawn or similar: the server did something we cannot predict
                                                        } else {
                                                            pred_err.push((err * 1000.0) as u32);
                                                        }
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                        // Input-to-ack latency, once per input seq.
                                        let s = info.last_input_seq;
                                        if s != 0 && s != b.rtt_seq {
                                            let (q, at) = b.sent_at[s as usize % 128];
                                            if q == s {
                                                let us = (t - at).as_micros() as u32;
                                                rtts.push(us);
                                                window_rtts.push(us);
                                                b.rtt_seq = s;
                                            }
                                        }
                                    }
                                    Ok(_) => tot.stale += 1,
                                    Err(DecodeError::MissingBaseline) => tot.missing_baseline += 1,
                                    Err(_) => tot.decode_err += 1,
                                }
                            }
                            _ => tot.decode_err += 1,
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break, // WouldBlock
                }
            }

            match b.state {
                State::Connecting => {
                    if b.last_hello.elapsed() >= Duration::from_millis(250) {
                        // HELLO until a CHALLENGE arrives, then CONNECT until the ACCEPT does.
                        let _ = match b.hs.connect_packet() {
                            Some(c) => b.sock.send(c),
                            None => b.sock.send(&b.hs.hello()),
                        };
                        b.last_hello = Instant::now();
                    }
                }
                State::Playing => {
                    // Server restarted, dropped us or changed content: start over with fresh keys.
                    if b.last_rx.elapsed() > Duration::from_secs(3) {
                        b.hs = new_hs(&cfg, bi, hash);
                        b.session = None;
                        b.chat = Default::default();
                        b.state = State::Connecting;
                        b.last_hello = Instant::now() - Duration::from_secs(10);
                        tot.reconnects += 1;
                        continue;
                    }
                    // Random walk: new direction every 1 to 3 seconds, sometimes stand still.
                    if b.dir_left == 0 {
                        b.dir_left = cfg.hz + b.rng.below(cfg.hz * 2);
                        b.dir = if b.rng.below(4) == 0 {
                            (0, 0)
                        } else {
                            const D: [i8; 3] = [-127, 0, 127];
                            (D[b.rng.below(3) as usize], D[b.rng.below(3) as usize])
                        };
                    }
                    b.dir_left -= 1;

                    // Zap the nearest neighbour if one is in range (the server still validates).
                    let (mut action, mut target) = (0u8, 0u32);
                    if let Some(me) = b.view.get(b.entity) {
                        let (mx, my) = (dpos(me.x), dpos(me.y));
                        let mut best = attack_range * attack_range;
                        for e in b.view.entries() {
                            if e.id == b.entity {
                                continue;
                            }
                            let (dx, dy) = (dpos(e.x) - mx, dpos(e.y) - my);
                            let d2 = dx * dx + dy * dy;
                            if d2 <= best {
                                best = d2;
                                action = attack_action;
                                target = e.id;
                            }
                        }
                    }
                    if action != 0 {
                        tot.zaps_sent += 1;
                    }

                    if cfg.chat > 0.0 && (b.rng.f32() as f64) < cfg.chat / 60.0 / cfg.hz as f64 {
                        b.chat_n += 1;
                        let ch = if b.chat_n % 10 == 0 { proto::chat::channel::SHOUT } else { proto::chat::channel::SAY };
                        b.chat.say(ch, &format!("bot {} line {}", b.entity & 0xFFFFFF, b.chat_n));
                        tot.chat_sent += 1;
                    }
                    if b.chat.outgoing(&mut b.chat_buf) {
                        if let Some(sess) = b.session.as_mut() {
                            if sess.seal(&b.chat_buf, &mut sealed) {
                                let _ = b.sock.send(&sealed);
                            }
                        }
                    }
                    b.seq += 1;
                    if let (Some(pr), Some(wd)) = (b.pred.as_mut(), world.as_ref()) {
                        pr.apply_input(wd, b.seq, b.dir.0, b.dir.1);
                    }
                    b.sent_at[b.seq as usize % 128] = (b.seq, Instant::now());
                    wire::encode_input(
                        &mut out,
                        &Input {
                            seq: b.seq,
                            ack_tick: b.view.last_tick,
                            dir_x: b.dir.0,
                            dir_y: b.dir.1,
                            action,
                            target,
                            chat_ack: b.chat.ack(),
                        },
                    );
                    if let Some(sess) = b.session.as_mut() {
                        if sess.seal(&out, &mut sealed) {
                            match b.sock.send(&sealed) {
                                Ok(_) => tot.inputs_sent += 1,
                                Err(_) => tot.send_errors += 1,
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        if last_report.elapsed() >= Duration::from_secs(1) {
            let secs = last_report.elapsed().as_secs_f64();
            window_rtts.sort_unstable();
            let playing = bots.iter().filter(|b| b.state == State::Playing).count();
            if !cfg.quiet {
                let avg = if window_rtts.is_empty() {
                    0.0
                } else {
                    window_rtts.iter().map(|&x| x as f64).sum::<f64>() / window_rtts.len() as f64
                };
                eprintln!(
                    "[{:>4}s] playing {:>5}/{} | snaps {:.1}k/s avg {:.0} B | input->ack avg {:.1} p99 {:.1} ms | missing-baseline {} decode-err {} self-missing {} | late {}",
                    start.elapsed().as_secs(),
                    playing,
                    cfg.bots,
                    w_snaps as f64 / secs / 1000.0,
                    if w_snaps > 0 { w_bytes as f64 / w_snaps as f64 } else { 0.0 },
                    avg / 1000.0,
                    pct(&window_rtts, 0.99) as f64 / 1000.0,
                    tot.missing_baseline,
                    tot.decode_err,
                    tot.self_missing,
                    late_ticks,
                );
            }
            window_rtts.clear();
            w_snaps = 0;
            w_bytes = 0;
            last_report = Instant::now();
        }

        if start.elapsed().as_secs() >= cfg.duration {
            break;
        }
    }

    // Summary.
    let playing = bots.iter().filter(|b| b.state == State::Playing).count();
    rtts.sort_unstable();
    let wall = start.elapsed().as_secs_f64();
    println!("\n==== bots summary ====");
    println!(
        "bots playing at end {playing}/{}, handshakes ok {}, rejected {}, reconnects {}, untrusted server key {}",
        cfg.bots, tot.welcomes, tot.rejects, tot.reconnects, tot.untrusted
    );
    println!(
        "inputs sent {} ({:.1}k/s), zaps requested {}, send errors {}",
        tot.inputs_sent,
        tot.inputs_sent as f64 / wall / 1000.0,
        tot.zaps_sent,
        tot.send_errors
    );
    println!(
        "snapshots applied {} ({:.1}k/s), avg {:.0} B, avg visible entities {:.1}",
        tot.snaps,
        tot.snaps as f64 / wall / 1000.0,
        tot.snap_bytes as f64 / tot.snaps.max(1) as f64,
        tot.visible as f64 / tot.snaps.max(1) as f64
    );
    println!(
        "input->ack latency ms: p50 {:.1} p99 {:.1} p99.9 {:.1} max {:.1}  (includes up to one tick of server wait)",
        pct(&rtts, 0.50) as f64 / 1000.0,
        pct(&rtts, 0.99) as f64 / 1000.0,
        pct(&rtts, 0.999) as f64 / 1000.0,
        *rtts.last().unwrap_or(&0) as f64 / 1000.0
    );
    println!("items: {} inventory and {} ground updates decoded", tot.inventories, tot.grounds);
    println!(
        "integrity: decode errors {}, missing baseline {}, stale {}, own entity missing from snapshot {}, failed decryption {}",
        tot.decode_err, tot.missing_baseline, tot.stale, tot.self_missing, tot.auth_fail
    );
    pred_err.sort_unstable();
    println!(
        "prediction: {} reconciliations, correction p50 {:.3} p99 {:.3} max {:.3} units, {} teleports (respawns) excluded",
        pred_err.len(),
        pct(&pred_err, 0.5) as f64 / 1000.0,
        pct(&pred_err, 0.99) as f64 / 1000.0,
        pred_err.last().copied().unwrap_or(0) as f64 / 1000.0,
        tot.teleports
    );
    if cfg.chat > 0.0 {
        let pending: usize = bots.iter().map(|b| b.chat.unsent()).sum();
        println!("chat: lines sent {}, lines received {} (say is heard nearby, shout by all), malformed {}, still unacknowledged {}", tot.chat_sent, tot.chat_rx, tot.chat_bad, pending);
    }
    println!("tester late ticks {late_ticks} of {tick} (if high, the load generator, not the server, is the bottleneck)");
}

fn pct(sorted: &[u32], p: f64) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    sorted[((sorted.len() as f64 - 1.0) * p).round() as usize]
}
