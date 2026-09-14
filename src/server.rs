//! A tiny dependency-free HTTP server that serves the embedded dashboard and a
//! JSON API over the in-memory session index.

use crate::json::escape as esc;
use crate::scan::Session;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

pub const INDEX_HTML: &str = include_str!("index.html");

pub struct State {
    pub base: PathBuf,
    pub sessions: RwLock<Vec<Session>>,
    pub cache: Mutex<HashMap<String, Session>>,
}

fn session_to_json(s: &Session) -> String {
    let models = s
        .models
        .iter()
        .map(|m| format!("\"{}\"", esc(m)))
        .collect::<Vec<_>>()
        .join(",");
    let tools = s
        .tools
        .iter()
        .map(|(n, c)| format!("{{\"name\":\"{}\",\"count\":{}}}", esc(n), c))
        .collect::<Vec<_>>()
        .join(",");
    let files = s
        .files
        .iter()
        .map(|f| format!("\"{}\"", esc(f)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        concat!(
            "{{\"id\":\"{}\",\"project\":\"{}\",\"projectPath\":\"{}\",\"title\":\"{}\",",
            "\"firstPrompt\":\"{}\",\"lastPrompt\":\"{}\",\"messages\":{},",
            "\"inputTokens\":{},\"outputTokens\":{},\"cacheRead\":{},\"cacheCreation\":{},",
            "\"models\":[{}],\"tools\":[{}],\"files\":[{}],\"filesCount\":{},",
            "\"gitBranch\":\"{}\",\"version\":\"{}\",\"entrypoint\":\"{}\",",
            "\"created\":\"{}\",\"modified\":\"{}\",\"mtime\":{},\"size\":{},",
            "\"subagents\":{},\"workflows\":{},\"sidechain\":{},\"recovered\":{},\"vaulted\":{},",
            "\"agent\":\"{}\",\"live\":\"{}\",\"status\":\"{}\",\"pid\":{},\"lastState\":\"{}\"}}"
        ),
        esc(&s.id),
        esc(&s.project_name),
        esc(&s.project_path),
        esc(&s.title),
        esc(&s.first_prompt),
        esc(&s.last_prompt),
        s.message_count,
        s.input_tokens,
        s.output_tokens,
        s.cache_read,
        s.cache_creation,
        models,
        tools,
        files,
        s.files.len(),
        esc(&s.git_branch),
        esc(&s.version),
        esc(&s.entrypoint),
        esc(&s.created),
        esc(&s.modified),
        s.mtime_ms,
        s.size,
        s.subagents,
        s.workflows,
        s.is_sidechain,
        s.is_ghost(),
        s.is_vaulted(),
        esc(if s.agent.is_empty() { "claude" } else { &s.agent }),
        esc(&s.live),
        esc(&s.status),
        s.pid,
        esc(s.last_state()),
    )
}

pub fn sessions_json(list: &[Session]) -> String {
    let mut out = String::from("[");
    for (i, s) in list.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&session_to_json(s));
    }
    out.push(']');
    out
}

fn respond(stream: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) {
    // Bound the write: a client that opens the connection then stops reading
    // must not pin this handler thread forever (with MAX_CONN that would starve
    // the pool). Loopback writes complete near-instantly, so 15s is generous.
    let _ = stream.set_write_timeout(Some(Duration::from_secs(15)));
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

/// Read one request header value (case-insensitive), empty if absent.
fn header_value(req: &str, name: &str) -> String {
    for line in req.lines().skip(1) {
        if line.is_empty() {
            break; // blank line = end of headers
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case(name) {
                return v.trim().to_string();
            }
        }
    }
    String::new()
}

/// True if a `Host`/authority value points at the loopback interface.
/// Empty is allowed (non-browser clients may omit Host); anything else is not.
fn host_is_local(authority: &str) -> bool {
    let a = authority.trim();
    if a.is_empty() {
        return true;
    }
    let name = if let Some(rest) = a.strip_prefix('[') {
        rest.split(']').next().unwrap_or("") // [::1]:port -> ::1
    } else {
        a.split(':').next().unwrap_or("") // 127.0.0.1:port -> 127.0.0.1
    };
    matches!(name, "127.0.0.1" | "localhost" | "::1")
}

/// True if an `Origin` header is same-origin (loopback) or genuinely absent.
fn origin_is_local(origin: &str) -> bool {
    let o = origin.trim();
    if o.is_empty() {
        return true; // same-origin requests may omit Origin
    }
    if o == "null" {
        return false; // file:// / sandboxed iframe -> treat as cross-origin
    }
    let after = o.split_once("://").map(|(_, b)| b).unwrap_or(o);
    host_is_local(after)
}

/// Read the full request header block (up to the blank line) into a string.
/// Fails closed (returns None) on an incomplete, oversized, or timed-out request
/// so a security-relevant header (Host/Origin) can never be silently dropped by a
/// short read. We don't need the body — every endpoint here is header-only.
fn read_headers(stream: &mut TcpStream) -> Option<String> {
    const MAX: usize = 16 * 1024;
    // Absolute deadline: a fixed per-read timeout resets on every byte, so a slow
    // client that trickles one byte just under it could hold the thread for hours
    // (slowloris). We cap the WHOLE header read at 15s by shrinking each read's
    // timeout to the time left until the deadline — so the effective cap really is
    // ~15s, not (deadline + one full per-read timeout).
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    let mut data: Vec<u8> = Vec::with_capacity(4096);
    let mut buf = [0u8; 4096];
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return None; // overall deadline hit -> fail closed
        }
        let _ = stream.set_read_timeout(Some(remaining));
        let n = stream.read(&mut buf).ok()?;
        if n == 0 {
            break; // peer closed
        }
        data.extend_from_slice(&buf[..n]);
        if data.windows(4).any(|w| w == b"\r\n\r\n") || data.windows(2).any(|w| w == b"\n\n") {
            return Some(String::from_utf8_lossy(&data).into_owned());
        }
        if data.len() >= MAX {
            return None; // headers too large -> fail closed
        }
    }
    None // connection ended before headers completed
}

fn handle(mut stream: TcpStream, state: Arc<State>) {
    let req = match read_headers(&mut stream) {
        Some(r) => r,
        None => return,
    };
    let first = req.lines().next().unwrap_or("");
    let method = first.split_whitespace().next().unwrap_or("");
    let target = first.split_whitespace().nth(1).unwrap_or("/");
    let path = target.split('?').next().unwrap_or("/");
    let query = target.split('?').nth(1).unwrap_or("");

    // Defeat DNS-rebinding: a malicious page that resolves its own domain to
    // 127.0.0.1 still sends its domain in Host, so reject any non-loopback Host.
    if !host_is_local(&header_value(&req, "host")) {
        respond(&mut stream, "403 Forbidden", "text/plain; charset=utf-8", b"forbidden host");
        return;
    }

    match path {
        "/" => respond(&mut stream, "200 OK", "text/html; charset=utf-8", INDEX_HTML.as_bytes()),
        "/api/sessions" => {
            let body = {
                let s = state.sessions.read().unwrap();
                sessions_json(&s)
            };
            respond(&mut stream, "200 OK", "application/json", body.as_bytes());
        }
        "/api/refresh" => {
            // State-changing (re-scans + rewrites the cache): same-origin POST only,
            // consistent with /api/resume so a cross-site page can't drive it.
            if method != "POST" || !origin_is_local(&header_value(&req, "origin")) {
                respond(
                    &mut stream,
                    "405 Method Not Allowed",
                    "application/json",
                    b"{\"ok\":false,\"error\":\"usa POST same-origin\"}",
                );
                return;
            }
            crate::rescan(&state);
            let count = state.sessions.read().unwrap().len();
            let body = format!("{{\"ok\":true,\"count\":{count}}}");
            respond(&mut stream, "200 OK", "application/json", body.as_bytes());
        }
        "/api/resume" => {
            // State-changing (spawns a process): require a same-origin POST so a
            // cross-site page can't trigger it via CSRF.
            if method != "POST" || !origin_is_local(&header_value(&req, "origin")) {
                respond(
                    &mut stream,
                    "405 Method Not Allowed",
                    "application/json",
                    b"{\"ok\":false,\"error\":\"usa POST same-origin\"}",
                );
                return;
            }
            let id = query_get(query, "id");
            let body = resume(&state, &id);
            respond(&mut stream, "200 OK", "application/json", body.as_bytes());
        }
        _ => respond(&mut stream, "404 Not Found", "text/plain; charset=utf-8", b"not found"),
    }
}

fn query_get(query: &str, key: &str) -> String {
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return v.to_string();
            }
        }
    }
    String::new()
}

/// Launch `claude --resume <id>` in a fresh terminal at the session's cwd.
/// The id is validated and the cwd comes from our trusted index (never the
/// query), so the command can't be injected.
fn resume(state: &Arc<State>, id: &str) -> String {
    if id.is_empty()
        || id.len() > 40
        || !id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
        || !id.starts_with(|c: char| c.is_ascii_hexdigit())
    {
        return "{\"ok\":false,\"error\":\"id non valido\"}".to_string();
    }
    let found = {
        let s = state.sessions.read().unwrap();
        s.iter()
            .find(|x| x.id == id)
            .map(|x| (x.path.clone(), x.project_path.clone()))
    };
    let cwd = match found {
        // Correct a drifted (subdir) cwd to the transcript's startup-cwd folder
        // so claude --resume can locate the session (see resume_cwd_for).
        Some((path, pp)) if !pp.is_empty() => crate::resume_cwd_for(&path, &pp),
        _ => return "{\"ok\":false,\"error\":\"sessione non trovata\"}".to_string(),
    };
    let ok = crate::resume_session(&cwd, id);
    format!("{{\"ok\":{ok}}}")
}

/// RAII counter slot: decrements the active-connection count on Drop, so a
/// panicking handler can never permanently leak a connection slot.
struct ConnSlot(Arc<AtomicUsize>);
impl Drop for ConnSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

pub fn serve(state: Arc<State>, port: u16, open: bool) {
    let addr = format!("127.0.0.1:{port}");
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Impossibile aprire {addr}: {e}");
            eprintln!("Prova un'altra porta:  phosphor --web --port 9000");
            return;
        }
    };
    let url = format!("http://{addr}/");
    println!("\n  claudescan in ascolto su  {url}");
    println!("  (Ctrl+C per uscire)\n");
    if open {
        open_browser(&url);
    }
    // Cap concurrent connections so a flood can't spawn unbounded threads.
    // A browser uses ~6; 64 is generous headroom while still bounding the worst case.
    const MAX_CONN: usize = 64;
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        if let Ok(mut stream) = stream {
            if active.load(Ordering::Relaxed) >= MAX_CONN {
                respond(&mut stream, "503 Service Unavailable", "text/plain; charset=utf-8", b"busy");
                continue;
            }
            active.fetch_add(1, Ordering::Relaxed);
            let slot = ConnSlot(active.clone());
            let state = state.clone();
            std::thread::spawn(move || {
                let _slot = slot; // releases the slot on Drop, even if handle() panics
                handle(stream, state);
            });
        }
    }
}

fn open_browser(url: &str) {
    #[cfg(windows)]
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}
