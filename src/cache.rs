//! Persistent incremental cache: stores one compact JSON line per session so
//! repeated launches reuse parse results for files whose size+mtime are
//! unchanged. Short keys keep the file small.

use crate::json::{escape, P};
use crate::scan::Session;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn cache_path(base: &Path) -> PathBuf {
    // versioned: bumping this invalidates old caches when the schema changes
    base.join(".phosphor-cache.v5.jsonl")
}

pub fn load(base: &Path) -> HashMap<String, Session> {
    let mut map = HashMap::new();
    // NB: we never delete anything here. Superseded cache files (old name/schema)
    // are simply ignored and left in place (harmless ~100KB orphans).
    let p = cache_path(base);
    let data = match std::fs::read(&p) {
        Ok(d) => d,
        Err(_) => return map,
    };
    for line in data.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        if let Some(s) = parse_line(line) {
            map.insert(s.path.clone(), s);
        }
    }
    map
}

pub fn save(base: &Path, sessions: &[Session]) {
    let mut buf = String::new();
    for s in sessions {
        buf.push_str(&to_line(s));
        buf.push('\n');
    }
    let tmp = base.join(".phosphor-cache.tmp");
    if std::fs::write(&tmp, &buf).is_ok() {
        let _ = std::fs::rename(&tmp, cache_path(base));
    }
}

fn arr_str(v: &[String]) -> String {
    v.iter()
        .map(|x| format!("\"{}\"", escape(x)))
        .collect::<Vec<_>>()
        .join(",")
}

fn to_line(s: &Session) -> String {
    let tools = s
        .tools
        .iter()
        .map(|(n, c)| format!("[\"{}\",{}]", escape(n), c))
        .collect::<Vec<_>>()
        .join(",");
    // kin sketch: fixed-width hex u64s concatenated, so no JSON-number f64
    // precision loss on round-trip.
    let ks: String = s.kin_sketch.iter().map(|h| format!("{:016x}", h)).collect();
    format!(
        concat!(
            "{{\"id\":\"{}\",\"path\":\"{}\",\"pp\":\"{}\",\"pn\":\"{}\",\"t\":\"{}\",",
            "\"sm\":\"{}\",\"fp\":\"{}\",\"lp\":\"{}\",\"mc\":{},\"it\":{},\"ot\":{},",
            "\"cr\":{},\"cc\":{},\"md\":[{}],\"tl\":[{}],\"fl\":[{}],\"gb\":\"{}\",",
            "\"v\":\"{}\",\"ep\":\"{}\",\"cd\":\"{}\",\"mo\":\"{}\",\"mt\":{},\"sz\":{},",
            "\"sc\":{},\"ct\":{},\"lk\":{},\"st\":\"{}\",\"ks\":\"{}\"}}"
        ),
        escape(&s.id),
        escape(&s.path),
        escape(&s.project_path),
        escape(&s.project_name),
        escape(&s.title),
        escape(&s.summary),
        escape(&s.first_prompt),
        escape(&s.last_prompt),
        s.message_count,
        s.input_tokens,
        s.output_tokens,
        s.cache_read,
        s.cache_creation,
        arr_str(&s.models),
        tools,
        arr_str(&s.files),
        escape(&s.git_branch),
        escape(&s.version),
        escape(&s.entrypoint),
        escape(&s.created),
        escape(&s.modified),
        s.mtime_ms,
        s.size,
        s.is_sidechain,
        s.is_continuation,
        s.last_kind,
        escape(&s.search_text),
        ks,
    )
}

fn parse_str_arr(p: &mut P) -> Vec<String> {
    let mut v = Vec::new();
    if p.arr_begin() {
        loop {
            if let Some(x) = p.take_string() {
                v.push(x);
            }
            if !p.arr_sep() {
                break;
            }
        }
    }
    v
}

fn parse_tools(p: &mut P) -> Vec<(String, u64)> {
    let mut v = Vec::new();
    if p.arr_begin() {
        loop {
            if p.peek_ws() == b'[' && p.arr_begin() {
                let name = p.take_string().unwrap_or_default();
                let mut cnt = 0u64;
                if p.arr_sep() {
                    cnt = p.take_number() as u64;
                    while p.arr_sep() {
                        let _ = p.skip();
                    }
                }
                v.push((name, cnt));
            } else {
                let _ = p.skip();
            }
            if !p.arr_sep() {
                break;
            }
        }
    }
    v
}

fn parse_line(buf: &[u8]) -> Option<Session> {
    let mut p = P::new(buf);
    if !p.obj_begin() {
        return None;
    }
    let mut s = Session::default();
    loop {
        let k = match p.obj_key() {
            Some(k) => k,
            None => break,
        };
        match k.as_str() {
            "id" => s.id = p.take_string().unwrap_or_default(),
            "path" => s.path = p.take_string().unwrap_or_default(),
            "pp" => s.project_path = p.take_string().unwrap_or_default(),
            "pn" => s.project_name = p.take_string().unwrap_or_default(),
            "t" => s.title = p.take_string().unwrap_or_default(),
            "sm" => s.summary = p.take_string().unwrap_or_default(),
            "fp" => s.first_prompt = p.take_string().unwrap_or_default(),
            "lp" => s.last_prompt = p.take_string().unwrap_or_default(),
            "mc" => s.message_count = p.take_number() as u64,
            "it" => s.input_tokens = p.take_number() as u64,
            "ot" => s.output_tokens = p.take_number() as u64,
            "cr" => s.cache_read = p.take_number() as u64,
            "cc" => s.cache_creation = p.take_number() as u64,
            "md" => s.models = parse_str_arr(&mut p),
            "tl" => s.tools = parse_tools(&mut p),
            "fl" => s.files = parse_str_arr(&mut p),
            "gb" => s.git_branch = p.take_string().unwrap_or_default(),
            "v" => s.version = p.take_string().unwrap_or_default(),
            "ep" => s.entrypoint = p.take_string().unwrap_or_default(),
            "cd" => s.created = p.take_string().unwrap_or_default(),
            "mo" => s.modified = p.take_string().unwrap_or_default(),
            "mt" => s.mtime_ms = p.take_number() as u64,
            "sz" => s.size = p.take_number() as u64,
            "sc" => s.is_sidechain = p.take_bool(),
            "ct" => s.is_continuation = p.take_bool(),
            "lk" => s.last_kind = p.take_number() as u8,
            "st" => s.search_text = p.take_string().unwrap_or_default(),
            "ks" => {
                let hx = p.take_string().unwrap_or_default();
                s.kin_sketch = hx
                    .as_bytes()
                    .chunks(16)
                    .filter_map(|c| std::str::from_utf8(c).ok())
                    .filter_map(|x| u64::from_str_radix(x, 16).ok())
                    .collect();
            }
            _ => {
                let _ = p.skip();
            }
        }
        if !p.obj_sep() {
            break;
        }
    }
    if s.id.is_empty() || s.path.is_empty() {
        return None;
    }
    Some(s)
}
