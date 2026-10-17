//! Authoritative world server (phase 1 and 2 spike).
//!
//! One pinned thread owns the zone: drain inputs, step the simulation at a fixed
//! rate, build interest-filtered delta snapshots, send them in batches.

mod ctl;
mod io;
mod net;
mod sim;

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use proto::game::GameFile;
use proto::secure::{from_hex32, to_hex, ServerKeys};
use proto::world::World;

use net::{Clients, Handshake, NetStats, SnapStats, Worker};
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
    workers: usize,
    game: PathBuf,
    key_file: PathBuf,
    admin: Option<String>,
    ui_dir: Option<PathBuf>,
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
        workers: 1,
        game: PathBuf::from("game.json"),
        key_file: PathBuf::from("server.key"),
        admin: Some("127.0.0.1:7780".into()),
        ui_dir: None,
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
            "--workers" => c.workers = val(a).parse().unwrap_or_else(|_| usage("bad --workers")),
            "--game" => c.game = PathBuf::from(val(a)),
            "--key-file" => c.key_file = PathBuf::from(val(a)),
            "--admin" => c.admin = Some(val(a)),
            "--no-admin" => c.admin = None,
            "--ui-dir" => c.ui_dir = Some(PathBuf::from(val(a))),
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
        "world server\n\n  --port N          UDP port (7777)\n  --hz N            tick rate (20)\n  --world-size N    world edge in units, 64..4095 (2048)\n  --seed N          world seed (1)\n  --max-clients N   (8192)\n  --npcs N          server-side NPC entities (0)\n  --duration S      exit after S seconds, 0 = run forever (0)\n  --pin CORE        pin the sim thread to a CPU core\n  --timeout S       drop silent clients after S seconds (5)\n  --workers N       snapshot lanes: 1 = sim thread only, N > 1 uses N-1 more cores (1)\n  --game FILE       game definitions, created with defaults if missing (game.json)\n  --key-file FILE   server identity key, created if missing (server.key)\n  --admin ADDR      admin/UI http address, loopback only (127.0.0.1:7780)\n  --no-admin        do not start the admin interface\n  --ui-dir DIR      built UI to serve (ui/dist if present)\n  --quiet           only print the final summary"
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
    let mut game_file = load_game(&cfg.game);
    let game = game_file.build().expect("validated by load_game");
    let defs_hash = game.defs.content_hash();
    let world = World::generate(cfg.world_size, cfg.seed);
    let n_boxes = world.boxes.len();
    let bots: u32 = game.archs.iter().map(|a| a.count).sum();
    let cap = cfg.max_clients + cfg.npcs + bots as usize + 4096;
    let mut sim = Sim::new(game, world, cap, cfg.hz, cfg.seed);
    sim.populate_bots();
    for _ in 0..cfg.npcs {
        sim.spawn(NONE);
    }
    let keys = load_keys(&cfg.key_file);

    let sock = io::bind(cfg.port, 8 << 20, 8 << 20).unwrap_or_else(|e| {
        eprintln!("cannot bind UDP port {}: {e}", cfg.port);
        std::process::exit(1);
    });
    let (rcv, snd) = io::effective_buffers(&sock);
    let fd = std::os::fd::AsRawFd::as_raw_fd(&sock);
    let mut rx = io::Rx::new(fd);
    let mut tx = io::Tx::new(fd);
    let lanes_n = cfg.workers.max(1);
    let mut lanes: Vec<Worker> = (0..lanes_n).map(|_| Worker::new(fd)).collect();

    if let Some(core) = cfg.pin {
        if !io::pin_to_core(core) {
            eprintln!("warning: could not pin to core {core}");
        }
    }

    eprintln!(
        "world server up: udp/{} {} Hz, world {}x{} ({} static boxes), {} bots + {} plain npcs, content hash {:016x}, socket buffers rx {} KiB tx {} KiB, {} snapshot lane(s)",
        cfg.port, cfg.hz, cfg.world_size, cfg.world_size, n_boxes, bots, cfg.npcs, defs_hash, rcv / 1024, snd / 1024, lanes_n
    );
    eprintln!("server public key (pin this in clients with --server-key): {}", to_hex(&keys.public));

    let mut hs = Handshake { keys, defs_hash, max_clients: cfg.max_clients };
    let params = ctl::Params {
        cap,
        world_size: cfg.world_size,
        seed: cfg.seed,
        hz: cfg.hz,
        port: cfg.port,
        plain_npcs: cfg.npcs,
        game_path: Some(cfg.game.clone()),
    };
    let mut live = ctl::LiveStats { snap_lanes: lanes_n, ..Default::default() };
    let admin_rx = cfg.admin.as_ref().map(|a| {
        let addr: std::net::SocketAddr = a.parse().unwrap_or_else(|_| usage("bad --admin address"));
        let ui = cfg.ui_dir.clone().or_else(|| {
            let d = PathBuf::from("ui/dist");
            d.is_dir().then_some(d)
        });
        let rx = ctl::start(addr, ui.clone()).unwrap_or_else(|e| {
            eprintln!("cannot start admin interface on {a}: {e}");
            std::process::exit(1);
        });
        eprintln!(
            "admin ui: http://{a}/  ({})",
            ui.map_or("no UI directory, API only".to_string(), |d| format!("serving {}", d.display()))
        );
        rx
    });
    let mut clients = Clients::default();
    let mut nstats = NetStats::default();
    let mut scratch: Vec<u8> = Vec::with_capacity(1500);
    let mut plain: Vec<u8> = Vec::with_capacity(256);
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

        // 1. Inputs and handshakes.
        let now_secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as u32);
        loop {
            let n = rx.recv();
            for i in 0..n {
                let (addr, data) = rx.packet(i);
                net::handle_packet(&mut sim, &mut clients, &mut tx, &hs, &mut nstats, addr, data, &mut scratch, &mut plain, now_secs);
            }
            if n < io::RBATCH {
                break;
            }
        }
        // Handshake replies go out now rather than after the whole tick.
        tx.flush();
        if let Some(rx) = &admin_rx {
            let mut c = ctl::Ctl {
                sim: &mut sim,
                clients: &mut clients,
                hs: &mut hs,
                game: &mut game_file,
                net: &nstats,
                live: &live,
                params: &params,
            };
            ctl::service(rx, &mut c);
        }
        let t1 = Instant::now();

        // 2. Simulate.
        net::drain_actions(&mut clients, &mut pending);
        if sim.tick % 20 == 0 {
            net::reap(&mut sim, &mut clients, timeout_ticks, &mut nstats);
        }
        sim.step(&pending);
        net::route_chat(&mut sim, &mut clients);
        let t2 = Instant::now();

        // 3. Snapshots: built, encrypted and sent by the lanes (in parallel when workers > 1).
        net::build_snapshots(&sim, &mut clients, &mut lanes, cfg.pin);
        let t3 = Instant::now();
        let t4 = t3;

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
            let (t_sent, t_bytes, t_drop) = totals(&tx, &lanes);
            let out_rate = (t_sent - w_out) as f64 / secs;
            let mbit = (t_bytes - w_bytes) as f64 * 8.0 / secs / 1e6;
            live = ctl::LiveStats {
                tick_avg_ms: avg / 1000.0,
                tick_p99_ms: pct(&w, 0.99) as f64 / 1000.0,
                recv_ms: phase_w.recv as f64 / n / 1000.0,
                sim_ms: phase_w.sim as f64 / n / 1000.0,
                snap_ms: phase_w.snap as f64 / n / 1000.0,
                send_ms: 0.0,
                in_pps: in_rate,
                out_pps: out_rate,
                mbit,
                snap_lanes: lanes_n,
            };
            if !cfg.quiet {
                eprintln!(
                    "[{:>4}s] clients {:>5} ents {:>5} | tick avg {} p99 {} max {} ms (recv {} sim {} snap+send {}) | in {:.1}k/s out {:.1}k/s {:.1} Mbit/s | late {} over {} drops {}",
                    (t4 - start).as_secs(),
                    clients.count,
                    sim.ents.live,
                    ms(avg),
                    ms(pct(&w, 0.99) as f64),
                    ms(*w.last().unwrap_or(&0) as f64),
                    ms(phase_w.recv as f64 / n),
                    ms(phase_w.sim as f64 / n),
                    ms(phase_w.snap as f64 / n),
                    in_rate / 1000.0,
                    out_rate / 1000.0,
                    mbit,
                    late,
                    overruns,
                    t_drop,
                );
            }
            window.clear();
            phase_w = Phase::default();
            w_in = nstats.in_pkts;
            w_out = t_sent;
            w_bytes = t_bytes;
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
        "packets in {} (hello {}, connect {}, input {}), unparseable {}, bad cookie {}, failed authentication {}, rejected handshakes {}, timeouts {}",
        nstats.in_pkts, nstats.hellos, nstats.connects, nstats.inputs, nstats.bad, nstats.bad_cookie, nstats.auth_fail, nstats.rejected, nstats.timeouts
    );
    let (t_sent, t_bytes, t_drop) = totals(&tx, &lanes);
    println!("packets out {} ({:.1} MiB), send drops {}, snapshot lanes {}", t_sent, t_bytes as f64 / 1048576.0, t_drop, lanes_n);
    let mut sstats = SnapStats::default();
    for l in &lanes {
        sstats.snapshots += l.st.snapshots;
        sstats.full += l.st.full;
        sstats.entries_visible += l.st.entries_visible;
        sstats.entries_changed += l.st.entries_changed;
    }
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

fn totals(tx: &io::Tx, lanes: &[Worker]) -> (u64, u64, u64) {
    lanes.iter().fold((tx.sent, tx.bytes, tx.dropped), |a, l| (a.0 + l.tx.sent, a.1 + l.tx.bytes, a.2 + l.tx.dropped))
}

/// Load the game file, writing the defaults the first time so there is
/// something to edit. A broken file is a startup error, not a silent fallback.
fn load_game(path: &PathBuf) -> GameFile {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let f = GameFile::from_json(&text).unwrap_or_else(|e| {
                eprintln!("{}: {e}", path.display());
                std::process::exit(1);
            });
            if let Err(e) = f.build() {
                eprintln!("{}: {e}", path.display());
                std::process::exit(1);
            }
            f
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let f = GameFile::default_game();
            match std::fs::write(path, f.to_json()) {
                Ok(()) => eprintln!("wrote default game definitions to {}", path.display()),
                Err(e) => eprintln!("warning: could not write {}: {e}", path.display()),
            }
            f
        }
        Err(e) => {
            eprintln!("cannot read {}: {e}", path.display());
            std::process::exit(1);
        }
    }
}

/// The server's long-term identity. Created on first run; keep the file private.
fn load_keys(path: &PathBuf) -> ServerKeys {
    use std::os::unix::fs::OpenOptionsExt;
    if let Ok(text) = std::fs::read_to_string(path) {
        let secret = from_hex32(&text).unwrap_or_else(|| {
            eprintln!("{} must hold 64 hex characters", path.display());
            std::process::exit(1);
        });
        return ServerKeys::from_secret(secret);
    }
    let (keys, secret) = ServerKeys::generate();
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, to_hex(&secret).as_bytes()));
    match written {
        Ok(()) => eprintln!("created server identity key {}", path.display()),
        Err(e) => eprintln!("warning: could not save {}: {e} (clients must re-pin after a restart)", path.display()),
    }
    keys
}
