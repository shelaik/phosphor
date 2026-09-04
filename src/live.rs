//! Determine which sessions are currently running by correlating
//! ~/.claude/sessions/<pid>.json + daemon/roster.json with live OS processes.

use crate::json::P;
use crate::scan::Session;
use std::collections::{HashMap, HashSet};
use std::path::Path;

struct LiveInfo {
    pid: u64,
    status: String,
    entrypoint: String,
}

/// Parse a `~/.claude/sessions/<pid>.json` file (flat object of scalars).
fn parse_session_file(bytes: &[u8]) -> Option<(String, LiveInfo)> {
    let mut p = P::new(bytes);
    let mut sid = String::new();
    let mut pid = 0u64;
    let mut status = String::new();
    let mut entrypoint = String::new();
    if !p.obj_begin() {
        return None;
    }
    loop {
        let k = match p.obj_key() {
            Some(k) => k,
            None => break,
        };
        match k.as_str() {
            "sessionId" => sid = p.take_string().unwrap_or_default(),
            "pid" => pid = p.take_number() as u64,
            "status" => status = p.take_string().unwrap_or_default(),
            "entrypoint" => entrypoint = p.take_string().unwrap_or_default(),
            _ => {
                let _ = p.skip();
            }
        }
        if !p.obj_sep() {
            break;
        }
    }
    if sid.is_empty() {
        return None;
    }
    Some((
        sid,
        LiveInfo {
            pid,
            status,
            entrypoint,
        },
    ))
}

/// Read the daemon roster: { workers: { id: { pid, sessionId, ... } } }.
fn parse_roster(bytes: &[u8], out: &mut HashMap<String, LiveInfo>) {
    let mut p = P::new(bytes);
    if !p.obj_begin() {
        return;
    }
    loop {
        let k = match p.obj_key() {
            Some(k) => k,
            None => break,
        };
        if k == "workers" {
            if p.obj_begin() {
                loop {
                    // worker id -> worker object
                    if p.obj_key().is_none() {
                        break;
                    }
                    let mut sid = String::new();
                    let mut pid = 0u64;
                    if p.obj_begin() {
                        loop {
                            let wk = match p.obj_key() {
                                Some(k) => k,
                                None => break,
                            };
                            match wk.as_str() {
                                "sessionId" => sid = p.take_string().unwrap_or_default(),
                                "pid" => pid = p.take_number() as u64,
                                _ => {
                                    let _ = p.skip();
                                }
                            }
                            if !p.obj_sep() {
                                break;
                            }
                        }
                    }
                    if !sid.is_empty() {
                        out.entry(sid).or_insert(LiveInfo {
                            pid,
                            status: "busy".into(),
                            entrypoint: String::new(),
                        });
                    }
                    if !p.obj_sep() {
                        break;
                    }
                }
            }
        } else {
            let _ = p.skip();
        }
        if !p.obj_sep() {
            break;
        }
    }
}

fn load_live(base: &Path) -> HashMap<String, LiveInfo> {
    let mut map = HashMap::new();
    if let Ok(rd) = std::fs::read_dir(base.join("sessions")) {
        for e in rd.flatten() {
            if e.path().extension().and_then(|x| x.to_str()) == Some("json") {
                if let Ok(b) = std::fs::read(e.path()) {
                    if let Some((sid, info)) = parse_session_file(&b) {
                        map.insert(sid, info);
                    }
                }
            }
        }
    }
    if let Ok(b) = std::fs::read(base.join("daemon").join("roster.json")) {
        parse_roster(&b, &mut map);
    }
    map
}

/// Currently-running processes: their PIDs, and their lowercased image names
/// (Windows: `tasklist`). The names let the Codex pass tell a live agent from a
/// lock file a crash left behind.
#[cfg(windows)]
fn running_procs() -> (HashSet<u64>, HashSet<String>) {
    let (mut pids, mut names) = (HashSet::new(), HashSet::new());
    if let Ok(out) = std::process::Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .output()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            // "image.exe","1234","Console","1","12,345 K"
            let cols: Vec<&str> = line.split("\",\"").collect();
            if cols.len() >= 2 {
                if let Ok(pid) = cols[1].trim_matches('"').trim().parse::<u64>() {
                    pids.insert(pid);
                }
                names.insert(cols[0].trim_matches('"').trim().to_lowercase());
            }
        }
    }
    (pids, names)
}

#[cfg(not(windows))]
fn running_procs() -> (HashSet<u64>, HashSet<String>) {
    let (mut pids, mut names) = (HashSet::new(), HashSet::new());
    if let Ok(rd) = std::fs::read_dir("/proc") {
        for e in rd.flatten() {
            if let Some(name) = e.file_name().to_str() {
                if let Ok(pid) = name.parse::<u64>() {
                    pids.insert(pid);
                    if let Ok(comm) = std::fs::read_to_string(e.path().join("comm")) {
                        names.insert(comm.trim().to_lowercase());
                    }
                }
            }
        }
    }
    (pids, names)
}

/// Annotate sessions with live/idle/ended state.
pub fn annotate(base: &Path, sessions: &mut [Session]) {
    let live = load_live(base);
    let (pids, names) = running_procs();
    // Codex tracks its open threads with a lock file per thread instead of the
    // per-pid session files Claude Code writes. A crash can strand a lock, so it
    // only counts while a codex process is actually up.
    let codex_home = crate::codex::home();
    let codex_open: HashSet<String> = codex_home
        .as_deref()
        .map(crate::codex::open_threads)
        .unwrap_or_default()
        .into_iter()
        .collect();
    let codex_up = names.iter().any(|n| n.starts_with("codex"));
    for s in sessions.iter_mut() {
        if s.is_codex() {
            s.live = if codex_up && codex_open.contains(&s.id) {
                "running".into()
            } else {
                "ended".into()
            };
            continue;
        }
        match live.get(&s.id) {
            Some(info) if info.pid != 0 && pids.contains(&info.pid) => {
                s.pid = info.pid;
                s.status = info.status.clone();
                s.live = if info.status == "busy" {
                    "running".into()
                } else {
                    "idle".into()
                };
                if !info.entrypoint.is_empty() && s.entrypoint.is_empty() {
                    s.entrypoint = info.entrypoint.clone();
                }
            }
            _ => {
                s.live = "ended".into();
            }
        }
    }
}
