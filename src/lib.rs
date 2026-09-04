//! Phosphor core library: scans local Claude Code session transcripts and
//! exposes them to the front-ends (TUI binary and the pixel-art adventure GUI).

pub mod bundle;
pub mod cache;
pub mod config;
pub mod fleet;
pub mod json;
pub mod live;
pub mod mcp;
pub mod plan;
pub mod recover;
pub mod scan;
pub mod server;
pub mod tui;
pub mod wrapped;

use scan::Session;
use server::State;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Semantic version from Cargo.toml (e.g. "0.3.0").
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Short git commit the binary was built from (e.g. "a1b2c3d4e"), or "nogit".
pub const GIT_HASH: &str = env!("PHOSPHOR_GIT");
/// "+" when the working tree had uncommitted changes at build time, else "".
pub const GIT_DIRTY: &str = env!("PHOSPHOR_GIT_DIRTY");
/// Commit date (YYYY-MM-DD) the binary was built from, or "" when unknown.
pub const COMMIT_DATE: &str = env!("PHOSPHOR_COMMIT_DATE");

/// A PRECISE one-line build identity, e.g. `v0.3.0 · ga1b2c3d4e · 2026-06-24`.
/// Lets the running exe report exactly which source revision it came from —
/// `+` after the hash means it was built from an uncommitted (dirty) tree.
pub fn version_line() -> String {
    let mut s = format!("v{} · g{}{}", VERSION, GIT_HASH, GIT_DIRTY);
    if !COMMIT_DATE.is_empty() {
        s.push_str(" · ");
        s.push_str(COMMIT_DATE);
    }
    s
}

/// The user's `~/.claude` directory.
pub fn default_base() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".claude")
}

/// Write `data` to `path` only if it does not already exist — never overwrites.
/// Returns an error (AlreadyExists) instead of clobbering an existing file.
pub fn write_new(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    f.write_all(data)
}

/// Quote one value for a CSV cell, safely.
/// Doubles embedded quotes, flattens newlines, and neutralises spreadsheet
/// *formula injection*: a value starting with `= + - @` (or a tab) is prefixed
/// with a single quote so Excel/Sheets treat it as text, never as a formula.
pub fn csv_field(x: &str) -> String {
    let mut v = x.replace('"', "\"\"").replace(['\n', '\r'], " ");
    if matches!(v.chars().next(), Some('=' | '+' | '-' | '@' | '\t')) {
        v.insert(0, '\'');
    }
    format!("\"{}\"", v)
}

/// One incremental scan pass: reuse `cache`, parse changes, annotate liveness,
/// persist the cache, and return the sessions (newest first).
pub fn scan_once(base: &Path, cache: &mut HashMap<String, Session>) -> Vec<Session> {
    let projects = base.join("projects");
    let (mut sessions, changed) = scan::scan_incremental(&projects, cache);
    live::annotate(base, &mut sessions);
    if changed {
        cache::save(base, &sessions);
    }
    add_recovered(base, &mut sessions);
    sessions
}

/// Readable turns for ANY session: the real transcript when a file backs it,
/// the prompts rebuilt from `history.jsonl` when it is a recovered ghost (whose
/// `path` points at nothing). Callers never need to branch on the kind.
pub fn turns_of(base: &Path, s: &scan::Session) -> Vec<scan::Turn> {
    if s.is_ghost() {
        recover::read_recovered(base, s)
    } else {
        scan::read_transcript(Path::new(&s.path))
    }
}

/// [`turns_of`] + grep, so global content search covers recovered sessions too.
pub fn grep_of(base: &Path, s: &scan::Session, needle_lower: &str, max: usize) -> Vec<scan::Hit> {
    if s.is_ghost() {
        scan::grep_turns(&recover::read_recovered(base, s), needle_lower, max)
    } else {
        scan::grep_transcript(Path::new(&s.path), needle_lower, max)
    }
}

/// Append the sessions Claude Code's retention deleted, rebuilt from
/// `history.jsonl`, and re-sort newest-first.
///
/// ALWAYS call this AFTER `cache::save`: a recovered session has no file behind
/// it, so it must never enter the on-disk cache — it would never be evicted
/// (the cache drops entries whose transcript vanished, and a ghost's never
/// existed) and would make every scan report itself as "changed". Rebuilding it
/// costs one pass over `history.jsonl` instead.
pub fn add_recovered(base: &Path, sessions: &mut Vec<scan::Session>) {
    let ghosts = recover::recover(base, sessions);
    if ghosts.is_empty() {
        return;
    }
    sessions.extend(ghosts);
    sessions.sort_by(|a, b| b.mtime_ms.cmp(&a.mtime_ms));
}

/// Resume a Claude Code session in a fresh terminal — safely.
/// Hardened against command injection: the id must be UUID-shaped, the cwd must
/// be a real existing directory free of quote/control chars, and the directory
/// is set via `current_dir` (OS-handled) rather than interpolated into a
/// `cd ... && ...` shell string.
pub fn resume_session(cwd: &str, id: &str) -> bool {
    resume_session_opt(cwd, id, false)
}

/// Like [`resume_session`] but starts a forked session (`--fork-session`), so
/// resuming in a DIFFERENT working directory (e.g. after a cross-PC path remap)
/// branches the conversation instead of writing back into the original.
pub fn resume_session_fork(cwd: &str, id: &str) -> bool {
    resume_session_opt(cwd, id, true)
}

/// True when `id` is safe to place in an argv passed to `claude --resume` (or
/// forwarded over ssh): [0-9a-fA-F-] only AND starting with a hex digit — a
/// leading '-' would make it a flag-shaped token (e.g. "-c"). Real session ids
/// are UUIDs, always hex-first. Reused by the TUI, the web server and fleet.
pub fn valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id.chars().all(|ch| ch.is_ascii_hexdigit() || ch == '-')
        && id.starts_with(|ch: char| ch.is_ascii_hexdigit())
}

/// Strip control characters (ESC, CR, BEL, C0/C1, DEL) and Unicode bidi
/// override marks from untrusted strings before rendering them in a terminal:
/// a crafted session title (e.g. from an imported `.phx` or a remote
/// `phosphor json`) must not inject ANSI escapes or reorder the display. Each
/// dropped char becomes a space so adjacent tokens don't get glued together.
pub fn tame(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_control() || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}') {
                ' '
            } else {
                c
            }
        })
        .collect()
}

fn resume_session_opt(cwd: &str, id: &str, fork: bool) -> bool {
    if !valid_session_id(id) {
        return false;
    }
    if cwd.is_empty() || cwd.len() > 400 {
        return false;
    }
    if cwd.chars().any(|ch| matches!(ch, '"' | '\n' | '\r' | '\0' | '%')) {
        return false;
    }
    if !Path::new(cwd).is_dir() {
        return false;
    }
    spawn_resume(cwd, id, fork)
}

/// Resolve the working directory to resume a session in. Returns `(cwd, remapped)`
/// where `remapped == true` means the recorded path did not exist locally and was
/// rewritten via a user-configured `pathRemaps` rule (so the caller should fork
/// the session). Returns `None` if the path is missing and no remap resolves it.
/// Remaps match on a normalized (separator/case-insensitive) longest prefix.
pub fn resolve_cwd(recorded: &str, remaps: &[(String, String)]) -> Option<(String, bool)> {
    if !recorded.is_empty() && Path::new(recorded).is_dir() {
        return Some((recorded.to_string(), false));
    }
    let key = |s: &str| {
        s.trim_end_matches(['/', '\\'])
            .replace('\\', "/")
            .to_lowercase()
    };
    let rk = key(recorded);
    let mut best: Option<(&str, &str, usize)> = None;
    for (from, to) in remaps {
        let fk = key(from);
        if fk.is_empty() {
            continue;
        }
        let matches = rk == fk || rk.starts_with(&format!("{fk}/"));
        if matches && best.map_or(true, |b| fk.len() > b.2) {
            best = Some((from.as_str(), to.as_str(), fk.len()));
        }
    }
    let (from, to, _) = best?;
    let from_len = from.trim_end_matches(['/', '\\']).len().min(recorded.len());
    let remainder = recorded[from_len..].trim_start_matches(['/', '\\']);
    let mut cand = std::path::PathBuf::from(to);
    for part in remainder.split(['/', '\\']) {
        if !part.is_empty() {
            cand.push(part);
        }
    }
    if cand.is_dir() {
        Some((cand.to_string_lossy().to_string(), true))
    } else {
        None
    }
}

/// Encode a working directory to the folder name Claude Code stores its
/// transcripts under: every non-alphanumeric ASCII char becomes `-` (verified
/// empirically against real `projects/` folders). Lossy (not invertible) — used
/// to PLACE a remapped bundle where `claude --resume` will look once you're in
/// the target directory. Trailing path separators are ignored.
pub fn encode_cwd(cwd: &str) -> String {
    cwd.trim_end_matches(['/', '\\'])
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Pick the directory `claude --resume <id>` must run in to actually find the
/// session. Claude stores a transcript under `projects/<encoded-startup-cwd>/`
/// and `--resume` only looks inside the folder matching the *current* `$PWD`.
/// The cwd recorded in the transcript can drift to a SUBDIRECTORY the user
/// `cd`'d into mid-session (e.g. started in `…\myapp`, then worked in
/// `…\myapp\frontend`); launching there encodes a different folder
/// and claude reports "No conversation found with session ID".
///
/// The transcript's own location is the source of truth: its parent folder name
/// IS claude's encoding of the startup cwd (every non-alphanumeric char mapped
/// to `-`). So we walk up the recorded cwd's ancestors and return the first one
/// whose encoding equals that folder. Falls back to `recorded` unchanged when
/// nothing matches (path-agnostic: handles `/` and `\` so it stays testable off
/// Windows). Cache-proof — uses only fields every session already carries.
pub fn resume_cwd_for(jsonl_path: &str, recorded: &str) -> String {
    let folder = {
        let no_file = match jsonl_path.trim_end_matches(['/', '\\']).rfind(['/', '\\']) {
            Some(i) => &jsonl_path[..i],
            None => "",
        };
        match no_file.rfind(['/', '\\']) {
            Some(i) => &no_file[i + 1..],
            None => no_file,
        }
    };
    if folder.is_empty() {
        return recorded.to_string();
    }
    let enc = |s: &str| -> String { encode_cwd(s) };
    let mut cur = recorded.trim_end_matches(['/', '\\']);
    loop {
        if enc(cur) == folder {
            return cur.to_string();
        }
        match cur.rfind(['/', '\\']) {
            Some(i) if i > 0 => cur = &cur[..i],
            _ => break,
        }
    }
    recorded.to_string()
}

#[cfg(windows)]
fn spawn_resume(cwd: &str, id: &str, fork: bool) -> bool {
    use std::os::windows::process::CommandExt;
    // Launch through cmd's `start` builtin so the resumed session gets its OWN
    // fresh, INTERACTIVE console. Spawning `cmd /K claude …` directly with
    // CREATE_NEW_CONSOLE looks right but isn't: Rust's std unconditionally sets
    // STARTF_USESTDHANDLES and binds the child's stdin/stdout to *Phosphor's*
    // console handles, so the new window opens (the session even loads) but the
    // keyboard is read from Phosphor's console — the new window looks dead.
    // `start` re-creates the process with a real new console and no inherited
    // std handles, so keystrokes reach the resumed session. (Verified: the
    // direct child sees a non-console stdin; the start-launched child sees a
    // real console input handle.)
    //
    // CREATE_NO_WINDOW on the launcher cmd gives *it* its own hidden console so
    // it never touches Phosphor's console input mode (which would re-enable
    // QuickEdit / drop ENABLE_MOUSE_INPUT and kill Phosphor's mouse capture).
    // The launcher exits immediately after `start` returns, so it never flashes.
    // `cmd /K` keeps the session window open; cwd is set via current_dir
    // (OS-handled, never string-interpolated) and propagates through `start`;
    // id is validated to [0-9a-fA-F-] AND must start with a hex digit, so it is
    // a single inert argv token: it can't be flag-shaped nor carry any cmd.exe
    // metacharacter (& | < > ^ % " …), so no argument/command can be injected.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut cmd = std::process::Command::new("cmd");
    cmd.args(["/C", "start", "", "cmd", "/K", "claude", "--resume", id]);
    if fork {
        cmd.arg("--fork-session");
    }
    cmd.current_dir(cwd)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .is_ok()
}
#[cfg(not(windows))]
fn spawn_resume(cwd: &str, id: &str, fork: bool) -> bool {
    let mut args = vec!["--resume", id];
    if fork {
        args.push("--fork-session");
    }
    std::process::Command::new("claude")
        .args(&args)
        .current_dir(cwd)
        .spawn()
        .is_ok()
}

/// Resume a session that lives on ANOTHER machine (fleet): opens an interactive
/// `ssh -t <alias> phosphor resume-here <id>` so `claude --resume` runs on the
/// session's home PC, in the right cwd, drawing on the ssh terminal. Both
/// arguments are strictly validated (they are joined into a remote shell
/// command line by ssh itself): the alias by [`fleet::valid_alias`], the id by
/// [`valid_session_id`]. No BatchMode here — the user may need to answer a key
/// passphrase or host prompt interactively.
pub fn resume_remote_session(alias: &str, id: &str) -> bool {
    if !fleet::valid_alias(alias) || !valid_session_id(id) {
        return false;
    }
    spawn_remote_resume(alias, id)
}
#[cfg(windows)]
fn spawn_remote_resume(alias: &str, id: &str) -> bool {
    use std::os::windows::process::CommandExt;
    // Same launcher pattern as `spawn_resume` (see the long comment there):
    // `start` gives ssh a real fresh console; the outer `cmd /K` keeps the
    // window open when ssh exits or fails, so errors and host-key prompts stay
    // visible instead of flashing away. Alias and id charsets contain no cmd
    // metacharacters, so the extra cmd hop is inert.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("cmd")
        .args(["/C", "start", "", "cmd", "/K", "ssh", "-t", "--", alias, "phosphor", "resume-here", id])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .is_ok()
}
#[cfg(not(windows))]
fn spawn_remote_resume(alias: &str, id: &str) -> bool {
    // Mirrors the local unix resume: spawn inheriting the terminal. (Shares
    // its known limitation: the TUI and the child briefly contend for the tty.)
    std::process::Command::new("ssh")
        .args(["-t", "--", alias, "phosphor", "resume-here", id])
        .spawn()
        .is_ok()
}

/// Permanently (HARD) delete a project directory and ALL its transcripts.
/// IRREVERSIBLE. Strongly confined so a crafted/wrong path can never make
/// Phosphor delete anything outside the sessions store: `dir` must resolve to a
/// DIRECT child of `<base>/projects` — never `projects/` itself, never outside
/// it, and never through a symlink (the entry must be a real directory). All
/// three checks must pass before a single byte is removed.
pub fn delete_project_dir(base: &Path, dir: &Path) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};
    let projects = base.join("projects").canonicalize()?;
    // The entry itself must be a real directory, not a symlink/junction to one
    // (symlink_metadata does NOT follow the link).
    let meta = std::fs::symlink_metadata(dir)?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(Error::new(ErrorKind::InvalidInput, "non e' una directory reale"));
    }
    let canon = dir.canonicalize()?;
    if canon == projects {
        return Err(Error::new(ErrorKind::InvalidInput, "rifiuto: e' l'intera cartella projects"));
    }
    if canon.parent() != Some(projects.as_path()) {
        return Err(Error::new(ErrorKind::InvalidInput, "il percorso e' fuori da projects"));
    }
    std::fs::remove_dir_all(&canon)
}

/// Open a folder in the OS file manager (read-only convenience). Best-effort:
/// failures are ignored (the caller reports status).
pub fn open_folder(dir: &str) {
    #[cfg(windows)]
    let mut cmd = std::process::Command::new("explorer");
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = std::process::Command::new("xdg-open");
    let _ = cmd.arg(dir).spawn();
}

/// Copy text to the system clipboard by piping it to the platform tool
/// (`clip` / `pbcopy` / `xclip`). Zero dependencies; returns whether it ran.
pub fn copy_to_clipboard(text: &str) -> bool {
    use std::io::Write;
    use std::process::Stdio;
    #[cfg(windows)]
    let mut cmd = std::process::Command::new("clip");
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("pbcopy");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xclip");
        c.args(["-selection", "clipboard"]);
        c
    };
    let mut child = match cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        Ok(c) => c,
        Err(_) => return false,
    };
    if let Some(mut si) = child.stdin.take() {
        let _ = si.write_all(text.as_bytes());
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

/// Reversibly ARCHIVE a project: move `projects/<enc>` to `archived/<enc>` so it
/// leaves the sessions list (and Claude's `--resume`) WITHOUT being destroyed —
/// it can be restored later. Confined exactly like `delete_project_dir`, and
/// refuses if an archive of the same folder already exists (never overwrites).
pub fn archive_project_dir(base: &Path, dir: &Path) -> std::io::Result<PathBuf> {
    use std::io::{Error, ErrorKind};
    let projects = base.join("projects").canonicalize()?;
    let meta = std::fs::symlink_metadata(dir)?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(Error::new(ErrorKind::InvalidInput, "non e' una directory reale"));
    }
    let canon = dir.canonicalize()?;
    if canon == projects || canon.parent() != Some(projects.as_path()) {
        return Err(Error::new(ErrorKind::InvalidInput, "il percorso e' fuori da projects"));
    }
    let name = canon
        .file_name()
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "nome cartella mancante"))?;
    let archived = base.join("archived");
    std::fs::create_dir_all(&archived)?;
    let dest = archived.join(name);
    if dest.exists() {
        return Err(Error::new(ErrorKind::AlreadyExists, "esiste gia' un archivio con questo nome"));
    }
    std::fs::rename(&canon, &dest)?;
    Ok(dest)
}

/// Reverse of [`archive_project_dir`]: move `archived/<name>` back to
/// `projects/<name>`. `name` must be a bare folder name (no separators / `..`).
pub fn unarchive_project_dir(base: &Path, name: &str) -> std::io::Result<PathBuf> {
    use std::io::{Error, ErrorKind};
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(Error::new(ErrorKind::InvalidInput, "nome archivio non valido"));
    }
    let archived = base.join("archived").canonicalize()?;
    let src = archived.join(name);
    let meta = std::fs::symlink_metadata(&src)?; // errors if missing
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(Error::new(ErrorKind::InvalidInput, "archivio non valido"));
    }
    let canon_src = src.canonicalize()?;
    if canon_src.parent() != Some(archived.as_path()) {
        return Err(Error::new(ErrorKind::InvalidInput, "fuori da archived"));
    }
    let projects = base.join("projects");
    std::fs::create_dir_all(&projects)?;
    let dest = projects.join(name);
    if dest.exists() {
        return Err(Error::new(ErrorKind::AlreadyExists, "il progetto esiste gia' in projects"));
    }
    std::fs::rename(&canon_src, &dest)?;
    Ok(dest)
}

/// List archived project folders as (folder name, total bytes on disk).
pub fn list_archived(base: &Path) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(base.join("archived")) {
        for e in rd.flatten() {
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                out.push((e.file_name().to_string_lossy().into_owned(), dir_size_bytes(&e.path())));
            }
        }
    }
    out.sort();
    out
}

/// Delete ONE session transcript (and its sidecar `<id>/` folder of subagents/
/// workflows, if present), confined to `<base>/projects`. Real `.jsonl` file
/// only — never a symlink, never outside projects/. Used by bulk multi-select
/// delete. Irreversible.
pub fn delete_session_file(base: &Path, file: &Path) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};
    let projects = base.join("projects").canonicalize()?;
    let meta = std::fs::symlink_metadata(file)?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(Error::new(ErrorKind::InvalidInput, "non e' un file reale"));
    }
    if file.extension().and_then(|e| e.to_str()) != Some("jsonl") {
        return Err(Error::new(ErrorKind::InvalidInput, "non e' un .jsonl"));
    }
    let canon = file.canonicalize()?;
    if !canon.starts_with(&projects) {
        return Err(Error::new(ErrorKind::InvalidInput, "fuori da projects"));
    }
    // Remove the sidecar directory (same path minus the .jsonl extension) first.
    let sidecar = canon.with_extension("");
    if sidecar.is_dir() && sidecar.starts_with(&projects) {
        let _ = std::fs::remove_dir_all(&sidecar);
    }
    std::fs::remove_file(&canon)
}

fn dir_size_bytes(dir: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => total += dir_size_bytes(&e.path()),
                Ok(_) => total += e.metadata().map(|m| m.len()).unwrap_or(0),
                _ => {}
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::{
        archive_project_dir, csv_field, delete_project_dir, delete_session_file, encode_cwd,
        list_archived, resolve_cwd, resume_cwd_for, resume_session, unarchive_project_dir,
    };

    #[test]
    fn encode_cwd_matches_claude_folders() {
        // Every non-alphanumeric char -> '-', trailing separators ignored. These
        // outputs are exactly the folder names Claude stores transcripts under.
        assert_eq!(encode_cwd("C:\\Users\\dev\\Desktop\\demo"), "C--Users-dev-Desktop-demo");
        assert_eq!(encode_cwd("/home/u/my-app"), "-home-u-my-app");
        assert_eq!(encode_cwd("D:\\work\\proj\\"), "D--work-proj"); // trailing sep dropped
        assert_eq!(encode_cwd("C:/a.b_c"), "C--a-b-c"); // '.' and '_' both -> '-'
    }

    #[test]
    fn csv_field_neutralises_formulas_and_quotes() {
        // formula-leading values get an apostrophe so spreadsheets treat them as text
        assert_eq!(csv_field("=SUM(A1)"), "\"'=SUM(A1)\"");
        assert_eq!(csv_field("+1"), "\"'+1\"");
        assert_eq!(csv_field("-cmd"), "\"'-cmd\"");
        assert_eq!(csv_field("@x"), "\"'@x\"");
        assert_eq!(csv_field("\tx"), "\"'\tx\"");
        // ordinary values are left untouched (no spurious apostrophe)
        assert_eq!(csv_field("2026-06-21"), "\"2026-06-21\""); // starts with a digit
        assert_eq!(csv_field("claude-opus"), "\"claude-opus\"");
        // quotes doubled, newlines flattened to spaces
        assert_eq!(csv_field("a\"b"), "\"a\"\"b\"");
        assert_eq!(csv_field("a\nb"), "\"a b\"");
    }

    #[test]
    fn resume_rejects_unsafe_input() {
        // all of these must be rejected BEFORE any process is spawned
        assert!(!resume_session("C:/Windows", "../evil;rm"));        // id not uuid-shaped
        assert!(!resume_session("C:/Windows", "-c"));                // flag-shaped id (leading '-')
        assert!(!resume_session("C:/Windows", "-fork-session"));     // flag-shaped id
        assert!(!resume_session("C:/Windows", &"a".repeat(50)));      // id too long
        assert!(!resume_session("", "00000000-0000-0000-0000-000000000000")); // empty cwd
        assert!(!resume_session("C:/has\"quote", "0000-0000"));      // quote in cwd
        assert!(!resume_session("C:/has%percent", "0000-0000"));     // percent in cwd
        assert!(!resume_session("C:/__nope_not_a_dir_xyz__", "0000-0000")); // missing dir
    }

    #[test]
    fn delete_project_dir_is_confined() {
        let root = std::env::temp_dir().join(format!("phx-del-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let projects = root.join("projects");
        let proj = projects.join("C--Users-x-demo");
        std::fs::create_dir_all(proj.join("sub")).unwrap(); // a sidecar subfolder too
        std::fs::write(proj.join("a.jsonl"), b"x").unwrap();

        // refuses the projects root itself, a path outside projects, a missing dir
        assert!(delete_project_dir(&root, &projects).is_err());
        assert!(delete_project_dir(&root, &root).is_err());
        assert!(delete_project_dir(&root, &projects.join("nope")).is_err());

        // deletes a real direct-child project dir (with its sidecar subfolder)…
        assert!(proj.exists());
        assert!(delete_project_dir(&root, &proj).is_ok());
        assert!(!proj.exists());
        assert!(projects.exists()); // …leaving the projects/ parent intact

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn archive_unarchive_round_trips() {
        let root = std::env::temp_dir().join(format!("phx-arch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let projects = root.join("projects");
        let proj = projects.join("C--Users-x-demo");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(proj.join("a.jsonl"), b"x").unwrap();

        // archive: leaves projects/, shows up under archived/ (reversible, not destroyed)
        let dest = archive_project_dir(&root, &proj).unwrap();
        assert!(!proj.exists());
        assert!(dest.exists());
        assert_eq!(list_archived(&root).len(), 1);
        // still confined: refuses the projects root itself
        assert!(archive_project_dir(&root, &projects).is_err());

        // unarchive: back to projects/
        let back = unarchive_project_dir(&root, "C--Users-x-demo").unwrap();
        assert!(back.exists());
        assert!(proj.exists());
        assert!(list_archived(&root).is_empty());
        // a traversal name is rejected
        assert!(unarchive_project_dir(&root, "../evil").is_err());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_session_file_is_confined() {
        let root = std::env::temp_dir().join(format!("phx-sess-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let projects = root.join("projects");
        let proj = projects.join("C--Users-x-demo");
        std::fs::create_dir_all(proj.join("abc")).unwrap(); // sidecar dir
        let jsonl = proj.join("abc.jsonl");
        std::fs::write(&jsonl, b"x").unwrap();
        let outside = root.join("outside.jsonl");
        std::fs::write(&outside, b"x").unwrap();

        // refuses a file outside projects/ and a non-.jsonl
        assert!(delete_session_file(&root, &outside).is_err());
        assert!(delete_session_file(&root, &proj.join("nope.txt")).is_err());
        // deletes the .jsonl AND its sidecar folder
        assert!(delete_session_file(&root, &jsonl).is_ok());
        assert!(!jsonl.exists());
        assert!(!proj.join("abc").exists());
        assert!(outside.exists()); // untouched

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resume_cwd_follows_storage_folder() {
        // The transcript lives under the startup-cwd folder (…\myapp) but the
        // recorded cwd drifted to a subdir the user cd'd into mid-session.
        let jsonl = "C:\\Users\\dev\\.claude\\projects\\C--Users-dev-Desktop-myapp\\16b42417.jsonl";
        // recorded = deep subdir the user cd'd into -> walk back to the folder.
        assert_eq!(
            resume_cwd_for(jsonl, "C:\\Users\\dev\\Desktop\\myapp\\frontend\\src"),
            "C:\\Users\\dev\\Desktop\\myapp"
        );
        // already correct (no cd) -> returned unchanged.
        assert_eq!(
            resume_cwd_for(jsonl, "C:\\Users\\dev\\Desktop\\myapp"),
            "C:\\Users\\dev\\Desktop\\myapp"
        );
        // a literal '-' in a project name must survive (encoding maps '-'->'-').
        let jf = "/home/u/.claude/projects/-home-u-my-app/abc.jsonl";
        assert_eq!(resume_cwd_for(jf, "/home/u/my-app/src"), "/home/u/my-app");
        // no ancestor matches -> fall back to the recorded cwd unchanged.
        assert_eq!(resume_cwd_for(jsonl, "C:\\somewhere\\else"), "C:\\somewhere\\else");
    }

    #[test]
    fn resolve_cwd_uses_remaps() {
        // a real existing dir resolves as-is, never remapped
        let tmp = std::env::temp_dir();
        let tmp_s = tmp.to_string_lossy().to_string();
        assert_eq!(resolve_cwd(&tmp_s, &[]), Some((tmp_s.clone(), false)));

        // a missing path with no remap is unresolved
        assert_eq!(resolve_cwd("C:/__nope_xyz__/proj", &[]), None);

        // a prefix remap pointing at a real dir resolves (remapped = true).
        // map "C:/old" -> <tmp>, so "C:/old" itself resolves to <tmp>.
        let remaps = vec![("C:/old".to_string(), tmp_s.clone())];
        let got = resolve_cwd("C:\\old", &remaps);
        assert_eq!(got, Some((tmp_s.clone(), true)));

        // a remap whose target does not exist stays unresolved
        let bad = vec![("C:/old".to_string(), "C:/__also_nope__".to_string())];
        assert_eq!(resolve_cwd("C:/old/sub", &bad), None);
    }
}

/// Re-scan against a server State's live cache and swap its session list.
pub fn rescan(state: &Arc<State>) -> bool {
    let projects = state.base.join("projects");
    let (mut sessions, changed) = {
        let mut c = state.cache.lock().unwrap();
        scan::scan_incremental(&projects, &mut c)
    };
    live::annotate(&state.base, &mut sessions);
    if changed {
        cache::save(&state.base, &sessions);
    }
    *state.sessions.write().unwrap() = sessions;
    changed
}
