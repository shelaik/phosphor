//! Read and raise Claude Code's own retention, `cleanupPeriodDays`.
//!
//! Claude Code unlinks every transcript older than that many days on startup —
//! **30 by default**, with no recycle bin and no backup. It is the single
//! setting that decides whether Phosphor has a history to show at all, it lives
//! in someone else's config file, and nothing announces it: a project untouched
//! for a month is simply gone, and by the time you notice, it is not coming
//! back. So Phosphor reads it, says so, and offers to raise it.
//!
//! Codex has no equivalent. Its rollouts are filed by date and never pruned on
//! a timer — verified against a real store whose oldest thread was six months
//! old and intact. There is nothing to set on that side; what protects it is
//! [`crate::vault`].
//!
//! Writing here means editing a file Phosphor does not own, so `set` is
//! deliberately narrow: it changes ONE number, leaves every other key and even
//! the formatting untouched, refuses to write anything that does not parse
//! afterwards, keeps a `.bak` of the previous content, and replaces the file
//! atomically.

use crate::json::P;
use std::io;
use std::path::{Path, PathBuf};

/// What Claude Code assumes when the key is absent. The number that quietly
/// deletes a month-old project.
pub const DEFAULT_DAYS: u64 = 30;

/// Suggested value: ten years, i.e. "stop deleting my history".
pub const RECOMMENDED_DAYS: u64 = 3650;

/// Below this, the setting is worth warning about. A year is already long
/// enough that nobody loses work by surprise.
pub const AT_RISK_BELOW: u64 = 365;

/// Upper bound accepted by [`set`]: beyond this the number stops meaning
/// anything and starts looking like a typo.
const MAX_DAYS: u64 = 100_000;

pub fn settings_path(base: &Path) -> PathBuf {
    base.join("settings.json")
}

/// The configured value, or `None` when the key is absent (which means Claude
/// Code applies [`DEFAULT_DAYS`]).
pub fn configured(base: &Path) -> Option<u64> {
    let data = std::fs::read(settings_path(base)).ok()?;
    let mut p = P::new(&data);
    if !p.obj_begin() {
        return None;
    }
    let mut found = None;
    loop {
        let k = match p.obj_key() {
            Some(k) => k,
            None => break,
        };
        if k == "cleanupPeriodDays" {
            let n = p.take_number();
            if n >= 0.0 {
                found = Some(n as u64);
            }
        } else {
            let _ = p.skip();
        }
        if !p.obj_sep() {
            break;
        }
    }
    found
}

/// The number of days actually in force.
pub fn effective(base: &Path) -> u64 {
    configured(base).unwrap_or(DEFAULT_DAYS)
}

/// True when history is being deleted sooner than a user would expect.
pub fn at_risk(base: &Path) -> bool {
    effective(base) < AT_RISK_BELOW
}

/// A one-line summary for the status bar and the CLI.
pub fn summary(base: &Path) -> String {
    match configured(base) {
        None => format!(
            "Claude Code cancella i transcript dopo {DEFAULT_DAYS} giorni (impostazione assente = default)"
        ),
        Some(d) if d < AT_RISK_BELOW => {
            format!("Claude Code cancella i transcript dopo {d} giorni")
        }
        Some(d) => format!("Claude Code conserva i transcript {d} giorni"),
    }
}

/// Set `cleanupPeriodDays` to `days` in `<base>/settings.json`.
///
/// Surgical on purpose: the file belongs to Claude Code and holds the user's
/// own settings, so this rewrites the one number (or inserts the one key) and
/// touches nothing else — no reformatting, no reordering, no dropping of keys a
/// future Claude Code version may add and this parser would not understand.
/// The result must still parse as an object, or nothing is written.
pub fn set(base: &Path, days: u64) -> io::Result<PathBuf> {
    if days == 0 || days > MAX_DAYS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "valore fuori scala",
        ));
    }
    let path = settings_path(base);
    let original = std::fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_string());
    let updated = splice(&original, days).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "settings.json non e' un oggetto JSON riconoscibile: non lo tocco",
        )
    })?;
    // Never hand back something we cannot re-read: a settings.json Claude Code
    // chokes on is worse than a short retention.
    if !parses_as_object(&updated) || configured_in(&updated) != Some(days) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "la modifica non si rilegge: annullata",
        ));
    }
    if path.exists() {
        let _ = std::fs::copy(&path, path.with_extension("json.phosphor-bak"));
    }
    let tmp = path.with_extension("json.phosphor-tmp");
    std::fs::write(&tmp, updated.as_bytes())?;
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

fn parses_as_object(s: &str) -> bool {
    let mut p = P::new(s.as_bytes());
    p.peek_ws() == b'{' && p.skip().is_some()
}

fn configured_in(s: &str) -> Option<u64> {
    let mut p = P::new(s.as_bytes());
    if !p.obj_begin() {
        return None;
    }
    loop {
        let k = p.obj_key()?;
        if k == "cleanupPeriodDays" {
            return Some(p.take_number() as u64);
        }
        let _ = p.skip();
        if !p.obj_sep() {
            return None;
        }
    }
}

/// Replace the value of `cleanupPeriodDays`, or insert the key after the
/// opening brace. Returns `None` when the text is not a JSON object.
fn splice(src: &str, days: u64) -> Option<String> {
    let b = src.as_bytes();
    if let Some((val_start, val_end)) = find_value_span(src) {
        let mut out = String::with_capacity(src.len() + 8);
        out.push_str(&src[..val_start]);
        out.push_str(&days.to_string());
        out.push_str(&src[val_end..]);
        return Some(out);
    }
    // Not present: insert right after the opening brace, keeping the file's own
    // two-space indentation convention.
    let open = b.iter().position(|&c| c == b'{')?;
    let rest = src[open + 1..].trim_start();
    let mut out = String::with_capacity(src.len() + 40);
    out.push_str(&src[..=open]);
    out.push_str(&format!("\n  \"cleanupPeriodDays\": {days}"));
    if !rest.starts_with('}') {
        out.push(',');
    }
    out.push_str(&src[open + 1..]);
    Some(out)
}

/// Byte span of the NUMBER that follows a top-level `"cleanupPeriodDays":`.
/// Scans with the real JSON parser rather than by text search, so a key of the
/// same name nested inside another object — or mentioned inside a string — is
/// never mistaken for this one.
fn find_value_span(src: &str) -> Option<(usize, usize)> {
    let b = src.as_bytes();
    let mut p = P::new(b);
    if !p.obj_begin() {
        return None;
    }
    loop {
        let k = p.obj_key()?;
        if k == "cleanupPeriodDays" {
            let start = p.i;
            let _ = p.skip();
            return Some((start, p.i));
        }
        let _ = p.skip();
        if !p.obj_sep() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    use super::*;

    fn tmp() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "phosphor-ret-{}-{}",
            std::process::id(),
            // Il solo orologio non basta: su Windows ha una risoluzione di ~15 ms
            // e i test girano in parallelo, quindi due cartelle possono nascere
            // con lo stesso nome e cancellarsi a vicenda a meta' corsa.
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn absent_means_the_thirty_day_default() {
        let base = tmp();
        std::fs::write(settings_path(&base), "{\n  \"model\": \"opus\"\n}\n").unwrap();
        assert_eq!(configured(&base), None);
        assert_eq!(effective(&base), 30);
        assert!(at_risk(&base));
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn raising_it_keeps_every_other_setting_byte_for_byte() {
        let base = tmp();
        // Deliberately messy: odd spacing, a nested object, and a key this
        // parser has never heard of — all of it must survive.
        let before = "{\n  \"model\": \"opus[1m]\",\n  \"cleanupPeriodDays\":   30,\n  \"enabledPlugins\": { \"x@y\": true },\n  \"chiaveFutura\": [1, 2, 3]\n}\n";
        std::fs::write(settings_path(&base), before).unwrap();
        set(&base, 3650).unwrap();
        let after = std::fs::read_to_string(settings_path(&base)).unwrap();
        assert_eq!(configured(&base), Some(3650));
        assert_eq!(after, before.replace("  30,", "  3650,"));
        // and the previous content is kept alongside
        assert_eq!(
            std::fs::read_to_string(settings_path(&base).with_extension("json.phosphor-bak"))
                .unwrap(),
            before
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn inserting_the_key_when_it_is_missing() {
        let base = tmp();
        std::fs::write(settings_path(&base), "{\n  \"model\": \"opus\"\n}\n").unwrap();
        set(&base, 365).unwrap();
        let after = std::fs::read_to_string(settings_path(&base)).unwrap();
        assert_eq!(configured(&base), Some(365));
        assert!(after.contains("\"model\": \"opus\""), "l'altra chiave resta");
        // …and into an empty object, without a dangling comma
        std::fs::write(settings_path(&base), "{}").unwrap();
        set(&base, 365).unwrap();
        assert_eq!(configured(&base), Some(365));
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn a_nested_key_of_the_same_name_is_not_touched() {
        let base = tmp();
        // The top-level key is absent; the nested one must NOT be taken for it.
        let before = "{\n  \"altro\": { \"cleanupPeriodDays\": 7 }\n}\n";
        std::fs::write(settings_path(&base), before).unwrap();
        assert_eq!(configured(&base), None);
        set(&base, 3650).unwrap();
        let after = std::fs::read_to_string(settings_path(&base)).unwrap();
        assert_eq!(configured(&base), Some(3650));
        assert!(after.contains("\"cleanupPeriodDays\": 7"), "il nidificato resta 7");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn refuses_garbage_and_absurd_values() {
        let base = tmp();
        std::fs::write(settings_path(&base), "non sono json").unwrap();
        assert!(set(&base, 3650).is_err(), "non tocca un file illeggibile");
        assert_eq!(std::fs::read_to_string(settings_path(&base)).unwrap(), "non sono json");
        std::fs::write(settings_path(&base), "{}").unwrap();
        assert!(set(&base, 0).is_err());
        assert!(set(&base, 1_000_000).is_err());
        std::fs::remove_dir_all(&base).ok();
    }
}
