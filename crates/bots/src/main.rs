//! Bot load tester: N real UDP clients speaking the real protocol.
//!
//! Each bot handshakes, sends one input per tick (random walk plus the odd
//! zap), applies delta snapshots with the same `ClientView` a game client would
//! use, acks them, and measures input-to-ack latency. Because it decodes every
//! snapshot, it also catches protocol corruption, not just throughput problems.

use std::net::UdpSocket;
use std::time::{Duration, Instant};

use proto::defs::ACTION_ZAP;
use proto::wire::{self, kind, Input, ServerMsg, PROTO_VERSION};
use proto::world::dpos;
use proto::{decode_snapshot, ClientView, DecodeError, Defs, Rng};

struct Config {
    server: String,
    bots: usize,
    duration: u64,
    hz: u32,
    ramp: usize,
    seed: u64,
    quiet: bool,
}

fn usage(msg: &str) -> ! {
    if !msg.is_empty() {
        eprintln!("error: {msg}\n");
    }
    eprintln!(
        "bot load tester\n\n  --server ADDR   host:port (127.0.0.1:7777)\n  --bots N        number of bots (100)\n  --duration S    seconds to run (30)\n  --hz N          input rate, match the server (20)\n  --ramp N        bots connected per second (500)\n  --seed N        behaviour seed (1)\n  --quiet         only print the final summary"
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
    };
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
            "--quiet" => c.quiet = true,
            "-h" | "--help" => usage(""),
            other => usage(&format!("unknown argument {other}")),
        }
        i += 1;
    }
    c
}

#[derive(PartialEq)]
enum State {
    Idle,
    Connecting,
    Playing,
    Rejected,
}

struct Bot {
    sock: UdpSocket,
    state: State,
    last_hello: Instant,
    entity: u32,
    view: ClientView,
    seq: u32,
    dir: (i8, i8),
    dir_left: u32,
    rng: Rng,
    sent_at: [(u32, Instant); 128],
    rtt_seq: u32,
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
    inputs_sent: u64,
    zaps_sent: u64,
    send_errors: u64,
}

fn main() {
    let cfg = parse_args();
    let defs = Defs::default_game();
    let hash = defs.content_hash();
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
            state: State::Idle,
            last_hello: now,
            entity: 0,
            view: ClientView::new(),
            seq: 0,
            dir: (0, 0),
            dir_left: 0,
            rng: Rng::new(cfg.seed.wrapping_mul(1_000_003).wrapping_add(i as u64)),
            sent_at: [(u32::MAX, now); 128],
            rtt_seq: 0,
        });
    }

    let dt = Duration::from_secs_f64(1.0 / cfg.hz as f64);
    let start = Instant::now();
    let mut next = start + dt;
    let mut buf = [0u8; 2048];
    let mut out = Vec::with_capacity(64);
    let mut tot = Totals::default();
    let mut rtts: Vec<u32> = Vec::new();
    let mut window_rtts: Vec<u32> = Vec::new();
    let mut last_report = start;
    let mut w_snaps = 0u64;
    let mut w_bytes = 0u64;
    let mut activated = 0usize;
    let mut late_ticks = 0u64;
    let mut tick = 0u64;

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

        for b in bots.iter_mut() {
            if b.state == State::Idle || b.state == State::Rejected {
                continue;
            }
            // Receive everything waiting.
            loop {
                match b.sock.recv(&mut buf) {
                    Ok(n) if n > 0 => {
                        let t = Instant::now();
                        match buf[0] {
                            kind::SNAPSHOT => match decode_snapshot(&buf[..n], &defs, &mut b.view) {
                                Ok(info) if !info.stale => {
                                    tot.snaps += 1;
                                    tot.snap_bytes += n as u64;
                                    tot.visible += b.view.entries().len() as u64;
                                    w_snaps += 1;
                                    w_bytes += n as u64;
                                    if b.view.get(b.entity).is_none() {
                                        tot.self_missing += 1;
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
                            },
                            _ => match wire::decode_server(&buf[..n]) {
                                Some(ServerMsg::Welcome(w)) => {
                                    if b.state != State::Playing {
                                        b.state = State::Playing;
                                        b.entity = w.entity;
                                        tot.welcomes += 1;
                                    }
                                }
                                Some(ServerMsg::Reject { reason }) => {
                                    b.state = State::Rejected;
                                    tot.rejects += 1;
                                    eprintln!("rejected by server, reason {reason}");
                                }
                                None => tot.decode_err += 1,
                            },
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break, // WouldBlock
                }
            }

            match b.state {
                State::Connecting => {
                    if b.last_hello.elapsed() >= Duration::from_millis(250) {
                        wire::encode_hello(&mut out, hash);
                        let _ = b.sock.send(&out);
                        b.last_hello = Instant::now();
                    }
                }
                State::Playing => {
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
                        let mut best = 8.0f32 * 8.0;
                        for e in b.view.entries() {
                            if e.id == b.entity {
                                continue;
                            }
                            let (dx, dy) = (dpos(e.x) - mx, dpos(e.y) - my);
                            let d2 = dx * dx + dy * dy;
                            if d2 <= best {
                                best = d2;
                                action = ACTION_ZAP;
                                target = e.id;
                            }
                        }
                    }
                    if action != 0 {
                        tot.zaps_sent += 1;
                    }

                    b.seq += 1;
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
                        },
                    );
                    match b.sock.send(&out) {
                        Ok(_) => tot.inputs_sent += 1,
                        Err(_) => tot.send_errors += 1,
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
    println!("bots playing at end {playing}/{}, handshakes ok {}, rejected {}", cfg.bots, tot.welcomes, tot.rejects);
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
    println!(
        "integrity: decode errors {}, missing baseline {}, stale {}, own entity missing from snapshot {}",
        tot.decode_err, tot.missing_baseline, tot.stale, tot.self_missing
    );
    println!("tester late ticks {late_ticks} of {tick} (if high, the load generator, not the server, is the bottleneck)");
    let _ = PROTO_VERSION;
}

fn pct(sorted: &[u32], p: f64) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    sorted[((sorted.len() as f64 - 1.0) * p).round() as usize]
}
