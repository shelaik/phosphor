//! Read-only view of the user's Claude plan + limit-window reset, from the
//! locally-stored `~/.claude.json` (a sibling of the `.claude` directory).
//!
//! NB: the *official* 5-hour / weekly usage percentages are NOT persisted on
//! disk — Claude Code receives them at runtime as API/SDK rate-limit events and
//! never writes them down. What IS stored locally is the subscription tier and
//! `planLimitsEndDate` (when the current plan-limit window resets). So this
//! module surfaces only those two reliable facts; it never estimates usage.

use crate::json::P;
use std::path::Path;

#[derive(Clone, Default)]
pub struct PlanInfo {
    pub tier_label: String,        // friendly name, e.g. "Max 20x"
    pub limits_end_ms: Option<u64>, // planLimitsEndDate as epoch ms (window reset)
    pub extra_usage: bool,          // hasExtraUsageEnabled
}

impl PlanInfo {
    pub fn is_empty(&self) -> bool {
        self.tier_label.is_empty() && self.limits_end_ms.is_none()
    }
}

/// Map a raw rate-limit tier id to a short human label.
fn tier_label(raw: &str) -> String {
    let r = raw.to_lowercase();
    if r.contains("max_20x") || r.contains("max20x") {
        "Max 20x".into()
    } else if r.contains("max_5x") || r.contains("max5x") {
        "Max 5x".into()
    } else if r.contains("max") {
        "Max".into()
    } else if r.contains("team") {
        "Team".into()
    } else if r.contains("enterprise") {
        "Enterprise".into()
    } else if r.contains("pro") {
        "Pro".into()
    } else if r.contains("free") {
        "Free".into()
    } else {
        raw.to_string()
    }
}

/// Parse an RFC3339 / ISO-8601 timestamp into epoch milliseconds.
fn iso_to_ms(s: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp_millis().max(0) as u64)
}

#[derive(Default)]
struct Found {
    org: String,
    usr: String,
    end: String,
    extra: Option<bool>,
}

/// Recursively descend the JSON, capturing the four fields wherever they appear.
/// `~/.claude.json` nests them (e.g. tier under `oauthAccount`, the reset date
/// inside a statsig flag blob), so a depth-search is robust to the exact layout.
fn walk(p: &mut P, depth: u32, f: &mut Found) {
    if depth > 64 {
        let _ = p.skip();
        return;
    }
    match p.peek_ws() {
        b'{' => {
            if !p.obj_begin() {
                return; // empty object, already consumed
            }
            loop {
                let k = match p.obj_key() {
                    Some(k) => k,
                    None => break,
                };
                match k.as_str() {
                    "organizationRateLimitTier" if f.org.is_empty() => f.org = p.take_string().unwrap_or_default(),
                    "userRateLimitTier" if f.usr.is_empty() => f.usr = p.take_string().unwrap_or_default(),
                    "planLimitsEndDate" if f.end.is_empty() => f.end = p.take_string().unwrap_or_default(),
                    "hasExtraUsageEnabled" if f.extra.is_none() => f.extra = Some(p.take_bool()),
                    _ => walk(p, depth + 1, f),
                }
                if !p.obj_sep() {
                    break;
                }
            }
        }
        b'[' => {
            if !p.arr_begin() {
                return; // empty array, already consumed
            }
            loop {
                walk(p, depth + 1, f);
                if !p.arr_sep() {
                    break;
                }
            }
        }
        _ => {
            let _ = p.skip();
        }
    }
}

/// Load plan info from `<base>/../.claude.json`. Returns None if the file is
/// absent (e.g. an imported/foreign `.claude`) or carries none of the fields.
pub fn load(base: &Path) -> Option<PlanInfo> {
    let path = base.parent()?.join(".claude.json");
    let data = std::fs::read(&path).ok()?;
    let mut f = Found::default();
    let mut p = P::new(&data);
    walk(&mut p, 0, &mut f);

    // Prefer a user-specific tier over the org default.
    let tier_raw = if !f.usr.is_empty() { f.usr } else { f.org };
    let info = PlanInfo {
        tier_label: if tier_raw.is_empty() { String::new() } else { tier_label(&tier_raw) },
        limits_end_ms: if f.end.is_empty() { None } else { iso_to_ms(&f.end) },
        extra_usage: f.extra.unwrap_or(false),
    };
    if info.is_empty() {
        None
    } else {
        Some(info)
    }
}

/// Compact countdown from `now_ms` to the window reset `end_ms`, e.g.
/// "tra 5g 12h", "tra 3h 20m", "tra 4m", or "imminente" / "in aggiornamento".
pub fn reset_in(end_ms: u64, now_ms: u64) -> String {
    if end_ms <= now_ms {
        // The stored window end has passed; Claude Code will refresh it on next use.
        return "in aggiornamento".into();
    }
    let mut secs = (end_ms - now_ms) / 1000;
    let days = secs / 86_400;
    secs %= 86_400;
    let hours = secs / 3_600;
    secs %= 3_600;
    let mins = secs / 60;
    if days > 0 {
        format!("tra {days}g {hours}h")
    } else if hours > 0 {
        format!("tra {hours}h {mins}m")
    } else if mins > 0 {
        format!("tra {mins}m")
    } else {
        "imminente".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_labels() {
        assert_eq!(tier_label("default_claude_max_20x"), "Max 20x");
        assert_eq!(tier_label("claude_max_5x"), "Max 5x");
        assert_eq!(tier_label("pro"), "Pro");
        assert_eq!(tier_label("weird_unknown_tier"), "weird_unknown_tier");
    }

    #[test]
    fn iso_parsing() {
        let ms = iso_to_ms("2026-06-22T10:00:00Z").unwrap();
        assert!(ms > 1_700_000_000_000); // sane epoch-ms magnitude
        assert!(iso_to_ms("not-a-date").is_none());
    }

    #[test]
    fn reset_countdown() {
        let now = 1_000_000_000_000u64;
        assert_eq!(reset_in(now + 5 * 86_400_000 + 12 * 3_600_000, now), "tra 5g 12h");
        assert_eq!(reset_in(now + 3 * 3_600_000 + 20 * 60_000, now), "tra 3h 20m");
        assert_eq!(reset_in(now + 4 * 60_000, now), "tra 4m");
        assert_eq!(reset_in(now - 1000, now), "in aggiornamento");
    }
}
