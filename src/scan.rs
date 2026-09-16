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
    /// The same tokens, split **by model and by kind** — see [`KIND_IN`] and
    /// friends for the order.
    ///
    /// The aggregates above cannot be priced or weighed honestly on their own:
    /// a single session routinely spans two to four models (Opus and Fable in
    /// the same conversation is the normal case here), and charging all of it
    /// to whichever model spoke first is simply wrong. Cache writes are split
    /// by TTL for the same reason — Anthropic bills the 1-hour cache at twice
    /// the base input rate and the 5-minute one at 1.25x, and on this machine
    /// essentially all of it is the expensive kind.
    ///
    /// Empty for rows that never had per-message detail: recovered ghosts,
    /// fleet rows ingested from another PC. Costing falls back to the
    /// aggregates for those.
    pub usage: Vec<(String, [u64; 5])>,
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
    /// Quante volte l'utente ha dovuto correggere l'agente (stima: vedi
    /// [`crate::corrections`]). E' l'unico numero della card che non lusinga.
    pub corrections: u64,
}

/// Indices into a [`Session::usage`] bucket. Kept as constants rather than an
/// enum so the array stays a plain `[u64; 5]` that serialises as five numbers.
pub const KIND_IN: usize = 0;
pub const KIND_OUT: usize = 1;
pub const KIND_CACHE_READ: usize = 2;
/// Cache written with the 5-minute TTL: billed at 1.25x the base input rate.
pub const KIND_CACHE_5M: usize = 3;
/// Cache written with the 1-hour TTL: billed at 2x. Not a rounding difference —
/// it is 60% more than the 5-minute rate, and it is what nearly all of this
/// machine's cache writes actually are.
pub const KIND_CACHE_1H: usize = 4;
pub const KINDS: usize = 5;

impl Session {
    /// Add `n` tokens of one kind to a model's bucket, and to the aggregate.
    /// The two are always written together so they can never drift apart.
    pub fn add_usage(&mut self, model: &str, kind: usize, n: u64) {
        if n == 0 {
            return;
        }
        match kind {
            KIND_IN => self.input_tokens += n,
            KIND_OUT => self.output_tokens += n,
            KIND_CACHE_READ => self.cache_read += n,
            _ => self.cache_creation += n,
        }
        let m = if model.is_empty() { "?" } else { model };
        if let Some(e) = self.usage.iter_mut().find(|(k, _)| k == m) {
            e.1[kind] += n;
            return;
        }
        // A session with more models than this is a bug in the transcript, not
        // a workflow: the cap keeps a hostile file from growing the row.
        if self.usage.len() < 16 {
            let mut v = [0u64; KINDS];
            v[kind] = n;
            self.usage.push((m.to_string(), v));
        }
    }

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
    // The title a PERSON typed (`/title`, or a worktree name), kept apart from
    // everything else because it outranks the generated one no matter where in
    // the file the two records happen to sit.
    let mut custom_title = String::new();

    let mut line: Vec<u8> = Vec::new();
    loop {
        line.clear();
        let n = read_line_capped(&mut r, &mut line).ok()?;
        if n == 0 {
            break;
        }
        parse_line(&line, &mut s, &mut tools, &mut models, &mut files, &mut search, &mut uuids, &mut custom_title);
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

    // A title someone typed wins over one a model generated, whichever record
    // came last in the file: one is a decision, the other is a guess.
    if !custom_title.trim().is_empty() {
        s.title = custom_title.chars().take(80).collect();
    }
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
    custom_title: &mut String,
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
            // `{"type":"custom-title","customTitle":"…"}` — the name a person
            // gave the session. It was being ignored entirely, so a renamed
            // session showed the generated title instead of the chosen one.
            "customTitle" => {
                if let Some(v) = p.take_string() {
                    if !v.is_empty() {
                        *custom_title = v;
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
                if crate::corrections::is_correction(&t) {
                    s.corrections += 1;
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
    // Model and usage live in the same message object but in no guaranteed
    // order, so both are buffered and attributed once the object closes —
    // reading them as they arrive would charge a message to whatever model
    // happened to be named first.
    let mut msg_model = String::new();
    let mut msg_usage = [0u64; KINDS];
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
                    if !m.is_empty() {
                        if !models.iter().any(|x| x == &m) {
                            models.push(m.clone());
                        }
                        msg_model = m;
                    }
                }
            }
            "usage" => {
                parse_usage(p, &mut msg_usage);
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
    for (kind, n) in msg_usage.iter().enumerate() {
        s.add_usage(&msg_model, kind, *n);
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

/// Read one `usage` object into a per-kind tally.
///
/// `cache_creation_input_tokens` is the TOTAL of the nested `cache_creation`
/// breakdown, so it is only used when that breakdown is absent (older
/// transcripts). When both appear the split wins, because the TTL is what
/// decides the price.
fn parse_usage(p: &mut P, out: &mut [u64; KINDS]) {
    if !p.obj_begin() {
        return;
    }
    let mut cache_total = 0u64;
    let mut split_seen = false;
    loop {
        let k = match p.obj_key() {
            Some(k) => k,
            None => break,
        };
        match k.as_str() {
            "input_tokens" => out[KIND_IN] += p.take_number().max(0.0) as u64,
            "output_tokens" => out[KIND_OUT] += p.take_number().max(0.0) as u64,
            "cache_read_input_tokens" => out[KIND_CACHE_READ] += p.take_number().max(0.0) as u64,
            "cache_creation_input_tokens" => cache_total += p.take_number().max(0.0) as u64,
            "cache_creation" => {
                if p.obj_begin() {
                    loop {
                        let ck = match p.obj_key() {
                            Some(x) => x,
                            None => break,
                        };
                        let n = match ck.as_str() {
                            "ephemeral_5m_input_tokens" => {
                                let n = p.take_number().max(0.0) as u64;
                                out[KIND_CACHE_5M] += n;
                                n
                            }
                            "ephemeral_1h_input_tokens" => {
                                let n = p.take_number().max(0.0) as u64;
                                out[KIND_CACHE_1H] += n;
                                n
                            }
                            _ => {
                                let _ = p.skip();
                                0
                            }
                        };
                        split_seen |= n > 0;
                        if !p.obj_sep() {
                            break;
                        }
                    }
                } else {
                    let _ = p.skip();
                }
            }
            _ => {
                let _ = p.skip();
            }
        }
        if !p.obj_sep() {
            break;
        }
    }
    if !split_seen {
        // No breakdown: assume the cheaper TTL rather than inflate the bill.
        out[KIND_CACHE_5M] += cache_total;
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

    // -----------------------------------------------------------------------
    // Il corpus.
    //
    // Righe SINTETICHE ma costruite sulla forma dei transcript veri: i tipi di
    // record, l'ordine in cui arrivano, i campi che ci sono e quelli che
    // mancano. Sintetiche e non copiate perche' un transcript reale contiene
    // prompt, percorsi e nomi di progetto di chi lo ha scritto, e questo repo
    // finisce in mano ad altri.
    //
    // Fino a ieri questo modulo aveva UN test, su 1.158 righe che leggono il
    // formato di qualcun altro. Un campo letto male non si vede: diventa un
    // numero plausibile e sbagliato che resta in un CSV per mesi.
    // -----------------------------------------------------------------------

    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn tmp() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "phosphor-scan-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Scrive le righe date come transcript e lo fa leggere allo scanner vero.
    fn scan_lines(lines: &[String]) -> Session {
        let dir = tmp();
        let f = dir.join("16b42417-0000-4000-8000-00000000000a.jsonl");
        std::fs::write(&f, lines.join("\n") + "\n").unwrap();
        let size = std::fs::metadata(&f).unwrap().len();
        let s = parse_one(&f, size).expect("il transcript deve produrre una sessione");
        std::fs::remove_dir_all(&dir).ok();
        s
    }

    /// Una riga utente come la scrive Claude Code: `parentUuid` per primo, il
    /// contesto (cwd, ramo, versione) ripetuto su OGNI riga.
    fn user(uuid: &str, text: &str) -> String {
        format!(
            r#"{{"parentUuid":null,"isSidechain":false,"userType":"external","cwd":"C:\\Users\\dev\\proj","gitBranch":"main","version":"2.0.1","entrypoint":"cli","type":"user","uuid":"{uuid}","timestamp":"2026-09-10T08:00:00.000Z","message":{{"role":"user","content":"{text}"}}}}"#
        )
    }

    /// Una riga assistente con uso di token, nella forma nuova: il totale delle
    /// scritture di cache PIU' la ripartizione 5m/1h.
    fn assistant(uuid: &str, model: &str, out: u64, cache_5m: u64, cache_1h: u64) -> String {
        let total = cache_5m + cache_1h;
        format!(
            r#"{{"parentUuid":"p","isSidechain":false,"cwd":"C:\\Users\\dev\\proj","type":"assistant","uuid":"{uuid}","timestamp":"2026-09-10T08:01:00.000Z","message":{{"role":"assistant","model":"{model}","content":[{{"type":"text","text":"ok"}}],"usage":{{"input_tokens":100,"output_tokens":{out},"cache_read_input_tokens":2000,"cache_creation_input_tokens":{total},"cache_creation":{{"ephemeral_5m_input_tokens":{cache_5m},"ephemeral_1h_input_tokens":{cache_1h}}},"thinking_tokens":42,"service_tier":"standard"}}}}}}"#
        )
    }

    #[test]
    fn a_title_typed_by_a_person_beats_one_written_by_a_model() {
        // Trovato costruendo queste fixture: 9 dei 22 transcript di questo PC
        // portano un `custom-title` — il nome dato a mano alla sessione — e lo
        // scanner non lo leggeva affatto, mostrando il titolo generato.
        let s = scan_lines(&[
            r#"{"type":"custom-title","customTitle":"il nome che ho scelto io","sessionId":"x"}"#.into(),
            user("u1", "sistemiamo il parser"),
            // L'aiTitle arriva DOPO, a sessione avviata: non deve vincere.
            r#"{"type":"ai-title","aiTitle":"Sistemare il parser dei transcript","sessionId":"x"}"#.into(),
            assistant("a1", "claude-opus-5", 10, 0, 0),
        ]);
        assert_eq!(s.title, "il nome che ho scelto io");

        // Senza titolo scelto a mano vince quello generato, come prima.
        let s = scan_lines(&[
            user("u1", "sistemiamo il parser"),
            r#"{"type":"ai-title","aiTitle":"Sistemare il parser","sessionId":"x"}"#.into(),
        ]);
        assert_eq!(s.title, "Sistemare il parser");

        // Senza nessuno dei due si ripiega sul primo prompt.
        let s = scan_lines(&[user("u1", "sistemiamo il parser")]);
        assert_eq!(s.title, "sistemiamo il parser");
    }

    #[test]
    fn the_wrapper_first_prompt_does_not_become_the_title() {
        // Una sessione ripresa o compattata si apre con lo stesso messaggio
        // involucro di tutte le altre: prenderlo come titolo renderebbe le
        // sorelle indistinguibili in lista, che e' il caso in cui il titolo
        // serve di piu'.
        let s = scan_lines(&[
            user("u1", "<local-command-caveat>attenzione</local-command-caveat>"),
            r#"{"type":"summary","summary":"Ripulitura della cache incrementale","leafUuid":"u1"}"#.into(),
        ]);
        assert_eq!(s.title, "Ripulitura della cache incrementale");
    }

    #[test]
    fn tokens_are_attributed_to_the_model_that_spent_them() {
        // Una sessione che passa da Opus a Haiku non e' una sessione Opus: e'
        // l'errore piu' grosso che questa stima abbia mai portato.
        let s = scan_lines(&[
            user("u1", "ciao"),
            assistant("a1", "claude-opus-5", 1000, 0, 5000),
            assistant("a2", "claude-haiku-4-5", 3000, 700, 0),
        ]);
        let get = |m: &str| s.usage.iter().find(|(n, _)| n == m).map(|(_, u)| *u);
        let opus = get("claude-opus-5").expect("bucket opus");
        let haiku = get("claude-haiku-4-5").expect("bucket haiku");
        assert_eq!(opus[KIND_OUT], 1000);
        assert_eq!(haiku[KIND_OUT], 3000);
        // La ripartizione 5m/1h e' quella che decide il prezzo: la scrittura a
        // un'ora costa il doppio dell'input, quella a cinque minuti 1.25x.
        assert_eq!(opus[KIND_CACHE_1H], 5000, "1h su opus");
        assert_eq!(opus[KIND_CACHE_5M], 0);
        assert_eq!(haiku[KIND_CACHE_5M], 700, "5m su haiku");
        assert_eq!(haiku[KIND_CACHE_1H], 0);
        // I totali di sessione restano la somma dei bucket.
        assert_eq!(s.output_tokens, 4000);
        assert_eq!(s.cache_creation, 5700);
        assert_eq!(s.cache_read, 4000, "2000 per riga assistente");
        assert_eq!(s.models.len(), 2, "modelli: {:?}", s.models);
    }

    #[test]
    fn a_cache_write_without_a_breakdown_is_priced_as_the_cheap_one() {
        // I transcript vecchi hanno solo il totale. Contarlo come 1h
        // gonfierebbe il conto di chi non puo' piu' verificarlo.
        let line = format!(
            r#"{{"type":"assistant","uuid":"a1","timestamp":"2026-09-10T08:01:00.000Z","message":{{"role":"assistant","model":"claude-sonnet-5","content":[],"usage":{{"input_tokens":10,"output_tokens":20,"cache_creation_input_tokens":9000}}}}}}"#
        );
        let s = scan_lines(&[user("u1", "ciao"), line]);
        let u = s.usage.iter().find(|(n, _)| n == "claude-sonnet-5").expect("bucket").1;
        assert_eq!(u[KIND_CACHE_5M], 9000, "senza ripartizione si sceglie la tariffa bassa");
        assert_eq!(u[KIND_CACHE_1H], 0);
    }

    #[test]
    fn the_junk_records_around_the_conversation_do_not_become_messages() {
        // Un transcript vero e' in gran parte righe di servizio: modalita',
        // permessi, code, istantanee di file. Contarle come messaggi
        // gonfierebbe ogni sessione della lista.
        let s = scan_lines(&[
            r#"{"type":"mode","mode":"default","sessionId":"x"}"#.into(),
            r#"{"type":"permission-mode","permissionMode":"acceptEdits","sessionId":"x"}"#.into(),
            r#"{"type":"queue-operation","operation":"enqueue","sessionId":"x"}"#.into(),
            r#"{"type":"bridge-session","sessionId":"x"}"#.into(),
            user("u1", "prima domanda"),
            assistant("a1", "claude-opus-5", 5, 0, 0),
            r#"{"type":"file-history-snapshot","snapshot":{"trackedFileBackups":{"C:\\Users\\dev\\proj\\src\\main.rs":{}}}}"#.into(),
            user("u2", "seconda domanda"),
        ]);
        assert_eq!(s.message_count, 3, "due utente e uno assistente, nient'altro");
        assert!(s.files.iter().any(|f| f.ends_with("main.rs")), "file: {:?}", s.files);
        assert_eq!(s.first_prompt.trim(), "prima domanda");
        assert!(s.last_prompt.contains("seconda") || s.search_text.contains("seconda"));
    }

    #[test]
    fn a_truncated_last_line_does_not_lose_the_session() {
        // Succede davvero: il file e' aperto in scrittura mentre lo leggiamo.
        // L'ultima riga e' mezza. Perdere l'intera sessione per questo
        // significherebbe che le sessioni VIVE — le uniche che interessano
        // davvero — sono quelle che spariscono dalla lista.
        let s = scan_lines(&[
            user("u1", "domanda"),
            assistant("a1", "claude-opus-5", 7, 0, 0),
            r#"{"type":"assistant","uuid":"a2","message":{"role":"assistant","conten"#.into(),
        ]);
        // Il conteggio arriva a 3: il record mozzo porta il suo `role` PRIMA
        // del taglio, quindi conta come messaggio — ed e' giusto cosi', quel
        // messaggio lo si sta scrivendo davvero. Alla scansione dopo la riga
        // sara' intera e continuera' a contare una volta sola.
        assert_eq!(s.message_count, 3);
        assert_eq!(s.output_tokens, 7, "i token della riga intera non si perdono");
        assert_eq!(s.project_name, "proj");
        assert_eq!(s.title, "domanda", "il titolo regge");
    }

    #[test]
    fn a_sub_agent_transcript_is_marked_as_one() {
        // I sotto-agenti hanno un transcript tutto loro: se finissero in lista
        // come sessioni normali, il conto delle sessioni sarebbe il doppio di
        // quelle che una persona ricorda di aver aperto.
        let s = scan_lines(&[
            r#"{"parentUuid":null,"isSidechain":true,"cwd":"C:\\Users\\dev\\proj","type":"user","uuid":"u1","timestamp":"2026-09-10T08:00:00.000Z","message":{"role":"user","content":"cerca nei file"}}"#.into(),
            assistant("a1", "claude-haiku-4-5", 9, 0, 0),
        ]);
        assert!(s.is_sidechain, "e' un sotto-agente");
    }

    #[test]
    fn tools_and_corrections_are_counted_from_the_conversation() {
        let s = scan_lines(&[
            user("u1", "leggi il file"),
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-09-10T08:01:00.000Z","message":{"role":"assistant","model":"claude-opus-5","content":[{"type":"tool_use","name":"Read","input":{}},{"type":"tool_use","name":"Read","input":{}},{"type":"tool_use","name":"Edit","input":{}}],"usage":{"input_tokens":1,"output_tokens":1}}}"#.into(),
            user("u2", "no, non e' quello che ho chiesto"),
        ]);
        let tool = |n: &str| s.tools.iter().find(|(t, _)| t == n).map(|(_, c)| *c).unwrap_or(0);
        assert_eq!(tool("Read"), 2, "due letture, non una");
        assert_eq!(tool("Edit"), 1);
        assert_eq!(s.corrections, 1, "«no, non e' quello» e' una correzione");
    }

    #[test]
    fn context_fields_come_from_the_lines_that_carry_them() {
        let s = scan_lines(&[
            user("u1", "ciao"),
            assistant("a1", "claude-opus-5", 1, 0, 0),
        ]);
        assert_eq!(s.project_path, r"C:\Users\dev\proj");
        assert_eq!(s.project_name, "proj");
        assert_eq!(s.git_branch, "main");
        assert_eq!(s.version, "2.0.1");
        assert_eq!(s.entrypoint, "cli");
        assert_eq!(s.created, "2026-09-10T08:00:00.000Z", "il PRIMO timestamp");
        assert_eq!(s.modified, "2026-09-10T08:01:00.000Z", "l'ULTIMO timestamp");
        assert!(!s.kin_sketch.is_empty(), "le uuid dei messaggi fanno da impronta");
    }

    #[test]
    fn an_empty_or_unreadable_file_is_still_a_session_with_nothing_in_it() {
        // Verrebbe da dire che un file vuoto non e' una sessione. E' il
        // contrario: `clean --delete-empty` cerca proprio le righe con <= 1
        // messaggio e zero token, e non puo' proporre di cancellare quello che
        // lo scanner ha gia' buttato via. Questo test tiene fermo quel patto.
        let dir = tmp();
        let empty = dir.join("16b42417-0000-4000-8000-00000000000b.jsonl");
        std::fs::write(&empty, b"").unwrap();
        let s = parse_one(&empty, 0).expect("resta elencabile");
        assert_eq!(s.message_count, 0);
        assert_eq!(s.input_tokens + s.output_tokens, 0);
        assert_eq!(s.title, "(senza titolo)");

        let junk = dir.join("16b42417-0000-4000-8000-00000000000c.jsonl");
        std::fs::write(&junk, b"non sono json\naltra riga\n").unwrap();
        let size = std::fs::metadata(&junk).unwrap().len();
        let s = parse_one(&junk, size).expect("nemmeno la spazzatura fa saltare lo scanner");
        assert_eq!(s.message_count, 0, "righe illeggibili non sono messaggi");
        std::fs::remove_dir_all(&dir).ok();
    }
}
