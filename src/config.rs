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
    /// Cache written with the 5-minute TTL (1.25x base input).
    pub pcw: f64,
    /// Cache written with the 1-hour TTL (2x base input). A separate rate
    /// because it is 60% dearer and, in practice, the one actually used: on the
    /// store this was built against, 718M of 718M cache-write tokens were 1h.
    pub pcw1h: f64,
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
            opus: Price { pin: 5.0, pout: 25.0, pcr: 0.5, pcw: 6.25, pcw1h: 10.0 },
            sonnet: Price { pin: 3.0, pout: 15.0, pcr: 0.30, pcw: 3.75, pcw1h: 6.0 },
            haiku: Price { pin: 1.0, pout: 5.0, pcr: 0.10, pcw: 1.25, pcw1h: 2.0 },
            // GPT-5 a listino: 1.25 in / 10 out, cache read 0.1x. OpenAI non
            // fattura la scrittura di cache, quindi entrambe le pcw = pin.
            gpt: Price { pin: 1.25, pout: 10.0, pcr: 0.125, pcw: 1.25, pcw1h: 1.25 },
            default: Price { pin: 5.0, pout: 25.0, pcr: 0.5, pcw: 6.25, pcw1h: 10.0 },
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
    /// Energia di UN token GENERATO su un modello di taglia media (Wh). Default
    /// 0.0005, ancorato agli ~0.24 Wh per prompt mediano pubblicati da Google: e' la
    /// generazione, la sola grandezza di cui esista una misura. Input, scritture di
    /// cache e letture vengono scalate da qui (vedi footprint).
    pub energy_wh_per_output_token: f64,
    /// On-site cooling water per kWh of inference energy (WUE, L/kWh). Default 1.0,
    /// tipico di un data center efficiente. Derivata dall'energia, non contata sui
    /// token una seconda volta, cosi' le due cifre non possono divergere. Il
    /// footprint TOTALE (inclusa l'acqua per generare l'energia) puo' essere ~100x.
    pub water_l_per_kwh: f64,
    /// Keep transcripts alive by hard-linking them into `<base>/phosphor-vault`
    /// (see [`crate::vault`]). **Off by default**: it is the one feature that
    /// creates files, and Phosphor is read-only until the user says otherwise.
    /// A link costs no extra bytes, so the only real cost is the transcripts
    /// that would have been deleted.
    pub vault: bool,
    /// La domanda sulla retention di Claude Code e' gia' stata posta una volta.
    /// Serve solo a non ripeterla: la risposta vera vive in settings.json.
    pub retention_asked: bool,
}
impl Default for Config {
    fn default() -> Self {
        Config { prices: Prices::default(), theme: "fosfori".into(), watch: 5, pixel: false, budget: 0.0, path_remaps: Vec::new(), sync_repo: String::new(), sync_encrypt: String::new(), sync_identity: String::new(), favorites: Vec::new(), notes: Vec::new(), aliases: Vec::new(), remotes: Vec::new(), energy_wh_per_output_token: 0.0005, water_l_per_kwh: 1.0, vault: false, retention_asked: false }
    }
}

/// How much more energy one OUTPUT token costs than one input-class token.
///
/// Prefill reads the whole prompt in one batched, compute-bound pass; decoding
/// emits one token per forward pass and is bound by reading the weights from
/// memory each time. The per-token gap is large and well documented in
/// direction, not in magnitude — 8x is a middle estimate, and the point of
/// having it at all is that a reply-heavy month should not look identical to a
/// month spent pasting context.
const OUTPUT_ENERGY_FACTOR: f64 = 8.0;

/// Cache reads skip prefill but still move the tokens through the machine.
/// Cheap, not free — previously counted as exactly zero.
const CACHE_READ_ENERGY_FACTOR: f64 = 0.05;

/// Energy scale by model class, relative to a mid-size (Sonnet-class) model.
/// Energy per token tracks the number of active parameters, so a Haiku token
/// and an Opus token are not the same token. Deliberately coarse: the ratios
/// are defensible, three significant figures would not be.
fn model_energy_factor(model: &str, codex: bool) -> f64 {
    let m = model.to_lowercase();
    if m.contains("haiku") {
        0.25
    } else if m.contains("sonnet") {
        1.0
    } else if m.contains("opus") || m.contains("fable") {
        3.0
    } else if codex || m.starts_with("gpt") || m.starts_with("o1") || m.starts_with("o3") {
        1.0
    } else {
        1.0
    }
}

/// Estimated (energy Wh, water mL) footprint of a session.
///
/// `energy_wh_per_output_token` is the anchor: the energy of ONE **generated**
/// token on a mid-size model. Generation is what the published figures actually
/// describe — Google's ~0.24 Wh per median Gemini prompt is the energy of
/// producing a reply — so tying the constant to anything else quietly
/// misapplies the only number anyone has measured. Input-class tokens (prompt,
/// cache writes) are that divided by [`OUTPUT_ENERGY_FACTOR`]; cache reads are
/// cheaper still; and the whole lot scales with the model actually used.
///
/// `water_l_per_kwh` then converts energy into on-site cooling water, which is
/// how data centres report it (WUE). Deriving water from energy instead of
/// counting tokens a second time means the two figures can never disagree.
///
/// Still an order-of-magnitude estimate: no vendor publishes per-token numbers.
/// What changed is that it is now *relatively* honest — a Haiku session no
/// longer weighs the same as an Opus one, and generating is no longer as cheap
/// as reading.
pub fn footprint(s: &Session, energy_wh_per_output_token: f64, water_l_per_kwh: f64) -> (f64, f64) {
    let mut wh = 0.0;
    let codex = s.is_codex();
    let per_input = energy_wh_per_output_token / OUTPUT_ENERGY_FACTOR;
    let mut add = |model: &str, u: &[u64; crate::scan::KINDS]| {
        let f = model_energy_factor(model, codex);
        let weighted = u[crate::scan::KIND_IN] as f64
            + u[crate::scan::KIND_OUT] as f64 * OUTPUT_ENERGY_FACTOR
            + u[crate::scan::KIND_CACHE_READ] as f64 * CACHE_READ_ENERGY_FACTOR
            + u[crate::scan::KIND_CACHE_5M] as f64
            + u[crate::scan::KIND_CACHE_1H] as f64;
        wh += weighted * per_input * f;
    };
    if s.usage.is_empty() {
        // No per-model detail (a ghost, or a row ingested from another PC):
        // fall back to the aggregates under the session's first model.
        let model = s.models.first().cloned().unwrap_or_default();
        let u = [s.input_tokens, s.output_tokens, s.cache_read, s.cache_creation, 0];
        add(&model, &u);
    } else {
        for (model, u) in &s.usage {
            add(model, u);
        }
    }
    (wh, wh / 1000.0 * water_l_per_kwh * 1000.0)
}

/// Prices for one model name.
fn price_for<'a>(p: &'a Prices, model: &str, codex: bool) -> &'a Price {
    let m = model.to_lowercase();
    if m.contains("sonnet") {
        &p.sonnet
    } else if m.contains("haiku") {
        &p.haiku
    } else if m.contains("opus") || m.contains("fable") {
        // Fable is an Opus-class model: pricing it as "default" happened to give
        // the same numbers, but only by accident.
        &p.opus
    } else if codex || m.starts_with("gpt") || m.starts_with("o1") || m.starts_with("o3") {
        // Codex bills against OpenAI's list, not Anthropic's; a session with no
        // model recorded still costs like the agent that produced it.
        &p.gpt
    } else {
        &p.default
    }
}

/// Estimated USD cost of a session using the configured prices.
///
/// Priced **per model**: a session that ran Opus and Fable and Sonnet is billed
/// as three, because that is what happened. Charging the whole thing to
/// whichever model spoke first is the single largest error this estimate used
/// to carry — sessions here routinely span two to four of them.
pub fn cost(s: &Session, p: &Prices) -> f64 {
    let codex = s.is_codex();
    let one = |model: &str, u: &[u64; crate::scan::KINDS]| {
        let pr = price_for(p, model, codex);
        (u[crate::scan::KIND_IN] as f64 * pr.pin
            + u[crate::scan::KIND_OUT] as f64 * pr.pout
            + u[crate::scan::KIND_CACHE_READ] as f64 * pr.pcr
            + u[crate::scan::KIND_CACHE_5M] as f64 * pr.pcw
            + u[crate::scan::KIND_CACHE_1H] as f64 * pr.pcw1h)
            / 1e6
    };
    if s.usage.is_empty() {
        let model = s.models.first().cloned().unwrap_or_default();
        let u = [s.input_tokens, s.output_tokens, s.cache_read, s.cache_creation, 0];
        return one(&model, &u);
    }
    s.usage.iter().map(|(m, u)| one(m, u)).sum()
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
    let mut pr = Price { pin: 0.0, pout: 0.0, pcr: 0.0, pcw: 0.0, pcw1h: -1.0 };
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
            "cacheWrite1h" => pr.pcw1h = p.take_number(),
            _ => {
                let _ = p.skip();
            }
        }
        if !p.obj_sep() {
            break;
        }
    }
    // A config written before the 1-hour rate existed has no such key. Derive
    // it from the base input rate (Anthropic charges 2x) instead of leaving it
    // at zero, which would silently make the dearest tokens free.
    if pr.pcw1h < 0.0 {
        pr.pcw1h = pr.pin * 2.0;
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
            "energyWhPerOutputToken" => c.energy_wh_per_output_token = p.take_number().max(0.0),
            "waterLPerKwh" => c.water_l_per_kwh = p.take_number().max(0.0),
            "vault" => c.vault = p.take_bool(),
            "retentionAsked" => c.retention_asked = p.take_bool(),
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
            "{{ \"in\": {}, \"out\": {}, \"cacheRead\": {}, \"cacheWrite\": {}, \"cacheWrite1h\": {} }}",
            x.pin, x.pout, x.pcr, x.pcw, x.pcw1h
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
        "{{\n  \"theme\": \"{}\",\n  \"pixel\": {},\n  \"watch\": {},\n  \"budget\": {},\n  \"energyWhPerOutputToken\": {},\n  \"waterLPerKwh\": {},\n  \"vault\": {},\n  \"retentionAsked\": {},\n  \"syncRepo\": \"{}\",\n  \"syncEncrypt\": \"{}\",\n  \"syncIdentity\": \"{}\",\n  \"favorites\": [{}],\n  \"remotes\": [{}],\n  \"notes\": {{{}}},\n  \"aliases\": {{{}}},\n  \"pathRemaps\": {{{}}},\n  \"prices\": {{\n    \"opus\":    {},\n    \"sonnet\":  {},\n    \"haiku\":   {},\n    \"gpt\":     {},\n    \"default\": {}\n  }}\n}}\n",
        crate::json::escape(&c.theme),
        c.pixel,
        c.watch,
        c.budget,
        c.energy_wh_per_output_token,
        c.water_l_per_kwh,
        c.vault,
        c.retention_asked,
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
    fn a_session_is_priced_model_by_model_not_by_the_first_one() {
        use crate::scan::{KIND_IN, KIND_OUT};
        let p = Prices::default();
        let mut s = crate::scan::Session::default();
        // L'ordine conta: il modello caro parla per primo, quello economico fa
        // il grosso del lavoro. Prezzare tutto col primo gonfia il conto.
        s.models = vec!["claude-opus-5".into(), "claude-haiku-4-5".into()];
        s.add_usage("claude-opus-5", KIND_OUT, 1_000_000);
        s.add_usage("claude-haiku-4-5", KIND_OUT, 1_000_000);
        // 25 (opus) + 5 (haiku), non 50 come darebbe il vecchio calcolo
        assert!((cost(&s, &p) - 30.0).abs() < 1e-9, "costo: {}", cost(&s, &p));
        // gli aggregati restano coerenti coi bucket
        assert_eq!(s.output_tokens, 2_000_000);
        // …e senza dettaglio per modello si ripiega sul primo, come prima
        let mut old = crate::scan::Session::default();
        old.models = vec!["claude-opus-5".into()];
        old.output_tokens = 2_000_000;
        assert!((cost(&old, &p) - 50.0).abs() < 1e-9);
        let _ = KIND_IN;
    }

    #[test]
    fn the_one_hour_cache_costs_more_than_the_five_minute_one() {
        use crate::scan::{KIND_CACHE_1H, KIND_CACHE_5M};
        let p = Prices::default();
        let mk = |kind: usize| {
            let mut s = crate::scan::Session::default();
            s.models = vec!["claude-opus-5".into()];
            s.add_usage("claude-opus-5", kind, 1_000_000);
            cost(&s, &p)
        };
        // Opus: input 5 -> 5m = 1.25x = 6.25, 1h = 2x = 10.00
        assert!((mk(KIND_CACHE_5M) - 6.25).abs() < 1e-9);
        assert!((mk(KIND_CACHE_1H) - 10.0).abs() < 1e-9);
        assert!(mk(KIND_CACHE_1H) > mk(KIND_CACHE_5M), "la 1h non puo' costare meno");
    }

    #[test]
    fn energy_separates_generating_from_reading_and_big_from_small() {
        use crate::scan::{KIND_CACHE_READ, KIND_IN, KIND_OUT};
        let wh = |model: &str, kind: usize| {
            let mut s = crate::scan::Session::default();
            s.models = vec![model.into()];
            s.add_usage(model, kind, 1_000_000);
            footprint(&s, 0.0005, 1.0).0
        };
        // generare costa piu' che leggere, sullo stesso modello
        assert!(wh("claude-sonnet-4", KIND_OUT) > wh("claude-sonnet-4", KIND_IN) * 5.0);
        // leggere dalla cache costa pochissimo, ma non zero
        let cr = wh("claude-sonnet-4", KIND_CACHE_READ);
        assert!(cr > 0.0 && cr < wh("claude-sonnet-4", KIND_IN));
        // un token Haiku non e' un token Opus
        assert!(wh("claude-opus-5", KIND_OUT) > wh("claude-haiku-4-5", KIND_OUT) * 5.0);
        // l'acqua deriva dall'energia: raddoppia il WUE, raddoppia l'acqua
        let mut s = crate::scan::Session::default();
        s.add_usage("claude-sonnet-4", KIND_OUT, 1_000_000);
        let (e1, w1) = footprint(&s, 0.0005, 1.0);
        let (e2, w2) = footprint(&s, 0.0005, 2.0);
        assert!((e1 - e2).abs() < 1e-9, "il WUE non tocca l'energia");
        assert!((w2 - w1 * 2.0).abs() < 1e-6);
    }

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
