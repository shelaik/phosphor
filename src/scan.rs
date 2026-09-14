//! Walk ~/.claude/projects, stream every session transcript, and summarise it.

use crate::json::P;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

// Last-event classification, used to guess "where the session is at".
const KIND_NONE: u8 = 0;
pub(crate) const KIND_HUMAN: u8 = 1; // user typed a prompt -> waiting for assistant
pub(crate) const KIND_ASSISTANT_TEXT: u8 = 2; // assistant final text -> idle / done
const KIND_ASSISTANT_TOOL: u8 = 3; // assistant requested a tool, no result yet
const KIND_TOOLRESULT: u8 = 4; // tool result delivered -> assistant thinking

const PROMPT_CAP: usize = 6000;

#[derive(Default, Clone)]
pub struct Session {
    pub id: String,
    pub path: String,
    pub project_path: String,
    pub project_name: String,
    pub title: String,
    pub summary: String,
    pub first_prompt: String,
    pub last_prompt: String,
    pub message_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
    pub models: Vec<String>,
    pub tools: Vec<(String, u64)>,
    pub files: Vec<String>,
    pub git_branch: String,
    pub version: String,
    pub entrypoint: String,
    pub created: String,
    pub modified: String,
    pub mtime_ms: u64,
    pub size: u64,
    pub subagents: u64,
    pub workflows: u64,
    pub is_sidechain: bool,
    /// This transcript begins with the compact "continued from a previous
    /// conversation" preamble, i.e. it continues an earlier session (resume /
    /// compact). Used to group chains and de-confuse same-titled siblings.
    pub is_continuation: bool,
    /// Best-effort link to the session this one continues (filled after scan).
    pub parent_id: String,
    pub parent_title: String,
    /// Bottom-K sketch of this transcript's message-uuid hashes (the K smallest
    /// 64-bit hashes). Two sessions that share >=2 of these provably replayed the
    /// same messages -> a CERTAIN resume/compact link (vs the title heuristic).
    /// Cached. See `link_kin`.
    pub kin_sketch: Vec<u64>,
    /// Derived (not cached): non-zero id shared by all members of a certain
    /// uuid-linked family; 0 if this session has no proven kin. Set by `link_kin`.
    pub kin_group: u32,
    pub last_kind: u8,
    pub search_text: String, // lowercased index of prompts/files/tools for full-text search
    // filled by the liveness pass
    pub live: String, // "running" | "idle" | "ended"
    pub status: String,
    pub pid: u64,
    /// SSH alias of the machine this session lives on (fleet view); empty =
    /// local. Never cached, never exported: set only when ingesting a remote
    /// `phosphor json` (see `fleet`), so a rescan can tell local from remote.
    pub host: String,
    /// Which coding agent wrote this session: empty (or "claude") = Claude
    /// Code, "codex" = OpenAI Codex CLI (see `crate::codex`). A string rather
    /// than an enum so a third agent costs no schema change.
    pub agent: String,
}

impl Session {
    /// True for a session RECONSTRUCTED from `history.jsonl` by
    /// [`crate::recover`]: the transcript it describes was deleted by Claude
    /// Code's retention, so there is no file at `path` to read, resume or
    /// export. Callers that touch the file must check this first.
    pub fn is_ghost(&self) -> bool {
        self.path.ends_with(crate::recover::GHOST_EXT)
    }

    /// True for a session read out of the vault (`crate::vault`) because its
    /// transcript no longer exists in its agent's store. The file is real and
    /// complete — unlike a ghost — but the agent cannot see it, so resuming
    /// means restoring it first.
    pub fn is_vaulted(&self) -> bool {
        Path::new(&self.path)
            .components()
            .any(|c| c.as_os_str() == crate::vault::DIR)
    }

    /// True for a session recorded by the Codex CLI rather than Claude Code.
    /// The two store transcripts in different trees, in different formats, and
    /// resume with different commands — so anything that touches the file or
    /// launches the agent must branch on this.
    pub fn is_codex(&self) -> bool {
        self.agent == "codex"
    }

    pub fn last_state(&self) -> &'static str {
        match self.last_kind {
            KIND_HUMAN => "in attesa di risposta",
            KIND_ASSISTANT_TEXT => "completata / idle",
            KIND_ASSISTANT_TOOL => "tool in esecuzione (forse interrotta)",
            KIND_TOOLRESULT => "in elaborazione",
            _ => "sconosciuto",
        }
    }
}

/// How many of the smallest message-uuid hashes to keep per session (a bottom-k
/// / KMV sketch, position-independent). The shared-count between two sketches
/// tracks Jaccard (~K·|A∩B|/max(|A|,|B|)), so it shrinks with size asymmetry; K
/// is sized generously (a short session resumed into a long one still clears the
/// >=2 threshold). Validated on real transcripts: the true families share 16 and
/// 33 bottom-256 hashes — comfortably above 2 — with zero false positives. Cache
/// cost stays small and fixed (<=256·16 hex chars per session).
const KIN_SKETCH_K: usize = 256;

/// FNV-1a 64-bit: a fast, dependency-free, stable string hash. Stability across
/// runs matters because the sketch is cached and compared between sessions.
fn fnv1a_64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

const SEARCH_CAP: usize = 16384;
pub(crate) fn push_search(buf: &mut String, txt: &str) {
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

pub(crate) fn trunc(mut s: String) -> String {
    if s.len() > PROMPT_CAP {
        let mut end = PROMPT_CAP;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s.push('…');
    }
    s
}

fn mtime_ms(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A `.jsonl` file found under projects/, with its position in the tree.
struct Entry {
    path: PathBuf,
    parts: Vec<String>, // path components relative to projects/
    size: u64,
    mtime: u64,
}

fn walk(dir: &Path, base: &Path, out: &mut Vec<Entry>) {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for e in rd.flatten() {
        let path = e.path();
        let ft = match e.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if ft.is_dir() {
            walk(&path, base, out);
        } else if path.extension().and_then(|x| x.to_str()) == Some("jsonl") {
            let parts: Vec<String> = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .components()
                .filter_map(|c| c.as_os_str().to_str().map(|s| s.to_string()))
                .collect();
            let md = e.metadata().ok();
            let size = md.as_ref().map(|m| m.len()).unwrap_or(0);
            let mtime = md
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            out.push(Entry {
                path,
                parts,
                size,
                mtime,
            });
        }
    }
}

/// Incremental scan: reuse `cache` entries (keyed by file path) whose size and
/// mtime are unchanged, re-parse only what changed. Updates `cache` in place.
/// Returns (sessions, changed) where `changed` is true if anything was
/// (re)parsed or removed since last time.
pub fn scan_incremental(
    projects_dir: &Path,
    cache: &mut HashMap<String, Session>,
) -> (Vec<Session>, bool) {
    let mut entries = Vec::new();
    walk(projects_dir, projects_dir, &mut entries);

    // Split into top-level sessions vs. nested subagent/workflow transcripts.
    let mut mains: Vec<(PathBuf, u64, u64)> = Vec::new();
    let mut child_counts: HashMap<String, (u64, u64)> = HashMap::new();
    for e in &entries {
        if e.parts.len() == 2 {
            mains.push((e.path.clone(), e.size, e.mtime));
        } else if e.parts.len() > 2 {
            let owner = e.parts[1].clone(); // <sessionUuid> directory
            let is_wf = e.parts.iter().any(|p| p.contains("workflows"));
            let is_sub = e.parts.iter().any(|p| p == "subagents");
            let c = child_counts.entry(owner).or_insert((0, 0));
            if is_wf {
                c.1 += 1;
            } else if is_sub {
                c.0 += 1;
            }
        }
    }

    // Decide which files can be reused from cache and which must be parsed.
    let mut reused: Vec<Session> = Vec::new();
    let mut to_parse: Vec<(PathBuf, u64)> = Vec::new();
    let mut valid: HashSet<String> = HashSet::new();
    for (path, size, mtime) in &mains {
        let key = path.to_string_lossy().to_string();
        valid.insert(key.clone());
        match cache.get(&key) {
            Some(c) if c.size == *size && c.mtime_ms == *mtime => reused.push(c.clone()),
            _ => to_parse.push((path.clone(), *size)),
        }
    }
    // The cache map is SHARED with the Codex scanner (`crate::codex`), which
    // owns its own keys: count and evict only the rows this scanner produced,
    // or every pass would look changed and thrash the file.
    let removed = cache.values().filter(|v| !v.is_codex() && !v.is_vaulted()).count() != reused.len();
    let changed = !to_parse.is_empty() || removed;

    // Parse the changed/new transcripts in parallel.
    let to_parse = Arc::new(to_parse);
    let next = Arc::new(AtomicUsize::new(0));
    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 16);
    let (tx, rx) = std::sync::mpsc::channel::<Session>();
    let mut handles = Vec::new();
    for _ in 0..n_threads {
        let to_parse = to_parse.clone();
        let next = next.clone();
        let tx = tx.clone();
        handles.push(std::thread::spawn(move || loop {
            let i = next.fetch_add(1, Ordering::Relaxed);
            if i >= to_parse.len() {
                break;
            }
            let (path, size) = &to_parse[i];
            let r = std::panic::catch_unwind(|| parse_session(path, *size));
            if let Ok(Some(s)) = r {
                let _ = tx.send(s);
            }
        }));
    }
    drop(tx);
    let parsed: Vec<Session> = rx.iter().collect();
    for h in handles {
        let _ = h.join();
    }

    // Refresh the cache: insert freshly parsed, drop entries that disappeared.
    for s in &parsed {
        cache.insert(s.path.clone(), s.clone());
    }
    cache.retain(|k, v| valid.contains(k) || v.is_codex() || v.is_vaulted());

    let mut sessions = reused;
    sessions.extend(parsed);
    for s in &mut sessions {
        let (sub, wf) = child_counts.get(&s.id).copied().unwrap_or((0, 0));
        s.subagents = sub;
        s.workflows = wf;
    }
    sessions.sort_by(|a, b| b.mtime_ms.cmp(&a.mtime_ms));
    (sessions, changed)
}

/// Parse a single transcript file into a Session (used for sub-agent drill-down).
pub fn parse_one(path: &Path, size: u64) -> Option<Session> {
    parse_session(path, size)
}

// ---------------------------------------------------------------------------
// Transcript reader: extract the human-readable conversation for in-app reading.
// ---------------------------------------------------------------------------

pub(crate) const READER_MSG_CAP: usize = 6000;
pub(crate) const READER_TEXT_CAP: usize = 12000;

/// One readable turn of a conversation. `role`: 0 = user, 1 = assistant,
/// 2 = note (tool/other). Thinking blocks and raw tool I/O are omitted.
pub struct Turn {
    pub role: u8,
    pub text: String,
}

/// A content match inside one transcript: which turn, its role, and a snippet.
pub struct Hit {
    pub turn: usize,
    pub role: u8,
    pub snippet: String,
}

/// Grep a transcript's readable turns for `needle_lower` (already lowercased).
/// Returns up to `max` hits with a short snippet around each match.
pub fn grep_transcript(path: &Path, needle_lower: &str, max: usize) -> Vec<Hit> {
    grep_turns(&read_transcript(path), needle_lower, max)
}

/// Same search over turns already in memory — used for recovered sessions,
/// whose "transcript" is rebuilt from `history.jsonl` rather than read from a
/// file (see [`crate::recover`]).
pub fn grep_turns(turns: &[Turn], needle_lower: &str, max: usize) -> Vec<Hit> {
    if needle_lower.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for (i, t) in turns.iter().enumerate() {
        let lower = t.text.to_lowercase();
        if let Some(pos) = lower.find(needle_lower) {
            hits.push(Hit {
                turn: i,
                role: t.role,
                snippet: snippet_around(&t.text, pos, needle_lower.len()),
            });
            if hits.len() >= max {
                break;
            }
        }
    }
    hits
}

/// A ~one-line snippet of `text` around byte `pos` (length `len`), with ellipses,
/// newlines flattened, clamped to char boundaries.
fn snippet_around(text: &str, pos: usize, len: usize) -> String {
    let pos = pos.min(text.len());
    let mut start = pos.saturating_sub(34);
    while start > 0 && !text.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (pos + len + 46).min(text.len());
    while end < text.len() && !text.is_char_boundary(end) {
        end += 1;
    }
    let mut s = String::new();
    if start > 0 {
        s.push('…');
    }
    s.push_str(text[start..end].trim());
    if end < text.len() {
        s.push('…');
    }
    s.replace(['\n', '\r', '\t'], " ")
}

/// Read a transcript `.jsonl` into a clean list of conversation turns: user
/// Hard cap on a single transcript line we buffer in memory. Transcript files
/// are UNTRUSTED (a `.phx` bundle can be crafted by anyone): a single multi-GB
/// line with no newline would otherwise be read wholly into RAM and exhaust
/// memory (OOM/abort) across the parse threads or the reader. We accumulate at
/// most MAX_LINE bytes and drain the rest of an oversized line WITHOUT keeping
/// it; the JSON parser then simply fails on that (truncated) line — the safe
/// outcome for a hostile giant line.
pub(crate) const MAX_LINE: usize = 8 * 1024 * 1024; // 8 MiB

/// Like `BufRead::read_until(b'\n', line)` but bounded: `line` never grows past
/// MAX_LINE. Still consumes the whole physical line from the stream (so the
/// caller keeps advancing), returning Ok(0) only at EOF. An oversized line is
/// truncated to MAX_LINE and its overflow discarded instead of allocated.
pub(crate) fn read_line_capped<R: BufRead>(
    r: &mut R,
    line: &mut Vec<u8>,
) -> std::io::Result<usize> {
    let mut consumed = 0usize;
    loop {
        let (done, used) = {
            let buf = match r.fill_buf() {
                Ok(b) => b,
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if buf.is_empty() {
                return Ok(consumed); // EOF (0 iff nothing consumed since last newline)
            }
            match buf.iter().position(|&b| b == b'\n') {
                Some(i) => {
                    if line.len() < MAX_LINE {
                        let take = (MAX_LINE - line.len()).min(i + 1);
                        line.extend_from_slice(&buf[..take]);
                    }
                    (true, i + 1)
                }
                None => {
                    if line.len() < MAX_LINE {
                        let take = (MAX_LINE - line.len()).min(buf.len());
                        line.extend_from_slice(&buf[..take]);
                    }
                    (false, buf.len())
                }
            }
        };
        r.consume(used);
        consumed += used;
        if done {
            return Ok(consumed);
        }
    }
}

/// prompts and assistant text, with compact `· tool: …` markers. Skips thinking,
/// system reminders, attachments and bare tool-result echoes. Bounded in size.
pub fn read_transcript(path: &Path) -> Vec<Turn> {
    let f = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let mut r = BufReader::new(f);
    let mut out = Vec::new();
    let mut line: Vec<u8> = Vec::new();
    loop {
        if out.len() >= READER_MSG_CAP {
            break;
        }
        line.clear();
        let n = match read_line_capped(&mut r, &mut line) {
            Ok(n) => n,
            Err(_) => break,
        };
        if n == 0 {
            break;
        }
        extract_turn(&line, &mut out);
    }
    out
}

fn extract_turn(buf: &[u8], out: &mut Vec<Turn>) {
    let mut p = P::new(buf);
    if !p.obj_begin() {
        return;
    }
    loop {
        let k = match p.obj_key() {
            Some(k) => k,
            None => break,
        };
        if k == "message" {
            extract_message(&mut p, out);
        } else {
            let _ = p.skip();
        }
        if !p.obj_sep() {
            break;
        }
    }
}

fn extract_message(p: &mut P, out: &mut Vec<Turn>) {
    if !p.obj_begin() {
        return;
    }
    let mut role = String::new();
    let mut text = String::new();
    let mut tools: Vec<String> = Vec::new();
    loop {
        let k = match p.obj_key() {
            Some(k) => k,
            None => break,
        };
        match k.as_str() {
            "role" => role = p.take_string().unwrap_or_default(),
            "content" => match p.peek_ws() {
                b'"' => {
                    if let Some(s) = p.take_string() {
                        text.push_str(&s);
                    }
                }
                b'[' => extract_content(p, &mut text, &mut tools),
                _ => {
                    let _ = p.skip();
                }
            },
            _ => {
                let _ = p.skip();
            }
        }
        if !p.obj_sep() {
            break;
        }
    }
    let role_n = match role.as_str() {
        "assistant" => 1u8,
        "user" => 0u8,
        _ => 2u8,
    };
    let mut t = text.trim().to_string();
    // Drop tool-result-only user echoes and the injected slash-command wrappers:
    // they are noise for a human reader.
    if t.starts_with("<local-command") || t.starts_with("<command-") {
        return;
    }
    if !tools.is_empty() {
        if !t.is_empty() {
            t.push('\n');
        }
        t.push_str(&format!("· tool: {}", tools.join(", ")));
    }
    if t.is_empty() {
        return;
    }
    if t.len() > READER_TEXT_CAP {
        let mut end = READER_TEXT_CAP;
        while end > 0 && !t.is_char_boundary(end) {
            end -= 1;
        }
        t.truncate(end);
        t.push('…');
    }
    out.push(Turn { role: role_n, text: t });
}

fn extract_content(p: &mut P, text: &mut String, tools: &mut Vec<String>) {
    if !p.arr_begin() {
        return;
    }
    loop {
        if p.peek_ws() == b'{' {
            let mut btype = String::new();
            let mut bname = String::new();
            let mut btext = String::new();
            if p.obj_begin() {
                loop {
                    let k = match p.obj_key() {
                        Some(k) => k,
                        None => break,
                    };
                    match k.as_str() {
                        "type" => btype = p.take_string().unwrap_or_default(),
                        "name" => bname = p.take_string().unwrap_or_default(),
                        "text" => btext = p.take_string().unwrap_or_default(),
                        _ => {
                            let _ = p.skip();
                        }
                    }
                    if !p.obj_sep() {
                        break;
                    }
                }
            }
            match btype.as_str() {
                "text" => {
                    if !btext.is_empty() {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(&btext);
                    }
                }
                "tool_use" => {
                    if !bname.is_empty() {
                        tools.push(bname);
                    }
                }
                _ => {} // thinking, tool_result, image, … omitted
            }
        } else {
            let _ = p.skip();
        }
        if !p.arr_sep() {
            break;
        }
    }
}

fn parse_session(path: &Path, size: u64) -> Option<Session> {
    let f = File::open(path).ok()?;
    let mut r = BufReader::new(f);
    let mut s = Session::default();
    s.size = size;
    s.mtime_ms = mtime_ms(path);
    s.path = path.to_string_lossy().to_string();
    s.id = path
        .file_stem()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_string();

    let mut tools: HashMap<String, u64> = HashMap::new();
    let mut models: Vec<String> = Vec::new();
    let mut files: HashSet<String> = HashSet::new();
    let mut search = String::new();
    let mut uuids: Vec<u64> = Vec::new();

    let mut line: Vec<u8> = Vec::new();
    loop {
        line.clear();
        let n = read_line_capped(&mut r, &mut line).ok()?;
        if n == 0 {
            break;
        }
        parse_line(&line, &mut s, &mut tools, &mut models, &mut files, &mut search, &mut uuids);
    }
    // Keep the K smallest distinct message-uuid hashes as this session's kin
    // sketch (position-independent, fixed-size; see KIN_SKETCH_K).
    uuids.sort_unstable();
    uuids.dedup();
    uuids.truncate(KIN_SKETCH_K);
    s.kin_sketch = uuids;

    // Finalise derived fields.
    s.models = models;
    let mut tv: Vec<(String, u64)> = tools.into_iter().collect();
    tv.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    tv.truncate(20);
    s.tools = tv;
    let mut fv: Vec<String> = files.into_iter().collect();
    fv.sort();
    fv.truncate(300);
    s.files = fv;

    if s.project_path.is_empty() {
        // Fallback: best-effort from the encoded directory name.
        s.project_path = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|x| x.to_str())
            .unwrap_or("")
            .to_string();
    }
    s.project_name = s
        .project_path
        .rsplit(|c| c == '\\' || c == '/')
        .next()
        .unwrap_or(&s.project_path)
        .to_string();

    if s.title.is_empty() {
        let fp = s.first_prompt.trim();
        // A session resumed/compacted from another one opens with the same wrapper
        // message (e.g. "<local-command-caveat>", "<command-name>", "Caveat:"),
        // so deriving the title from it makes sibling sessions look identical.
        // Prefer the (distinct) summary in that case; fall back to the raw prompt.
        let looks_wrapper = fp.is_empty()
            || fp == "No prompt"
            || fp.starts_with("<local-command")
            || fp.starts_with("<command-")
            || fp.starts_with("Caveat:");
        if !looks_wrapper {
            s.title = fp.chars().take(80).collect();
        } else if !s.summary.is_empty() {
            s.title = s.summary.chars().take(80).collect();
        } else if !fp.is_empty() && fp != "No prompt" {
            s.title = fp.chars().take(80).collect();
        } else {
            s.title = "(senza titolo)".to_string();
        }
    }
    if s.modified.is_empty() {
        s.modified = s.created.clone();
    }
    // round out the search index with title/summary/project/tools/files
    push_search(&mut search, &s.title);
    push_search(&mut search, &s.summary);
    push_search(&mut search, &s.project_name);
    for (n, _) in &s.tools {
        push_search(&mut search, n);
    }
    for f in &s.files {
        let base = f.rsplit(|c| c == '\\' || c == '/').next().unwrap_or(f);
        push_search(&mut search, base);
    }
    s.search_text = search;
    Some(s)
}

#[allow(clippy::too_many_arguments)]
fn parse_line(
    buf: &[u8],
    s: &mut Session,
    tools: &mut HashMap<String, u64>,
    models: &mut Vec<String>,
    files: &mut HashSet<String>,
    search: &mut String,
    uuids: &mut Vec<u64>,
) {
    let mut p = P::new(buf);
    if !p.obj_begin() {
        return;
    }
    let mut role: Option<String> = None;
    let mut has_tool_use = false;
    let mut content_string: Option<String> = None;
    let mut content_is_array = false;
    let mut rec_type: Option<String> = None;
    let mut rec_uuid: Option<String> = None;

    loop {
        let k = match p.obj_key() {
            Some(k) => k,
            None => break,
        };
        match k.as_str() {
            "cwd" => {
                if let Some(v) = p.take_string() {
                    if !v.is_empty() {
                        s.project_path = v;
                    }
                }
            }
            "gitBranch" => {
                if let Some(v) = p.take_string() {
                    s.git_branch = v;
                }
            }
            "version" => {
                if let Some(v) = p.take_string() {
                    s.version = v;
                }
            }
            "entrypoint" => {
                if let Some(v) = p.take_string() {
                    s.entrypoint = v;
                }
            }
            "isSidechain" => {
                if p.take_bool() {
                    s.is_sidechain = true;
                }
            }
            "type" => rec_type = p.take_string(),
            "uuid" => rec_uuid = p.take_string(),
            "timestamp" => {
                if let Some(v) = p.take_string() {
                    if s.created.is_empty() {
                        s.created = v.clone();
                    }
                    s.modified = v;
                }
            }
            "aiTitle" => {
                if let Some(v) = p.take_string() {
                    if !v.is_empty() {
                        s.title = v;
                    }
                }
            }
            "lastPrompt" => {
                if let Some(v) = p.take_string() {
                    if !v.is_empty() {
                        s.last_prompt = trunc(v);
                    }
                }
            }
            "summary" => {
                if let Some(v) = p.take_string() {
                    if s.summary.is_empty() {
                        s.summary = trunc(v);
                    }
                }
            }
            "message" => {
                parse_message(
                    &mut p,
                    &mut role,
                    &mut has_tool_use,
                    &mut content_string,
                    &mut content_is_array,
                    s,
                    tools,
                    models,
                );
            }
            "snapshot" => {
                parse_snapshot(&mut p, files);
            }
            _ => {
                let _ = p.skip();
            }
        }
        if !p.obj_sep() {
            break;
        }
    }

    // Fingerprint user/assistant messages by uuid for cross-file kin detection.
    if let (Some(t), Some(u)) = (rec_type.as_deref(), rec_uuid.as_deref()) {
        if (t == "user" || t == "assistant") && !u.is_empty() {
            uuids.push(fnv1a_64(u));
        }
    }

    if let Some(r) = role.as_deref() {
        s.message_count += 1;
        if r == "user" {
            if let Some(t) = content_string {
                push_search(search, &t);
                if !s.is_continuation
                    && t.contains("This session is being continued from a previous conversation")
                {
                    s.is_continuation = true;
                }
                if s.first_prompt.is_empty() {
                    s.first_prompt = t.clone();
                }
                s.last_prompt = t;
                s.last_kind = KIND_HUMAN;
            } else if content_is_array {
                s.last_kind = KIND_TOOLRESULT;
            }
        } else if r == "assistant" {
            s.last_kind = if has_tool_use {
                KIND_ASSISTANT_TOOL
            } else {
                KIND_ASSISTANT_TEXT
            };
        }
    }
    let _ = KIND_NONE;
}

#[allow(clippy::too_many_arguments)]
fn parse_message(
    p: &mut P,
    role: &mut Option<String>,
    has_tool_use: &mut bool,
    content_string: &mut Option<String>,
    content_is_array: &mut bool,
    s: &mut Session,
    tools: &mut HashMap<String, u64>,
    models: &mut Vec<String>,
) {
    if !p.obj_begin() {
        return;
    }
    loop {
        let k = match p.obj_key() {
            Some(k) => k,
            None => break,
        };
        match k.as_str() {
            "role" => {
                *role = p.take_string();
            }
            "model" => {
                if let Some(m) = p.take_string() {
                    if !m.is_empty() && !models.iter().any(|x| x == &m) {
                        models.push(m);
                    }
                }
            }
            "usage" => {
                parse_usage(p, s);
            }
            "content" => match p.peek_ws() {
                b'"' => {
                    *content_string = p.take_string().map(trunc);
                }
                b'[' => {
                    *content_is_array = true;
                    parse_content(p, has_tool_use, tools);
                }
                _ => {
                    let _ = p.skip();
                }
            },
            _ => {
                let _ = p.skip();
            }
        }
        if !p.obj_sep() {
            break;
        }
    }
}

fn parse_content(p: &mut P, has_tool_use: &mut bool, tools: &mut HashMap<String, u64>) {
    if !p.arr_begin() {
        return;
    }
    loop {
        if p.peek_ws() == b'{' {
            let mut btype: Option<String> = None;
            let mut bname: Option<String> = None;
            if p.obj_begin() {
                loop {
                    let k = match p.obj_key() {
                        Some(k) => k,
                        None => break,
                    };
                    match k.as_str() {
                        "type" => btype = p.take_string(),
                        "name" => bname = p.take_string(),
                        _ => {
                            let _ = p.skip();
                        }
                    }
                    if !p.obj_sep() {
                        break;
                    }
                }
            }
            if btype.as_deref() == Some("tool_use") {
                *has_tool_use = true;
                if let Some(n) = bname {
                    *tools.entry(n).or_insert(0) += 1;
                }
            }
        } else {
            let _ = p.skip();
        }
        if !p.arr_sep() {
            break;
        }
    }
}

fn parse_usage(p: &mut P, s: &mut Session) {
    if !p.obj_begin() {
        return;
    }
    loop {
        let k = match p.obj_key() {
            Some(k) => k,
            None => break,
        };
        match k.as_str() {
            "input_tokens" => s.input_tokens += p.take_number() as u64,
            "output_tokens" => s.output_tokens += p.take_number() as u64,
            "cache_read_input_tokens" => s.cache_read += p.take_number() as u64,
            "cache_creation_input_tokens" => s.cache_creation += p.take_number() as u64,
            _ => {
                let _ = p.skip();
            }
        }
        if !p.obj_sep() {
            break;
        }
    }
}

fn parse_snapshot(p: &mut P, files: &mut HashSet<String>) {
    if !p.obj_begin() {
        return;
    }
    loop {
        let k = match p.obj_key() {
            Some(k) => k,
            None => break,
        };
        if k == "trackedFileBackups" {
            if p.obj_begin() {
                loop {
                    let fk = match p.obj_key() {
                        Some(k) => k,
                        None => break,
                    };
                    if files.len() < 1000 {
                        files.insert(fk);
                    }
                    let _ = p.skip();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn read_line_capped_bounds_a_giant_line() {
        // A single line far larger than MAX_LINE (a hostile .phx transcript) must
        // NOT be buffered whole: `line` is capped at MAX_LINE, yet the reader still
        // advances past the entire physical line and reaches the next one.
        let big = MAX_LINE + 10_000;
        let mut data = vec![b'a'; big];
        data.push(b'\n');
        data.extend_from_slice(b"next\n");
        let mut r = BufReader::new(Cursor::new(data));
        let mut line = Vec::new();

        let n1 = read_line_capped(&mut r, &mut line).unwrap();
        assert!(n1 > MAX_LINE, "consumed the whole giant physical line");
        assert!(line.len() <= MAX_LINE, "but buffered at most MAX_LINE bytes");

        line.clear();
        let n2 = read_line_capped(&mut r, &mut line).unwrap();
        assert_eq!(n2, 5);
        assert_eq!(&line, b"next\n");

        line.clear();
        let n3 = read_line_capped(&mut r, &mut line).unwrap();
        assert_eq!(n3, 0, "EOF");
    }
}
