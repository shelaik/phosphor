//! Portable session bundle: export/import "totale" of Claude Code sessions
//! between PCs.
//!
//! A bundle is a SINGLE self-contained file (`.phx`) so it is trivial to move
//! (USB, cloud, e-mail). It carries, for each selected session, the main
//! transcript `<id>.jsonl` **and** its sidecar folder `<id>/` (sub-agent and
//! workflow transcripts) — i.e. "i transcript + relativi file/sottocartelle".
//!
//! Design goals (same as the rest of Phosphor):
//!   * Dependency-free: a tiny length-prefixed container, binary-safe, parsed by
//!     hand. No zip/tar crate.
//!   * Data-safe on import: path-traversal is rejected, and files are written
//!     with [`crate::write_new`] so an existing file is NEVER overwritten — an
//!     import can only adds new transcripts, never change or delete yours.
//!
//! Container layout (all integers are decimal ASCII):
//! ```text
//!   PHX1\n                                  magic
//!   M <manifest_len>\n<manifest_bytes>       JSON manifest
//!   F <rel_len> <content_len>\n<rel><content> (repeated, one per file)
//!   E\n                                      end marker
//! ```
//! `rel` is the path relative to `<base>/projects`, using `/` as separator.
//! Lengths are explicit so contents may be arbitrary bytes.

use crate::json::{escape, P};
use crate::scan::Session;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const MAGIC: &[u8] = b"PHX1\n";
/// Highest container version this build can READ. We bump this only for a
/// structural change and always keep reading every older version, so a bundle
/// created today stays importable forever (backward compatibility is a promise).
/// Additive metadata goes into the manifest JSON instead (unknown keys are
/// ignored on read), which never needs a version bump.
const FORMAT_VERSION: u64 = 1;
const MAX_FILES: usize = 200_000;
const MAX_REL: usize = 4096;

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

struct ManSession {
    id: String,
    project_path: String,
    project_name: String,
    title: String,
    files: usize,
    bytes: u64,
}

/// Path of `p` relative to `projects/`, as a `/`-joined string, or `None` if
/// `p` is not inside `projects/`.
fn rel_of(projects: &Path, p: &Path) -> Option<String> {
    let r = p.strip_prefix(projects).ok()?;
    let parts: Vec<String> = r
        .components()
        .filter_map(|c| c.as_os_str().to_str().map(|s| s.to_string()))
        .collect();
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

/// Recursively collect every file under `dir`.
fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for e in rd.flatten() {
        let path = e.path();
        match e.file_type() {
            Ok(ft) if ft.is_dir() => walk_files(&path, out),
            Ok(ft) if ft.is_file() => out.push(path),
            _ => {}
        }
    }
}

fn source_json() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default();
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default();
    format!(
        "{{\"os\":\"{}\",\"host\":\"{}\",\"user\":\"{}\"}}",
        escape(std::env::consts::OS),
        escape(&host),
        escape(&user)
    )
}

fn manifest_json(created: &str, sessions: &[ManSession]) -> String {
    let mut s = String::new();
    s.push_str("{\"format\":\"phosphor-bundle\",\"version\":1,");
    s.push_str(&format!("\"created\":\"{}\",", escape(created)));
    s.push_str(&format!("\"source\":{},", source_json()));
    s.push_str(&format!("\"count\":{},\"sessions\":[", sessions.len()));
    for (i, m) in sessions.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "{{\"id\":\"{}\",\"project_path\":\"{}\",\"project_name\":\"{}\",\"title\":\"{}\",\"files\":{},\"bytes\":{}}}",
            escape(&m.id),
            escape(&m.project_path),
            escape(&m.project_name),
            escape(&m.title),
            m.files,
            m.bytes
        ));
    }
    s.push_str("]}");
    s
}

/// Build a bundle of the given sessions. Reads each session's main `.jsonl`
/// transcript plus its `<id>/` sidecar folder (sub-agents/workflows), preserving
/// the `projects/<encoded>/…` layout. `created` is an opaque timestamp string
/// recorded in the manifest.
pub fn build(base: &Path, sessions: &[Session], created: &str) -> Vec<u8> {
    let projects = base.join("projects");
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut man: Vec<ManSession> = Vec::new();

    for s in sessions {
        // Gather this session's files (main transcript + sidecar folder), then
        // fold them into the dedup'd output list.
        let mut mine: Vec<(String, PathBuf)> = Vec::new();
        let main = PathBuf::from(&s.path);
        if let Some(rel) = rel_of(&projects, &main) {
            mine.push((rel, main.clone()));
        }
        // sidecar folder named exactly after the session id
        if let Some(parent) = main.parent() {
            let dir = parent.join(&s.id);
            if dir.is_dir() {
                let mut sub = Vec::new();
                walk_files(&dir, &mut sub);
                sub.sort();
                for f in sub {
                    if let Some(rel) = rel_of(&projects, &f) {
                        mine.push((rel, f));
                    }
                }
            }
        }
        let count = mine.len();
        let mut bytes = 0u64;
        for (rel, abs) in mine {
            bytes += std::fs::metadata(&abs).map(|m| m.len()).unwrap_or(0);
            if seen.insert(rel.clone()) {
                files.push((rel, abs));
            }
        }
        man.push(ManSession {
            id: s.id.clone(),
            project_path: s.project_path.clone(),
            project_name: s.project_name.clone(),
            title: s.title.clone(),
            files: count,
            bytes,
        });
    }

    let manifest = manifest_json(created, &man);
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(format!("M {}\n", manifest.len()).as_bytes());
    out.extend_from_slice(manifest.as_bytes());
    for (rel, abs) in &files {
        let content = std::fs::read(abs).unwrap_or_default();
        out.extend_from_slice(format!("F {} {}\n", rel.len(), content.len()).as_bytes());
        out.extend_from_slice(rel.as_bytes());
        out.extend_from_slice(&content);
    }
    out.extend_from_slice(b"E\n");
    out
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
pub struct Manifest {
    pub version: u64,
    pub created: String,
    pub source_os: String,
    pub source_host: String,
    pub source_user: String,
    pub count: u64,
    /// Distinct original working directories of the bundled sessions (from each
    /// session's `project_path`). Human-readable source paths shown at import
    /// time so the user knows what to remap onto this machine's layout.
    pub projects: Vec<String>,
}

/// One file inside the bundle, located by byte span (no content copy).
pub struct FileSpan {
    pub rel: String,
    pub off: usize,
    pub len: usize,
}

pub struct Parsed {
    pub manifest: Manifest,
    pub files: Vec<FileSpan>,
}

/// Read one `\n`-terminated header line starting at `pos`. Returns the line
/// (without the newline) and the position just past it.
fn read_line(data: &[u8], pos: usize) -> Option<(String, usize)> {
    let rest = data.get(pos..)?;
    let nl = rest.iter().position(|&b| b == b'\n')?;
    if nl > 256 {
        return None; // header lines are tiny; refuse a runaway
    }
    let line = String::from_utf8_lossy(&rest[..nl]).trim_end().to_string();
    Some((line, pos + nl + 1))
}

fn parse_manifest(bytes: &[u8]) -> Manifest {
    let mut m = Manifest::default();
    let mut p = P::new(bytes);
    if !p.obj_begin() {
        return m;
    }
    loop {
        let k = match p.obj_key() {
            Some(k) => k,
            None => break,
        };
        match k.as_str() {
            "version" => m.version = p.take_number() as u64,
            "created" => m.created = p.take_string().unwrap_or_default(),
            "count" => m.count = p.take_number() as u64,
            "sessions" => {
                // Collect the distinct source project_path values for the import UI.
                if p.arr_begin() {
                    loop {
                        if p.obj_begin() {
                            loop {
                                let sk = match p.obj_key() {
                                    Some(k) => k,
                                    None => break,
                                };
                                if sk == "project_path" {
                                    if let Some(v) = p.take_string() {
                                        if !v.is_empty() && !m.projects.contains(&v) {
                                            m.projects.push(v);
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
                }
            }
            "source" => {
                if p.obj_begin() {
                    loop {
                        let sk = match p.obj_key() {
                            Some(k) => k,
                            None => break,
                        };
                        match sk.as_str() {
                            "os" => m.source_os = p.take_string().unwrap_or_default(),
                            "host" => m.source_host = p.take_string().unwrap_or_default(),
                            "user" => m.source_user = p.take_string().unwrap_or_default(),
                            _ => {
                                let _ = p.skip();
                            }
                        }
                        if !p.obj_sep() {
                            break;
                        }
                    }
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
    m
}

/// Read the magic line "PHX<n>\n" and return (version, position past the line).
/// Version-agnostic: it locates the version by parsing the digits after "PHX",
/// so a longer magic (e.g. "PHX12\n") still works.
fn read_magic(data: &[u8]) -> Result<(u64, usize), String> {
    if !data.starts_with(b"PHX") {
        return Err("file non valido: non è un bundle Phosphor (.phx)".into());
    }
    let nl = data
        .iter()
        .position(|&b| b == b'\n')
        .filter(|&n| n > 3 && n <= 16)
        .ok_or("file non valido: non è un bundle Phosphor (.phx)")?;
    let version: u64 = std::str::from_utf8(&data[3..nl])
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .ok_or("file non valido: non è un bundle Phosphor (.phx)")?;
    if version == 0 || version > FORMAT_VERSION {
        return Err(format!(
            "versione bundle non supportata: PHX{version} (aggiorna Phosphor per leggerlo)"
        ));
    }
    Ok((version, nl + 1))
}

/// Parse a bundle's structure (magic, manifest, file spans) without copying
/// file contents. Returns a human-readable error string on a malformed file.
pub fn inspect(data: &[u8]) -> Result<Parsed, String> {
    // Dispatch on the magic version "PHX<n>\n". Today only v1 exists, but
    // resolving the version explicitly means a future PHX2 reader can branch
    // here, and an unknown/newer version gets a clear message instead of a
    // generic "not a bundle" error.
    let (version, mut pos) = read_magic(data)?;
    let _ = version; // single layout for now; future: match version { .. }
    let mut manifest: Option<Manifest> = None;
    let mut files: Vec<FileSpan> = Vec::new();
    loop {
        let (line, np) = read_line(data, pos).ok_or("bundle troncato (intestazione)")?;
        pos = np;
        let mut it = line.split(' ');
        match it.next() {
            Some("M") => {
                let len: usize = it
                    .next()
                    .and_then(|x| x.parse().ok())
                    .ok_or("lunghezza manifest non valida")?;
                let end = pos
                    .checked_add(len)
                    .filter(|&e| e <= data.len())
                    .ok_or("manifest troncato")?;
                manifest = Some(parse_manifest(&data[pos..end]));
                pos = end;
            }
            Some("F") => {
                let rlen: usize = it
                    .next()
                    .and_then(|x| x.parse().ok())
                    .ok_or("lunghezza percorso non valida")?;
                let clen: usize = it
                    .next()
                    .and_then(|x| x.parse().ok())
                    .ok_or("lunghezza contenuto non valida")?;
                if rlen > MAX_REL {
                    return Err("percorso troppo lungo nel bundle".into());
                }
                let rend = pos
                    .checked_add(rlen)
                    .filter(|&e| e <= data.len())
                    .ok_or("percorso troncato")?;
                let rel = String::from_utf8_lossy(&data[pos..rend]).to_string();
                pos = rend;
                let cend = pos
                    .checked_add(clen)
                    .filter(|&e| e <= data.len())
                    .ok_or("contenuto troncato")?;
                files.push(FileSpan { rel, off: pos, len: clen });
                pos = cend;
                if files.len() > MAX_FILES {
                    return Err("troppi file nel bundle".into());
                }
            }
            Some("E") => break,
            _ => return Err("record sconosciuto nel bundle".into()),
        }
    }
    let manifest = manifest.ok_or("manifest mancante nel bundle")?;
    Ok(Parsed { manifest, files })
}

/// Validate a bundle-relative path and split it into safe components.
/// Rejects anything that could escape `projects/`: empty, `.`/`..`, absolute,
/// backslash, drive-colon, or NUL.
pub fn safe_rel(rel: &str) -> Option<Vec<String>> {
    if rel.is_empty() || rel.len() > MAX_REL {
        return None;
    }
    let mut parts = Vec::new();
    for comp in rel.split('/') {
        if comp.is_empty() || comp == "." || comp == ".." {
            return None;
        }
        if comp.contains('\\') || comp.contains('\0') || comp.contains(':') {
            return None;
        }
        parts.push(comp.to_string());
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts)
}

#[derive(Default)]
pub struct Report {
    pub added: usize,
    pub skipped: usize,  // already present — left untouched
    pub rejected: usize, // unsafe path — refused
    pub bytes: u64,
}

/// Apply (or, with `dry == true`, just plan) an import into `<base>/projects`.
///
/// Guarantees: never overwrites (existing files are skipped via [`crate::write_new`]),
/// never escapes `projects/` (unsafe paths are rejected). With `dry == true`
/// nothing is written — it only computes how many files would be added/skipped/
/// rejected, for the confirmation prompt.
pub fn apply(base: &Path, data: &[u8], parsed: &Parsed, dry: bool) -> Report {
    apply_remapped(base, data, parsed, dry, &[])
}

/// Like [`apply`], but relocates sessions whose original cwd matches a remap.
/// `remaps` is a list of `(source_cwd, target_cwd)`: any bundled file whose
/// leading `<encoded-cwd>` folder equals `encode_cwd(source_cwd)` is written
/// under `encode_cwd(target_cwd)` instead — so `claude --resume` finds it when
/// run in `target_cwd` on THIS machine. The transcript bytes are untouched
/// (read-only); only the destination folder changes. All the same safety
/// guarantees hold (never overwrite, never escape `projects/`).
pub fn apply_remapped(base: &Path, data: &[u8], parsed: &Parsed, dry: bool, remaps: &[(String, String)]) -> Report {
    let projects = base.join("projects");
    // Precompute folder renames: encode(source) -> encode(target). Both encoded
    // names are alphanumeric-or-`-`, so they are always single safe components.
    let renames: Vec<(String, String)> = remaps
        .iter()
        .filter(|(s, t)| !s.is_empty() && !t.is_empty())
        .map(|(s, t)| (crate::encode_cwd(s), crate::encode_cwd(t)))
        .filter(|(o, n)| !o.is_empty() && !n.is_empty() && o != n)
        .collect();
    let mut rep = Report::default();
    for f in &parsed.files {
        let mut parts = match safe_rel(&f.rel) {
            Some(p) => p,
            None => {
                rep.rejected += 1;
                continue;
            }
        };
        // Remap the leading encoded-cwd folder if it matches a source. The new
        // component comes from encode_cwd (alnum + `-`), so it stays safe.
        if let Some(new) = renames.iter().find(|(old, _)| *old == parts[0]).map(|(_, n)| n.clone()) {
            parts[0] = new;
        }
        let mut target = projects.clone();
        for p in &parts {
            target.push(p);
        }
        if target.exists() {
            rep.skipped += 1;
            continue;
        }
        if dry {
            rep.added += 1;
            rep.bytes += f.len as u64;
            continue;
        }
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let content = match data.get(f.off..f.off + f.len) {
            Some(c) => c,
            None => {
                rep.rejected += 1;
                continue;
            }
        };
        if crate::write_new(&target, content).is_ok() {
            rep.added += 1;
            rep.bytes += f.len as u64;
        } else {
            // create_new failed: the file appeared meanwhile — treat as skipped,
            // never overwrite.
            rep.skipped += 1;
        }
    }
    rep
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_rel_rejects_traversal() {
        assert!(safe_rel("../evil").is_none());
        assert!(safe_rel("a/../b").is_none());
        assert!(safe_rel("/etc/passwd").is_none());
        assert!(safe_rel("C:/Windows").is_none()); // drive colon
        assert!(safe_rel("a\\b").is_none()); // backslash component
        assert!(safe_rel("").is_none());
        // legitimate layout passes
        assert_eq!(
            safe_rel("proj-dir/uuid.jsonl"),
            Some(vec!["proj-dir".into(), "uuid.jsonl".into()])
        );
        assert_eq!(
            safe_rel("proj/uuid/subagents/a.jsonl").unwrap().len(),
            4
        );
    }

    #[test]
    fn build_inspect_apply_roundtrip() {
        // unique temp base for this test process
        let root = std::env::temp_dir().join(format!("phx-test-{}", std::process::id()));
        let src = root.join("src");
        let dst = root.join("dst");
        let _ = std::fs::remove_dir_all(&root);
        let enc = "C--proj";
        let id = "11111111-2222-3333-4444-555555555555";
        let sess_dir = src.join("projects").join(enc);
        std::fs::create_dir_all(sess_dir.join(id).join("subagents")).unwrap();
        std::fs::write(sess_dir.join(format!("{id}.jsonl")), b"{\"main\":1}\n").unwrap();
        std::fs::write(
            sess_dir.join(id).join("subagents").join("a.jsonl"),
            b"{\"sub\":1}\n",
        )
        .unwrap();

        let mut s = Session::default();
        s.id = id.into();
        s.path = sess_dir.join(format!("{id}.jsonl")).to_string_lossy().to_string();
        s.project_name = "proj".into();

        let bytes = build(&src, &[s], "2026-06-21T00:00:00");
        let parsed = inspect(&bytes).expect("valid bundle");
        assert_eq!(parsed.manifest.count, 1);
        assert_eq!(parsed.files.len(), 2); // main + 1 sidecar file

        // dry run plans 2 additions, writes nothing
        let plan = apply(&dst, &bytes, &parsed, true);
        assert_eq!(plan.added, 2);
        assert!(!dst.join("projects").exists());

        // real import writes both files under dst/projects, preserving layout
        let rep = apply(&dst, &bytes, &parsed, false);
        assert_eq!(rep.added, 2);
        assert!(dst.join("projects").join(enc).join(format!("{id}.jsonl")).exists());
        assert!(dst
            .join("projects")
            .join(enc)
            .join(id)
            .join("subagents")
            .join("a.jsonl")
            .exists());

        // second import is idempotent: everything already present, nothing overwritten
        let again = apply(&dst, &bytes, &parsed, false);
        assert_eq!(again.added, 0);
        assert_eq!(again.skipped, 2);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_remapped_relocates_to_target_folder() {
        let root = std::env::temp_dir().join(format!("phx-remap-{}", std::process::id()));
        let dst = root.join("dst");
        let _ = std::fs::remove_dir_all(&root);
        // Source bundle: session recorded under C:\src\proj  (encoded folder).
        let src = root.join("src");
        let src_cwd = "C:\\src\\proj";
        let enc_src = crate::encode_cwd(src_cwd); // C--src-proj
        let id = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
        let sess_dir = src.join("projects").join(&enc_src);
        std::fs::create_dir_all(sess_dir.join(id)).unwrap();
        std::fs::write(sess_dir.join(format!("{id}.jsonl")), b"{\"cwd\":\"C:\\\\src\\\\proj\"}\n").unwrap();
        std::fs::write(sess_dir.join(id).join("sub.jsonl"), b"{}\n").unwrap();

        let mut s = Session::default();
        s.id = id.into();
        s.path = sess_dir.join(format!("{id}.jsonl")).to_string_lossy().to_string();
        s.project_path = src_cwd.into();
        let bytes = build(&src, &[s], "2026-07-08T00:00:00");
        let parsed = inspect(&bytes).unwrap();
        // manifest surfaces the original project path for the import UI
        assert_eq!(parsed.manifest.projects, vec![src_cwd.to_string()]);

        // Import remapping the project onto D:\work\proj on this machine.
        let target_cwd = "D:\\work\\proj";
        let enc_dst = crate::encode_cwd(target_cwd); // D--work-proj
        let remaps = vec![(src_cwd.to_string(), target_cwd.to_string())];
        let rep = apply_remapped(&dst, &bytes, &parsed, false, &remaps);
        assert_eq!(rep.added, 2);
        // Files landed under the TARGET encoded folder, not the source one.
        assert!(dst.join("projects").join(&enc_dst).join(format!("{id}.jsonl")).exists());
        assert!(dst.join("projects").join(&enc_dst).join(id).join("sub.jsonl").exists());
        assert!(!dst.join("projects").join(&enc_src).exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn magic_version_dispatch() {
        // current version reads fine
        assert_eq!(read_magic(b"PHX1\nM 2\n{}E\n").unwrap().0, 1);
        // a newer/unknown version is refused with a clear message, not "not a bundle"
        let e = read_magic(b"PHX2\nrest").unwrap_err();
        assert!(e.contains("non supportata"), "got: {e}");
        // non-bundle and version 0 are rejected
        assert!(read_magic(b"NOPE\n").is_err());
        assert!(read_magic(b"PHX0\n").is_err());
        // a real bundle still inspects (regression guard for the new dispatch path)
        let bytes = build(
            &std::env::temp_dir().join("phx-nope-does-not-exist"),
            &[],
            "2026-06-24T00:00:00",
        );
        assert!(inspect(&bytes).is_ok());
    }
}
