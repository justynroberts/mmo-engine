//! Authoritative world server (phase 1 and 2 spike).
//!
//! One pinned thread owns the zone: drain inputs, step the simulation at a fixed
//! rate, build interest-filtered delta snapshots, send them in batches.

mod io;
mod net;
mod sim;

use std::time::{Duration, Instant};

use proto::world::World;
use proto::Defs;

use net::{Clients, Handshake, NetStats, SnapScratch, SnapStats};
use sim::{PendingAction, Sim, NONE};

struct Config {
    port: u16,
    hz: u32,
    world_size: u16,
    seed: u64,
    max_clients: usize,
    npcs: usize,
    duration: u64,
    pin: Option<usize>,
    quiet: bool,
    timeout_secs: u32,
}

fn parse_args() -> Config {
    let mut c = Config {
        port: 7777,
        hz: 20,
        world_size: 2048,
        seed: 1,
        max_clients: 8192,
        npcs: 0,
        duration: 0,
        pin: None,
        quiet: false,
        timeout_secs: 5,
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let mut val = |name: &str| -> String {
            i += 1;
            args.get(i).cloned().unwrap_or_else(|| usage(&format!("{name} needs a value")))
        };
        match a {
            "--port" => c.port = val(a).parse().unwrap_or_else(|_| usage("bad --port")),
            "--hz" => c.hz = val(a).parse().unwrap_or_else(|_| usage("bad --hz")),
            "--world-size" => c.world_size = val(a).parse().unwrap_or_else(|_| usage("bad --world-size")),
            "--seed" => c.seed = val(a).parse().unwrap_or_else(|_| usage("bad --seed")),
            "--max-clients" => c.max_clients = val(a).parse().unwrap_or_else(|_| usage("bad --max-clients")),
            "--npcs" => c.npcs = val(a).parse().unwrap_or_else(|_| usage("bad --npcs")),
            "--duration" => c.duration = val(a).parse().unwrap_or_else(|_| usage("bad --duration")),
            "--pin" => c.pin = Some(val(a).parse().unwrap_or_else(|_| usage("bad --pin"))),
            "--timeout" => c.timeout_secs = val(a).parse().unwrap_or_else(|_| usage("bad --timeout")),
            "--quiet" => c.quiet = true,
            "-h" | "--help" => usage(""),
            other => usage(&format!("unknown argument {other}")),
        }
        i += 1;
    }
    c
}

fn usage(msg: &str) -> ! {
    if !msg.is_empty() {
        eprintln!("error: {msg}\n");
    }
    eprintln!(
        "world server\n\n  --port N          UDP port (7777)\n  --hz N            tick rate (20)\n  --world-size N    world edge in units, 64..4095 (2048)\n  --seed N          world seed (1)\n  --max-clients N   (8192)\n  --npcs N          server-side NPC entities (0)\n  --duration S      exit after S seconds, 0 = run forever (0)\n  --pin CORE        pin the sim thread to a CPU core\n  --timeout S       drop silent clients after S seconds (5)\n  --quiet           only print the final summary"
    );
    std::process::exit(if msg.is_empty() { 0 } else { 2 });
}

#[derive(Default, Clone, Copy)]
struct Phase {
    recv: u64,
    sim: u64,
    snap: u64,
    send: u64,
}

fn pct(sorted: &[u32], p: f64) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let i = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[i]
}

fn ms(us: f64) -> String {
    format!("{:.2}", us / 1000.0)
}

fn main() {
    let cfg = parse_args();
    let defs = Defs::default_game();
    let defs_hash = defs.content_hash();
    let world = World::generate(cfg.world_size, cfg.seed);
    let n_boxes = world.boxes.len();
    let cap = cfg.max_clients + cfg.npcs + 16;
    let mut sim = Sim::new(defs, world, cap, cfg.hz, cfg.seed);
    for _ in 0..cfg.npcs {
        sim.spawn(NONE);
    }

    let sock = io::bind(cfg.port, 8 << 20, 8 << 20).unwrap_or_else(|e| {
        eprintln!("cannot bind UDP port {}: {e}", cfg.port);
        std::process::exit(1);
    });
    let (rcv, snd) = io::effective_buffers(&sock);
    let fd = std::os::fd::AsRawFd::as_raw_fd(&sock);
    let mut rx = io::Rx::new(fd);
    let mut tx = io::Tx::new(fd);

    if let Some(core) = cfg.pin {
        if !io::pin_to_core(core) {
            eprintln!("warning: could not pin to core {core}");
        }
    }

    eprintln!(
        "world server up: udp/{} {} Hz, world {}x{} ({} static boxes), {} npcs, content hash {:016x}, socket buffers rx {} KiB tx {} KiB",
        cfg.port, cfg.hz, cfg.world_size, cfg.world_size, n_boxes, cfg.npcs, defs_hash, rcv / 1024, snd / 1024
    );

    let hs = Handshake { defs_hash, max_clients: cfg.max_clients };
    let mut clients = Clients::default();
    let mut nstats = NetStats::default();
    let mut sstats = SnapStats::default();
    let mut scratch: Vec<u8> = Vec::with_capacity(1500);
    let mut snap_scratch = SnapScratch::default();
    let mut pending: Vec<PendingAction> = Vec::new();

    let dt = Duration::from_secs_f64(1.0 / cfg.hz as f64);
    let start = Instant::now();
    let mut next = start + dt;
    let mut last_report = start;
    let mut window: Vec<u32> = Vec::with_capacity(cfg.hz as usize * 2);
    let mut phase_w = Phase::default();
    let mut all_ticks: Vec<u32> = Vec::new();
    let mut overruns = 0u64; // tick work took longer than one tick
    let mut late = 0u64; // tick started after its deadline
    let (mut w_in, mut w_out, mut w_bytes) = (0u64, 0u64, 0u64);
    let timeout_ticks = cfg.timeout_secs * cfg.hz;
    let mut peak_clients = 0usize;

    loop {
        // Wait for the deadline: sleep most of it, spin the last stretch.
        let now = Instant::now();
        if next > now + Duration::from_micros(300) {
            std::thread::sleep(next - now - Duration::from_micros(250));
        }
        while Instant::now() < next {
            std::hint::spin_loop();
        }
        let t0 = Instant::now();
        if t0 > next + Duration::from_micros(2000) {
            late += 1;
        }

        // 1. Inputs.
        loop {
            let n = rx.recv();
            for i in 0..n {
                let (addr, data) = rx.packet(i);
                net::handle_packet(&mut sim, &mut clients, &mut tx, &hs, &mut nstats, addr, data, &mut scratch);
            }
            if n < io::RBATCH {
                break;
            }
        }
        let t1 = Instant::now();

        // 2. Simulate.
        net::drain_actions(&mut clients, &mut pending);
        if sim.tick % 20 == 0 {
            net::reap(&mut sim, &mut clients, timeout_ticks, &mut nstats);
        }
        sim.step(&pending);
        let t2 = Instant::now();

        // 3. Snapshots.
        net::build_snapshots(&sim, &mut clients, &mut tx, &mut snap_scratch, &mut sstats);
        let t3 = Instant::now();

        // 4. Send.
        tx.flush();
        let t4 = Instant::now();

        let total = (t4 - t0).as_micros() as u32;
        window.push(total);
        all_ticks.push(total);
        if t4 - t0 > dt {
            overruns += 1;
        }
        phase_w.recv += (t1 - t0).as_micros() as u64;
        phase_w.sim += (t2 - t1).as_micros() as u64;
        phase_w.snap += (t3 - t2).as_micros() as u64;
        phase_w.send += (t4 - t3).as_micros() as u64;
        peak_clients = peak_clients.max(clients.count);

        next += dt;
        let after = Instant::now();
        if after > next {
            // Fell behind. Do not try to replay missed ticks; resync.
            if after - next > dt * 3 {
                next = after + dt;
            }
        }

        // Per-second report.
        if t4 - last_report >= Duration::from_secs(1) {
            let secs = (t4 - last_report).as_secs_f64();
            let n = window.len().max(1) as f64;
            let mut w = window.clone();
            w.sort_unstable();
            let avg = w.iter().map(|&x| x as f64).sum::<f64>() / n;
            let in_rate = (nstats.in_pkts - w_in) as f64 / secs;
            let out_rate = (tx.sent - w_out) as f64 / secs;
            let mbit = (tx.bytes - w_bytes) as f64 * 8.0 / secs / 1e6;
            if !cfg.quiet {
                eprintln!(
                    "[{:>4}s] clients {:>5} ents {:>5} | tick avg {} p99 {} max {} ms (recv {} sim {} snap {} send {}) | in {:.1}k/s out {:.1}k/s {:.1} Mbit/s | late {} over {} drops {}",
                    (t4 - start).as_secs(),
                    clients.count,
                    sim.ents.live,
                    ms(avg),
                    ms(pct(&w, 0.99) as f64),
                    ms(*w.last().unwrap_or(&0) as f64),
                    ms(phase_w.recv as f64 / n),
                    ms(phase_w.sim as f64 / n),
                    ms(phase_w.snap as f64 / n),
                    ms(phase_w.send as f64 / n),
                    in_rate / 1000.0,
                    out_rate / 1000.0,
                    mbit,
                    late,
                    overruns,
                    tx.dropped,
                );
            }
            window.clear();
            phase_w = Phase::default();
            w_in = nstats.in_pkts;
            w_out = tx.sent;
            w_bytes = tx.bytes;
            last_report = t4;
        }

        if cfg.duration > 0 && (t4 - start).as_secs() >= cfg.duration {
            break;
        }
    }

    // Final summary.
    let wall = start.elapsed().as_secs_f64();
    all_ticks.sort_unstable();
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) };
    let cpu = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1e6;
    let (user, sys) = (cpu(ru.ru_utime), cpu(ru.ru_stime));
    let deadline_us = dt.as_micros() as f64;
    println!("\n==== server summary ====");
    println!("wall {:.1}s, {} ticks at {} Hz, peak clients {}", wall, all_ticks.len(), cfg.hz, peak_clients);
    println!(
        "tick time ms: p50 {} p99 {} p99.9 {} max {}   (budget {})",
        ms(pct(&all_ticks, 0.50) as f64),
        ms(pct(&all_ticks, 0.99) as f64),
        ms(pct(&all_ticks, 0.999) as f64),
        ms(*all_ticks.last().unwrap_or(&0) as f64),
        ms(deadline_us)
    );
    println!("ticks over budget {overruns}, ticks that started late {late}");
    println!(
        "cpu: user {:.2}s sys {:.2}s = {:.0}% of one core over the run",
        user,
        sys,
        (user + sys) / wall * 100.0
    );
    println!(
        "packets in {} (hello {}, input {}, bad {}), rejected handshakes {}, timeouts {}",
        nstats.in_pkts, nstats.hellos, nstats.inputs, nstats.bad, nstats.rejected, nstats.timeouts
    );
    println!(
        "packets out {} ({:.1} MiB), send drops {}",
        tx.sent,
        tx.bytes as f64 / 1048576.0,
        tx.dropped
    );
    let snaps = sstats.snapshots.max(1) as f64;
    println!(
        "snapshots {}: {:.1}% full, avg visible {:.1}, avg changed {:.1} entries",
        sstats.snapshots,
        sstats.full as f64 / snaps * 100.0,
        sstats.entries_visible as f64 / snaps,
        sstats.entries_changed as f64 / snaps
    );
    println!("gameplay: zaps {} respawns {} rejected actions {}", sim.stats.zaps, sim.stats.respawns, sim.stats.rejected_actions);
}
