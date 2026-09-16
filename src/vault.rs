//! Keep transcripts alive after something deletes them — for **zero extra
//! bytes**.
//!
//! Every agent prunes its own store, and tools that hunt for big folders find
//! these irresistible. Claude Code unlinks any transcript older than
//! `cleanupPeriodDays` (30 by default, see [`crate::recover`]); on this machine
//! a whole `~/.codex` and several `.git/objects` went the same way in a week.
//!
//! The vault answers that with a **hard link**, not a copy. A second name for
//! the same bytes on the same volume costs nothing while the original is there,
//! and when the original is unlinked the vault link simply becomes the file's
//! only name — the data never moves and never doubles. The vault therefore
//! grows by exactly what would otherwise have been lost, and by nothing else.
//!
//! ```text
//! <base>/phosphor-vault/claude/<encoded-project>/<id>.jsonl
//! <base>/phosphor-vault/codex/<YYYY>/<MM>/<DD>/rollout-<…>.jsonl
//! ```
//!
//! The layout mirrors each store below its root, so restoring is the same link
//! made in the opposite direction: [`restore`] puts the transcript back where
//! its agent expects it and `claude --resume` / `codex resume` find it again.
//!
//! Opt-in. Phosphor is read-only by default and this is the one feature that
//! writes into the stores' neighbourhood, so it stays off until `"vault": true`
//! is set in `phosphor.json` — nothing is linked, read or created before that.

use crate::scan::Session;
use std::collections::HashMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Folder name under `<base>`; also the marker that tells a vaulted session
/// from one still sitting in its agent's store (see `Session::is_vaulted`).
pub const DIR: &str = "phosphor-vault";

/// Per-agent subdirectory. Kept separate so a restore knows which store — and
/// which path shape — a transcript came from.
const CLAUDE: &str = "claude";
const CODEX: &str = "codex";

pub fn dir(base: &Path) -> PathBuf {
    base.join(DIR)
}

/// Where `path` (a transcript inside its agent's store) is mirrored.
/// `None` when the path is not inside a store this function understands, which
/// is also what keeps a vault file from being re-vaulted into itself.
fn vault_path(base: &Path, codex_home: Option<&Path>, s: &Session) -> Option<PathBuf> {
    if s.is_ghost() || s.is_vaulted() || !s.host.is_empty() {
        return None;
    }
    let p = Path::new(&s.path);
    if s.is_codex() {
        let root = codex_home?.join("sessions");
        let rel = p.strip_prefix(&root).ok()?;
        Some(dir(base).join(CODEX).join(rel))
    } else {
        let root = base.join("projects");
        let rel = p.strip_prefix(&root).ok()?;
        // Only the top-level transcript, never the subagents/ sidecars: those
        // are re-derivable noise and would multiply the vault's file count.
        if rel.components().count() != 2 {
            return None;
        }
        Some(dir(base).join(CLAUDE).join(rel))
    }
}

/// Reverse of [`vault_path`]: where a vault file belongs in its agent's store.
fn origin_path(base: &Path, codex_home: Option<&Path>, vaulted: &Path) -> Option<PathBuf> {
    let v = dir(base);
    let rel = vaulted.strip_prefix(&v).ok()?;
    // Rust non normalizza i percorsi: un `..` sopravvive a strip_prefix e a
    // join, quindi un `path` costruito ad arte — una riga arrivata da un
    // bundle importato, o una cache modificata a mano — potrebbe far atterrare
    // il ripristino FUORI dallo store dell'agente. Qui si accettano solo nomi
    // veri: niente risalite, niente radici, niente prefissi di volume.
    if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
        return None;
    }
    let mut it = rel.components();
    let agent = it.next()?.as_os_str().to_str()?.to_string();
    let rest: PathBuf = it.collect();
    match agent.as_str() {
        CLAUDE => Some(base.join("projects").join(rest)),
        CODEX => Some(codex_home?.join("sessions").join(rest)),
        _ => None,
    }
}

/// Link every transcript in `sessions` that is not in the vault yet. Returns
/// `(linked, failed)`.
///
/// A failure is almost always "different volume" (a `CODEX_HOME` on another
/// drive): hard links cannot cross volumes, and Phosphor will NOT silently fall
/// back to copying — that would double the bytes the whole design exists to
/// avoid. The count is surfaced instead so the user can decide.
pub fn link_all(base: &Path, sessions: &[Session]) -> (u64, u64) {
    let codex_home = crate::codex::home();
    let (mut linked, mut failed) = (0u64, 0u64);
    for s in sessions {
        let dest = match vault_path(base, codex_home.as_deref(), s) {
            Some(d) => d,
            None => continue,
        };
        if dest.exists() {
            continue;
        }
        if let Some(parent) = dest.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                failed += 1;
                continue;
            }
        }
        match std::fs::hard_link(&s.path, &dest) {
            Ok(()) => linked += 1,
            // Someone won the race, or the file went away mid-scan: neither is
            // an error worth reporting.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(_) => failed += 1,
        }
    }
    (linked, failed)
}

/// A vault file, with the store path it mirrors.
struct Entry {
    path: PathBuf,
    origin: PathBuf,
    size: u64,
    mtime: u64,
    codex: bool,
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 8 {
        return;
    }
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for e in rd.flatten() {
        let p = e.path();
        match e.file_type() {
            Ok(ft) if ft.is_dir() => walk(&p, out, depth + 1),
            Ok(_) if p.extension().and_then(|x| x.to_str()) == Some("jsonl") => out.push(p),
            _ => {}
        }
    }
}

fn entries(base: &Path, codex_home: Option<&Path>) -> Vec<Entry> {
    let mut files = Vec::new();
    walk(&dir(base), &mut files, 0);
    files
        .into_iter()
        .filter_map(|path| {
            let origin = origin_path(base, codex_home, &path)?;
            let md = std::fs::metadata(&path).ok()?;
            let codex = path.strip_prefix(dir(base)).ok()?.starts_with(CODEX);
            Some(Entry {
                size: md.len(),
                mtime: md
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0),
                path,
                origin,
                codex,
            })
        })
        .collect()
}

/// What the vault holds: `(files, bytes, orphans, orphan_bytes)`.
///
/// `orphans` are the transcripts whose original is gone — the ones the vault
/// actually saved. `bytes` counts every vault file, but only the orphan bytes
/// are storage the user would not otherwise be paying: the rest is shared with
/// the live transcript and costs nothing.
pub fn stats(base: &Path) -> (u64, u64, u64, u64) {
    let codex_home = crate::codex::home();
    let (mut n, mut bytes, mut orphans, mut obytes) = (0, 0, 0, 0);
    for e in entries(base, codex_home.as_deref()) {
        n += 1;
        bytes += e.size;
        if !e.origin.exists() {
            orphans += 1;
            obytes += e.size;
        }
    }
    (n, bytes, orphans, obytes)
}

/// Sessions that exist ONLY in the vault, parsed in full — tokens, tools, the
/// conversation, everything the transcript still holds. This is the payoff: a
/// session Claude Code deleted stays a first-class row instead of decaying into
/// a prompts-only ghost.
///
/// Incremental like the other scanners, sharing their cache map and evicting
/// only its own rows.
pub fn scan_incremental(base: &Path, cache: &mut HashMap<String, Session>) -> (Vec<Session>, bool) {
    let codex_home = crate::codex::home();
    let all = entries(base, codex_home.as_deref());
    if all.is_empty() {
        return (Vec::new(), false);
    }
    let mut titles: Option<HashMap<String, String>> = None;
    let mut out = Vec::new();
    let mut valid = std::collections::HashSet::new();
    let mut changed = false;
    for e in all {
        // Still in its store: the normal scan owns that row, and listing it
        // here would double it.
        if e.origin.exists() {
            continue;
        }
        let key = e.path.to_string_lossy().to_string();
        valid.insert(key.clone());
        if let Some(c) = cache.get(&key) {
            if c.size == e.size && c.mtime_ms == e.mtime && c.is_vaulted() {
                out.push(c.clone());
                continue;
            }
        }
        let parsed = if e.codex {
            let t = titles.get_or_insert_with(|| {
                codex_home
                    .as_deref()
                    .map(crate::codex::titles)
                    .unwrap_or_default()
            });
            crate::codex::parse_one(&e.path, e.size, e.mtime, t)
        } else {
            crate::scan::parse_one(&e.path, e.size)
        };
        let mut s = match parsed {
            Some(s) => s,
            None => continue,
        };
        // The parser derives the project from the transcript's own fields, so
        // the row keeps naming the project it belonged to, not the vault.
        s.path = key.clone();
        changed = true;
        cache.insert(key, s.clone());
        out.push(s);
    }
    let before = cache.len();
    cache.retain(|k, v| !v.is_vaulted() || valid.contains(k));
    if cache.len() != before {
        changed = true;
    }
    out.sort_by(|a, b| b.mtime_ms.cmp(&a.mtime_ms));
    (out, changed)
}

/// Put a vaulted transcript back where its agent looks for it, so the session
/// becomes resumable again. Another hard link, so nothing is copied and the
/// vault keeps its own.
///
/// Never overwrites: if something already sits at the destination, that file is
/// the live transcript and the vault copy is the stale one.
pub fn restore(base: &Path, s: &Session) -> io::Result<PathBuf> {
    if !s.is_vaulted() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "non e' una sessione del vault",
        ));
    }
    let codex_home = crate::codex::home();
    let src = PathBuf::from(&s.path);
    let dest = origin_path(base, codex_home.as_deref(), &src).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "percorso fuori dal vault")
    })?;
    if dest.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "il transcript e' gia' al suo posto",
        ));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::hard_link(&src, &dest)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    use super::*;

    fn tmp() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "phosphor-vault-{}-{}",
            std::process::id(),
            // Il solo orologio non basta: su Windows ha una risoluzione di ~15 ms
            // e i test girano in parallelo, quindi due cartelle possono nascere
            // con lo stesso nome e cancellarsi a vicenda a meta' corsa.
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// A minimal but real Claude transcript: one user line, one assistant line.
    fn write_transcript(dir: &Path, name: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join(name);
        std::fs::write(
            &p,
            concat!(
                r#"{"type":"user","sessionId":"11111111-2222-3333-4444-555555555555","cwd":"C:\\proj\\demo","timestamp":"2026-09-14T10:00:00.000Z","message":{"role":"user","content":"ciao come va"}}"#,
                "\n",
                r#"{"type":"assistant","sessionId":"11111111-2222-3333-4444-555555555555","timestamp":"2026-09-14T10:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"bene"}],"usage":{"input_tokens":10,"output_tokens":3}}}"#,
                "\n",
            ),
        )
        .unwrap();
        p
    }

    #[test]
    fn a_link_costs_nothing_and_outlives_the_original() {
        let base = tmp();
        let proj = base.join("projects").join("C--proj-demo");
        let orig = write_transcript(&proj, "11111111-2222-3333-4444-555555555555.jsonl");

        let mut s = crate::scan::parse_one(&orig, std::fs::metadata(&orig).unwrap().len()).unwrap();
        s.path = orig.to_string_lossy().into_owned();
        let (linked, failed) = link_all(&base, std::slice::from_ref(&s));
        assert_eq!((linked, failed), (1, 0));

        let vaulted = dir(&base)
            .join("claude")
            .join("C--proj-demo")
            .join("11111111-2222-3333-4444-555555555555.jsonl");
        assert!(vaulted.exists());
        // while the original is alive the vault adds no session: the normal scan
        // already owns that row
        let mut cache = HashMap::new();
        assert!(scan_incremental(&base, &mut cache).0.is_empty());
        // …and the bytes are shared, not doubled
        let (n, _, orphans, obytes) = stats(&base);
        assert_eq!((n, orphans, obytes), (1, 0, 0));

        // now the retention (or a disk cleaner) takes the original
        std::fs::remove_file(&orig).unwrap();
        let (v, changed) = scan_incremental(&base, &mut cache);
        assert!(changed);
        assert_eq!(v.len(), 1, "la sessione sopravvive alla cancellazione");
        assert!(v[0].is_vaulted());
        // and it is a FULL session, not a prompts-only ghost
        assert!(!v[0].is_ghost());
        assert_eq!(v[0].project_name, "demo");
        assert_eq!(v[0].input_tokens, 10);
        assert_eq!(v[0].output_tokens, 3);
        assert_eq!(stats(&base).2, 1, "ora e' un orfano: quei byte li paga il vault");

        // a second pass reuses the cache instead of re-parsing
        assert!(!scan_incremental(&base, &mut cache).1);

        // restore puts it back where claude --resume looks
        let back = restore(&base, &v[0]).unwrap();
        assert_eq!(back, orig);
        assert!(orig.exists());
        assert!(vaulted.exists(), "il vault tiene comunque il suo nome");
        // …and with the original back, the vault stops listing it
        assert!(scan_incremental(&base, &mut cache).0.is_empty());
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn a_crafted_path_cannot_make_a_restore_land_outside_the_store() {
        // Rust non normalizza i percorsi: un `..` sopravvive a strip_prefix e a
        // join. Una riga con un path costruito ad arte — da un bundle
        // importato, o da una cache modificata a mano — avrebbe potuto far
        // scrivere il ripristino fuori dallo store dell'agente.
        let base = Path::new("C:/u/.claude");
        let codex = PathBuf::from("C:/u/.codex");
        let v = dir(base);

        // Il caso buono continua a funzionare.
        let ok = v.join("claude").join("C--p").join("aaa.jsonl");
        assert_eq!(
            origin_path(base, Some(&codex), &ok),
            Some(base.join("projects").join("C--p").join("aaa.jsonl"))
        );

        // Le risalite vengono rifiutate, non normalizzate.
        for evil in [
            v.join("claude").join("..").join("..").join("evil.jsonl"),
            v.join("codex").join("..").join("evil.jsonl"),
            v.join("claude").join("sub").join("..").join("..").join("evil.jsonl"),
        ] {
            assert_eq!(origin_path(base, Some(&codex), &evil), None, "«{}»", evil.display());
        }

        // E un agente che non conosciamo non porta da nessuna parte.
        let unknown = v.join("altro").join("x.jsonl");
        assert_eq!(origin_path(base, Some(&codex), &unknown), None);
    }

    #[test]
    fn restore_never_overwrites_a_live_transcript() {
        let base = tmp();
        let proj = base.join("projects").join("C--proj-demo");
        let orig = write_transcript(&proj, "11111111-2222-3333-4444-555555555555.jsonl");
        let mut s = crate::scan::parse_one(&orig, std::fs::metadata(&orig).unwrap().len()).unwrap();
        s.path = orig.to_string_lossy().into_owned();
        link_all(&base, std::slice::from_ref(&s));
        let mut vaulted = s.clone();
        vaulted.path = dir(&base)
            .join("claude")
            .join("C--proj-demo")
            .join("11111111-2222-3333-4444-555555555555.jsonl")
            .to_string_lossy()
            .into_owned();
        assert!(vaulted.is_vaulted());
        let err = restore(&base, &vaulted).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn sidecars_and_foreign_rows_are_left_alone() {
        let base = tmp();
        let proj = base.join("projects").join("C--proj-demo");
        let id = "11111111-2222-3333-4444-555555555555";
        let orig = write_transcript(&proj, &format!("{id}.jsonl"));
        // a subagent sidecar: re-derivable, never vaulted
        let side = write_transcript(&proj.join(id).join("subagents"), "agent-a1.jsonl");

        let mk = |p: &Path| {
            let mut s = crate::scan::parse_one(p, std::fs::metadata(p).unwrap().len()).unwrap();
            s.path = p.to_string_lossy().into_owned();
            s
        };
        let mut remote = mk(&orig);
        remote.host = "pc-casa".into(); // lives on another machine: no local file
        let mut ghost = mk(&orig);
        ghost.path = format!("{}{}", proj.to_string_lossy(), "\\rec-0.recovered");

        let (linked, failed) = link_all(&base, &[mk(&orig), mk(&side), remote, ghost]);
        assert_eq!((linked, failed), (1, 0), "solo il transcript principale");
        assert_eq!(stats(&base).0, 1);
        std::fs::remove_dir_all(&base).ok();
    }
}
