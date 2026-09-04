//! User config: ~/.claude/claudescan.json — editable prices (no recompile),
//! default theme and watch interval. A default file is written on first run.

use crate::json::P;
use crate::scan::Session;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
pub struct Price {
    pub pin: f64,
    pub pout: f64,
    pub pcr: f64,
    pub pcw: f64,
}
#[derive(Clone)]
pub struct Prices {
    pub opus: Price,
    pub sonnet: Price,
    pub haiku: Price,
    /// OpenAI GPT-5 class, used for the Codex sessions (`crate::codex`).
    pub gpt: Price,
    pub default: Price,
}
impl Default for Prices {
    fn default() -> Self {
        Prices {
            // Prezzi a listino API per milione di token (in/out, cache read ~0.1x
            // input, cache write 5min ~1.25x input). Opus 4.x = 5/25 (NON 15/75:
            // quello era Opus 3 / 4.0-4.1). Editabili in phosphor.json.
            opus: Price { pin: 5.0, pout: 25.0, pcr: 0.5, pcw: 6.25 },
            sonnet: Price { pin: 3.0, pout: 15.0, pcr: 0.30, pcw: 3.75 },
            haiku: Price { pin: 1.0, pout: 5.0, pcr: 0.10, pcw: 1.25 },
            // GPT-5 a listino: 1.25 in / 10 out, cache read 0.1x. OpenAI non
            // fattura la scrittura di cache, quindi pcw = pin.
            gpt: Price { pin: 1.25, pout: 10.0, pcr: 0.125, pcw: 1.25 },
            default: Price { pin: 5.0, pout: 25.0, pcr: 0.5, pcw: 6.25 },
        }
    }
}

pub struct Config {
    pub prices: Prices,
    pub theme: String,
    pub watch: u64,
    pub pixel: bool,
    pub budget: f64, // monthly USD budget (0 = none)
    /// Cross-PC working-directory remaps for Resume: (recorded path -> local
    /// path). Lets `claude --resume` find the project on a different machine.
    pub path_remaps: Vec<(String, String)>,
    /// Local path of a git working tree used to sync `.phx` bundles between PCs
    /// (`phosphor sync`). Empty = not configured. Phosphor only runs `git` here;
    /// the repo and its credentials are set up by the user.
    pub sync_repo: String,
    /// Optional `age` recipient (e.g. `age1…`) to encrypt bundles to on push.
    /// Public key only — no secret is stored. Empty = no encryption.
    pub sync_encrypt: String,
    /// Optional path to an `age` identity file used to decrypt on pull. Empty =
    /// age's default/prompt. The private key never passes through Phosphor.
    pub sync_identity: String,
    /// Session ids the user pinned as favorites (shown with ★, filterable).
    pub favorites: Vec<String>,
    /// Per-session free-text notes: (session id -> note).
    pub notes: Vec<(String, String)>,
    /// Per-session custom title/alias shown in the list instead of the (often
    /// greeting-like) auto-title: (session id -> alias).
    pub aliases: Vec<(String, String)>,
    /// SSH aliases of the user's OTHER PCs for the fleet view (`phosphor fleet`,
    /// TUI key F). Aliases only — hosts, users and keys live in ~/.ssh/config
    /// and ssh-agent; Phosphor never stores or sees credentials.
    pub remotes: Vec<String>,
    /// Rough inference energy per token (Wh). Default 0.0005 ≈ 0.5 Wh/1k token
    /// (triangolato: Google Gemini 0.24 Wh/prompt, letteratura ~5e-4 Wh/token).
    pub energy_wh_per_token: f64,
    /// Rough on-site cooling water per token (mL). Default 0.0005 (operational,
    /// data center efficiente). Il footprint TOTALE (incl. acqua per l'energia)
    /// può essere ~100x più alto — vedi LCA Mistral.
    pub water_ml_per_token: f64,
}
impl Default for Config {
    fn default() -> Self {
        Config { prices: Prices::default(), theme: "fosfori".into(), watch: 5, pixel: false, budget: 0.0, path_remaps: Vec::new(), sync_repo: String::new(), sync_encrypt: String::new(), sync_identity: String::new(), favorites: Vec::new(), notes: Vec::new(), aliases: Vec::new(), remotes: Vec::new(), energy_wh_per_token: 0.0005, water_ml_per_token: 0.0005 }
    }
}

/// Estimated (energy Wh, water mL) footprint of a session. Applied to the
/// tokens that required fresh compute — input, output, and cache *creation* —
/// and NOT cache reads (served from cache, near-zero marginal compute). A rough
/// order-of-magnitude estimate; there is no official Anthropic per-token figure.
pub fn footprint(s: &Session, energy_wh_per_token: f64, water_ml_per_token: f64) -> (f64, f64) {
    let compute_tokens = (s.input_tokens + s.output_tokens + s.cache_creation) as f64;
    (compute_tokens * energy_wh_per_token, compute_tokens * water_ml_per_token)
}

/// Estimated USD cost of a session using the configured prices.
pub fn cost(s: &Session, p: &Prices) -> f64 {
    let m = s.models.first().map(|x| x.to_lowercase()).unwrap_or_default();
    let pr = if m.contains("sonnet") {
        &p.sonnet
    } else if m.contains("haiku") {
        &p.haiku
    } else if m.contains("opus") {
        &p.opus
    } else if s.is_codex() || m.starts_with("gpt") || m.starts_with("o1") || m.starts_with("o3") {
        // Codex bills against OpenAI's list, not Anthropic's; a session with no
        // model recorded still costs like the agent that produced it.
        &p.gpt
    } else {
        &p.default
    };
    (s.input_tokens as f64 * pr.pin
        + s.output_tokens as f64 * pr.pout
        + s.cache_read as f64 * pr.pcr
        + s.cache_creation as f64 * pr.pcw)
        / 1e6
}

fn cfg_path(base: &Path) -> PathBuf {
    base.join("phosphor.json")
}

pub fn load(base: &Path) -> Config {
    let p = cfg_path(base);
    // migrate a pre-rename config so saved prices/theme survive
    if !p.exists() {
        let legacy = base.join("claudescan.json");
        if legacy.exists() {
            let _ = std::fs::rename(&legacy, &p);
        }
    }
    let mut c = Config::default();
    match std::fs::read(&p) {
        Ok(data) => parse(&data, &mut c),
        Err(_) => write_default(base),
    }
    // Migrate stale Opus list price (15/75 = Opus 3 / 4.0-4.1) to current 5/25.
    // A config saved before this fix carries the old default; replace it so cost
    // estimates for Opus sessions stop being ~3x too high.
    if c.prices.opus.pin == 15.0 && c.prices.opus.pout == 75.0 {
        c.prices.opus = Prices::default().opus;
    }
    if c.prices.default.pin == 15.0 && c.prices.default.pout == 75.0 {
        c.prices.default = Prices::default().default;
    }
    c
}

fn parse_price(p: &mut P) -> Option<Price> {
    let mut pr = Price { pin: 0.0, pout: 0.0, pcr: 0.0, pcw: 0.0 };
    if !p.obj_begin() {
        return None;
    }
    loop {
        let k = match p.obj_key() {
            Some(k) => k,
            None => break,
        };
        match k.as_str() {
            "in" => pr.pin = p.take_number(),
            "out" => pr.pout = p.take_number(),
            "cacheRead" => pr.pcr = p.take_number(),
            "cacheWrite" => pr.pcw = p.take_number(),
            _ => {
                let _ = p.skip();
            }
        }
        if !p.obj_sep() {
            break;
        }
    }
    Some(pr)
}

fn parse(buf: &[u8], c: &mut Config) {
    let mut p = P::new(buf);
    if !p.obj_begin() {
        return;
    }
    loop {
        let k = match p.obj_key() {
            Some(k) => k,
            None => break,
        };
        match k.as_str() {
            "theme" => {
                if let Some(t) = p.take_string() {
                    c.theme = t;
                }
            }
            "watch" => c.watch = (p.take_number() as u64).max(1),
            "pixel" => c.pixel = p.take_bool(),
            "budget" => c.budget = p.take_number(),
            "syncRepo" => {
                if let Some(v) = p.take_string() {
                    c.sync_repo = v;
                }
            }
            "energyWhPerToken" => c.energy_wh_per_token = p.take_number().max(0.0),
            "waterMlPerToken" => c.water_ml_per_token = p.take_number().max(0.0),
            "syncEncrypt" => {
                if let Some(v) = p.take_string() {
                    c.sync_encrypt = v;
                }
            }
            "syncIdentity" => {
                if let Some(v) = p.take_string() {
                    c.sync_identity = v;
                }
            }
            "pathRemaps" => {
                if p.obj_begin() {
                    loop {
                        let from = match p.obj_key() {
                            Some(k) => k,
                            None => break,
                        };
                        if let Some(to) = p.take_string() {
                            if !from.is_empty() && !to.is_empty() {
                                c.path_remaps.push((from, to));
                            }
                        } else {
                            let _ = p.skip();
                        }
                        if !p.obj_sep() {
                            break;
                        }
                    }
                }
            }
            "favorites" => {
                if p.arr_begin() {
                    loop {
                        if let Some(v) = p.take_string() {
                            if !v.is_empty() {
                                c.favorites.push(v);
                            }
                        }
                        if !p.arr_sep() {
                            break;
                        }
                    }
                }
            }
            "remotes" => {
                if p.arr_begin() {
                    loop {
                        if let Some(v) = p.take_string() {
                            if !v.is_empty() {
                                c.remotes.push(v);
                            }
                        }
                        if !p.arr_sep() {
                            break;
                        }
                    }
                }
            }
            "notes" => {
                if p.obj_begin() {
                    loop {
                        let id = match p.obj_key() {
                            Some(k) => k,
                            None => break,
                        };
                        if let Some(note) = p.take_string() {
                            if !id.is_empty() && !note.is_empty() {
                                c.notes.push((id, note));
                            }
                        } else {
                            let _ = p.skip();
                        }
                        if !p.obj_sep() {
                            break;
                        }
                    }
                }
            }
            "aliases" => {
                if p.obj_begin() {
                    loop {
                        let id = match p.obj_key() {
                            Some(k) => k,
                            None => break,
                        };
                        if let Some(alias) = p.take_string() {
                            if !id.is_empty() && !alias.is_empty() {
                                c.aliases.push((id, alias));
                            }
                        } else {
                            let _ = p.skip();
                        }
                        if !p.obj_sep() {
                            break;
                        }
                    }
                }
            }
            "prices" => {
                if p.obj_begin() {
                    loop {
                        let mk = match p.obj_key() {
                            Some(k) => k,
                            None => break,
                        };
                        if let Some(pr) = parse_price(&mut p) {
                            match mk.as_str() {
                                "opus" => c.prices.opus = pr,
                                "sonnet" => c.prices.sonnet = pr,
                                "haiku" => c.prices.haiku = pr,
                                "gpt" => c.prices.gpt = pr,
                                "default" => c.prices.default = pr,
                                _ => {}
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
}

fn write_default(base: &Path) {
    save(base, &Config::default());
}

fn obj_block(pairs: &[(String, String)]) -> String {
    let body = pairs
        .iter()
        .map(|(k, v)| format!("    \"{}\": \"{}\"", crate::json::escape(k), crate::json::escape(v)))
        .collect::<Vec<_>>()
        .join(",\n");
    if body.is_empty() { String::new() } else { format!("\n{body}\n  ") }
}

/// Persist the whole config atomically. Hand-edited keys (`pathRemaps`, the
/// `sync*` fields, `favorites`, `notes`) round-trip through [`Config`], so a
/// theme change or other rewrite never drops them.
pub fn save(base: &Path, c: &Config) {
    let pr = |x: &Price| {
        format!(
            "{{ \"in\": {}, \"out\": {}, \"cacheRead\": {}, \"cacheWrite\": {} }}",
            x.pin, x.pout, x.pcr, x.pcw
        )
    };
    let fav_json = c
        .favorites
        .iter()
        .map(|id| format!("    \"{}\"", crate::json::escape(id)))
        .collect::<Vec<_>>()
        .join(",\n");
    let rem_json = c
        .remotes
        .iter()
        .map(|a| format!("    \"{}\"", crate::json::escape(a)))
        .collect::<Vec<_>>()
        .join(",\n");
    let txt = format!(
        "{{\n  \"theme\": \"{}\",\n  \"pixel\": {},\n  \"watch\": {},\n  \"budget\": {},\n  \"energyWhPerToken\": {},\n  \"waterMlPerToken\": {},\n  \"syncRepo\": \"{}\",\n  \"syncEncrypt\": \"{}\",\n  \"syncIdentity\": \"{}\",\n  \"favorites\": [{}],\n  \"remotes\": [{}],\n  \"notes\": {{{}}},\n  \"aliases\": {{{}}},\n  \"pathRemaps\": {{{}}},\n  \"prices\": {{\n    \"opus\":    {},\n    \"sonnet\":  {},\n    \"haiku\":   {},\n    \"gpt\":     {},\n    \"default\": {}\n  }}\n}}\n",
        crate::json::escape(&c.theme),
        c.pixel,
        c.watch,
        c.budget,
        c.energy_wh_per_token,
        c.water_ml_per_token,
        crate::json::escape(&c.sync_repo),
        crate::json::escape(&c.sync_encrypt),
        crate::json::escape(&c.sync_identity),
        if fav_json.is_empty() { String::new() } else { format!("\n{fav_json}\n  ") },
        if rem_json.is_empty() { String::new() } else { format!("\n{rem_json}\n  ") },
        obj_block(&c.notes),
        obj_block(&c.aliases),
        obj_block(&c.path_remaps),
        pr(&c.prices.opus),
        pr(&c.prices.sonnet),
        pr(&c.prices.haiku),
        pr(&c.prices.gpt),
        pr(&c.prices.default)
    );
    // Atomic write (tmp + rename) so a crash mid-write can't corrupt the config.
    let tmp = base.join(".phosphor-config.tmp");
    if std::fs::write(&tmp, txt).is_ok() {
        let _ = std::fs::rename(&tmp, cfg_path(base));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn codex_sessions_are_priced_off_the_gpt_list() {
        let p = Prices::default();
        let mut s = crate::scan::Session::default();
        s.agent = "codex".into();
        s.models = vec!["gpt-5.6-sol".into()];
        s.input_tokens = 1_000_000;
        s.output_tokens = 1_000_000;
        // 1.25 in + 10 out, not Anthropic's 5/25
        assert!((cost(&s, &p) - 11.25).abs() < 1e-9);
        // and a Codex thread whose model was never recorded still bills as one
        s.models.clear();
        assert!((cost(&s, &p) - 11.25).abs() < 1e-9);
        // …while a Claude session with no model keeps the Anthropic default
        s.agent.clear();
        assert!((cost(&s, &p) - 30.0).abs() < 1e-9);
    }

    use super::*;

    #[test]
    fn config_round_trips() {
        let dir = std::env::temp_dir().join(format!("phx-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = Config::default();
        c.theme = "matrix".into();
        c.budget = 42.5;
        c.sync_repo = "C:\\repo\\sync".into();
        c.sync_encrypt = "age1xyz".into();
        c.path_remaps = vec![("C:\\old".into(), "D:\\new".into())];
        c.favorites = vec!["aaa-111".into(), "bbb-222".into()];
        c.notes = vec![("aaa-111".into(), "una \"nota\" con virgolette".into())];
        c.aliases = vec![("aaa-111".into(), "il mio titolo".into())];
        c.remotes = vec!["pc-casa".into(), "utente@vps".into()];
        save(&dir, &c);
        let r = load(&dir);
        assert_eq!(r.theme, "matrix");
        assert_eq!(r.budget, 42.5);
        assert_eq!(r.sync_repo, "C:\\repo\\sync");
        assert_eq!(r.sync_encrypt, "age1xyz");
        assert_eq!(r.path_remaps, c.path_remaps);
        assert_eq!(r.favorites, c.favorites);
        assert_eq!(r.notes, c.notes);
        assert_eq!(r.aliases, c.aliases);
        assert_eq!(r.remotes, c.remotes);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn theme_with_special_chars_round_trips() {
        // A hand-edited phosphor.json with quotes/backslashes in `theme` must not
        // corrupt the file on re-save: theme is JSON-escaped like every other field,
        // so it survives the round-trip AND an adjacent field is never clobbered.
        let dir = std::env::temp_dir().join(format!("phx-cfg-theme-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = Config::default();
        c.theme = "ma\"tr\\ix".into();
        c.budget = 7.0;
        save(&dir, &c);
        let r = load(&dir);
        assert_eq!(r.theme, "ma\"tr\\ix"); // escape/unescape round-trip
        assert_eq!(r.budget, 7.0); // adjacent field not clobbered by an injection
        let _ = std::fs::remove_dir_all(&dir);
    }
}
