//! Check a game file the way the server does before it applies one: `cargo run -p proto --example check_game FILE`.
fn main() {
    let path = std::env::args().nth(1).expect("usage: check_game GAME.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let g: proto::game::GameFile = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
    match g.build() {
        Ok(game) => println!("ok: {} places, {} bot types, {} routes", game.places.len(), game.archs.len(), game.routes.len()),
        Err(e) => {
            eprintln!("{path}: {e}");
            std::process::exit(1);
        }
    }
}
