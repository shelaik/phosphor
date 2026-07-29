//! Fleet view: see the sessions of the user's OTHER PCs and resume them there.
//!
//! Transport is `ssh` by shell-out — Phosphor contains no network client and
//! stores no credentials: only the ssh ALIASES live in `phosphor.json`; hosts,
//! users and keys belong to `~/.ssh/config` and ssh-agent, and the channel is
//! encrypted by SSH itself. Fetch runs `ssh <alias> phosphor json` and parses
//! the same JSON `phosphor json` prints locally; resume runs
//! `ssh -t <alias> phosphor resume-here <id>` so `claude --resume` executes ON
//! the session's home machine, in the right cwd, inheriting the ssh terminal.
//!
//! Everything a remote returns is UNTRUSTED input for this process: ids are
//! re-validated (they end up in argv), every string is stripped of control and
//! bidi-override chars before it can reach the TUI, sizes and counts are
//! capped, and parse is lenient (unknown keys skipped) so mixed Phosphor
//! versions across the fleet keep working.

use crate::json::P;
use crate::scan::Session;
use std::io::Read;

/// Hard cap on bytes accepted from a remote `phosphor json` (a compromised or
/// buggy remote must not OOM us). 64 MiB comfortably fits tens of thousands of
/// sessions.
const MAX_FETCH: usize = 64 * 1024 * 1024;
/// Hard deadline for one host: connect (5s, ssh-side) + remote scan + transfer.
const FETCH_DEADLINE_SECS: u64 = 60;
/// Caps on parsed content (defense in depth, not expected in practice).
const MAX_SESSIONS: usize = 20_000;
const MAX_FIELD: usize = 16 * 1024;
const MAX_LIST: usize = 512;
/// Numeric clamp: token/size counters beyond this are garbage and would wreck
/// the cost/usage aggregates.
const MAX_NUM: f64 = 1e15;
/// search_text synthesized locally for remote sessions (the JSON doesn't carry
/// it) — same spirit as scan.rs's index, capped.
const SEARCH_CAP: usize = 8 * 1024;

/// True when `a` is safe to use as the ssh destination in an argv: ASCII
/// alphanumerics plus `_ . @ -`, starting alphanumeric (never flag-shaped).
/// Accepts both `~/.ssh/config` aliases ("pc-casa") and `user@host` forms.
/// Checked at `remote add` AND again before every spawn — phosphor.json is
/// hand-editable.
pub fn valid_alias(a: &str) -> bool {
    !a.is_empty()
        && a.len() <= 64
        && a.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && a.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '@' | '-'))
}

/// Sanitize + clamp one untrusted string field: control/bidi chars become
/// spaces (see [`crate::tame`]), length is bounded.
fn clean(v: Option<String>) -> String {
    let t = crate::tame(&v.unwrap_or_default());
    if t.len() <= MAX_FIELD {
        return t;
    }
    let mut end = MAX_FIELD;
    while end > 0 && !t.is_char_boundary(end) {
        end -= 1;
    }
    t[..end].to_string()
}

/// Take a numeric value, clamped to a sane non-negative range.
fn num(p: &mut P) -> u64 {
    let n = p.take_number();
    if n.is_finite() && n > 0.0 {
        n.min(MAX_NUM) as u64
    } else {
        0
    }
}

/// Append lowercased text to the synthesized search index (bounded).
fn push_search(buf: &mut String, txt: &str) {
    if buf.len() >= SEARCH_CAP || txt.is_empty() {
        return;
    }
    let lower = txt.to_lowercase();
    let mut end = (SEARCH_CAP - buf.len()).min(lower.len());
    while end > 0 && !lower.is_char_boundary(end) {
        end -= 1;
    }
    buf.push_str(&lower[..end]);
    buf.push(' ');
}

/// Parse the output of a remote `phosphor json` (the exact format of
/// [`crate::server::sessions_json`]) into Sessions tagged with `host`.
///
/// Lenient (unknown keys skipped, missing keys default) so version skew across
/// the fleet is fine. Sessions whose id is not argv-safe are DROPPED — a
/// crafted remote id must never reach `marked`, config keys or an ssh argv.
/// Note: the JSON carries no `path`, `summary` or chain fields; remote
/// sessions therefore have an empty `path` (local file actions are gated on
/// `host` being non-empty) and never chain-link with local ones.
pub fn parse_sessions_json(data: &[u8], host: &str) -> Vec<Session> {
    let mut out: Vec<Session> = Vec::new();
    let mut p = P::new(data);
    if !p.arr_begin() {
        return out;
    }
    loop {
        if p.obj_begin() {
            let mut s = Session::default();
            loop {
                let k = match p.obj_key() {
                    Some(k) => k,
                    None => break,
                };
                match k.as_str() {
                    "id" => s.id = clean(p.take_string()),
                    "project" => s.project_name = clean(p.take_string()),
                    "projectPath" => s.project_path = clean(p.take_string()),
                    "title" => s.title = clean(p.take_string()),
                    "firstPrompt" => s.first_prompt = clean(p.take_string()),
                    "lastPrompt" => s.last_prompt = clean(p.take_string()),
                    "messages" => s.message_count = num(&mut p),
                    "inputTokens" => s.input_tokens = num(&mut p),
                    "outputTokens" => s.output_tokens = num(&mut p),
                    "cacheRead" => s.cache_read = num(&mut p),
                    "cacheCreation" => s.cache_creation = num(&mut p),
                    "models" => {
                        if p.arr_begin() {
                            loop {
                                if let Some(v) = p.take_string() {
                                    if s.models.len() < MAX_LIST {
                                        s.models.push(clean(Some(v)));
                                    }
                                }
                                if !p.arr_sep() {
                                    break;
                                }
                            }
                        } else {
                            let _ = p.skip();
                        }
                    }
                    "tools" => {
                        if p.arr_begin() {
                            loop {
                                if p.obj_begin() {
                                    let (mut name, mut count) = (String::new(), 0u64);
                                    loop {
                                        let tk = match p.obj_key() {
                                            Some(k) => k,
                                            None => break,
                                        };
                                        match tk.as_str() {
                                            "name" => name = clean(p.take_string()),
                                            "count" => count = num(&mut p),
                                            _ => {
                                                let _ = p.skip();
                                            }
                                        }
                                        if !p.obj_sep() {
                                            break;
                                        }
                                    }
                                    if !name.is_empty() && s.tools.len() < MAX_LIST {
                                        s.tools.push((name, count));
                                    }
                                } else {
                                    let _ = p.skip();
                                }
                                if !p.arr_sep() {
                                    break;
                                }
                            }
                        } else {
                            let _ = p.skip();
                        }
                    }
                    "files" => {
                        if p.arr_begin() {
                            loop {
                                if let Some(v) = p.take_string() {
                                    if s.files.len() < MAX_LIST {
                                        s.files.push(clean(Some(v)));
                                    }
                                }
                                if !p.arr_sep() {
                                    break;
                                }
                            }
                        } else {
                            let _ = p.skip();
                        }
                    }
                    "gitBranch" => s.git_branch = clean(p.take_string()),
                    "version" => s.version = clean(p.take_string()),
                    "entrypoint" => s.entrypoint = clean(p.take_string()),
                    "created" => s.created = clean(p.take_string()),
                    "modified" => s.modified = clean(p.take_string()),
                    "mtime" => s.mtime_ms = num(&mut p),
                    "size" => s.size = num(&mut p),
                    "subagents" => s.subagents = num(&mut p),
                    "workflows" => s.workflows = num(&mut p),
                    "sidechain" => s.is_sidechain = p.take_bool(),
                    "live" => s.live = clean(p.take_string()),
                    "status" => s.status = clean(p.take_string()),
                    "pid" => s.pid = num(&mut p),
                    _ => {
                        let _ = p.skip();
                    }
                }
                if !p.obj_sep() {
                    break;
                }
            }
            if crate::valid_session_id(&s.id) && out.len() < MAX_SESSIONS {
                s.host = host.to_string();
                let mut search = String::new();
                push_search(&mut search, &s.title);
                push_search(&mut search, &s.first_prompt);
                push_search(&mut search, &s.last_prompt);
                push_search(&mut search, &s.project_name);
                push_search(&mut search, host);
                for (n, _) in &s.tools {
                    push_search(&mut search, n);
                }
                for f in &s.files {
                    let base = f.rsplit(|c| c == '\\' || c == '/').next().unwrap_or(f);
                    push_search(&mut search, base);
                }
                s.search_text = search;
                out.push(s);
            }
        } else {
            let _ = p.skip();
        }
        if !p.arr_sep() {
            break;
        }
    }
    out
}

/// Read up to `cap` bytes, then keep DRAINING to EOF without storing (so the
/// child never blocks on a full pipe). Returns `(bytes, overflowed)`.
fn read_capped(r: &mut impl Read, cap: usize) -> (Vec<u8>, bool) {
    let mut buf = Vec::new();
    let mut over = false;
    let mut chunk = [0u8; 65536];
    loop {
        match r.read(&mut chunk) {
            Ok(0) | Err(_) => return (buf, over),
            Ok(n) => {
                if !over && buf.len() + n > cap {
                    over = true;
                    buf.clear();
                }
                if !over {
                    buf.extend_from_slice(&chunk[..n]);
                }
            }
        }
    }
}

/// Run `ssh <alias> <remote_cmd…>` non-interactively and capture stdout.
/// BatchMode forbids any prompt (an unknown host key fails cleanly instead of
/// hanging a background thread) and a watchdog kills the child at the deadline
/// so a stalled remote can never wedge the fetch. Returns
/// `Err((exit_code, message))` on failure.
fn run_ssh(alias: &str, remote_cmd: &[&str]) -> Result<Vec<u8>, (Option<i32>, String)> {
    use std::process::Stdio;
    let mut cmd = std::process::Command::new("ssh");
    cmd.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "--"]).arg(alias);
    for a in remote_cmd {
        cmd.arg(a);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: don't disturb the TUI console
    }
    let mut child = cmd.spawn().map_err(|e| (None, format!("ssh non eseguibile: {e}")))?;
    let mut so = child.stdout.take().expect("stdout piped");
    let mut se = child.stderr.take().expect("stderr piped");
    let h_out = std::thread::spawn(move || read_capped(&mut so, MAX_FETCH));
    let h_err = std::thread::spawn(move || read_capped(&mut se, 16 * 1024));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(FETCH_DEADLINE_SECS);
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err((None, format!("timeout ({FETCH_DEADLINE_SECS}s): host bloccato o scan remoto troppo lento")));
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => return Err((None, format!("attesa ssh fallita: {e}"))),
        }
    };
    let (out, over) = h_out.join().unwrap_or((Vec::new(), false));
    let (err_raw, _) = h_err.join().unwrap_or((Vec::new(), false));
    if over {
        return Err((status.code(), format!("risposta oltre {} MiB: rifiutata", MAX_FETCH / 1024 / 1024)));
    }
    if !status.success() {
        let msg: String = crate::tame(String::from_utf8_lossy(&err_raw).trim()).chars().take(200).collect();
        let code = status.code();
        let hint = if code == Some(255) && msg.contains("Host key") {
            "  (host sconosciuto: connettiti una volta a mano con `ssh <alias>`)"
        } else {
            ""
        };
        return Err((code, if msg.is_empty() { format!("ssh exit {}", code.unwrap_or(-1)) } else { format!("{msg}{hint}") }));
    }
    Ok(out)
}

/// Fetch the session list from one remote: `ssh <alias> phosphor json`.
///
/// If ssh reached the host but the command failed (exit != 255 — 255 is ssh's
/// own transport/auth error, not worth a retry), retries once through a login
/// shell: non-interactive ssh on Unix skips `~/.profile`, so `~/.local/bin`
/// may be missing from PATH. The retry MUST be a single argv token — ssh joins
/// the remote argv with spaces and hands the string to the remote shell, so
/// `["sh", "-lc", "phosphor json"]` would run `phosphor` with `json` as `$0`.
pub fn fetch_host(alias: &str) -> Result<Vec<u8>, String> {
    if !valid_alias(alias) {
        return Err("alias non valido (ammessi: alfanumerici e _ . @ -, iniziale alfanumerica)".into());
    }
    match run_ssh(alias, &["phosphor", "json"]) {
        Ok(out) => Ok(out),
        Err((Some(255), msg)) => Err(msg),
        Err((_, first_msg)) => {
            run_ssh(alias, &["sh -lc 'phosphor json'"]).map_err(|(_, m)| {
                format!("{first_msg}  (retry con login shell: {m})")
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(id: &str, title: &str) -> Session {
        let mut s = Session::default();
        s.id = id.into();
        s.title = title.into();
        s.project_name = "proj".into();
        s.project_path = "C:\\x\\proj".into();
        s.input_tokens = 1234;
        s.models = vec!["claude-opus-4".into()];
        s.tools = vec![("Bash".into(), 7)];
        s.files = vec!["C:\\x\\proj\\main.rs".into()];
        s.live = "idle".into();
        s
    }

    #[test]
    fn parse_round_trips_and_defends() {
        let good = mk("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", "titolo ok");
        // hostile: ANSI escape + bidi override in the title, huge token count
        let mut evil_title = mk("11111111-2222-3333-4444-555555555555", "x");
        evil_title.title = "rosso \u{1b}[31m finto\u{202E}odnetta".into();
        evil_title.input_tokens = u64::MAX;
        // hostile: flag-shaped id must be dropped entirely
        let evil_id = mk("-cfg", "id malevolo");

        let json = crate::server::sessions_json(&[good, evil_title, evil_id]);
        let got = parse_sessions_json(json.as_bytes(), "pc-casa");

        assert_eq!(got.len(), 2, "flag-shaped id dropped");
        assert!(got.iter().all(|s| s.host == "pc-casa"));
        let g = &got[0];
        assert_eq!(g.id, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        assert_eq!(g.title, "titolo ok");
        assert_eq!(g.input_tokens, 1234);
        assert_eq!(g.models, vec!["claude-opus-4".to_string()]);
        assert_eq!(g.tools, vec![("Bash".to_string(), 7)]);
        assert_eq!(g.live, "idle");
        assert!(g.path.is_empty(), "remote sessions carry no local path");
        assert!(g.search_text.contains("titolo ok"));
        assert!(g.search_text.contains("pc-casa"), "host searchable");
        let e = &got[1];
        assert!(!e.title.contains('\u{1b}'), "ESC stripped: {:?}", e.title);
        assert!(!e.title.contains('\u{202E}'), "bidi override stripped");
        assert_eq!(e.input_tokens, MAX_NUM as u64, "token count clamped");
    }

    #[test]
    fn parse_tolerates_junk_and_unknown_keys() {
        // unknown keys, non-object array entries, truncated tail
        let j = br#"[{"id":"abc123","futureKey":{"x":[1,2]},"title":"ok"},42,{"id":"not valid!!"}]"#;
        let got = parse_sessions_json(j, "h");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].title, "ok");
        assert!(parse_sessions_json(b"not json at all", "h").is_empty());
        assert!(parse_sessions_json(b"", "h").is_empty());
    }

    #[test]
    fn alias_validation() {
        assert!(valid_alias("pc-casa"));
        assert!(valid_alias("utente@host.lan"));
        assert!(valid_alias("a"));
        assert!(!valid_alias(""));
        assert!(!valid_alias("-oProxyCommand=calc")); // flag-shaped
        assert!(!valid_alias("pc casa")); // space
        assert!(!valid_alias("pc;rm")); // shell metachar
        assert!(!valid_alias("pc'x"));
        assert!(!valid_alias(&"x".repeat(65)));
    }
}
