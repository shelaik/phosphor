//! Recover projects and sessions whose transcripts Claude Code has DELETED.
//!
//! Claude Code enforces its own retention (`cleanupPeriodDays`, 30 days by
//! default): at startup it unlinks every `projects/<enc>/<uuid>.jsonl` older
//! than that window. The folder survives (its `memory/` sidecar is kept), but
//! the conversation is gone from disk — so a scanner that only walks
//! `projects/` shows an empty list and the project silently vanishes.
//!
//! Two artefacts outlive the cleanup and let us rebuild a usable skeleton:
//!
//! * `~/.claude/history.jsonl` — every prompt the user ever typed, with its
//!   millisecond timestamp and the working directory it was typed in. Never
//!   pruned.
//! * `projects/<enc>/memory/*.md` — the per-project auto-memory files.
//!
//! From the prompt stream we cut *bursts* (runs of prompts in the same cwd
//! separated by less than [`GAP_MS`]) and turn each into a "recovered" session:
//! real title, real prompt list, real time span, no transcript behind it. The
//! assistant's side, token counts and cost are unrecoverable and stay zero.
//!
//! A burst that overlaps a session still present on disk is dropped — we only
//! materialise what was actually lost.

use crate::json::P;
use crate::scan::Session;
use chrono::{TimeZone, Utc};
use std::collections::HashMap;
use std::path::Path;

/// Idle time that ends a burst. Two prompts more than three hours apart are
/// assumed to belong to different sittings — the same rule of thumb a human
/// would use reading the history, and cheap to reason about when it is wrong
/// (a merged or split ghost, never a lost prompt).
const GAP_MS: u64 = 3 * 60 * 60 * 1000;

/// Slack around a surviving session's [created, modified] span when deciding
/// whether a burst is already covered by it.
const SLACK_MS: u64 = 10 * 60 * 1000;

/// Upper bound on reconstructed sessions, so a pathological history can never
/// flood the list.
const MAX_GHOSTS: usize = 5000;

/// Marker extension of a reconstructed session's (non-existent) path. Chosen so
/// the scanner's `.jsonl` walk can never pick it up.
pub const GHOST_EXT: &str = ".recovered";

struct Prompt {
    ts: u64,
    text: String,
}

/// Parse `~/.claude/history.jsonl`: one flat object per line,
/// `{display, pastedContents, timestamp, project}`. Malformed lines are
/// skipped. Returns prompts grouped by the cwd they were typed in.
fn read_history(base: &Path) -> HashMap<String, Vec<Prompt>> {
    let mut by_cwd: HashMap<String, Vec<Prompt>> = HashMap::new();
    let data = match std::fs::read(base.join("history.jsonl")) {
        Ok(d) => d,
        Err(_) => return by_cwd,
    };
    for line in data.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let mut p = P::new(line);
        let mut display = String::new();
        let mut ts = 0u64;
        let mut project = String::new();
        if !p.obj_begin() {
            continue;
        }
        loop {
            let k = match p.obj_key() {
                Some(k) => k,
                None => break,
            };
            match k.as_str() {
                "display" => display = p.take_string().unwrap_or_default(),
                "timestamp" => ts = p.take_number() as u64,
                "project" => project = p.take_string().unwrap_or_default(),
                _ => {
                    let _ = p.skip();
                }
            }
            if !p.obj_sep() {
                break;
            }
        }
        let text = display.trim().to_string();
        if ts == 0 || text.is_empty() || project.is_empty() {
            continue;
        }
        let cwd = project.trim_end_matches(['/', '\\']).to_string();
        by_cwd.entry(cwd).or_default().push(Prompt { ts, text });
    }
    by_cwd
}

/// The cwd and each of its ancestors, longest first — the candidate startup
/// directories a transcript for this prompt could have been filed under (the
/// user may have `cd`'d into a subdirectory mid-session).
fn ancestors(cwd: &str) -> Vec<String> {
    let mut out = vec![cwd.to_string()];
    let mut cur = cwd;
    while let Some(i) = cur.rfind(['/', '\\']) {
        if i == 0 {
            break;
        }
        cur = &cur[..i];
        out.push(cur.to_string());
    }
    out
}

/// Milliseconds since the epoch for an RFC3339 timestamp, 0 when unparsable.
fn rfc3339_ms(s: &str) -> u64 {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.timestamp_millis().max(0) as u64)
        .unwrap_or(0)
}

/// Format an epoch-millisecond stamp the way the transcripts do: RFC3339 in
/// **UTC**. Both agents record UTC, and the list renders these strings raw, so
/// a local-time ghost would sort and display hours away from the real sessions
/// around it.
fn iso(ms: u64) -> String {
    Utc.timestamp_millis_opt(ms as i64)
        .single()
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default()
}

/// Reconstruct the sessions Claude Code's retention deleted.
///
/// `live` is the set of sessions found on disk by the normal scan; bursts that
/// overlap one of them are skipped so a surviving conversation is never
/// shadowed by a ghost of itself. The returned sessions carry a `.recovered`
/// path (see `Session::is_ghost`) and are NEVER written to the cache.
pub fn recover(base: &Path, live: &[Session]) -> Vec<Session> {
    let history = read_history(base);
    if history.is_empty() {
        return Vec::new();
    }
    let projects = base.join("projects");

    // Index the surviving sessions by the folder they are filed under, with the
    // wall-clock span they cover.
    let mut spans: HashMap<String, Vec<(u64, u64)>> = HashMap::new();
    for s in live {
        let folder = Path::new(&s.path)
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|x| x.to_str())
            .unwrap_or("")
            .to_string();
        if folder.is_empty() {
            continue;
        }
        let end = s.mtime_ms.max(rfc3339_ms(&s.modified));
        let start = match rfc3339_ms(&s.created) {
            0 => end,
            c => c.min(end),
        };
        spans.entry(folder).or_default().push((start, end));
    }

    let mut out: Vec<Session> = Vec::new();
    let mut cwds: Vec<&String> = history.keys().collect();
    cwds.sort();
    for cwd in cwds {
        let mut prompts: Vec<&Prompt> = history[cwd].iter().collect();
        prompts.sort_by_key(|p| p.ts);

        // Where would this project's transcripts live? Prefer the ancestor whose
        // encoding is a real folder under projects/ (the startup cwd); fall back
        // to encoding the recorded cwd itself.
        let cands = ancestors(cwd);
        let folder = cands
            .iter()
            .map(|c| crate::encode_cwd(c))
            .find(|enc| projects.join(enc).is_dir())
            .unwrap_or_else(|| crate::encode_cwd(cwd));
        // Any surviving session under this project, or under an ancestor folder.
        let covered: Vec<(u64, u64)> = cands
            .iter()
            .map(|c| crate::encode_cwd(c))
            .chain(std::iter::once(folder.clone()))
            .filter_map(|enc| spans.get(&enc))
            .flatten()
            .copied()
            .collect();

        // Cut the prompt stream into bursts.
        let mut bursts: Vec<Vec<&Prompt>> = Vec::new();
        for p in prompts.drain(..) {
            match bursts.last_mut() {
                Some(b) if p.ts.saturating_sub(b.last().map(|x| x.ts).unwrap_or(0)) <= GAP_MS => {
                    // The TUI's history keeps duplicate submissions milliseconds
                    // apart (a slash command echoes twice); collapse them.
                    if b.last().map(|x| x.text.as_str()) != Some(p.text.as_str()) {
                        b.push(p);
                    }
                }
                _ => bursts.push(vec![p]),
            }
        }

        for (i, b) in bursts.iter().enumerate() {
            let (b0, b1) = (b[0].ts, b[b.len() - 1].ts);
            let overlaps = covered.iter().any(|(s0, s1)| {
                b0 <= s1.saturating_add(SLACK_MS) && b1 >= s0.saturating_sub(SLACK_MS)
            });
            if overlaps {
                continue;
            }
            out.push(ghost(&projects, &folder, cwd, i, b));
            if out.len() >= MAX_GHOSTS {
                return out;
            }
        }
    }

    // Hang each project's memory sidecar off its most recent ghost, so the notes
    // that outlived the transcripts are reachable from the list.
    attach_memory(&projects, &mut out);
    out
}

/// Build one reconstructed session from a burst of prompts.
fn ghost(projects: &Path, folder: &str, cwd: &str, idx: usize, b: &[&Prompt]) -> Session {
    let mut s = Session::default();
    let (b0, b1) = (b[0].ts, b[b.len() - 1].ts);
    // Not a UUID and not hex-first: `valid_session_id` rejects it, so every
    // id-driven action (resume, ssh forward, MCP fetch) refuses it on its own.
    let tail: String = folder.chars().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect();
    s.id = format!("rec-{}-{}", tail, idx);
    s.path = projects
        .join(folder)
        .join(format!("rec-{}{}", idx, GHOST_EXT))
        .to_string_lossy()
        .into_owned();
    s.project_path = cwd.to_string();
    s.project_name = cwd.rsplit(['\\', '/']).next().unwrap_or(cwd).to_string();
    s.first_prompt = b[0].text.clone();
    s.last_prompt = b[b.len() - 1].text.clone();
    s.title = {
        // Same rule as the scanner: a wrapper line makes a useless title. A
        // sitting also tends to OPEN with housekeeping (`/model`, `/effort`,
        // `/clear`), so a slash command is only used as the title when the
        // burst has nothing else in it.
        let usable = |t: &&str| {
            !t.is_empty()
                && !t.starts_with("<local-command")
                && !t.starts_with("<command-")
                && !t.starts_with("Caveat:")
        };
        let texts: Vec<&str> = b.iter().map(|p| p.text.trim()).collect();
        let pick = texts
            .iter()
            .copied()
            .find(|t| usable(t) && !t.starts_with('/'))
            .or_else(|| texts.iter().copied().find(usable))
            .unwrap_or_else(|| b[0].text.trim());
        let t: String = pick.chars().take(80).collect();
        if t.is_empty() {
            "(senza titolo)".to_string()
        } else {
            t
        }
    };
    s.message_count = b.len() as u64;
    s.created = iso(b0);
    s.modified = iso(b1);
    s.mtime_ms = b1;
    s.entrypoint = "recuperata".to_string();
    s.live = "ended".to_string();
    let mut search = String::new();
    search.push_str(&s.project_name.to_lowercase());
    for p in b {
        search.push(' ');
        search.push_str(&p.text.to_lowercase());
        if search.len() > 20_000 {
            break;
        }
    }
    s.search_text = search;
    s
}

/// List `projects/<folder>/memory/*.md` and attach it to the newest ghost of
/// that folder — the auto-memory survives the transcript cleanup and is often
/// the only surviving description of what the project was.
fn attach_memory(projects: &Path, out: &mut [Session]) {
    let mut newest: HashMap<String, usize> = HashMap::new();
    for (i, s) in out.iter().enumerate() {
        let folder = match Path::new(&s.path).parent().and_then(|p| p.file_name()) {
            Some(f) => f.to_string_lossy().into_owned(),
            None => continue,
        };
        match newest.get(&folder) {
            Some(&j) if out[j].mtime_ms >= s.mtime_ms => {}
            _ => {
                newest.insert(folder, i);
            }
        }
    }
    for (folder, i) in newest {
        let dir = projects.join(&folder).join("memory");
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        let mut files: Vec<String> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        files.sort();
        files.truncate(50);
        for f in &files {
            let base = f.rsplit(['\\', '/']).next().unwrap_or(f);
            out[i].search_text.push(' ');
            out[i].search_text.push_str(&base.to_lowercase());
        }
        out[i].files = files;
    }
}

/// Read back a recovered session's prompts for the reader pane: the transcript
/// is gone, but `history.jsonl` still holds every prompt of the burst. Returns
/// the same `Turn` shape the real reader uses (user turns only).
pub fn read_recovered(base: &Path, s: &Session) -> Vec<crate::scan::Turn> {
    if !s.is_ghost() {
        return Vec::new();
    }
    let (from, to) = (rfc3339_ms(&s.created), rfc3339_ms(&s.modified));
    if from == 0 {
        return Vec::new();
    }
    let history = read_history(base);
    let mut prompts: Vec<&Prompt> = match history.get(&s.project_path) {
        Some(v) => v
            .iter()
            .filter(|p| p.ts >= from && p.ts <= to.max(from))
            .collect(),
        None => return Vec::new(),
    };
    prompts.sort_by_key(|p| p.ts);
    let mut out = Vec::new();
    let mut last = String::new();
    for p in prompts {
        if p.text == last {
            continue;
        }
        last = p.text.clone();
        out.push(crate::scan::Turn {
            role: 0,
            text: p.text.clone(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    use super::*;

    fn hist(root: &Path, lines: &[(&str, u64, &str)]) {
        let mut s = String::new();
        for (d, ts, proj) in lines {
            s.push_str(&format!(
                "{{\"display\":\"{}\",\"pastedContents\":{{}},\"timestamp\":{},\"project\":\"{}\"}}\n",
                d,
                ts,
                proj.replace('\\', "\\\\")
            ));
        }
        std::fs::write(root.join("history.jsonl"), s).unwrap();
    }

    fn tmp() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "phosphor-rec-{}-{}",
            std::process::id(),
            // Il solo orologio non basta: su Windows ha una risoluzione di ~15 ms
            // e i test girano in parallelo, quindi due cartelle possono nascere
            // con lo stesso nome e cancellarsi a vicenda a meta' corsa.
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(p.join("projects")).unwrap();
        p
    }

    #[test]
    fn bursts_split_on_a_long_idle_gap() {
        let root = tmp();
        let h = 3_600_000u64;
        hist(
            &root,
            &[
                ("primo", 1_000_000, "/home/u/app"),
                ("secondo", 1_000_000 + 60_000, "/home/u/app"),
                ("dopo una pausa", 1_000_000 + 5 * h, "/home/u/app"),
            ],
        );
        let g = recover(&root, &[]);
        assert_eq!(g.len(), 2, "due sittings distinte");
        assert_eq!(g[0].message_count, 2);
        assert_eq!(g[0].title, "primo");
        assert_eq!(g[1].title, "dopo una pausa");
        assert!(g.iter().all(|s| s.is_ghost()));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_burst_covered_by_a_surviving_transcript_is_not_duplicated() {
        let root = tmp();
        hist(&root, &[("ciao", 1_000_000, "/home/u/app")]);
        let enc = crate::encode_cwd("/home/u/app");
        let mut live = Session::default();
        live.path = root
            .join("projects")
            .join(&enc)
            .join("a.jsonl")
            .to_string_lossy()
            .into_owned();
        live.created = iso(900_000);
        live.modified = iso(1_100_000);
        live.mtime_ms = 1_100_000;
        assert!(recover(&root, &[live]).is_empty());
        // …but with no surviving session the same burst comes back as a ghost.
        assert_eq!(recover(&root, &[]).len(), 1);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ghost_ids_are_refused_by_resume() {
        let root = tmp();
        hist(&root, &[("x", 1_000_000, "/home/u/app")]);
        let g = recover(&root, &[]);
        assert!(!crate::valid_session_id(&g[0].id));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn memory_sidecar_is_attached_to_the_newest_ghost() {
        let root = tmp();
        let h = 3_600_000u64;
        hist(
            &root,
            &[
                ("vecchia", 1_000_000, "/home/u/app"),
                ("nuova", 1_000_000 + 9 * h, "/home/u/app"),
            ],
        );
        let enc = crate::encode_cwd("/home/u/app");
        let mem = root.join("projects").join(&enc).join("memory");
        std::fs::create_dir_all(&mem).unwrap();
        std::fs::write(mem.join("MEMORY.md"), "# index").unwrap();
        let g = recover(&root, &[]);
        assert_eq!(g.len(), 2);
        assert!(g[0].files.is_empty(), "la piu' vecchia non porta la memoria");
        assert_eq!(g[1].files.len(), 1);
        assert!(g[1].files[0].ends_with("MEMORY.md"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_slash_command_is_the_title_only_as_a_last_resort() {
        let root = tmp();
        hist(
            &root,
            &[
                ("/model", 1_000_000, "/home/u/app"),
                ("/effort", 1_000_100, "/home/u/app"),
                ("sistemami il parser", 1_060_000, "/home/u/app"),
            ],
        );
        assert_eq!(recover(&root, &[])[0].title, "sistemami il parser");
        hist(&root, &[("/model", 1_000_000, "/home/u/app")]);
        assert_eq!(recover(&root, &[])[0].title, "/model");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn recovered_turns_come_back_from_the_history() {
        let root = tmp();
        hist(
            &root,
            &[
                ("uno", 1_000_000, "/home/u/app"),
                ("uno", 1_000_100, "/home/u/app"), // doppione del TUI
                ("due", 1_060_000, "/home/u/app"),
            ],
        );
        let g = recover(&root, &[]);
        let turns = read_recovered(&root, &g[0]);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].text, "uno");
        assert_eq!(turns[1].text, "due");
        std::fs::remove_dir_all(&root).ok();
    }
}
