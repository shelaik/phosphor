//! Read the OpenAI **Codex CLI**'s local sessions and present them as the same
//! [`Session`] every other part of Phosphor already understands.
//!
//! Codex keeps its own store, laid out and shaped nothing like Claude Code's:
//!
//! ```text
//! ~/.codex/sessions/YYYY/MM/DD/rollout-<ISO>-<uuid>.jsonl   one file per thread
//! ~/.codex/session_index.jsonl                              id -> thread name
//! ~/.codex/thread-writer-locks/<id>.lock                    open threads
//! ```
//!
//! A rollout is JSONL, one record per line, every record wrapped as
//! `{timestamp, ordinal, type, payload}`. The records that carry what a session
//! list needs:
//!
//! * `session_meta` — id, cwd, originator, cli version, git branch, and for a
//!   sub-agent thread the `parent_thread_id` it belongs to.
//! * `response_item` with `payload.type == "message"` — the conversation
//!   (`role` user/assistant/developer, `content[].text`).
//! * `response_item` `custom_tool_call` / `function_call` — the tool names.
//! * `turn_context` — the model for that turn.
//! * `token_usage_record` (newer) or `event_msg`/`token_count` (older) — running
//!   token totals for the whole thread.
//! * `event_msg`/`patch_apply_end` — the absolute paths of the files it edited.
//!
//! Records are dispatched on the KEYS a payload carries, not on the outer
//! `type`, so the parser survives both key reordering and the record-type
//! renames seen across Codex 0.108 → 0.153.
//!
//! **Sub-agent threads are not sessions.** A rollout with a `parent_thread_id`
//! (a `guardian_review` pass, a spawned agent) folds into its parent's
//! `subagents` count, exactly like Claude Code's `subagents/` sidecars — it is
//! never listed as a row of its own.

use crate::json::P;
use crate::scan::{
    push_search, read_line_capped, trunc, Session, Turn, KIND_ASSISTANT_TEXT, KIND_HUMAN,
    MAX_LINE, READER_MSG_CAP, READER_TEXT_CAP,
};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Value of [`Session::agent`] for everything this module produces.
pub const AGENT: &str = "codex";

/// Longest a single reconstructed field is allowed to get.
const TEXT_CAP: usize = 6000;

/// Codex's home directory: `$CODEX_HOME` when set, else `~/.codex`. Returns
/// `None` when neither resolves — Codex simply isn't installed for this user,
/// which is not an error.
pub fn home() -> Option<PathBuf> {
    if let Ok(h) = std::env::var("CODEX_HOME") {
        if !h.trim().is_empty() {
            return Some(PathBuf::from(h));
        }
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()?;
    let p = PathBuf::from(home).join(".codex");
    p.is_dir().then_some(p)
}

/// A rollout file on disk.
struct Entry {
    path: PathBuf,
    size: u64,
    mtime: u64,
}

/// Collect `sessions/**/rollout-*.jsonl`. The tree is date-partitioned
/// (`YYYY/MM/DD`), so the walk is shallow and bounded.
fn walk(dir: &Path, out: &mut Vec<Entry>, depth: usize) {
    if depth > 6 {
        return;
    }
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
            walk(&path, out, depth + 1);
            continue;
        }
        let name = match path.file_name().and_then(|x| x.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if !name.starts_with("rollout-") || !name.ends_with(".jsonl") {
            continue;
        }
        let md = e.metadata().ok();
        out.push(Entry {
            size: md.as_ref().map(|m| m.len()).unwrap_or(0),
            mtime: md
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            path,
        });
    }
}

/// `session_index.jsonl`: `{id, thread_name, updated_at}`, appended to as Codex
/// renames a thread, so the LAST entry for an id is the current name.
fn read_titles(home: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let data = match std::fs::read(home.join("session_index.jsonl")) {
        Ok(d) => d,
        Err(_) => return map,
    };
    for line in data.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let mut p = P::new(line);
        let (mut id, mut name) = (String::new(), String::new());
        if !p.obj_begin() {
            continue;
        }
        loop {
            let k = match p.obj_key() {
                Some(k) => k,
                None => break,
            };
            match k.as_str() {
                "id" => id = p.take_string().unwrap_or_default(),
                "thread_name" => name = p.take_string().unwrap_or_default(),
                _ => {
                    let _ = p.skip();
                }
            }
            if !p.obj_sep() {
                break;
            }
        }
        if !id.is_empty() && !name.is_empty() {
            map.insert(id, name);
        }
    }
    map
}

/// Fields harvested from one rollout, before they become a [`Session`].
#[derive(Default)]
struct Roll {
    id: String,
    parent: String,
    cwd: String,
    originator: String,
    cli_version: String,
    branch: String,
    thread_source: String,
    created: String,
    modified: String,
    models: Vec<String>,
    input: u64,
    cached: u64,
    cache_write: u64,
    output: u64,
    messages: u64,
    first_prompt: String,
    last_prompt: String,
    last_kind: u8,
    tools: HashMap<String, u64>,
    files: Vec<String>,
    search: String,
}

/// Scaffolding injected into the `user` role rather than something the user
/// typed. Codex opens a thread with an `<environment_context>` block, replays
/// the project's `AGENTS.md` as a user message, and slips in
/// `<codex_internal_context>` / `<turn_aborted>` markers mid-thread; letting any
/// of those become a title or a first prompt would make every session look the
/// same. The list is empirical — these are the shapes that actually occur in
/// the rollouts, not a guess at the general form, so a real prompt that merely
/// starts with `<` is left alone.
fn is_wrapper(t: &str) -> bool {
    const TAGS: [&str; 6] = [
        "<environment_context",
        "<codex_internal_context",
        "<turn_aborted",
        "<user_instructions",
        "<model_switch",
        "<system_reminder",
    ];
    let t = t.trim_start();
    t.is_empty()
        || t.starts_with("# AGENTS.md instructions for ")
        || TAGS.iter().any(|tag| t.starts_with(tag))
}

/// Read one rollout file and fold it into a `Roll`.
fn read_roll(path: &Path) -> Option<Roll> {
    let f = File::open(path).ok()?;
    let mut r = BufReader::new(f);
    let mut r0 = Roll::default();
    let mut line: Vec<u8> = Vec::new();
    loop {
        line.clear();
        let n = read_line_capped(&mut r, &mut line).ok()?;
        if n == 0 {
            break;
        }
        if line.len() >= MAX_LINE {
            continue; // oversized record: skipped, never buffered whole
        }
        parse_record(&line, &mut r0);
    }
    (!r0.id.is_empty()).then_some(r0)
}

/// Parse one `{timestamp, ordinal, type, payload}` record.
fn parse_record(buf: &[u8], r: &mut Roll) {
    let mut p = P::new(buf);
    if !p.obj_begin() {
        return;
    }
    let mut outer_ts = String::new();
    loop {
        let k = match p.obj_key() {
            Some(k) => k,
            None => break,
        };
        match k.as_str() {
            "timestamp" => outer_ts = p.take_string().unwrap_or_default(),
            "payload" => parse_payload(&mut p, r),
            _ => {
                let _ = p.skip();
            }
        }
        if !p.obj_sep() {
            break;
        }
    }
    // Every record is stamped, so the last one seen is the thread's last
    // activity — more precise than the file mtime, which a copy would reset.
    if !outer_ts.is_empty() {
        r.modified = outer_ts;
    }
}

/// Parse a record's `payload`, dispatching on the keys it carries.
fn parse_payload(p: &mut P, r: &mut Roll) {
    if !p.obj_begin() {
        let _ = p.skip();
        return;
    }
    // Scratch for the fields whose meaning depends on their siblings.
    let mut kind = String::new(); // payload.type, when present
    let mut role = String::new();
    let mut name = String::new();
    let mut text = String::new();
    let (mut id, mut cwd) = (String::new(), String::new());
    let mut meta_ts = String::new();
    let mut usage: Option<(u64, u64, u64, u64)> = None;
    loop {
        let k = match p.obj_key() {
            Some(k) => k,
            None => break,
        };
        match k.as_str() {
            "type" => kind = p.take_string().unwrap_or_default(),
            // --- session_meta -------------------------------------------------
            // `id` is the thread's OWN id; `session_id` is the parent's on a
            // sub-agent rollout, so it must never be used as the identity.
            "id" => id = p.take_string().unwrap_or_default(),
            "parent_thread_id" => r.parent = p.take_string().unwrap_or_default(),
            "cwd" => cwd = p.take_string().unwrap_or_default(),
            "originator" => r.originator = p.take_string().unwrap_or_default(),
            "cli_version" => r.cli_version = p.take_string().unwrap_or_default(),
            "thread_source" => r.thread_source = p.take_string().unwrap_or_default(),
            "timestamp" => meta_ts = p.take_string().unwrap_or_default(),
            "git" => {
                if let Some(b) = take_nested_string(p, "branch") {
                    r.branch = b;
                }
            }
            // --- turn_context -------------------------------------------------
            // A thread can switch model mid-way, so every distinct one is kept
            // (first wins for costing, like the Claude Code side).
            "model" => {
                let m = p.take_string().unwrap_or_default();
                if !m.is_empty() && !r.models.contains(&m) && r.models.len() < 8 {
                    r.models.push(m);
                }
            }
            // --- messages and tool calls -------------------------------------
            "role" => role = p.take_string().unwrap_or_default(),
            "name" => name = p.take_string().unwrap_or_default(),
            "content" => text = take_content_text(p),
            // --- token accounting ---------------------------------------------
            // Both are running totals for the whole thread, so the last record
            // wins rather than accumulating (which would multiply-count).
            "thread_token_usage" => usage = Some(take_usage(p)),
            "info" => {
                if let Some(u) = take_nested_usage(p, "total_token_usage") {
                    usage = Some(u);
                }
            }
            // --- edited files ---------------------------------------------------
            "changes" => take_change_paths(p, &mut r.files),
            _ => {
                let _ = p.skip();
            }
        }
        if !p.obj_sep() {
            break;
        }
    }

    // session_meta: the FIRST one in the file establishes identity and start.
    // Messages and tool calls carry an `id` too (`msg_…`, `ctc_…`), so require a
    // key only session_meta has before treating one as the thread's identity.
    let is_meta = !cwd.is_empty() || !r.originator.is_empty() || !r.cli_version.is_empty();
    if is_meta && !id.is_empty() && r.id.is_empty() {
        r.id = id;
        r.cwd = cwd;
        r.created = meta_ts;
    }
    if let Some((i, c, w, o)) = usage {
        r.input = i;
        r.cached = c;
        r.cache_write = w;
        r.output = o;
    }
    if kind == "message" {
        // `developer` carries the injected system prompts, not the conversation.
        match role.as_str() {
            "user" if !is_wrapper(&text) => {
                r.messages += 1;
                push_search(&mut r.search, &text);
                if r.first_prompt.is_empty() {
                    r.first_prompt = trunc(text.clone());
                }
                r.last_prompt = trunc(text);
                r.last_kind = KIND_HUMAN;
            }
            "user" => r.messages += 1,
            "assistant" => {
                r.messages += 1;
                push_search(&mut r.search, &text);
                r.last_kind = KIND_ASSISTANT_TEXT;
            }
            _ => {}
        }
    } else if (kind == "custom_tool_call" || kind == "function_call") && !name.is_empty() {
        *r.tools.entry(name).or_insert(0) += 1;
    }
}

/// Read `{"branch": "..."}`-shaped nested object, returning one key's string.
fn take_nested_string(p: &mut P, want: &str) -> Option<String> {
    let mut found = None;
    if !p.obj_begin() {
        let _ = p.skip();
        return None;
    }
    loop {
        let k = match p.obj_key() {
            Some(k) => k,
            None => break,
        };
        if k == want {
            found = p.take_string();
        } else {
            let _ = p.skip();
        }
        if !p.obj_sep() {
            break;
        }
    }
    found
}

/// `{input_tokens, cached_input_tokens, cache_write_input_tokens, output_tokens}`.
fn take_usage(p: &mut P) -> (u64, u64, u64, u64) {
    let (mut i, mut c, mut w, mut o) = (0u64, 0u64, 0u64, 0u64);
    if !p.obj_begin() {
        let _ = p.skip();
        return (i, c, w, o);
    }
    loop {
        let k = match p.obj_key() {
            Some(k) => k,
            None => break,
        };
        match k.as_str() {
            "input_tokens" => i = p.take_number().max(0.0) as u64,
            "cached_input_tokens" => c = p.take_number().max(0.0) as u64,
            "cache_write_input_tokens" => w = p.take_number().max(0.0) as u64,
            "output_tokens" => o = p.take_number().max(0.0) as u64,
            _ => {
                let _ = p.skip();
            }
        }
        if !p.obj_sep() {
            break;
        }
    }
    (i, c, w, o)
}

/// Dig one level for a usage object (`info.total_token_usage`).
fn take_nested_usage(p: &mut P, want: &str) -> Option<(u64, u64, u64, u64)> {
    let mut found = None;
    if !p.obj_begin() {
        let _ = p.skip();
        return None;
    }
    loop {
        let k = match p.obj_key() {
            Some(k) => k,
            None => break,
        };
        if k == want {
            found = Some(take_usage(p));
        } else {
            let _ = p.skip();
        }
        if !p.obj_sep() {
            break;
        }
    }
    found
}

/// Concatenate the `text` of a message's `content` array. Both the user side
/// (`input_text`) and the assistant side (`output_text`) use the same key.
fn take_content_text(p: &mut P) -> String {
    let mut out = String::new();
    if !p.arr_begin() {
        let _ = p.skip();
        return out;
    }
    loop {
        if p.peek_ws() == b'{' && p.obj_begin() {
            loop {
                let k = match p.obj_key() {
                    Some(k) => k,
                    None => break,
                };
                if k == "text" {
                    if let Some(t) = p.take_string() {
                        if out.len() < TEXT_CAP {
                            if !out.is_empty() {
                                out.push('\n');
                            }
                            out.push_str(&t);
                        }
                    }
                } else {
                    let _ = p.skip();
                }
                if !p.obj_sep() {
                    break;
                }
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

/// `patch_apply_end.changes` is keyed BY absolute file path, so the keys are
/// the edited files; the values (whole file contents) are skipped unread.
fn take_change_paths(p: &mut P, files: &mut Vec<String>) {
    if !p.obj_begin() {
        let _ = p.skip();
        return;
    }
    loop {
        let k = match p.obj_key() {
            Some(k) => k,
            None => break,
        };
        if files.len() < 300 && !files.contains(&k) {
            files.push(k);
        }
        let _ = p.skip();
        if !p.obj_sep() {
            break;
        }
    }
}

/// Turn a parsed rollout into a [`Session`].
fn to_session(r: Roll, path: &Path, size: u64, mtime: u64, titles: &HashMap<String, String>) -> Session {
    let mut s = Session::default();
    s.agent = AGENT.to_string();
    s.id = r.id;
    s.path = path.to_string_lossy().into_owned();
    s.project_path = r.cwd;
    s.project_name = s
        .project_path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(&s.project_path)
        .to_string();
    // Codex names a thread itself (the picker shows these names); fall back to
    // the first real prompt, the way the Claude Code side falls back off aiTitle.
    s.title = titles
        .get(&s.id)
        .cloned()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| {
            let fp = r.first_prompt.trim();
            if fp.is_empty() {
                "(senza titolo)".to_string()
            } else {
                fp.chars().take(80).collect()
            }
        });
    s.first_prompt = r.first_prompt;
    s.last_prompt = r.last_prompt;
    s.message_count = r.messages;
    // OpenAI reports `input_tokens` INCLUSIVE of the cached prefix, Anthropic
    // reports it exclusive. Subtract so one cost formula fits both and cached
    // reads are not billed at the full input rate.
    s.input_tokens = r.input.saturating_sub(r.cached);
    s.cache_read = r.cached;
    s.cache_creation = r.cache_write;
    s.output_tokens = r.output;
    s.models = r.models;
    let mut tv: Vec<(String, u64)> = r.tools.into_iter().collect();
    tv.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    tv.truncate(20);
    s.tools = tv;
    let mut fv = r.files;
    fv.sort();
    fv.truncate(300);
    s.files = fv;
    s.git_branch = r.branch;
    s.version = r.cli_version;
    s.entrypoint = r.originator;
    s.created = r.created;
    s.modified = r.modified;
    if s.modified.is_empty() {
        s.modified = s.created.clone();
    }
    s.mtime_ms = mtime;
    s.size = size;
    // Set for a sub-agent rollout (a `guardian_review` pass, a spawned agent):
    // it is what tells the scan this row belongs to another thread rather than
    // standing on its own.
    s.parent_id = r.parent;
    s.is_sidechain =
        !s.parent_id.is_empty() || !matches!(r.thread_source.as_str(), "" | "user");
    s.last_kind = r.last_kind;
    let mut search = r.search;
    push_search(&mut search, &s.title);
    push_search(&mut search, &s.project_name);
    push_search(&mut search, AGENT);
    for (n, _) in &s.tools {
        push_search(&mut search, n);
    }
    for f in &s.files {
        let base = f.rsplit(['\\', '/']).next().unwrap_or(f);
        push_search(&mut search, base);
    }
    s.search_text = search;
    s
}

/// Incremental scan of the Codex store, mirroring `scan::scan_incremental`:
/// entries whose size+mtime are unchanged are reused from `cache`, the rest are
/// re-read. Returns `(sessions, changed)`.
///
/// Sub-agent rollouts (those with a `parent_thread_id`) are folded into their
/// parent's `subagents` count instead of being listed.
pub fn scan_incremental(home: &Path, cache: &mut HashMap<String, Session>) -> (Vec<Session>, bool) {
    let mut entries = Vec::new();
    walk(&home.join("sessions"), &mut entries, 0);
    if entries.is_empty() {
        return (Vec::new(), false);
    }
    let titles = read_titles(home);

    let mut sessions: Vec<Session> = Vec::new();
    let mut children: HashMap<String, u64> = HashMap::new();
    let mut valid: HashSet<String> = HashSet::new();
    let mut changed = false;
    for e in &entries {
        let key = e.path.to_string_lossy().to_string();
        if let Some(c) = cache.get(&key) {
            // Only parent threads are ever cached, so a hit is a session.
            if c.size == e.size && c.mtime_ms == e.mtime && c.is_codex() {
                valid.insert(key);
                sessions.push(c.clone());
                continue;
            }
        }
        // Classify from `session_meta` (the first line) before paying for a full
        // read: a sub-agent rollout only contributes a count, and caching it
        // would put a row in the map that never reaches `cache::save` — every
        // launch would then re-read it and report the scan as changed.
        if let Some(parent) = parent_of(&e.path) {
            *children.entry(parent).or_insert(0) += 1;
            continue;
        }
        let r = match read_roll(&e.path) {
            Some(r) => r,
            None => continue,
        };
        let s = to_session(r, &e.path, e.size, e.mtime, &titles);
        changed = true;
        valid.insert(key.clone());
        cache.insert(key, s.clone());
        sessions.push(s);
    }
    // Evict Codex rows whose rollout is gone, leaving the Claude Code rows in
    // this shared map untouched (the other scanner owns those).
    let before = cache.len();
    cache.retain(|k, v| !v.is_codex() || valid.contains(k));
    if cache.len() != before {
        changed = true;
    }

    for s in &mut sessions {
        s.subagents = children.get(&s.id).copied().unwrap_or(0);
    }
    sessions.sort_by(|a, b| b.mtime_ms.cmp(&a.mtime_ms));
    (sessions, changed)
}

/// The `parent_thread_id` of a rollout, read from its `session_meta` (the first
/// line) alone. `None` for a top-level thread — and for a file whose first line
/// is unreadable, which the full parse then rejects on its own.
fn parent_of(path: &Path) -> Option<String> {
    let f = File::open(path).ok()?;
    let mut r = BufReader::new(f);
    let mut line: Vec<u8> = Vec::new();
    read_line_capped(&mut r, &mut line).ok()?;
    if line.len() >= MAX_LINE {
        return None;
    }
    let mut roll = Roll::default();
    parse_record(&line, &mut roll);
    (!roll.parent.is_empty()).then_some(roll.parent)
}

/// Read a rollout into readable turns for the in-app reader, mirroring
/// `scan::read_transcript`: user prompts, assistant text, compact tool markers.
pub fn read_transcript(path: &Path) -> Vec<Turn> {
    let f = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let mut r = BufReader::new(f);
    let mut out: Vec<Turn> = Vec::new();
    let mut line: Vec<u8> = Vec::new();
    while out.len() < READER_MSG_CAP {
        line.clear();
        match read_line_capped(&mut r, &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if line.len() >= MAX_LINE {
            continue;
        }
        let mut roll = Roll::default();
        let mut turn = ReaderTurn::default();
        read_turn(&line, &mut roll, &mut turn);
        if let Some(t) = turn.into_turn() {
            out.push(t);
        }
    }
    out
}

#[derive(Default)]
struct ReaderTurn {
    kind: String,
    role: String,
    name: String,
    text: String,
}

impl ReaderTurn {
    fn into_turn(self) -> Option<Turn> {
        let clip = |t: String| -> String {
            if t.len() <= READER_TEXT_CAP {
                return t;
            }
            let mut end = READER_TEXT_CAP;
            while end > 0 && !t.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}…", &t[..end])
        };
        match self.kind.as_str() {
            "message" => match self.role.as_str() {
                "user" if !is_wrapper(&self.text) => Some(Turn {
                    role: 0,
                    text: clip(self.text),
                }),
                "assistant" if !self.text.trim().is_empty() => Some(Turn {
                    role: 1,
                    text: clip(self.text),
                }),
                _ => None,
            },
            "custom_tool_call" | "function_call" if !self.name.is_empty() => Some(Turn {
                role: 2,
                text: format!("· {}", self.name),
            }),
            _ => None,
        }
    }
}

/// Like [`parse_record`] but keeping the one record's message/tool fields, which
/// the scanning path folds away.
fn read_turn(buf: &[u8], _r: &mut Roll, t: &mut ReaderTurn) {
    let mut p = P::new(buf);
    if !p.obj_begin() {
        return;
    }
    loop {
        let k = match p.obj_key() {
            Some(k) => k,
            None => break,
        };
        if k == "payload" {
            if p.obj_begin() {
                loop {
                    let pk = match p.obj_key() {
                        Some(x) => x,
                        None => break,
                    };
                    match pk.as_str() {
                        "type" => t.kind = p.take_string().unwrap_or_default(),
                        "role" => t.role = p.take_string().unwrap_or_default(),
                        "name" => t.name = p.take_string().unwrap_or_default(),
                        "content" => t.text = take_content_text(&mut p),
                        _ => {
                            let _ = p.skip();
                        }
                    }
                    if !p.obj_sep() {
                        break;
                    }
                }
            } else {
                let _ = p.skip();
            }
        } else {
            let _ = p.skip();
        }
        if !p.obj_sep() {
            break;
        }
    }
}

/// Ids of the Codex threads with an open writer lock. Codex drops a
/// `thread-writer-locks/<id>.lock` while a thread is attached to a running
/// process and removes it on a clean exit — so a lock plus a live `codex`
/// process means the session is running. A crash can leave one behind, which is
/// why the caller must also see a process.
pub fn open_threads(home: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(home.join("thread-writer-locks")) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(id) = name.strip_suffix(".lock") {
                if crate::valid_session_id(id) {
                    out.push(id.to_string());
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "phosphor-codex-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(p.join("sessions").join("2026").join("09").join("04")).unwrap();
        p
    }

    fn write_rollout(home: &Path, name: &str, lines: &[&str]) -> PathBuf {
        let p = home
            .join("sessions")
            .join("2026")
            .join("09")
            .join("04")
            .join(name);
        std::fs::write(&p, lines.join("\n")).unwrap();
        p
    }

    const META: &str = r#"{"timestamp":"2026-09-04T10:00:00.000Z","type":"session_meta","payload":{"session_id":"aaaa1111-0000-0000-0000-000000000001","id":"aaaa1111-0000-0000-0000-000000000001","timestamp":"2026-09-04T09:59:00.000Z","cwd":"C:\\proj\\demo","originator":"codex-tui","cli_version":"0.153.1","thread_source":"user","git":{"commit_hash":"abc","branch":"main"}}}"#;

    #[test]
    fn parses_a_rollout_into_a_session() {
        let home = tmp();
        write_rollout(
            &home,
            "rollout-2026-09-04T10-00-00-aaaa1111-0000-0000-0000-000000000001.jsonl",
            &[
                META,
                r#"{"timestamp":"2026-09-04T10:00:01.000Z","type":"turn_context","payload":{"turn_id":"t1","model":"gpt-5.6-sol","effort":"xhigh"}}"#,
                r#"{"timestamp":"2026-09-04T10:00:02.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context><cwd>x</cwd></environment_context>"}]}}"#,
                r#"{"timestamp":"2026-09-04T10:00:03.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"sistemami il parser"}]}}"#,
                r#"{"timestamp":"2026-09-04T10:00:04.000Z","type":"response_item","payload":{"type":"custom_tool_call","name":"exec","input":"ls"}}"#,
                r#"{"timestamp":"2026-09-04T10:00:05.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"fatto"}]}}"#,
                r#"{"timestamp":"2026-09-04T10:00:06.000Z","type":"event_msg","payload":{"type":"patch_apply_end","success":true,"changes":{"C:\\proj\\demo\\src\\main.rs":{"type":"add","content":"x"}}}}"#,
                r#"{"timestamp":"2026-09-04T10:00:07.000Z","type":"token_usage_record","payload":{"thread_token_usage":{"input_tokens":1000,"cached_input_tokens":400,"cache_write_input_tokens":50,"output_tokens":200,"total_tokens":1200}}}"#,
            ],
        );
        let mut cache = HashMap::new();
        let (v, changed) = scan_incremental(&home, &mut cache);
        assert!(changed);
        assert_eq!(v.len(), 1);
        let s = &v[0];
        assert!(s.is_codex());
        assert_eq!(s.id, "aaaa1111-0000-0000-0000-000000000001");
        assert_eq!(s.project_name, "demo");
        assert_eq!(s.project_path, r"C:\proj\demo");
        assert_eq!(s.git_branch, "main");
        assert_eq!(s.models, vec!["gpt-5.6-sol"]);
        assert_eq!(s.version, "0.153.1");
        // the <environment_context> wrapper never becomes the title/first prompt
        assert_eq!(s.title, "sistemami il parser");
        assert_eq!(s.first_prompt, "sistemami il parser");
        // OpenAI's input_tokens includes the cached prefix: it is split out so
        // the shared cost formula does not bill cache reads at the input rate.
        assert_eq!(s.input_tokens, 600);
        assert_eq!(s.cache_read, 400);
        assert_eq!(s.cache_creation, 50);
        assert_eq!(s.output_tokens, 200);
        assert_eq!(s.tools, vec![("exec".to_string(), 1)]);
        assert_eq!(s.files, vec![r"C:\proj\demo\src\main.rs"]);
        assert_eq!(s.modified, "2026-09-04T10:00:07.000Z");
        assert_eq!(s.created, "2026-09-04T09:59:00.000Z");
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn the_index_name_wins_over_the_first_prompt() {
        let home = tmp();
        write_rollout(
            &home,
            "rollout-2026-09-04T10-00-00-aaaa1111-0000-0000-0000-000000000001.jsonl",
            &[
                META,
                r#"{"timestamp":"2026-09-04T10:00:03.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"sistemami il parser"}]}}"#,
            ],
        );
        std::fs::write(
            home.join("session_index.jsonl"),
            "{\"id\":\"aaaa1111-0000-0000-0000-000000000001\",\"thread_name\":\"vecchio nome\"}\n{\"id\":\"aaaa1111-0000-0000-0000-000000000001\",\"thread_name\":\"Parser fix\"}\n",
        )
        .unwrap();
        let mut cache = HashMap::new();
        // the LAST name for an id wins: Codex appends on every rename
        assert_eq!(scan_incremental(&home, &mut cache).0[0].title, "Parser fix");
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn a_sub_agent_rollout_counts_but_is_not_listed() {
        let home = tmp();
        write_rollout(
            &home,
            "rollout-2026-09-04T10-00-00-aaaa1111-0000-0000-0000-000000000001.jsonl",
            &[META],
        );
        write_rollout(
            &home,
            "rollout-2026-09-04T10-05-00-bbbb2222-0000-0000-0000-000000000002.jsonl",
            &[r#"{"timestamp":"2026-09-04T10:05:00.000Z","type":"session_meta","payload":{"session_id":"aaaa1111-0000-0000-0000-000000000001","id":"bbbb2222-0000-0000-0000-000000000002","parent_thread_id":"aaaa1111-0000-0000-0000-000000000001","timestamp":"2026-09-04T10:05:00.000Z","cwd":"C:\\proj\\demo","originator":"codex-tui","thread_source":"guardian_review"}}"#],
        );
        let mut cache = HashMap::new();
        let (v, _) = scan_incremental(&home, &mut cache);
        assert_eq!(v.len(), 1, "solo il thread padre e' una sessione");
        assert_eq!(v[0].subagents, 1);
        // and the count survives a fully cached second pass
        let (v2, changed) = scan_incremental(&home, &mut cache);
        assert!(!changed);
        assert_eq!(v2.len(), 1);
        assert_eq!(v2[0].subagents, 1);
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn reader_skips_wrappers_and_marks_tools() {
        let home = tmp();
        let p = write_rollout(
            &home,
            "rollout-2026-09-04T10-00-00-aaaa1111-0000-0000-0000-000000000001.jsonl",
            &[
                META,
                r#"{"timestamp":"2026-09-04T10:00:02.000Z","type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"istruzioni di sistema"}]}}"#,
                r#"{"timestamp":"2026-09-04T10:00:03.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>x</environment_context>"}]}}"#,
                r#"{"timestamp":"2026-09-04T10:00:04.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"ciao"}]}}"#,
                r#"{"timestamp":"2026-09-04T10:00:05.000Z","type":"response_item","payload":{"type":"custom_tool_call","name":"exec","input":"ls"}}"#,
                r#"{"timestamp":"2026-09-04T10:00:06.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"eccomi"}]}}"#,
            ],
        );
        let turns = read_transcript(&p);
        let got: Vec<(u8, &str)> = turns.iter().map(|t| (t.role, t.text.as_str())).collect();
        assert_eq!(got, vec![(0, "ciao"), (2, "· exec"), (1, "eccomi")]);
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn open_threads_lists_only_valid_ids() {
        let home = tmp();
        let locks = home.join("thread-writer-locks");
        std::fs::create_dir_all(&locks).unwrap();
        std::fs::write(locks.join("aaaa1111-0000-0000-0000-000000000001.lock"), "").unwrap();
        std::fs::write(locks.join(".coordination.lock"), "").unwrap();
        let v = open_threads(&home);
        assert_eq!(v, vec!["aaaa1111-0000-0000-0000-000000000001"]);
        std::fs::remove_dir_all(&home).ok();
    }
}
