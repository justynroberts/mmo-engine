//! Authoritative world server (phase 1 and 2 spike).
//!
//! One pinned thread owns the zone: drain inputs, step the simulation at a fixed
//! rate, build interest-filtered delta snapshots, send them in batches.

mod blocks;
mod botdir;
mod ctl;
mod director;
mod encounter;
mod factions;
mod hail;
mod items;
mod life;
mod market;
mod io;
mod nav;
mod quests;
mod realm;
mod net;
mod sim;
mod social;
mod statics;
mod store;
mod trade;
mod web;
mod worldfile;
mod zone;

/// Most packets one tick handles per connected client (an input a tick, plus chat and room to catch up after a
/// hiccup); the rest of a backlog is shed.
const RECV_PER_CLIENT: usize = 4;
/// Most packets one tick reads only to drop; anything past it waits in the socket for the next tick.
const SHED_MAX: usize = 65_536;

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use proto::game::GameFile;
use proto::secure::{from_hex32, to_hex, ServerKeys};

use net::{Clients, Handshake, NetStats, SnapStats, Worker};
use sim::{PendingAction, Sim, NONE};

struct Config {
    port: u16,
    hz: u32,
    world_size: u32,
    /// A world file (terrain, water and objects from worldgen); its solid objects become the walls.
    world_file: Option<PathBuf>,
    /// The world generator the console's World tab runs.
    worldgen: Option<PathBuf>,
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
    db: Option<PathBuf>,
    hello_rate: u32,
    limit_loopback: bool,
    db_expire_days: u32,
    fresh_world: bool,
    web: Option<String>,
    web_max_conns: usize,
    web_per_ip: u32,
    web_trust_proxy: bool,
    no_browser: bool,
    new_ids_per_hour: u32,
    zone: Option<(u8, u8)>,
    zone_listen: Option<String>,
    zone_peers: Vec<String>,
    zone_public: Vec<String>,
    zone_web: Vec<String>,
    zone_secret: Option<String>,
    zone_lines: bool,
    zone_names: Vec<String>,
    bots_dir: Option<PathBuf>,
}

fn parse_args() -> Config {
    let mut c = Config {
        port: 7777,
        hz: 20,
        world_size: 2048,
        world_file: None,
        worldgen: None,
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
        db: Some(PathBuf::from("data/world.db")),
        hello_rate: 100,
        limit_loopback: false,
        db_expire_days: 90,
        fresh_world: false,
        web: None,
        web_max_conns: 2000,
        web_per_ip: 8,
        web_trust_proxy: false,
        no_browser: false,
        new_ids_per_hour: 30,
        zone: None,
        zone_listen: None,
        zone_peers: Vec::new(),
        zone_public: Vec::new(),
        zone_web: Vec::new(),
        zone_secret: None,
        zone_lines: false,
        zone_names: Vec::new(),
        bots_dir: Some(PathBuf::from("bots.d")),
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
            "--world" => c.world_file = Some(PathBuf::from(val(a))),
            "--worldgen" => c.worldgen = Some(PathBuf::from(val(a))),
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
            "--db" => c.db = Some(PathBuf::from(val(a))),
            "--no-db" => c.db = None,
            "--hello-rate" => c.hello_rate = val(a).parse().unwrap_or_else(|_| usage("bad --hello-rate")),
            "--limit-loopback" => c.limit_loopback = true,
            "--new-identities-per-hour" => c.new_ids_per_hour = val(a).parse().unwrap_or_else(|_| usage("bad --new-identities-per-hour")),
            "--fresh-world" => c.fresh_world = true,
            "--web" => c.web = Some(val(a)),
            "--web-max-conns" => c.web_max_conns = val(a).parse().unwrap_or_else(|_| usage("bad --web-max-conns")),
            "--web-per-ip" => c.web_per_ip = val(a).parse().unwrap_or_else(|_| usage("bad --web-per-ip")),
            "--web-trust-proxy" => c.web_trust_proxy = true,
            "--no-browser" => c.no_browser = true,
            "--db-expire-days" => c.db_expire_days = val(a).parse().unwrap_or_else(|_| usage("bad --db-expire-days")),
            "--zone" => {
                let v = val(a);
                let (i, n) = v.split_once('/').unwrap_or_else(|| usage("--zone wants INDEX/COUNT, like 0/2"));
                let (i, n): (u8, u8) = (i.parse().unwrap_or_else(|_| usage("bad --zone")), n.parse().unwrap_or_else(|_| usage("bad --zone")));
                if n < 2 || i >= n {
                    usage("--zone INDEX/COUNT needs COUNT >= 2 and INDEX < COUNT");
                }
                c.zone = Some((i, n));
            }
            "--zone-listen" => c.zone_listen = Some(val(a)),
            "--zone-peers" => c.zone_peers = val(a).split(',').map(|s| s.trim().to_string()).collect(),
            "--zone-public" => c.zone_public = val(a).split(',').map(|s| s.trim().to_string()).collect(),
            "--zone-web" => c.zone_web = val(a).split(',').map(|s| s.trim().to_string()).collect(),
            "--zone-secret" => c.zone_secret = Some(val(a)),
            "--zone-lines" => c.zone_lines = true,
            "--zone-names" => c.zone_names = val(a).split(',').map(|s| s.trim().to_string()).collect(),
            "--bots-dir" => c.bots_dir = Some(PathBuf::from(val(a))),
            "--no-bots-dir" => c.bots_dir = None,
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
        "world server\n\n  --port N          UDP port (7777)\n  --hz N            tick rate (20)\n  --world-size N    world edge in units, 64..65536 (2048)\n  --seed N          world seed (1)\n  --world FILE      a world file (worldgen): its size and solid objects make the world; served to clients at /play/world (world.json if present)\n  --worldgen FILE   the world generator the console runs (worldgen beside this binary, else on PATH)\n  --max-clients N   (8192)\n  --npcs N          server-side NPC entities (0)\n  --duration S      exit after S seconds, 0 = run forever (0)\n  --pin CORE        pin the sim thread to a CPU core\n  --timeout S       drop silent clients after S seconds (5)\n  --workers N       snapshot lanes: 1 = sim thread only, N > 1 uses N-1 more cores (1)\n  --game FILE       game definitions, created with defaults if missing (game.json)\n  --bots-dir DIR    drop-in bot types: *.json files here become live bots, no restart (bots.d)\n  --no-bots-dir     do not watch a bots directory\n  --key-file FILE   server identity key, created if missing (server.key)\n  --admin ADDR      admin/UI http address, loopback only (127.0.0.1:7780)\n  --no-admin        do not start the admin interface\n  --ui-dir DIR      built UI to serve (ui/dist if present)\n  --db FILE         player and world database (data/world.db)\n  --fresh-world     ignore the saved bots and start from the archetype counts\n  --web ADDR        public web gateway: lets browsers play (WebSocket to UDP relay plus the play page), off by default\n  --web-max-conns N browsers connected at once (2000)\n  --web-per-ip N    browsers per address (8)\n  --web-trust-proxy read the real address from X-Forwarded-For (only behind a TLS proxy you control)\n  --no-browser      the web port serves only the data clients fetch: no play page, no WebSocket relay
  --no-db           do not remember players
  --db-expire-days N  forget players not seen for N days, checked at startup, 0 = never (90)
  --new-identities-per-hour N  new player identities one address may create per hour, 0 = unlimited (30)
  --hello-rate N    handshake packets per second allowed from one address, 0 = unlimited (100)
  --limit-loopback  apply that limit to 127.x too (off so local load tests work)
  --zone I/N        run as zone I of N: this process owns the I-th vertical strip of the world (off)
  --zone-listen ADDR  where this zone accepts handoffs from its neighbours (TCP), private network only
  --zone-peers A,B,..  handoff address of every zone in index order
  --zone-public A,B,.. public UDP address (ip:port) of every zone in index order, given to clients on handoff
  --zone-web U,V,..  public web base URL of every zone, for browser clients (optional)
  --zone-secret S   shared secret for the handoff link, or set MMO_ZONE_SECRET (at least 16 bytes)
  --zone-lines      each zone is a map of its own (its own --world and game file), joined by exits on places,
                    instead of a strip of one world
  --zone-names A,B,..  every zone's name in index order, for exits to name where they lead (default 0,1,..)
  --quiet           only print the final summary"
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

static STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn main() {
    let mut cfg = parse_args();
    // Stop cleanly on SIGINT/SIGTERM so players are saved on the way out.
    unsafe {
        libc::signal(libc::SIGINT, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t);
    }
    let mut game_file = load_game(&cfg.game);
    let game = game_file.build().expect("validated by load_game");
    let defs_hash = game.defs.content_hash();
    // A world file, when given, decides the world's size and walls; the game file's walls are not used. Without
    // --world, a world.json here is used if there is one: that is where the console's World tab saves a world.
    let world_path = cfg.world_file.clone().unwrap_or_else(|| PathBuf::from("world.json"));
    if cfg.world_file.is_none() && world_path.is_file() {
        eprintln!("using the world file {} (made in the console's World tab, or pass --world)", world_path.display());
        cfg.world_file = Some(world_path.clone());
    }
    let world_file = cfg.world_file.as_deref().map(|p| {
        let w = worldfile::load(p).unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(1);
        });
        if game_file.walls.is_some() {
            eprintln!("note: {} has hand-placed walls; the world file's walls are used instead", cfg.game.display());
        }
        cfg.world_size = w.size as u32;
        std::sync::Arc::new(w)
    });
    let world = match &world_file {
        Some(w) => w.world(cfg.seed),
        None => game_file.world(cfg.world_size, cfg.seed).unwrap_or_else(|e| {
            eprintln!("{}: {e}", cfg.game.display());
            std::process::exit(1);
        }),
    };
    // The walls as the admin API and the gateway serve them, read on their own threads.
    let tiles = match &world_file {
        Some(w) => w.tiles.clone(),
        None => std::sync::Arc::new(proto::tiles::TileSet::build(world.size, world.boxes.clone(), Vec::new()).unwrap_or_else(|e| {
            eprintln!("{}: {e}", cfg.game.display());
            std::process::exit(1);
        })),
    };
    if world_file.is_some() {
        eprintln!("{} walls, at most {} touching one {} unit tile", tiles.boxes.len(), tiles.densest(), proto::tiles::TILE);
    }
    let statics = statics::Statics::new(tiles, world_file.clone());
    let n_boxes = world.boxes.len();
    let bots: u32 = game.archs.iter().map(|a| a.count).sum();
    let cap = cfg.max_clients + cfg.npcs + bots as usize + 4096;
    let mut sim = Sim::new(game, world, cap, cfg.hz, cfg.seed);
    sim.player_reserve = cfg.max_clients;
    if let Some(w) = &world_file {
        sim.terrain = w.terrain.clone();
    }
    // What players built, kept by region beside the world file (or, from before, in one file: read once and
    // written out by region, the old file left as it was).
    let blocks_dir = blocks::dir_for(&world_path);
    let blocks_path = blocks::path_for(&world_path);
    let started = Instant::now();
    if let Some(cols) = blocks::load_dir(&blocks_dir) {
        let dropped = sim.set_blocks(cols);
        eprintln!("restored {} built block(s) in {} region(s) from {} in {:.0?}{}", sim.blocks.total, sim.blocks.regions.len(), blocks_dir.display(), started.elapsed(), if dropped > 0 { format!(" ({dropped} column(s) inside walls or under the ground left out)") } else { String::new() });
        sim.blocks.take_dirty();
        sim.blocks.unsaved = false;
    } else if let Ok(text) = std::fs::read_to_string(&blocks_path) {
        match blocks::Blocks::parse(&text) {
            Ok(cols) => {
                let dropped = sim.set_blocks(cols);
                eprintln!("restored {} built block(s) from {}, to be kept by region in {}{}", sim.blocks.total, blocks_path.display(), blocks_dir.display(), if dropped > 0 { format!(" ({dropped} column(s) inside walls or under the ground left out)") } else { String::new() });
                sim.blocks.take_dirty();
                sim.blocks.all_dirty();
            }
            Err(e) => eprintln!("{}: {e}; starting with no built blocks", blocks_path.display()),
        }
    }
    let block_writer = blocks::RegionWriter::new(blocks_dir.clone());
    let mut director = director::Director::new();
    director.sync(&game_file, 0, cfg.hz);
    let mut botdir = cfg.bots_dir.as_deref().map(botdir::BotDir::open);
    let mut zone: Option<zone::Zone> = cfg.zone.map(|(index, count)| {
        let secret = cfg.zone_secret.clone().or_else(|| std::env::var("MMO_ZONE_SECRET").ok()).unwrap_or_default();
        if secret.len() < 16 {
            usage("zones need a shared secret of at least 16 bytes: --zone-secret or MMO_ZONE_SECRET");
        }
        let listen = cfg.zone_listen.as_deref().unwrap_or_else(|| usage("--zone needs --zone-listen")).parse().unwrap_or_else(|_| usage("bad --zone-listen"));
        if cfg.zone_peers.len() != count as usize || cfg.zone_public.len() != count as usize {
            usage("--zone-peers and --zone-public need one entry per zone");
        }
        let public = cfg.zone_public.iter().map(|a| a.parse().unwrap_or_else(|_| usage("bad --zone-public address, want ip:port"))).collect();
        let names = if cfg.zone_names.is_empty() { (0..count).map(|i| i.to_string()).collect() } else { cfg.zone_names.clone() };
        if names.len() != count as usize || names.iter().any(|n| n.is_empty() || n.len() > 32) {
            usage("--zone-names needs one name per zone, 1..32 characters each");
        }
        let z = zone::Zone::start(zone::ZoneCfg { index, count, listen, peers: cfg.zone_peers.clone(), public, secret: secret.into_bytes(), lines: cfg.zone_lines, names }, sim.world.size).unwrap_or_else(|e| {
            eprintln!("cannot start zone link on {listen}: {e}");
            std::process::exit(1);
        });
        if z.shared.lines {
            eprintln!("zone {index} of {count} ('{}'): a map of its own, joined to the others by exits, handoffs on tcp/{listen}", z.shared.names[index as usize]);
        } else {
            sim.zone = Some((z.shared.lo, z.shared.hi));
            eprintln!("zone {index} of {count}: owns x {:.0}..{:.0}, handoffs on tcp/{listen}", z.shared.lo, z.shared.hi);
        }
        z
    });
    let keys = load_keys(&cfg.key_file);
    let store = cfg.db.as_ref().map(|p| {
        let st = store::Store::open(p).unwrap_or_else(|e| {
            eprintln!("cannot open player database: {e}");
            std::process::exit(1);
        });
        if cfg.db_expire_days > 0 {
            let n = st.expire(cfg.db_expire_days as i64 * 86_400);
            if n > 0 {
                eprintln!("forgot {n} player(s) not seen for {} days", cfg.db_expire_days);
            }
        }
        std::sync::Arc::new(st)
    });
    match store.as_ref().filter(|_| !cfg.fresh_world).and_then(|st| st.load_world()) {
        Some(w) => {
            sim.populate_from(&w);
            eprintln!("restored {} of {} saved bot(s)", sim.ents.bots, w.bots.len());
        }
        None => sim.populate_bots(),
    }
    // Guilds, land and crops in the ground (after the blocks, which a crop must still be in).
    if let Some(text) = store.as_ref().filter(|_| !cfg.fresh_world).and_then(|st| st.load_doc("realm")) {
        match serde_json::from_str::<realm::Saved>(&text) {
            Ok(saved) => {
                eprintln!("restored {} guild(s), {} plot(s), {} crop(s)", saved.guilds.len(), saved.plots.len(), saved.crops.len());
                sim.realm_restore(saved);
            }
            Err(e) => eprintln!("realm: saved state unreadable ({e}); starting without guilds, plots or crops"),
        }
    }
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

    let gateway = cfg.web.as_ref().map(|a| {
        let addr: std::net::SocketAddr = a.parse().unwrap_or_else(|_| usage("bad --web address"));
        let gw = web::Gateway::new(statics.clone());
        let ui = cfg.ui_dir.clone().or_else(|| {
            let d = PathBuf::from("ui/dist");
            d.is_dir().then_some(d)
        });
        let wc = web::WebCfg { addr, ui_dir: ui, udp_port: cfg.port, max_conns: cfg.web_max_conns, per_ip: cfg.web_per_ip, trust_proxy: cfg.web_trust_proxy, no_browser: cfg.no_browser, zone_hosts: cfg.zone_web.iter().map(|u| u.split("://").nth(1).unwrap_or(u).trim_end_matches('/').to_string()).collect() };
        match web::start(wc, gw.clone()) {
            Ok(local) if cfg.no_browser => eprintln!("web gateway: http://{local}/  (client data only: no play page, no WebSocket relay{})", if cfg.web_trust_proxy { ", trusting X-Forwarded-For" } else { "" }),
            Ok(local) => eprintln!("web gateway: http://{local}/  (browsers play through WebSocket, at most {} at once, {} per address{})", cfg.web_max_conns, cfg.web_per_ip, if cfg.web_trust_proxy { ", trusting X-Forwarded-For" } else { "" }),
            Err(e) => {
                eprintln!("cannot start web gateway on {a}: {e}");
                std::process::exit(1);
            }
        }
        gw
    });
    let mut hs = Handshake { keys, defs_hash, max_clients: cfg.max_clients, store, gateway, zone: zone.as_ref().map(|z| z.shared.clone()), zone_web: cfg.zone_web.clone(), world: world_file, statics: statics.clone() };
    if let Some(g) = &hs.gateway {
        g.set_info(ctl::play_info(&sim, &hs));
    }
    let params = ctl::Params {
        cap,
        world_size: cfg.world_size,
        seed: cfg.seed,
        hz: cfg.hz,
        port: cfg.port,
        plain_npcs: cfg.npcs,
        game_path: Some(cfg.game.clone()),
        max_clients: cfg.max_clients,
    };
    let mut live = ctl::LiveStats { snap_lanes: lanes_n, ..Default::default() };
    let admin_rx = cfg.admin.as_ref().map(|a| {
        let addr: std::net::SocketAddr = a.parse().unwrap_or_else(|_| usage("bad --admin address"));
        let ui = cfg.ui_dir.clone().or_else(|| {
            let d = PathBuf::from("ui/dist");
            d.is_dir().then_some(d)
        });
        // The generator: as given, else the worldgen built or installed beside this binary, else whatever is on PATH.
        let worldgen = cfg.worldgen.clone().unwrap_or_else(|| {
            let near = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("worldgen")));
            near.filter(|p| p.is_file()).unwrap_or_else(|| PathBuf::from("worldgen"))
        });
        let gen = ctl::GenCfg { worldgen, preview: world_path.with_extension("preview.json"), world: world_path.clone(), seed: cfg.seed };
        let rx = ctl::start(addr, ui.clone(), gen, statics.clone()).unwrap_or_else(|e| {
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
    // Claimed names stay taken while their players are away.
    if let Some(st) = &hs.store {
        for (id, n) in st.names() {
            clients.names.entry(proto::names::key(&n)).or_insert(id);
        }
    }
    clients.limiter = Some(net::RateLimiter::new(cfg.hello_rate, !cfg.limit_loopback));
    clients.id_limiter = Some(net::IdentityLimiter::new(cfg.new_ids_per_hour, !cfg.limit_loopback));
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
    // This tick's incoming packets, copied out of the receive ring so they can all be handled at once.
    let mut in_data: Vec<u8> = Vec::with_capacity(1 << 20);
    let mut in_pkts: Vec<(std::net::SocketAddrV4, usize, usize)> = Vec::with_capacity(16 * 1024);

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

        // 1. Inputs and handshakes. Everything that came in since the last tick is gathered first and handled in one
        // go (one parallel decryption across the lanes, not one per recvmmsg). Catching up never eats the tick: past a
        // few ticks' worth of packets, or a third of the tick spent reading, the rest of a backlog is read and dropped
        // (shed). After a stall that costs everyone a little loss, which the protocol shrugs off, instead of a spiral
        // where each tick works through a bigger backlog than the last.
        let now_secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as u32);
        let cap = (clients.count * RECV_PER_CLIENT).max(4096);
        let read_until = t0 + dt / 3;
        in_data.clear();
        in_pkts.clear();
        let mut shed = 0usize;
        loop {
            let n = rx.recv();
            // Time only counts once there is a real backlog: a hiccup in the middle of reading a handful of packets
            // must not drop them.
            if in_pkts.len() + n > cap || (in_pkts.len() >= cap / 4 && Instant::now() > read_until) {
                shed += n;
            } else {
                for i in 0..n {
                    let (addr, data) = rx.packet(i);
                    in_pkts.push((addr, in_data.len(), data.len()));
                    in_data.extend_from_slice(data);
                }
            }
            if n < io::RBATCH || shed >= SHED_MAX {
                break;
            }
        }
        nstats.shed += shed as u64;
        let pkts: Vec<(std::net::SocketAddrV4, &[u8])> = in_pkts.iter().map(|&(a, at, len)| (a, &in_data[at..at + len])).collect();
        net::handle_batch(&mut sim, &mut clients, &mut tx, &hs, &mut nstats, &pkts, &mut lanes, cfg.pin, &mut scratch, &mut plain, now_secs);
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
                bot_dir: botdir.as_ref().map(|b| &b.status),
                director: &mut director,
            };
            ctl::service(rx, &mut c);
        }
        // Bot types dropped into the bots directory join the running world.
        if let Some(bd) = botdir.as_mut() {
            if let Some(f) = bd.poll(&game_file) {
                let text = f.to_json();
                let mut c = ctl::Ctl {
                    sim: &mut sim,
                    clients: &mut clients,
                    hs: &mut hs,
                    game: &mut game_file,
                    net: &nstats,
                    live: &live,
                    params: &params,
                    bot_dir: None,
                    director: &mut director,
                };
                let (status, body) = ctl::apply_game(&text, &mut c);
                if status != 200 {
                    eprintln!("bots dir: not applied: {body}");
                    bd.status.error = Some(body);
                    bd.status.applied -= 1;
                }
            }
        }
        let t1 = Instant::now();

        // 2. Simulate.
        net::drain_actions(&mut clients, &mut pending);
        if sim.tick % 20 == 0 {
            net::reap(&mut sim, &mut clients, hs.store.as_deref(), timeout_ticks, &mut nstats);
            net::save_progress(&sim, &mut clients, hs.store.as_deref());
            if sim.tick % (20 * 30) == 0 {
                net::save_all(&sim, &clients, hs.store.as_deref());
            }
        }
        if let Some(z) = zone.as_mut() {
            z.ingest(&mut sim);
        }
        if director.tick(&mut sim, &mut clients) {
            if let Some(g) = &hs.gateway {
                g.set_info(ctl::play_info(&sim, &hs));
            }
        }
        sim.step(&pending);
        net::exits(&mut sim, &mut clients, zone.as_ref());
        if let Some(z) = zone.as_mut() {
            z.tick(&mut sim, &mut clients, hs.store.as_deref(), &mut tx, &mut scratch);
            tx.flush();
        }
        net::route_chat(&mut sim, &mut clients, hs.store.as_deref());
        // Names that changed this tick (spawns, removals, players renaming) go to the web gateway's table.
        if sim.names_all || !sim.ents.renamed.is_empty() {
            if let Some(g) = &hs.gateway {
                g.names_changed(&sim);
            }
            sim.ents.renamed.clear();
            sim.names_all = false;
        }
        // Blocks built or broken this tick go to the clients, and are saved every few seconds.
        let changes = sim.blocks.take_changes();
        if !changes.is_empty() {
            if let Some(g) = &hs.gateway {
                g.blocks_changed(changes, &sim.blocks.owners);
            }
        }
        if sim.blocks.unsaved && sim.tick % (cfg.hz * 5).max(1) == 0 {
            // Only the regions that changed, as text here and written by the writer thread.
            sim.blocks.unsaved = false;
            block_writer.save(sim.blocks.take_dirty());
        }
        if sim.rs.dirty && sim.tick % (cfg.hz * 5).max(1) == 0 {
            sim.rs.dirty = false;
            if let (Some(st), Ok(text)) = (&hs.store, serde_json::to_string(&sim.realm_saved())) {
                st.save_doc("realm", text);
            }
        }
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
                    "[{:>4}s] clients {:>5} ents {:>5} | tick avg {} p99 {} max {} ms (recv {} sim {} snap+send {}) | in {:.1}k/s out {:.1}k/s {:.1} Mbit/s | late {} over {} drops {} shed {}",
                    (t4 - start).as_secs(),
                    clients.count,
                    sim.ents.live - sim.ghost_count(),
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
                    nstats.shed,
                );
            }
            window.clear();
            phase_w = Phase::default();
            w_in = nstats.in_pkts;
            w_out = t_sent;
            w_bytes = t_bytes;
            last_report = t4;
        }

        if (cfg.duration > 0 && (t4 - start).as_secs() >= cfg.duration) || STOP.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
    }

    // Everyone online is saved before we exit, and so are the blocks, guilds, land and crops.
    net::save_all(&sim, &clients, hs.store.as_deref());
    block_writer.save(sim.blocks.take_dirty());
    block_writer.finish();
    if let (Some(st), Ok(text)) = (&hs.store, serde_json::to_string(&sim.realm_saved())) {
        st.save_doc("realm", text);
    }
    if let Some(st) = &hs.store {
        st.flush();
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
    println!("ticks over budget {overruns}, ticks that started late {late}, packets shed {}", nstats.shed);
    println!(
        "cpu: user {:.2}s sys {:.2}s = {:.0}% of one core over the run",
        user,
        sys,
        (user + sys) / wall * 100.0
    );
    println!(
        "packets in {} (hello {}, connect {}, input {}), unparseable {}, bad cookie {}, failed authentication {}, rejected handshakes {}, timeouts {}, decrypted on lanes {}, rate limited {}, identity limited {}",
        nstats.in_pkts, nstats.hellos, nstats.connects, nstats.inputs, nstats.bad, nstats.bad_cookie, nstats.auth_fail, nstats.rejected, nstats.timeouts, nstats.lane_decoded, nstats.rate_limited, nstats.identity_limited
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
