//! `phosphor wrapped` — turn the data Phosphor already computes into a single,
//! self-contained, screenshot-perfect "AI coding receipt" card (SVG). The hero
//! stat is the one nobody else can show: cumulative ENERGY (Wh) and WATER (L)
//! footprint, next to the familiar flex stats (tokens, cost, sessions, top
//! tool/model, busiest day, streak).
//!
//! Ethos: 100% offline and read-only. It only consumes already-scanned sessions
//! and emits SVG bytes from `std` (zero new dependencies — SVG is just text).
//! No network, ever; the user shares the image manually, by choice. Privacy is a
//! feature: `anonymous` (the default) shows ONLY numbers — no project names, no
//! file paths, no prompt text — so a card is safe to post publicly.

use crate::config::{self, Config};
use crate::scan::Session;
use chrono::{Datelike, Local, NaiveDate, TimeZone};
use std::collections::{BTreeMap, HashMap, HashSet};

const DAY_MS: u64 = 86_400_000;

// CRT phosphor palette.
const BG: &str = "#05110c";
const PANEL: &str = "#0a2018";
const FRAME: &str = "#1f6f4a";
const DIM: &str = "#4f9d77";
const GREEN: &str = "#39d98a";
const BRIGHT: &str = "#7dffb0";
const ENERGY: &str = "#8dffb6";
const WATER: &str = "#6fe0ff";
const FONT: &str = "Consolas,'DejaVu Sans Mono',monospace";

pub enum Window {
    Days(u64),
    Year(i32),
    All,
}

pub struct Opts {
    pub window: Window,
    /// Default true: show ONLY numbers (no project names/paths). Safe to post.
    pub anonymous: bool,
    /// Show the dollar line (false = `--no-cost`).
    pub show_cost: bool,
}

pub struct Card {
    pub svg: String,
    /// La stessa card come PNG. L'SVG non lo renderizzano X, Reddit e quasi
    /// nessuna chat: senza questo la card esiste ma non circola, che era
    /// l'unica cosa che doveva fare.
    pub png: Vec<u8>,
    /// Short tag used in the filename + title (e.g. "2026", "7g", "tutto").
    pub label: String,
    /// One-line terminal summary.
    pub summary: String,
    pub sessions_count: usize,
}

/// Parse a `--window` value. Defaults to the current calendar year on anything
/// unrecognised. `now_ms` is used to resolve "year" to the current year.
pub fn parse_window(arg: &str, now_ms: u64) -> Window {
    let cur_year = Local
        .timestamp_millis_opt(now_ms as i64)
        .single()
        .map(|d| d.year())
        .unwrap_or(1970);
    match arg.trim().to_lowercase().as_str() {
        "7d" | "7g" | "week" | "settimana" => Window::Days(7),
        "30d" | "30g" | "month" | "mese" => Window::Days(30),
        "all" | "tutto" | "everything" => Window::All,
        "year" | "anno" => Window::Year(cur_year),
        other => other.parse::<i32>().map(Window::Year).unwrap_or(Window::Year(cur_year)),
    }
}

fn window_label(w: &Window) -> String {
    match w {
        Window::Days(7) => "7g".to_string(),
        Window::Days(30) => "30g".to_string(),
        Window::Days(n) => format!("{n}g"),
        Window::Year(y) => y.to_string(),
        Window::All => "tutto".to_string(),
    }
}

/// `agents`: what actually appears in the window, so the card never claims a
/// tool the numbers did not come from.
fn window_subtitle(w: &Window, agents: &str) -> String {
    let span = match w {
        Window::Days(7) => "your week".to_string(),
        Window::Days(30) => "your month".to_string(),
        Window::Days(n) => format!("your last {n} days"),
        Window::Year(_) => "your year".to_string(),
        Window::All => "all your time".to_string(),
    };
    format!("{span} with {agents} · 100% offline")
}

fn in_window(s: &Session, now_ms: u64, w: &Window) -> bool {
    match w {
        Window::All => true,
        Window::Days(n) => now_ms.saturating_sub(s.mtime_ms) < n * DAY_MS,
        Window::Year(y) => Local
            .timestamp_millis_opt(s.mtime_ms as i64)
            .single()
            .map(|d| d.year() == *y)
            .unwrap_or(false),
    }
}

fn local_date(ms: u64) -> Option<NaiveDate> {
    Local.timestamp_millis_opt(ms as i64).single().map(|d| d.date_naive())
}

fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1}B", n as f64 / 1e9)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1_000 {
        format!("{:.0}k", n as f64 / 1e3)
    } else {
        n.to_string()
    }
}
fn usd(n: f64) -> String {
    if n >= 1.0 {
        format!("${:.0}", n)
    } else {
        format!("${:.2}", n)
    }
}
fn fmt_wh(wh: f64) -> String {
    if wh >= 1000.0 {
        format!("{:.1} kWh", wh / 1000.0)
    } else {
        format!("{:.0} Wh", wh)
    }
}
fn fmt_l(ml: f64) -> String {
    if ml >= 1000.0 {
        format!("{:.1} L", ml / 1000.0)
    } else {
        format!("{:.0} mL", ml)
    }
}

/// XML text escaping (SVG is XML). Names/values can contain `& < >` etc.
fn xesc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

fn txt(out: &mut String, x: i32, y: i32, size: i32, color: &str, weight: &str, anchor: &str, s: &str) {
    out.push_str(&format!(
        "<text x=\"{x}\" y=\"{y}\" font-size=\"{size}\" fill=\"{color}\" font-weight=\"{weight}\" text-anchor=\"{anchor}\" font-family=\"{FONT}\">{}</text>\n",
        xesc(s)
    ));
}

fn cell(out: &mut String, x: i32, y: i32, label: &str, value: &str, color: &str) {
    txt(out, x, y, 15, DIM, "400", "start", label);
    txt(out, x, y + 36, 30, color, "700", "start", value);
}

fn model_family(m: &str) -> &'static str {
    let m = m.to_lowercase();
    if m.contains("opus") {
        "Opus"
    } else if m.contains("sonnet") {
        "Sonnet"
    } else if m.contains("haiku") {
        "Haiku"
    } else if m.starts_with("gpt") || m.contains("codex") {
        "GPT"
    } else {
        "altro"
    }
}

/// Build the Wrapped card for the sessions that fall in `opts.window`.
pub fn render(sessions: &[Session], cfg: &Config, now_ms: u64, opts: &Opts) -> Card {
    let label = window_label(&opts.window);
    let sel: Vec<&Session> = sessions.iter().filter(|s| in_window(s, now_ms, &opts.window)).collect();

    // Aggregates over the existing per-session fields.
    let mut tok_io: u64 = 0; // input + output (the headline flex)
    let mut msgs: u64 = 0;
    let mut corrections: u64 = 0;
    let mut cost = 0.0;
    let mut wh = 0.0;
    let mut water_ml = 0.0;
    let mut tools: HashMap<String, u64> = HashMap::new();
    let mut models: HashMap<&'static str, u64> = HashMap::new();
    let mut files: HashSet<String> = HashSet::new();
    let mut by_proj: HashMap<String, f64> = HashMap::new();
    let mut by_day: BTreeMap<NaiveDate, u64> = BTreeMap::new();

    for s in &sel {
        tok_io += s.input_tokens + s.output_tokens;
        msgs += s.message_count;
        corrections += s.corrections;
        cost += config::cost(s, &cfg.prices);
        let (e, w) = config::footprint(s, cfg.energy_wh_per_output_token, cfg.water_l_per_kwh);
        wh += e;
        water_ml += w;
        for (name, n) in &s.tools {
            *tools.entry(name.clone()).or_insert(0) += n;
        }
        if let Some(m) = s.models.first() {
            *models.entry(model_family(m)).or_insert(0) += 1;
        }
        for f in &s.files {
            files.insert(f.clone());
        }
        *by_proj.entry(s.project_name.clone()).or_insert(0.0) += config::cost(s, &cfg.prices);
        if let Some(d) = local_date(s.mtime_ms) {
            *by_day.entry(d).or_insert(0) += s.input_tokens + s.output_tokens;
        }
    }

    let top_tool = tools
        .iter()
        .max_by_key(|(_, n)| **n)
        .map(|(name, n)| format!("{} ({})", name, n))
        .unwrap_or_else(|| "—".into());
    let top_model = models
        .iter()
        .max_by_key(|(_, n)| **n)
        .map(|(m, _)| m.to_string())
        .unwrap_or_else(|| "—".into());
    let busiest = by_day
        .iter()
        .max_by_key(|(_, t)| **t)
        .map(|(d, _)| d.format("%d/%m").to_string())
        .unwrap_or_else(|| "—".into());
    let streak = longest_streak(by_day.keys().copied().collect());

    // Top-3 projects by cost (names only used in non-anonymous mode).
    let mut projs: Vec<(String, f64)> = by_proj.into_iter().collect();
    projs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Local, deliberately rough analogies (labelled "≈" on the card).
    let phone_charges = (wh / 12.0).round() as u64; // ~12 Wh per phone charge
    let bottles = (water_ml / 500.0).round() as u64; // 0.5 L bottles

    let (cx, cc) = (
        sel.iter().filter(|s| s.is_codex()).count(),
        sel.iter().filter(|s| !s.is_codex()).count(),
    );
    let agents = match (cc > 0, cx > 0) {
        (true, true) => "Claude Code + Codex",
        (false, true) => "Codex",
        _ => "Claude Code",
    };
    let subtitle = window_subtitle(&opts.window, agents);
    let d = SvgData {
        label: &label,
        subtitle: &subtitle,
        sessions: sel.len(),
        tok_io,
        msgs,
        corrections,
        cost,
        wh,
        water_ml,
        phone_charges,
        bottles,
        files: files.len(),
        top_tool: &top_tool,
        top_model: &top_model,
        busiest: &busiest,
        streak,
        spark: by_day.values().copied().collect(),
        projs: &projs,
        opts,
    };
    let svg = draw(&d);
    let png = draw_png(&d);

    let cost_part = if opts.show_cost { format!(" · {}", usd(cost)) } else { String::new() };
    let summary = format!(
        "Wrapped {}: {} token{} · {} · {} su {} sessioni",
        label,
        fmt_tokens(tok_io),
        cost_part,
        fmt_wh(wh),
        fmt_l(water_ml),
        sel.len()
    );

    Card { svg, png, label, summary, sessions_count: sel.len() }
}

/// Longest run of consecutive calendar days that have activity.
fn longest_streak(mut days: Vec<NaiveDate>) -> u64 {
    days.sort();
    days.dedup();
    let mut best = 0u64;
    let mut cur = 0u64;
    let mut prev: Option<NaiveDate> = None;
    for d in days {
        cur = match prev {
            Some(p) if p.succ_opt() == Some(d) => cur + 1,
            _ => 1,
        };
        best = best.max(cur);
        prev = Some(d);
    }
    best
}

struct SvgData<'a> {
    label: &'a str,
    subtitle: &'a str,
    sessions: usize,
    tok_io: u64,
    msgs: u64,
    corrections: u64,
    cost: f64,
    wh: f64,
    water_ml: f64,
    phone_charges: u64,
    bottles: u64,
    files: usize,
    top_tool: &'a str,
    top_model: &'a str,
    busiest: &'a str,
    streak: u64,
    spark: Vec<u64>,
    projs: &'a [(String, f64)],
    opts: &'a Opts,
}

/// The same card, rasterised — see [`crate::png`] for why this exists at all.
///
/// It is not a rendering of the SVG (that would need a font engine); it is the
/// same content laid out for a pixel grid, which is the look Phosphor already
/// has. The numbers, and their order, are deliberately identical: the two files
/// must never disagree about what your year looked like.
fn draw_png(d: &SvgData) -> Vec<u8> {
    use crate::png::{text_width, Canvas};
    // La palette dell'SVG, in RGB.
    let rgb = |hex: &str| -> [u8; 3] {
        let h = hex.trim_start_matches('#');
        let n = u32::from_str_radix(h, 16).unwrap_or(0);
        [(n >> 16) as u8, (n >> 8) as u8, n as u8]
    };
    let (bg, panel, frame, dim, green, bright, energy, water) = (
        rgb(BG), rgb(PANEL), rgb(FRAME), rgb(DIM), rgb(GREEN), rgb(BRIGHT), rgb(ENERGY), rgb(WATER),
    );
    let (w, h) = (1200usize, 630usize);
    let mut c = Canvas::new(w, h, bg);
    c.rect_outline(12, 12, w - 24, h - 24, 2, frame);

    // intestazione
    c.text(44, 44, 5, green, "PHOSPHOR WRAPPED");
    c.text(44, 92, 3, bright, &d.label.to_uppercase());
    c.text(44, 124, 2, dim, d.subtitle);

    // pannello degli stat principali
    c.fill_rect(36, 168, w - 72, 150, panel);
    c.rect_outline(36, 168, w - 72, 150, 1, frame);
    let cell = |c: &mut Canvas, x: isize, label: &str, value: &str, col: [u8; 3]| {
        c.text(x, 190, 2, dim, label);
        c.text(x, 224, 5, col, value);
    };
    cell(&mut c, 64, "TOKENS", &fmt_tokens(d.tok_io), bright);
    cell(&mut c, 344, "SESSIONS", &d.sessions.to_string(), bright);
    // "Correzioni" scalza "messaggi" quando ce ne sono: è l'unica cifra della
    // card che parla della collaborazione invece che del volume, e l'unica che
    // non lusinga chi la pubblica.
    if d.corrections > 0 {
        cell(&mut c, 604, "CORRECTIONS (EST.)", &d.corrections.to_string(), bright);
    } else {
        cell(&mut c, 604, "MESSAGES", &fmt_tokens(d.msgs), bright);
    }
    if d.opts.show_cost {
        cell(&mut c, 884, "ESTIMATED COST", &usd(d.cost), green);
    }

    // l'eroe: energia e acqua, le uniche cifre che nessun altro mostra
    c.fill_rect(36, 336, 552, 150, panel);
    c.rect_outline(36, 336, 552, 150, 1, frame);
    c.text(64, 358, 2, dim, "ENERGY");
    c.text(64, 392, 6, energy, &fmt_wh(d.wh));
    c.text(64, 446, 2, dim, &format!("~ {} PHONE CHARGES", d.phone_charges));

    c.fill_rect(612, 336, 552, 150, panel);
    c.rect_outline(612, 336, 552, 150, 1, frame);
    c.text(640, 358, 2, dim, "WATER");
    c.text(640, 392, 6, water, &fmt_l(d.water_ml));
    c.text(640, 446, 2, dim, &format!("~ {} HALF-LITRE BOTTLES", d.bottles));

    // riga di coda
    let tail = format!(
        "TOP TOOL {} · MODEL {} · BUSIEST {} · STREAK {}d · {} FILES",
        d.top_tool.to_uppercase(),
        d.top_model.to_uppercase(),
        d.busiest,
        d.streak,
        d.files
    );
    c.text(44, 512, 2, green, &tail);
    if !d.opts.anonymous && !d.projs.is_empty() {
        let names: Vec<String> = d.projs.iter().take(3).map(|(n, _)| n.to_uppercase()).collect();
        c.text(44, 540, 2, dim, &format!("TOP: {}", names.join(" · ")));
    }
    let foot = "MADE WITH PHOSPHOR · 100% OFFLINE · ENERGY AND WATER ARE ORDER-OF-MAGNITUDE ESTIMATES";
    c.text((w as isize) - 44 - text_width(foot, 1) as isize, 580, 1, dim, foot);

    c.to_png()
}

fn draw(d: &SvgData) -> String {
    let (w, h) = (1200, 630);
    let mut s = String::with_capacity(8192);
    s.push_str(&format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">\n"
    ));
    // background + frame
    s.push_str(&format!("<rect width=\"{w}\" height=\"{h}\" fill=\"{BG}\"/>\n"));
    s.push_str(&format!(
        "<rect x=\"12\" y=\"12\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{FRAME}\" stroke-width=\"2\" rx=\"10\"/>\n",
        w - 24,
        h - 24
    ));
    // CRT scanlines (subtle)
    s.push_str("<g opacity=\"0.06\">\n");
    let mut y = 16;
    while y < h - 16 {
        s.push_str(&format!("<rect x=\"14\" y=\"{y}\" width=\"{}\" height=\"1\" fill=\"{BRIGHT}\"/>\n", w - 28));
        y += 4;
    }
    s.push_str("</g>\n");

    // title
    txt(&mut s, 48, 66, 34, BRIGHT, "800", "start", "▚ PHOSPHOR WRAPPED");
    txt(&mut s, w - 48, 66, 34, GREEN, "800", "end", d.label);
    txt(&mut s, 48, 92, 16, DIM, "400", "start", d.subtitle);

    // hero footprint panel
    s.push_str(&format!("<rect x=\"40\" y=\"112\" width=\"{}\" height=\"132\" fill=\"{PANEL}\" rx=\"8\"/>\n", w - 80));
    txt(&mut s, 72, 146, 16, DIM, "400", "start", "ESTIMATED ENERGY");
    txt(&mut s, 72, 200, 50, ENERGY, "800", "start", &fmt_wh(d.wh));
    txt(&mut s, 72, 230, 15, DIM, "400", "start", &format!("~ {} phone charges", d.phone_charges));
    txt(&mut s, 624, 146, 16, DIM, "400", "start", "ESTIMATED WATER");
    txt(&mut s, 624, 200, 50, WATER, "800", "start", &fmt_l(d.water_ml));
    txt(&mut s, 624, 230, 15, DIM, "400", "start", &format!("~ {} half-litre bottles", d.bottles));

    // stat grid 3x3
    let cols = [72, 458, 844];
    let rows = [300, 372, 444];
    let cost_val = if d.opts.show_cost { usd(d.cost) } else { "nascosto".into() };
    let grid: [(&str, String, &str); 9] = [
        ("TOKENS (in+out)", fmt_tokens(d.tok_io), BRIGHT),
        ("ESTIMATED COST", cost_val, BRIGHT),
        ("SESSIONS", d.sessions.to_string(), GREEN),
        ("MESSAGES", fmt_tokens(d.msgs), GREEN),
        ("FILES TOUCHED", d.files.to_string(), GREEN),
        ("TOP TOOL", d.top_tool.to_string(), GREEN),
        ("BUSIEST DAY", d.busiest.to_string(), GREEN),
        ("STREAK (days)", d.streak.to_string(), GREEN),
        ("TOP MODEL", d.top_model.to_string(), GREEN),
    ];
    for (i, (lab, val, col)) in grid.iter().enumerate() {
        cell(&mut s, cols[i % 3], rows[i / 3], lab, val, col);
    }

    // sparkline of daily activity
    txt(&mut s, 48, 500, 14, DIM, "400", "start", "daily activity (tokens)");
    draw_spark(&mut s, &d.spark, 48, 512, (w - 96) as i32, 46);

    // privacy ledger OR top projects
    if d.opts.anonymous {
        let line = format!(
            "[privacy] solo numeri · nessun nome progetto, percorso o prompt · {} progetti aggregati",
            d.projs.len()
        );
        txt(&mut s, 48, 586, 14, DIM, "400", "start", &line);
    } else {
        let names: Vec<String> = d
            .projs
            .iter()
            .take(3)
            .map(|(n, c)| if d.opts.show_cost { format!("{} ({})", n, usd(*c)) } else { n.clone() })
            .collect();
        txt(&mut s, 48, 586, 14, GREEN, "400", "start", &format!("top progetti: {}", names.join(" · ")));
    }

    // footer
    txt(
        &mut s,
        48,
        612,
        13,
        DIM,
        "400",
        "start",
        "made with Phosphor · github.com/shelaik/phosphor · energy & water are order-of-magnitude estimates: no vendor publishes per-token figures",
    );

    s.push_str("</svg>\n");
    s
}

fn draw_spark(out: &mut String, vals: &[u64], x: i32, y: i32, w: i32, h: i32) {
    if vals.is_empty() {
        return;
    }
    let max = *vals.iter().max().unwrap_or(&1).max(&1) as f64;
    let n = vals.len() as i32;
    let bw = (w / n).max(1);
    let gap = if bw > 2 { 1 } else { 0 };
    for (i, v) in vals.iter().enumerate() {
        let bh = ((*v as f64 / max) * h as f64).round() as i32;
        let bh = bh.max(if *v > 0 { 1 } else { 0 });
        let bx = x + i as i32 * bw;
        let by = y + h - bh;
        out.push_str(&format!(
            "<rect x=\"{bx}\" y=\"{by}\" width=\"{}\" height=\"{bh}\" fill=\"{GREEN}\"/>\n",
            (bw - gap).max(1)
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Session {
        let mut a = Session::default();
        a.input_tokens = 1_000_000;
        a.output_tokens = 500_000;
        a.cache_creation = 200_000;
        a.message_count = 42;
        a.models = vec!["claude-opus-4-8".into()];
        a.tools = vec![("Edit".into(), 10), ("Bash".into(), 5)];
        a.files = vec!["src/main.rs".into(), "src/lib.rs".into()];
        a.project_name = "topsecret-client".into();
        a.mtime_ms = 1_700_000_000_000; // fixed point in time
        a
    }

    #[test]
    fn renders_valid_card_and_respects_anonymity() {
        let cfg = Config::default();
        let now = 1_700_100_000_000;
        let card = render(
            &[sample()],
            &cfg,
            now,
            &Opts { window: Window::All, anonymous: true, show_cost: true },
        );
        assert!(card.svg.starts_with("<?xml"));
        assert!(card.svg.contains("<svg"));
        assert!(card.svg.contains("PHOSPHOR WRAPPED"));
        assert!(card.svg.contains("ESTIMATED ENERGY"));
        assert!(card.svg.trim_end().ends_with("</svg>"));
        assert_eq!(card.sessions_count, 1);
        // anonymous default must NOT leak the project name anywhere
        assert!(!card.svg.contains("topsecret-client"));

        // with --with-projects the name is shown
        let card2 = render(
            &[sample()],
            &cfg,
            now,
            &Opts { window: Window::All, anonymous: false, show_cost: true },
        );
        assert!(card2.svg.contains("topsecret-client"));
    }

    #[test]
    fn window_filtering_and_streak() {
        let cfg = Config::default();
        let now = 1_700_000_000_000;
        // session is ~400 days old -> excluded from a 7-day window
        let mut old = sample();
        old.mtime_ms = now - 400 * DAY_MS;
        let card = render(
            &[old],
            &cfg,
            now,
            &Opts { window: Window::Days(7), anonymous: true, show_cost: false },
        );
        assert_eq!(card.sessions_count, 0);

        // three consecutive days -> streak 3
        let base = NaiveDate::from_ymd_opt(2026, 1, 10).unwrap();
        let days = vec![base, base.succ_opt().unwrap(), base.succ_opt().unwrap().succ_opt().unwrap()];
        assert_eq!(longest_streak(days), 3);
        // a gap breaks the streak
        let gapped = vec![base, base.succ_opt().unwrap().succ_opt().unwrap()];
        assert_eq!(longest_streak(gapped), 1);
    }
}
