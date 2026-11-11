//! The admin console on a port of its own (`--admin-web ADDR`), behind a password, for a server whose admin interface
//! is on a loopback nobody can reach (a container). The public web port never serves it: the deployment publishes
//! this port only on a private network (the Coolify host's Tailscale address), so the console is not on the internet
//! at all.
//!
//! Off unless `MMO_ADMIN_PASSWORD` is set. Then `/admin/` asks for the password and, given it, sets a session
//! cookie (HttpOnly, SameSite=Strict) and serves the console; the console's `/api/*` calls are passed to the admin
//! interface on loopback as they are. A script or the MCP server can instead send `Authorization: Bearer <password>`
//! with each `/api/*` call. Wrong passwords are limited per address.
//!
//! The admin interface still binds only to loopback and still checks its Host header and the X-Admin header on
//! writes (which a page on another site cannot send here: no CORS is offered).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a login lasts.
const SESSION: Duration = Duration::from_secs(12 * 3600);
/// Wrong passwords allowed per address in `FAIL_WINDOW`.
const FAILS: u32 = 8;
const FAIL_WINDOW: Duration = Duration::from_secs(15 * 60);
/// Largest request body passed on (a game file is well under this).
const BODY_MAX: usize = 16 << 20;
const COOKIE: &str = "mmo_admin";

pub struct AdminWeb {
    password: blake3::Hash,
    ctl: SocketAddr,
    /// Served over TLS: the cookie is marked Secure.
    secure: bool,
    sessions: Mutex<HashMap<blake3::Hash, Instant>>,
    fails: Mutex<HashMap<std::net::IpAddr, (u32, Instant)>>,
}

/// What `web.rs` read of a request's head that matters here.
pub struct Req<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub query: &'a str,
    pub cookie: Option<&'a str>,
    pub auth: Option<&'a str>,
    pub content_type: Option<&'a str>,
    pub content_length: usize,
    pub x_admin: bool,
    pub ip: std::net::IpAddr,
}

fn random_token() -> String {
    let mut b = [0u8; 32];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).is_err() {
        // Never a guessable session: refuse to make one.
        return String::new();
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

impl AdminWeb {
    /// From the environment: None unless `MMO_ADMIN_PASSWORD` is set (12 characters at least).
    pub fn from_env(ctl: SocketAddr, secure: bool) -> Option<AdminWeb> {
        let pw = std::env::var("MMO_ADMIN_PASSWORD").ok().filter(|p| !p.is_empty())?;
        if pw.chars().count() < 12 {
            eprintln!("MMO_ADMIN_PASSWORD is shorter than 12 characters: the console is not offered");
            return None;
        }
        Some(AdminWeb { password: blake3::hash(pw.as_bytes()), ctl, secure, sessions: Mutex::default(), fails: Mutex::default() })
    }

    /// Whether this request is the console's.
    pub fn wants(path: &str) -> bool {
        path == "/admin" || path.starts_with("/admin/") || path.starts_with("/api/")
    }

    /// Compared as hashes, in constant time.
    fn password_ok(&self, given: &str) -> bool {
        blake3::hash(given.as_bytes()) == self.password
    }

    fn blocked(&self, ip: std::net::IpAddr) -> bool {
        let mut f = self.fails.lock().unwrap_or_else(|e| e.into_inner());
        f.retain(|_, (_, at)| at.elapsed() < FAIL_WINDOW);
        f.get(&ip).is_some_and(|(n, _)| *n >= FAILS)
    }

    fn failed(&self, ip: std::net::IpAddr) {
        let mut f = self.fails.lock().unwrap_or_else(|e| e.into_inner());
        let e = f.entry(ip).or_insert((0, Instant::now()));
        e.0 += 1;
        eprintln!("admin console: wrong password from {ip} ({} of {FAILS})", e.0);
    }

    fn session_ok(&self, cookie: Option<&str>) -> bool {
        let Some(token) = cookie.and_then(|c| c.split(';').find_map(|kv| kv.trim().strip_prefix(COOKIE).and_then(|v| v.strip_prefix('=')))) else { return false };
        let mut s = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        s.retain(|_, at| at.elapsed() < SESSION);
        s.contains_key(&blake3::hash(token.as_bytes()))
    }

    fn bearer_ok(&self, auth: Option<&str>) -> bool {
        auth.and_then(|a| a.strip_prefix("Bearer ")).is_some_and(|p| self.password_ok(p.trim()))
    }

    /// Answer a console request. `conn` has had the head consumed; the body (if any) is still to read.
    pub fn serve(&self, conn: &mut TcpStream, r: &Req, ui: Option<&Path>) -> std::io::Result<()> {
        let path = if r.path == "/admin" { "/admin/" } else { r.path };
        match (r.method, path) {
            ("POST", "/admin/login") => {
                if self.blocked(r.ip) {
                    return page(conn, 429, &login_page("Too many tries. Wait a while and try again."), "");
                }
                let body = read_body(conn, r.content_length.min(4096))?;
                let form = String::from_utf8_lossy(&body);
                let given = form.split('&').find_map(|kv| kv.strip_prefix("password=")).map(form_decode).unwrap_or_default();
                if !self.password_ok(&given) {
                    self.failed(r.ip);
                    return page(conn, 403, &login_page("That is not the password."), "");
                }
                let token = random_token();
                if token.is_empty() {
                    return page(conn, 500, &login_page("No randomness for a session on this machine."), "");
                }
                self.sessions.lock().unwrap_or_else(|e| e.into_inner()).insert(blake3::hash(token.as_bytes()), Instant::now());
                eprintln!("admin console: signed in from {}", r.ip);
                let secure = if self.secure { "; Secure" } else { "" };
                write!(
                    conn,
                    "HTTP/1.1 303 See Other\r\nLocation: /admin/\r\nSet-Cookie: {COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}{secure}\r\nCache-Control: no-store\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    SESSION.as_secs()
                )
            }
            ("POST", "/admin/logout") => {
                write!(conn, "HTTP/1.1 303 See Other\r\nLocation: /admin/\r\nSet-Cookie: {COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            }
            _ if path.starts_with("/api/") => {
                if !(self.session_ok(r.cookie) || self.bearer_ok(r.auth)) {
                    if r.auth.is_some() {
                        if self.blocked(r.ip) {
                            return plain(conn, 429, "too many wrong passwords");
                        }
                        self.failed(r.ip);
                    }
                    return plain(conn, 401, "sign in at /admin/ (or send Authorization: Bearer <password>)");
                }
                self.pass_on(conn, r)
            }
            ("GET", _) if !self.session_ok(r.cookie) => page(conn, 200, &login_page(""), ""),
            ("GET", "/admin/") => console_file(conn, ui, "index.html"),
            ("GET", p) => console_file(conn, ui, p.trim_start_matches("/admin/")),
            _ => plain(conn, 404, "not found"),
        }
    }

    /// Pass an `/api/*` request to the admin interface on loopback and its answer back.
    fn pass_on(&self, conn: &mut TcpStream, r: &Req) -> std::io::Result<()> {
        if r.content_length > BODY_MAX {
            return plain(conn, 413, "too large");
        }
        let body = read_body(conn, r.content_length)?;
        let Ok(mut up) = TcpStream::connect_timeout(&self.ctl, Duration::from_secs(5)) else {
            return plain(conn, 502, "the admin interface is not answering");
        };
        // World generation can take minutes.
        up.set_read_timeout(Some(Duration::from_secs(600)))?;
        up.set_write_timeout(Some(Duration::from_secs(30)))?;
        let target = if r.query.is_empty() { r.path.to_string() } else { format!("{}?{}", r.path, r.query) };
        let mut head = format!("{} {target} HTTP/1.1\r\nHost: {}\r\nContent-Length: {}\r\nConnection: close\r\n", r.method, self.ctl, body.len());
        if let Some(t) = r.content_type {
            head.push_str(&format!("Content-Type: {t}\r\n"));
        }
        if r.x_admin {
            head.push_str("X-Admin: 1\r\n");
        }
        head.push_str("\r\n");
        up.write_all(head.as_bytes())?;
        up.write_all(&body)?;
        conn.set_write_timeout(Some(Duration::from_secs(60)))?;
        std::io::copy(&mut up, conn)?;
        Ok(())
    }
}

/// Listen on `addr` for the console alone: `/` goes to `/admin/`, anything not the console's is 404.
pub fn start(addr: SocketAddr, admin: AdminWeb, ui: Option<std::path::PathBuf>) -> std::io::Result<SocketAddr> {
    let listener = TcpListener::bind(addr)?;
    let local = listener.local_addr()?;
    let (admin, ui) = (Arc::new(admin), Arc::new(ui));
    std::thread::Builder::new().name("admin-web".into()).spawn(move || {
        let open = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        for conn in listener.incoming().flatten() {
            use std::sync::atomic::Ordering::Relaxed;
            // A few people at most: bound the threads all the same.
            if open.load(Relaxed) >= 64 {
                continue;
            }
            open.fetch_add(1, Relaxed);
            let (admin, ui, open) = (admin.clone(), ui.clone(), open.clone());
            let _ = std::thread::Builder::new().name("admin-conn".into()).spawn(move || {
                let _ = serve_conn(conn, &admin, ui.as_deref());
                open.fetch_sub(1, Relaxed);
            });
        }
    })?;
    Ok(local)
}

fn serve_conn(mut conn: TcpStream, admin: &AdminWeb, ui: Option<&Path>) -> std::io::Result<()> {
    conn.set_read_timeout(Some(Duration::from_secs(5)))?;
    conn.set_write_timeout(Some(Duration::from_secs(5)))?;
    let ip = conn.peer_addr()?.ip();
    let Some((head, head_len)) = crate::web::peek_head(&conn) else { return Ok(()) };
    let mut sink = vec![0u8; head_len];
    conn.read_exact(&mut sink)?;
    if head.path == "/" {
        return write!(conn, "HTTP/1.1 303 See Other\r\nLocation: /admin/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    }
    if !AdminWeb::wants(&head.path) {
        return plain(&mut conn, 404, "not found");
    }
    let r = Req {
        method: &head.method,
        path: &head.path,
        query: &head.query,
        cookie: head.cookie.as_deref(),
        auth: head.auth.as_deref(),
        content_type: head.content_type.as_deref(),
        content_length: head.content_length,
        x_admin: head.x_admin,
        ip,
    };
    admin.serve(&mut conn, &r, ui)
}

fn read_body(conn: &mut TcpStream, n: usize) -> std::io::Result<Vec<u8>> {
    let mut body = vec![0u8; n];
    conn.set_read_timeout(Some(Duration::from_secs(30)))?;
    conn.read_exact(&mut body)?;
    Ok(body)
}

fn form_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    out.push(v);
                    i += 2;
                } else {
                    out.push(b'%');
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn plain(conn: &mut TcpStream, status: u16, text: &str) -> std::io::Result<()> {
    write!(conn, "HTTP/1.1 {status} {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{text}", reason(status), text.len())
}

fn page(conn: &mut TcpStream, status: u16, html: &str, extra: &str) -> std::io::Result<()> {
    write!(
        conn,
        "HTTP/1.1 {status} {}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Frame-Options: DENY\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n{extra}Connection: close\r\n\r\n{html}",
        reason(status),
        html.len()
    )
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        502 => "Bad Gateway",
        _ => "Error",
    }
}

/// A file of the console's build (`index.html` and its assets).
fn console_file(conn: &mut TcpStream, ui: Option<&Path>, rel: &str) -> std::io::Result<()> {
    let Some(root) = ui else { return plain(conn, 404, "no UI directory on this server") };
    let safe = rel.split('/').all(|p| !p.is_empty() && !p.starts_with('.') && !p.contains('\\') && !p.contains('\0'));
    if !safe || rel == "play.html" {
        return plain(conn, 404, "not found");
    }
    let file = root.join(rel);
    let Ok(bytes) = std::fs::read(&file) else { return plain(conn, 404, "not found") };
    let ctype = match file.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("woff2") => "font/woff2",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    };
    write!(conn, "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Frame-Options: DENY\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n", bytes.len())?;
    conn.write_all(&bytes)
}

fn login_page(message: &str) -> String {
    let msg = message.replace('&', "&amp;").replace('<', "&lt;");
    format!(
        r#"<!doctype html>
<!-- MIT License - Copyright (c) fintonlabs.com -->
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>World console</title>
<style>
/* MIT License - Copyright (c) fintonlabs.com */
:root {{ --bg: #0b0c0f; --line: #23262d; --text: #d8dbe2; --muted: #7d828d; --accent: #e0a020; }}
* {{ box-sizing: border-box; }}
body {{ margin: 0; background: var(--bg); color: var(--text); font: 13px/1.5 ui-monospace, "JetBrains Mono", Menlo, monospace; }}
main {{ max-width: 360px; margin: 18vh 0 0 12vw; padding: 0 16px; }}
h1 {{ font-size: 13px; font-weight: 700; letter-spacing: .08em; text-transform: uppercase; margin: 0 0 24px; }}
label {{ display: block; color: var(--muted); margin-bottom: 8px; }}
input {{ width: 100%; background: transparent; color: var(--text); border: 1px solid var(--line); border-radius: 2px; padding: 8px; font: inherit; }}
input:focus, button:focus-visible {{ outline: 1px solid var(--accent); outline-offset: 1px; }}
button {{ margin-top: 16px; background: var(--text); color: var(--bg); border: 0; border-radius: 2px; padding: 8px 16px; font: inherit; font-weight: 700; cursor: pointer; }}
p {{ color: var(--accent); min-height: 20px; }}
footer {{ position: fixed; left: 12vw; bottom: 16px; padding: 0 16px; color: var(--muted); }}
footer a {{ color: var(--muted); }}
@media (prefers-reduced-motion: reduce) {{ * {{ transition-duration: .01ms !important; animation-duration: .01ms !important; }} }}
</style></head>
<body><main>
<h1>World console</h1>
<form method="post" action="/admin/login">
<label for="pw">Password</label>
<input id="pw" name="password" type="password" autocomplete="current-password" autofocus required>
<button type="submit">Sign in</button>
</form>
<p role="status">{msg}</p>
</main>
<footer>Made by <a href="https://fintonlabs.com" target="_blank" rel="noopener">FintonLabs</a></footer>
</body></html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_form_password_is_decoded_and_compared() {
        assert_eq!(form_decode("a%26b+c%3D"), "a&b c=");
        let a = AdminWeb { password: blake3::hash(b"correct horse"), ctl: "127.0.0.1:1".parse().unwrap(), secure: false, sessions: Mutex::default(), fails: Mutex::default() };
        assert!(a.password_ok("correct horse"));
        assert!(!a.password_ok("correct hors"));
        assert!(a.bearer_ok(Some("Bearer correct horse")));
        assert!(!a.bearer_ok(Some("Basic correct horse")));
        assert!(!a.session_ok(Some("mmo_admin=nothing")));
        let ip: std::net::IpAddr = "10.0.0.1".parse().unwrap();
        for _ in 0..FAILS {
            assert!(!a.blocked(ip));
            a.failed(ip);
        }
        assert!(a.blocked(ip), "too many wrong passwords");
    }
}
