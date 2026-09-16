//! Persistent incremental cache: stores one compact JSON line per session so
//! repeated launches reuse parse results for files whose size+mtime are
//! unchanged. Short keys keep the file small.

use crate::json::{escape, P};
use crate::scan::Session;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Every cache file starts with this and ends with [`SUFFIX`]; what sits
/// between the two is the key, and nothing else in `~/.claude` looks like this.
const PREFIX: &str = ".phosphor-cache.";
const SUFFIX: &str = ".jsonl";

/// The name of the cache file for THIS build.
///
/// The key is computed by `build.rs` from the modules a cached row is derived
/// from (the parsers, the JSON primitives, this serializer). Change any of
/// them and the name changes, so the old cache is not read: a fix to a parser
/// reaches the transcripts that were already scanned, without anyone having to
/// remember anything.
///
/// It used to be a hand-written `v9` with a comment asking the next person to
/// bump it. A comment is the weakest defence there is — the one time it is
/// missed, the symptom is a fix that appears not to work.
pub fn cache_name() -> String {
    format!("{PREFIX}{}{SUFFIX}", env!("PHOSPHOR_CACHE_KEY"))
}

fn cache_path(base: &Path) -> PathBuf {
    base.join(cache_name())
}

/// Delete the cache files of other builds.
///
/// Now that the name follows the source, a week of work on a parser would
/// otherwise leave a small heap of dead caches in `~/.claude`. Deliberately
/// narrow: direct children of `base` only, regular files only, and only names
/// shaped exactly like ours. It never recurses, so the vault directory is out
/// of reach by construction.
fn sweep(base: &Path, keep: &Path) {
    let rd = match std::fs::read_dir(base) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.starts_with(PREFIX) || !name.ends_with(SUFFIX) || name.len() <= PREFIX.len() + SUFFIX.len() {
            continue;
        }
        let p = e.path();
        if p == keep || !e.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let _ = std::fs::remove_file(&p);
    }
}

pub fn load(base: &Path) -> HashMap<String, Session> {
    let mut map = HashMap::new();
    // Reading never deletes: a cache of another build is simply not ours to
    // read. They are cleared away by the next `save`, once this run has
    // produced something to replace them with.
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
    let p = cache_path(base);
    if std::fs::write(&tmp, &buf).is_ok() && std::fs::rename(&tmp, &p).is_ok() {
        // Only after ours is safely in place: a sweep that ran first would, on
        // a failed write, leave the user with no cache at all.
        sweep(base, &p);
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
    let usage = s
        .usage
        .iter()
        .map(|(m, u)| {
            format!(
                "[\"{}\",{},{},{},{},{}]",
                escape(m), u[0], u[1], u[2], u[3], u[4]
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        concat!(
            "{{\"id\":\"{}\",\"path\":\"{}\",\"pp\":\"{}\",\"pn\":\"{}\",\"t\":\"{}\",",
            "\"sm\":\"{}\",\"fp\":\"{}\",\"lp\":\"{}\",\"mc\":{},\"it\":{},\"ot\":{},",
            "\"cr\":{},\"cc\":{},\"md\":[{}],\"tl\":[{}],\"fl\":[{}],\"gb\":\"{}\",",
            "\"v\":\"{}\",\"ep\":\"{}\",\"cd\":\"{}\",\"mo\":\"{}\",\"mt\":{},\"sz\":{},",
            "\"sc\":{},\"ct\":{},\"lk\":{},\"st\":\"{}\",\"ks\":\"{}\",\"ag\":\"{}\",\"cx\":{},\"us\":[{}]}}"
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
        escape(&s.agent),
        s.corrections,
        usage,
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

/// `[["model", in, out, cacheRead, cache5m, cache1h], …]` — the per-model token
/// split. Written as arrays rather than objects: five numbers per model, and
/// this file is read on every launch.
fn parse_usage_buckets(p: &mut P) -> Vec<(String, [u64; crate::scan::KINDS])> {
    let mut v = Vec::new();
    if p.arr_begin() {
        loop {
            if p.peek_ws() == b'[' && p.arr_begin() {
                let name = p.take_string().unwrap_or_default();
                let mut u = [0u64; crate::scan::KINDS];
                let mut i = 0;
                while p.arr_sep() {
                    let n = p.take_number().max(0.0) as u64;
                    if i < crate::scan::KINDS {
                        u[i] = n;
                    }
                    i += 1;
                }
                if !name.is_empty() {
                    v.push((name, u));
                }
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
            "ag" => s.agent = p.take_string().unwrap_or_default(),
            "cx" => s.corrections = p.take_number() as u64,
            "us" => s.usage = parse_usage_buckets(&mut p),
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
    // Difesa in profondita': la cache la scriviamo noi con righe gia'
    // ripulite, ma e' un file di testo nella cartella dell'utente e non
    // costa niente non fidarsene.
    s.tame_display_fields();
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{KIND_CACHE_5M, KIND_IN, KIND_OUT};

    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn tmp() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "phosphor-cache-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn a_session() -> Session {
        let mut s = Session::default();
        s.id = "16b42417-0000-4000-8000-000000000001".into();
        s.path = r"C:\Users\dev\.claude\projects\C--p\16b42417.jsonl".into();
        s.project_name = "p".into();
        s.title = r#"una sessione con "virgolette" e \ backslash"#.into();
        s.size = 4096;
        s.mtime_ms = 1_700_000_000_000;
        s.corrections = 3;
        s.agent = "codex".into();
        s.kin_sketch = vec![0xdead_beef_dead_beef, 1];
        s.add_usage("opus", KIND_IN, 1000);
        s.add_usage("opus", KIND_OUT, 250);
        s.add_usage("haiku", KIND_CACHE_5M, 7);
        s
    }

    #[test]
    fn a_row_survives_the_round_trip_intact() {
        // Il serializzatore e' scritto a mano: se un campo si perde qui, il
        // programma mostra un numero sbagliato SOLO al secondo avvio, quando
        // la riga arriva dalla cache invece che dal transcript. E' il tipo di
        // bug che si vede giorni dopo averlo scritto.
        let s = a_session();
        let line = to_line(&s);
        let back = parse_line(line.trim().as_bytes()).expect("riga rileggibile");
        assert_eq!(back.id, s.id);
        assert_eq!(back.path, s.path);
        assert_eq!(back.title, s.title, "escape di virgolette e backslash");
        assert_eq!(back.size, s.size);
        assert_eq!(back.mtime_ms, s.mtime_ms);
        assert_eq!(back.corrections, s.corrections);
        assert_eq!(back.agent, s.agent);
        assert_eq!(back.kin_sketch, s.kin_sketch, "u64 esatti, non f64 JSON");
        assert_eq!(back.usage, s.usage, "i bucket per modello");
        assert_eq!(back.input_tokens, 1000);
        assert_eq!(back.output_tokens, 250);
    }

    #[test]
    fn the_name_carries_the_fingerprint_of_the_parsers() {
        let name = cache_name();
        let key = name
            .strip_prefix(PREFIX)
            .and_then(|r| r.strip_suffix(SUFFIX))
            .expect("nome nella forma attesa");
        assert_eq!(key.len(), 10, "chiave: {key}");
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()), "chiave: {key}");
    }

    #[test]
    fn save_clears_other_builds_caches_and_leaves_everything_else_alone() {
        // Ora che il nome segue il sorgente, senza pulizia una settimana di
        // lavoro sul parser lascerebbe un mucchietto di cache morte in
        // ~/.claude. La pulizia pero' e' dentro la cartella dell'utente:
        // questo test e' il recinto.
        let base = tmp();
        let decoys = [
            "phosphor.json",
            "settings.json",
            ".phosphor-cache.jsonl",   // prefisso+suffisso ma chiave vuota
            "phosphor-cache.v1.jsonl", // senza il punto iniziale
        ];
        for d in decoys {
            std::fs::write(base.join(d), b"non toccare").unwrap();
        }
        std::fs::write(base.join(".phosphor-cache.v9.jsonl"), b"vecchia").unwrap();
        std::fs::write(base.join(".phosphor-cache.0011223344.jsonl"), b"altra build").unwrap();
        // Una CARTELLA che si chiama come una cache: non e' un file, si salva.
        std::fs::create_dir(base.join(".phosphor-cache.aabbccddee.jsonl")).unwrap();

        save(&base, &[a_session()]);

        assert!(cache_path(&base).exists(), "la nostra cache c'e'");
        assert!(!base.join(".phosphor-cache.v9.jsonl").exists(), "la v9 va via");
        assert!(!base.join(".phosphor-cache.0011223344.jsonl").exists(), "l'altra build va via");
        assert!(base.join(".phosphor-cache.aabbccddee.jsonl").is_dir(), "la cartella resta");
        for d in decoys {
            assert!(base.join(d).exists(), "«{d}» non doveva essere toccato");
        }
        assert!(
            !base.join(".phosphor-cache.tmp").exists(),
            "il temporaneo se lo porta via il rename, non resta di mezzo"
        );
        // E quello che ha scritto si rilegge.
        let back = load(&base);
        assert_eq!(back.len(), 1);
        std::fs::remove_dir_all(&base).ok();
    }
}
