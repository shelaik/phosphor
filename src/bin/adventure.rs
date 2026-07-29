//! Phosphor — pixel-sprite "universe" GUI (macroquad).
//! Sessions become animated pixel robots living in per-project "colonies",
//! with a personalised profile/leaderboard, global stats, hover tooltips and
//! all sessions always visible (colonies wrap to multiple rows). Same backend.

use phosphor::config::{cost, Prices};
use phosphor::scan::Session;
use macroquad::prelude::*;
use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ---- palette ---------------------------------------------------------------
fn c(r: u8, g: u8, b: u8) -> Color { Color::from_rgba(r, g, b, 255) }
struct Pal {
    bg: Color, sky_top: Color, sky_bot: Color, platform: Color,
    panel: Color, panel2: Color, panel_lt: Color, ink: Color, dim: Color, accent: Color,
    run: Color, idle: Color, ended: Color, shadow: Color, moon: Color,
}
fn palette() -> Pal {
    Pal {
        bg: c(7, 8, 14), sky_top: c(13, 11, 34), sky_bot: c(52, 28, 62),
        platform: c(42, 36, 56), panel: c(15, 17, 28), panel2: c(20, 23, 38), panel_lt: c(40, 45, 66),
        ink: c(224, 232, 255), dim: c(116, 124, 162), accent: c(134, 238, 174),
        run: c(95, 255, 143), idle: c(255, 204, 51), ended: c(98, 106, 132),
        shadow: c(0, 0, 0), moon: c(234, 232, 212),
    }
}

// ---- helpers ---------------------------------------------------------------
fn now_ms() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0) }
fn rel(ms: u64) -> String {
    if ms == 0 { return "—".into(); }
    let d = now_ms().saturating_sub(ms) / 1000;
    if d < 60 { "ora".into() } else if d < 3600 { format!("{}m fa", d / 60) }
    else if d < 86400 { format!("{}h fa", d / 3600) } else { format!("{}g fa", d / 86400) }
}
fn fmt_tok(n: u64) -> String {
    if n >= 1_000_000 { format!("{:.1}M", n as f64 / 1e6) }
    else if n >= 1_000 { format!("{:.0}k", n as f64 / 1e3) } else { n.to_string() }
}
fn fmt_usd(n: f64) -> String { if n >= 1.0 { format!("${:.2}", n) } else { format!("${:.3}", n) } }
fn lamp(p: &Pal, live: &str) -> Color { match live { "running" => p.run, "idle" => p.idle, _ => p.ended } }
fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() }
    else { let mut o: String = s.chars().take(n.saturating_sub(1)).collect(); o.push('…'); o }
}
fn text(s: &str, x: f32, y: f32, size: u16, col: Color) { draw_text(s, x, y + size as f32 * 0.8, size as f32, col); }
fn text_w(s: &str, size: u16) -> f32 { measure_text(s, None, size, 1.0).width }
fn lerp_col(a: Color, b: Color, t: f32) -> Color {
    Color::new(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t, 1.0)
}
fn hsv(h: f32, s: f32, v: f32) -> Color {
    let h = (h.fract() + 1.0).fract() * 6.0;
    let i = h.floor() as i32 % 6; let f = h - h.floor();
    let (pp, q, tt) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match i { 0 => (v, tt, pp), 1 => (q, v, pp), 2 => (pp, v, tt), 3 => (pp, q, v), 4 => (tt, pp, v), _ => (v, pp, q) };
    Color::new(r, g, b, 1.0)
}
fn proj_color(name: &str) -> Color {
    let mut h: u32 = 2166136261;
    for b in name.bytes() { h = (h ^ b as u32).wrapping_mul(16777619); }
    hsv((h % 3600) as f32 / 3600.0, 0.55, 0.95)
}
fn model_kind(m: &str) -> u8 {
    let m = m.to_lowercase();
    if m.contains("opus") { 1 } else if m.contains("sonnet") { 2 } else if m.contains("haiku") { 3 } else { 0 }
}
fn model_name(k: u8) -> &'static str { match k { 1 => "opus", 2 => "sonnet", 3 => "haiku", _ => "altro" } }

fn panel(p: &Pal, r: Rect, border: Color) {
    draw_rectangle(r.x + 4.0, r.y + 5.0, r.w, r.h, p.shadow);
    draw_rectangle(r.x, r.y, r.w, r.h, p.panel);
    draw_rectangle(r.x, r.y, r.w, r.h * 0.5, p.panel2);
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, border);
}
fn button(p: &Pal, r: Rect, label: &str, active: bool) -> bool {
    let (mx, my) = mouse_position();
    let hover = r.contains(vec2(mx, my));
    let face = if active { p.accent } else if hover { p.panel_lt } else { p.panel2 };
    draw_rectangle(r.x + 3.0, r.y + 3.0, r.w, r.h, p.shadow);
    draw_rectangle(r.x, r.y, r.w, r.h, face);
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, if hover || active { p.accent } else { p.dim });
    let col = if active { p.bg } else { p.ink };
    text(label, r.x + (r.w - text_w(label, 22)) / 2.0, r.y + (r.h - 22.0) / 2.0, 22, col);
    hover && is_mouse_button_pressed(MouseButton::Left)
}
fn bar(p: &Pal, x: f32, y: f32, w: f32, h: f32, frac: f32, col: Color) {
    draw_rectangle(x, y, w, h, c(20, 24, 34));
    draw_rectangle(x, y, w * frac.clamp(0.0, 1.0), h, col);
    draw_rectangle_lines(x, y, w, h, 1.0, p.dim);
}

// ---- sprite ----------------------------------------------------------------
const BOT: [&str; 13] = [
    "....OOOO....", "...OBBBBO...", "..OBbbbbBO..", "..OBWWWWBO..", "..OBbbbbBO..",
    "..OBBBBBBO..", "...OBBBBO...", "..OBBBBBBO..", ".OBBBBBBBBO.", ".OBBBBBBBBO.",
    ".OBB.BB.BBO.", "..OO.OO.OO..", "..FF....FF..",
];
fn blit(map: &[&str], x: f32, y: f32, px: f32, body: Color, light: Color, feet: Color) {
    let outline = c(16, 18, 26); let visor = c(12, 18, 26);
    for (r, row) in map.iter().enumerate() {
        for (col, ch) in row.chars().enumerate() {
            let color = match ch { 'O' => outline, 'B' => body, 'b' => light, 'W' => visor, 'F' => feet, _ => continue };
            draw_rectangle(x + col as f32 * px, y + r as f32 * px, px + 0.6, px + 0.6, color);
        }
    }
}
struct SessView { project: String, live: String, tok: u64, agents: usize, model: u8 }

fn draw_bot(p: &Pal, cx: f32, ground: f32, px: f32, s: &SessView, t: f32, phase: f32) {
    let w = 12.0 * px; let h = 13.0 * px;
    let running = s.live == "running"; let ended = s.live == "ended";
    let bob = if running { (t * 4.0 + phase).sin() * (px * 0.5) }
        else if !ended { (t * 1.6 + phase).sin() * (px * 0.22) } else { 0.0 };
    let sway = if running { (t * 2.0 + phase).sin() * (px * 0.4) } else { 0.0 };
    let sit = if ended { 2.0 * px } else { 0.0 };
    let bx = cx - w / 2.0 + sway; let by = ground - h - bob + sit;
    draw_ellipse(cx, ground, w * 0.34, 3.0 + px, 0.0, Color::from_rgba(0, 0, 0, 90));
    let base = proj_color(&s.project);
    let body = if ended { lerp_col(base, c(40, 44, 60), 0.6) } else { base };
    let light = lerp_col(body, WHITE, 0.32); let feet = lerp_col(body, BLACK, 0.45);
    blit(&BOT, bx, by, px, body, light, feet);
    let eyc = lamp(p, &s.live); let ey = by + 3.0 * px;
    if ended {
        draw_rectangle(bx + 4.0 * px, ey + 0.5 * px, 1.4 * px, 0.4 * px, p.dim);
        draw_rectangle(bx + 6.6 * px, ey + 0.5 * px, 1.4 * px, 0.4 * px, p.dim);
    } else {
        let blink = (t * 3.0 + phase).sin() > 0.93;
        let eh = if blink { 0.3 * px } else { 1.3 * px };
        draw_rectangle(bx + 4.0 * px, ey, 1.4 * px, eh, eyc);
        draw_rectangle(bx + 6.6 * px, ey, 1.4 * px, eh, eyc);
    }
    let hx = cx + sway; let hy = by - 0.5 * px;
    match s.model {
        1 => { for k in -1..=1 { draw_rectangle(hx + k as f32 * 2.4 * px - 0.5 * px, hy - 2.2 * px, px, 2.2 * px, p.idle); }
               draw_rectangle(hx - 3.0 * px, hy, 6.0 * px, px, p.idle); }
        2 => { draw_rectangle(hx - 0.4 * px, hy - 3.0 * px, 0.8 * px, 3.0 * px, p.dim);
               draw_rectangle(hx - 1.0 * px, hy - 4.2 * px, 2.0 * px, 1.4 * px, c(120, 230, 255)); }
        3 => { draw_rectangle(hx - 0.7 * px, hy - 1.6 * px, 1.4 * px, 1.4 * px, c(255, 150, 200)); }
        _ => {}
    }
    if running && ((t * 6.0 + phase) as i64) % 2 == 0 { draw_rectangle(hx - 0.5 * px, by - 1.6 * px, px, px, p.accent); }
    let agents = s.agents.min(8);
    for k in 0..agents {
        let ang = t * 1.6 + k as f32 * (6.28 / agents.max(1) as f32) + phase;
        draw_rectangle(cx + ang.cos() * (w * 0.72) - px * 0.4, (by + h * 0.45) + ang.sin() * (h * 0.34) - px * 0.4, px * 0.9, px * 0.9, p.idle);
    }
    if running {
        for k in 0..5 {
            let ph = (t * 0.8 + k as f32 * 0.21 + phase * 0.3).rem_euclid(1.0);
            let a = ((1.0 - ph) * 210.0) as u8;
            draw_rectangle(cx + (k as f32 * 1.7 + phase).sin() * (w * 0.42), by - ph * (h * 1.1), px * 0.7, px * 0.7, Color::from_rgba(95, 255, 143, a));
        }
    }
    if ended {
        for k in 0..3 {
            let zt = (t * 0.5 + k as f32 * 0.45 + phase).rem_euclid(1.0);
            draw_text("z", cx + w * 0.28 + k as f32 * 4.0, by - zt * (10.0 + px), 16.0 + k as f32 * 2.0, Color::from_rgba(160, 170, 210, ((1.0 - zt) * 200.0) as u8));
        }
    }
}
fn mascot(p: &Pal, x: f32, y: f32, s: f32, t: f32) {
    let body = c(150, 160, 200); let dark = c(60, 66, 96);
    let step = ((t * 4.0).sin() * s).abs();
    draw_rectangle(x + 7.0 * s, y - 3.0 * s, s, 3.0 * s, dark);
    draw_rectangle(x + 6.0 * s, y - 5.0 * s, 3.0 * s, 2.0 * s, p.idle);
    draw_rectangle(x + 2.0 * s, y, 11.0 * s, 8.0 * s, body);
    draw_rectangle(x + 3.0 * s, y + 2.0 * s, 9.0 * s, 3.0 * s, c(20, 30, 40));
    let eye = if (t * 2.0) as i64 % 2 == 0 { p.run } else { p.dim };
    draw_rectangle(x + 4.0 * s, y + 3.0 * s, 2.0 * s, s, eye);
    draw_rectangle(x + 9.0 * s, y + 3.0 * s, 2.0 * s, s, eye);
    draw_rectangle(x + 1.0 * s, y + 9.0 * s, 13.0 * s, 7.0 * s, body);
    draw_rectangle(x + 3.0 * s, y + 16.0 * s, 3.0 * s, 2.0 * s + step, dark);
    draw_rectangle(x + 9.0 * s, y + 16.0 * s, 3.0 * s, 2.0 * s + (s - step), dark);
    draw_triangle(vec2(x + 7.0 * s, y + 5.0 * s), vec2(x - 6.0 * s, y + 20.0 * s), vec2(x + 20.0 * s, y + 20.0 * s), Color::from_rgba(95, 255, 143, 22));
}

// ---- grouping & stats ------------------------------------------------------
struct Band { proj: String, start: usize, len: usize, cost: f64, live: usize, tok: u64 }
fn regroup(sessions: &[Session], prices: &Prices) -> (Vec<usize>, Vec<Band>) {
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, s) in sessions.iter().enumerate() { groups.entry(s.project_name.clone()).or_default().push(i); }
    let mut projs: Vec<(String, u64)> = groups.iter()
        .map(|(k, v)| (k.clone(), v.iter().map(|&i| sessions[i].mtime_ms).max().unwrap_or(0))).collect();
    projs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut order = Vec::new(); let mut bands = Vec::new();
    for (proj, _) in projs {
        let mut idxs = groups.remove(&proj).unwrap();
        idxs.sort_by(|&a, &b| sessions[b].mtime_ms.cmp(&sessions[a].mtime_ms).then(sessions[a].id.cmp(&sessions[b].id)));
        let start = order.len();
        let cost: f64 = idxs.iter().map(|&i| cost(&sessions[i], prices)).sum();
        let tok: u64 = idxs.iter().map(|&i| sessions[i].input_tokens + sessions[i].output_tokens).sum();
        let live = idxs.iter().filter(|&&i| sessions[i].live != "ended").count();
        let len = idxs.len(); order.extend(idxs);
        bands.push(Band { proj, start, len, cost, live, tok });
    }
    (order, bands)
}

struct Stats { n: usize, projects: usize, live: usize, idle: usize, tok: u64, cost: f64, fav: String, last: String }
fn stats(app: &App) -> Stats {
    let live = app.sessions.iter().filter(|s| s.live == "running").count();
    let idle = app.sessions.iter().filter(|s| s.live == "idle").count();
    let tok: u64 = app.sessions.iter().map(|s| s.input_tokens + s.output_tokens).sum();
    let cost: f64 = app.sessions.iter().map(|s| cost(s, &app.prices)).sum();
    let mut mc: HashMap<u8, u64> = HashMap::new();
    for s in &app.sessions { *mc.entry(model_kind(s.models.first().map(|x| x.as_str()).unwrap_or(""))).or_insert(0) += 1; }
    let fav = mc.iter().filter(|(k, _)| **k != 0).max_by_key(|(_, v)| **v).map(|(k, _)| model_name(*k)).unwrap_or("—").to_string();
    let last = app.sessions.iter().max_by_key(|s| s.mtime_ms).map(|s| format!("{} · {}", clip(&s.project_name, 16), rel(s.mtime_ms))).unwrap_or_default();
    Stats { n: app.sessions.len(), projects: app.bands.len().max(1), live, idle, tok, cost, fav, last }
}

// ---- app -------------------------------------------------------------------

/// A side effect requested but awaiting the user's explicit confirmation.
enum AdvPending {
    Export { csv: std::path::PathBuf, json: std::path::PathBuf },
    Resume { id: String, cwd: String },
}

struct App {
    base: std::path::PathBuf, prices: Prices, sessions: Vec<Session>,
    sel: usize, scroll_px: f32, look: bool, status: String, hover: Option<usize>,
    order: Vec<usize>, bands: Vec<Band>, cols: usize,
    rx: mpsc::Receiver<Vec<Session>>, rescan_tx: mpsc::Sender<()>,
    pending: Option<AdvPending>, confirm_lines: Vec<String>, confirm_rects: Vec<Rect>,
}
const VERBS: [&str; 5] = ["Guarda", "Riprendi", "Esporta", "Aggiorna", "Esci"];
impl App {
    fn selected(&self) -> Option<&Session> { self.sessions.get(self.sel) }
    fn cur_pos(&self) -> usize { self.order.iter().position(|&x| x == self.sel).unwrap_or(0) }
    fn band_of(&self, pos: usize) -> usize { self.bands.iter().position(|b| pos >= b.start && pos < b.start + b.len).unwrap_or(0) }
    fn select_pos(&mut self, pos: usize) { if let Some(&idx) = self.order.get(pos) { self.sel = idx; } }
    fn move_lr(&mut self, d: i64) {
        if self.order.is_empty() { return; }
        self.select_pos((self.cur_pos() as i64 + d).clamp(0, self.order.len() as i64 - 1) as usize);
    }
    fn move_ud(&mut self, d: i64) {
        if self.order.is_empty() { return; }
        let cols = self.cols.max(1) as i64;
        self.select_pos((self.cur_pos() as i64 + d * cols).clamp(0, self.order.len() as i64 - 1) as usize);
    }
    fn do_verb(&mut self, v: usize) {
        match v { 0 => self.look = !self.look, 1 => self.request_resume(), 2 => self.request_export(),
            3 => { let _ = self.rescan_tx.send(()); self.status = "scansione in corso…".into(); }
            4 => std::process::exit(0), _ => {} }
    }
    /// Ask before resuming: surface the command + working directory.
    fn request_resume(&mut self) {
        let (id, cwd) = match self.selected() { Some(s) => (s.id.clone(), s.project_path.clone()), None => return };
        if cwd.is_empty() { self.status = "cwd mancante".into(); return; }
        self.confirm_lines = vec![
            "Aprire un NUOVO terminale ed eseguire".into(),
            format!("claude --resume {}", clip(&id, 12)),
            "nella cartella:".into(),
            cwd.clone(),
        ];
        self.pending = Some(AdvPending::Resume { id, cwd });
    }
    fn do_resume(&mut self, id: &str, cwd: &str) {
        self.status = if phosphor::resume_session(cwd, id) { format!("▶ riprendo {}…", clip(id, 8)) } else { "✗ non avviabile".into() };
    }
    /// Ask before exporting: timestamped names (never overwrite) + both paths.
    fn request_export(&mut self) {
        let dir = std::env::var("USERPROFILE").map(|h| std::path::PathBuf::from(h).join("Desktop")).unwrap_or_else(|_| self.base.clone());
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f").to_string();
        let csv = dir.join(format!("phosphor-export-{stamp}.csv"));
        let json = dir.join(format!("phosphor-export-{stamp}.json"));
        self.confirm_lines = vec![
            format!("Esportare {} sessioni in 2 NUOVI file:", self.sessions.len()),
            format!("{}", csv.display()),
            format!("{}", json.display()),
            "Nome con data/ora: non sovrascrive nulla.".into(),
        ];
        self.pending = Some(AdvPending::Export { csv, json });
    }
    fn do_export(&mut self, csv_path: &std::path::Path, json_path: &std::path::Path) {
        let mut csv = String::from("project,title,live,messages,input,output,cost_usd,modified\n");
        for s in &self.sessions {
            let f = |x: &str| phosphor::csv_field(x);
            csv.push_str(&format!("{},{},{},{},{},{},{:.4},{}\n", f(&s.project_name), f(&s.title), f(&s.live), s.message_count, s.input_tokens, s.output_tokens, cost(s, &self.prices), f(&s.modified)));
        }
        let json = phosphor::server::sessions_json(&self.sessions);
        let ok = phosphor::write_new(csv_path, csv.as_bytes()).is_ok() && phosphor::write_new(json_path, json.as_bytes()).is_ok();
        self.status = if ok { format!("✓ esportato in {} (+ .json)", csv_path.display()) } else { "✗ export fallito (file già esistente?)".into() };
    }
    fn run_pending(&mut self) {
        let action = self.pending.take();
        self.confirm_lines.clear();
        self.confirm_rects.clear();
        match action {
            Some(AdvPending::Export { csv, json }) => self.do_export(&csv, &json),
            Some(AdvPending::Resume { id, cwd }) => self.do_resume(&id, &cwd),
            None => {}
        }
    }
    fn cancel_pending(&mut self) {
        self.pending = None;
        self.confirm_lines.clear();
        self.confirm_rects.clear();
        self.status = "annullato".into();
    }
}

fn window_conf() -> Conf {
    Conf { window_title: "Phosphor — Sprite Universe".to_owned(), window_width: 1180, window_height: 760, high_dpi: false, ..Default::default() }
}

#[macroquad::main(window_conf)]
async fn main() {
    let base = phosphor::default_base();
    if !base.join("projects").is_dir() {
        loop { clear_background(c(7, 8, 14)); draw_text("Cartella ~/.claude/projects non trovata.", 40.0, 80.0, 28.0, WHITE);
            if is_key_pressed(KeyCode::Escape) { return; } next_frame().await; }
    }
    let cfg = phosphor::config::load(&base);
    let prices = cfg.prices.clone();
    let mut cache: HashMap<String, Session> = phosphor::cache::load(&base);
    let sessions = phosphor::scan_once(&base, &mut cache);
    let (tx, rx) = mpsc::channel::<Vec<Session>>();
    let (rescan_tx, rescan_rx) = mpsc::channel::<()>();
    {
        let base = base.clone(); let watch = cfg.watch.max(1);
        std::thread::spawn(move || { let mut cache = cache;
            loop { let _ = rescan_rx.recv_timeout(Duration::from_secs(watch));
                let s = phosphor::scan_once(&base, &mut cache); if tx.send(s).is_err() { break; } } });
    }
    let mut app = App {
        base, prices, sessions, sel: 0, scroll_px: 0.0, look: false, hover: None,
        status: "Benvenuto nel tuo universo di sessioni.".into(),
        order: Vec::new(), bands: Vec::new(), cols: 1, rx, rescan_tx,
        pending: None, confirm_lines: Vec::new(), confirm_rects: Vec::new(),
    };
    let p = palette();
    loop {
        while let Ok(s) = app.rx.try_recv() {
            let keep = app.selected().map(|x| x.id.clone());
            app.sessions = s;
            if let Some(id) = keep { if let Some(i) = app.sessions.iter().position(|x| x.id == id) { app.sel = i; } }
            if app.sel >= app.sessions.len() { app.sel = app.sessions.len().saturating_sub(1); }
        }
        if app.pending.is_some() {
            // A confirmation is open: only its keys/buttons respond (mouse uses
            // last frame's button rects, so the click that opened it can't leak in).
            let (mx, my) = mouse_position();
            let click = is_mouse_button_pressed(MouseButton::Left);
            let hit = |r: &Rect| mx >= r.x && mx <= r.x + r.w && my >= r.y && my <= r.y + r.h;
            let yes = is_key_pressed(KeyCode::S) || is_key_pressed(KeyCode::Y) || is_key_pressed(KeyCode::Enter)
                || (click && app.confirm_rects.first().is_some_and(hit));
            let no = is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::N)
                || (click && app.confirm_rects.get(1).is_some_and(hit));
            if yes { app.run_pending(); } else if no { app.cancel_pending(); }
        } else {
            if is_key_pressed(KeyCode::Escape) { if app.look { app.look = false; } else { std::process::exit(0); } }
            if is_key_pressed(KeyCode::Q) { std::process::exit(0); }
            if !app.sessions.is_empty() {
                if is_key_pressed(KeyCode::Right) { app.move_lr(1); }
                if is_key_pressed(KeyCode::Left) { app.move_lr(-1); }
                if is_key_pressed(KeyCode::Down) { app.move_ud(1); }
                if is_key_pressed(KeyCode::Up) { app.move_ud(-1); }
                if is_key_pressed(KeyCode::Enter) { app.look = !app.look; }
                if is_key_pressed(KeyCode::R) { app.request_resume(); }
                if is_key_pressed(KeyCode::E) { app.request_export(); }
            }
        }
        let sw = screen_width(); let sh = screen_height();
        let topbar = Rect::new(0.0, 0.0, sw, 78.0);
        let hud = Rect::new(0.0, sh - 176.0, sw, 176.0);
        let sidebar = Rect::new(0.0, topbar.h, 252.0, sh - topbar.h - hud.h);
        let main = Rect::new(sidebar.w, topbar.h, sw - sidebar.w, sh - topbar.h - hud.h);

        clear_background(p.bg);
        app.hover = None;
        draw_world(&p, &mut app, main);
        let st = stats(&app);
        draw_sidebar(&p, &app, &st, sidebar);
        draw_topbar(&p, &st, topbar);
        let verb = draw_hud(&p, &mut app, hud);
        if app.pending.is_none() { if let Some(v) = verb { app.do_verb(v); } }
        if let Some(idx) = app.hover { draw_tooltip(&p, &app, idx); }
        if app.look { draw_look(&p, &app, sw, sh); }
        if app.pending.is_some() { draw_confirm(&p, &mut app, sw, sh); }
        draw_scanlines(sw, sh);
        next_frame().await;
    }
}

fn draw_confirm(p: &Pal, app: &mut App, sw: f32, sh: f32) {
    draw_rectangle(0.0, 0.0, sw, sh, Color::from_rgba(0, 0, 0, 170)); // dim backdrop
    let bw = (sw * 0.72).min(760.0);
    let bh = 78.0 + app.confirm_lines.len() as f32 * 28.0 + 64.0;
    let bx = (sw - bw) / 2.0;
    let by = (sh - bh) / 2.0;
    draw_rectangle(bx, by, bw, bh, p.panel);
    draw_rectangle_lines(bx, by, bw, bh, 2.0, p.accent);
    draw_text("CONFERMA", bx + 18.0, by + 34.0, 26.0, p.accent);
    let mut y = by + 70.0;
    for line in &app.confirm_lines {
        draw_text(&clip(line, 72), bx + 18.0, y, 20.0, p.ink);
        y += 28.0;
    }
    let yb = by + bh - 50.0;
    let (mx, my) = mouse_position();
    let hov = |r: &Rect| mx >= r.x && mx <= r.x + r.w && my >= r.y && my <= r.y + r.h;
    let confirm_r = Rect::new(bx + 18.0, yb, 160.0, 36.0);
    let cancel_r = Rect::new(bx + 194.0, yb, 160.0, 36.0);
    draw_rectangle(confirm_r.x, confirm_r.y, confirm_r.w, confirm_r.h, if hov(&confirm_r) { p.accent } else { p.panel_lt });
    draw_rectangle_lines(confirm_r.x, confirm_r.y, confirm_r.w, confirm_r.h, 2.0, p.accent);
    draw_text("[S] Conferma", confirm_r.x + 14.0, confirm_r.y + 24.0, 20.0, p.bg);
    draw_rectangle(cancel_r.x, cancel_r.y, cancel_r.w, cancel_r.h, if hov(&cancel_r) { p.idle } else { p.panel_lt });
    draw_rectangle_lines(cancel_r.x, cancel_r.y, cancel_r.w, cancel_r.h, 2.0, p.idle);
    draw_text("[Esc] Annulla", cancel_r.x + 14.0, cancel_r.y + 24.0, 20.0, p.ink);
    app.confirm_rects = vec![confirm_r, cancel_r];
}

fn draw_scanlines(sw: f32, sh: f32) {
    let mut y = 0.0; let col = Color::from_rgba(0, 0, 0, 48);
    while y < sh { draw_rectangle(0.0, y, sw, 1.0, col); y += 3.0; }
}

fn draw_sky(p: &Pal, a: Rect, t: f32) {
    let bands = 26;
    for i in 0..bands {
        let f = i as f32 / bands as f32;
        draw_rectangle(a.x, a.y + a.h * f, a.w, a.h / bands as f32 + 1.0, lerp_col(p.sky_top, p.sky_bot, f));
    }
    let mx = a.x + a.w - 84.0; let my = a.y + 60.0;
    draw_circle(mx, my, 32.0, p.moon);
    draw_circle(mx - 11.0, my - 7.0, 6.0, lerp_col(p.moon, p.sky_top, 0.3));
    draw_circle(mx + 8.0, my + 9.0, 4.0, lerp_col(p.moon, p.sky_top, 0.3));
    let sw_ = (a.w as i32).max(1); let sh_ = (a.h as i32).max(1);
    for i in 0..120 {
        let sx = a.x + ((i * 9973) % sw_) as f32;
        let sy = a.y + ((i * 7919) % sh_) as f32;
        let tw = 120 + (((t * 2.0 + i as f32).sin() * 0.5 + 0.5) * 135.0) as u8;
        let sz = if i % 7 == 0 { 2.5 } else { 1.5 };
        draw_rectangle(sx, sy, sz, sz, Color::from_rgba(tw, tw, 255, 200));
    }
    let cyc = (t / 8.0).floor(); let local = (t / 8.0).fract();
    if local < 0.14 {
        let seed = ((cyc as i64 * 2654435761) & 0xffff) as f32 / 65535.0;
        let prog = local / 0.14;
        let hx = a.x + seed * a.w * 0.8 + prog * 220.0; let hy = a.y + 30.0 + seed * 80.0 + prog * 120.0;
        draw_line(hx, hy, hx - 38.0, hy - 18.0, 2.0, Color::from_rgba(255, 255, 255, ((1.0 - prog) * 220.0) as u8));
    }
}

fn draw_world(p: &Pal, app: &mut App, area: Rect) {
    let t = get_time() as f32;
    draw_sky(p, area, t);
    let (order, bands) = regroup(&app.sessions, &app.prices);
    app.order = order; app.bands = bands;

    let pad = 14.0;
    let cw = 110.0; let ch = 140.0; let hh = 36.0;
    let cols = (((area.w - pad * 2.0) / cw).floor() as usize).max(1);
    app.cols = cols;

    // content layout (y per colony)
    let mut colony_y = Vec::with_capacity(app.bands.len());
    let mut total = 8.0;
    for b in &app.bands {
        colony_y.push(total);
        let rows = (b.len + cols - 1) / cols;
        total += hh + rows as f32 * ch + 18.0;
    }
    let view_h = area.h;
    let max_scroll = (total - view_h).max(0.0);
    let (_, wy) = mouse_wheel();
    if wy != 0.0 { app.scroll_px = (app.scroll_px - wy * 48.0).clamp(0.0, max_scroll); }
    // keep selected visible
    if !app.order.is_empty() {
        let pos = app.cur_pos(); let b = app.band_of(pos); let k = pos - app.bands[b].start;
        let row = k / cols;
        let sy = colony_y[b] + hh + row as f32 * ch;
        if sy < app.scroll_px { app.scroll_px = sy; }
        if sy + ch > app.scroll_px + view_h { app.scroll_px = sy + ch - view_h; }
    }
    app.scroll_px = app.scroll_px.clamp(0.0, max_scroll);

    let maxtok = app.sessions.iter().map(|x| x.input_tokens + x.output_tokens).max().unwrap_or(1).max(1) as f32;
    let (mx, my) = mouse_position();
    let inside = area.contains(vec2(mx, my));
    let click = inside && is_mouse_button_pressed(MouseButton::Left);
    let sel_pos = app.cur_pos();

    for bi in 0..app.bands.len() {
        let cy = area.y + colony_y[bi] - app.scroll_px;
        let (b_proj, b_start, b_len, b_cost, b_live, b_tok) = {
            let b = &app.bands[bi];
            (b.proj.clone(), b.start, b.len, b.cost, b.live, b.tok)
        };
        let rows = (b_len + cols - 1) / cols;
        let colony_h = hh + rows as f32 * ch + 18.0;
        if cy + colony_h < area.y || cy > area.y + area.h { continue; } // cull

        let pc = proj_color(&b_proj);
        // colony header bar
        draw_rectangle(area.x + pad, cy, area.w - pad * 2.0, hh - 4.0, p.panel2);
        draw_rectangle(area.x + pad, cy, 6.0, hh - 4.0, pc);
        text(&clip(&b_proj, 28), area.x + pad + 14.0, cy + 6.0, 21, p.ink);
        let info = format!("{} sess · {} live · {} · {}", b_len, b_live, fmt_tok(b_tok), fmt_usd(b_cost));
        text(&info, area.x + area.w - pad - 14.0 - text_w(&info, 17), cy + 8.0, 17, p.dim);

        let court_x = area.x + pad + 6.0;
        for k in 0..b_len {
            let pos = b_start + k;
            let idx = app.order[pos];
            let col = k % cols; let row = k / cols;
            let cellx = court_x + col as f32 * cw;
            let celly = cy + hh + row as f32 * ch;
            let cxp = cellx + cw / 2.0;
            let ground = celly + ch - 44.0;
            let cell = Rect::new(cellx, celly, cw, ch - 6.0);

            let sv = {
                let s = &app.sessions[idx];
                SessView { project: s.project_name.clone(), live: s.live.clone(),
                    tok: s.input_tokens + s.output_tokens, agents: (s.subagents + s.workflows) as usize,
                    model: model_kind(s.models.first().map(|x| x.as_str()).unwrap_or("")) }
            };
            let hover = inside && cell.contains(vec2(mx, my));
            if pos == sel_pos {
                let g = (t * 4.0).sin().abs() * 2.0;
                draw_rectangle_lines(cell.x + 2.0, cell.y + 2.0, cell.w - 4.0, cell.h - 4.0, 2.0 + g, p.accent);
            } else if hover {
                draw_rectangle_lines(cell.x + 4.0, cell.y + 4.0, cell.w - 8.0, cell.h - 8.0, 1.5, p.panel_lt);
            }
            // ground tile under bot
            draw_rectangle(cellx + 8.0, ground, cw - 16.0, 5.0, p.platform);
            let px = (4.5 + 2.6 * ((sv.tok as f32 + 1.0).ln() / (maxtok + 1.0).ln())).clamp(4.0, 6.2);
            draw_bot(p, cxp, ground, px, &sv, t, pos as f32 * 1.37);
            draw_rectangle(cellx + 8.0, ground + 14.0, 8.0, 8.0, lamp(p, &sv.live));
            text(&clip(&sv.project, 12), cellx + 20.0, ground + 13.0, 15, if pos == sel_pos { p.accent } else { p.ink });
            bar(p, cellx + 8.0, ground + 30.0, cw - 16.0, 5.0, sv.tok as f32 / maxtok, lamp(p, &sv.live));

            if hover { app.hover = Some(idx); }
            if hover && click { app.sel = idx; app.status = format!("Hai scelto «{}» nella colonia {}.", clip(&sv.project, 18), clip(&b_proj, 16)); }
        }
    }
    // scrollbar
    if max_scroll > 0.0 {
        let frac = app.scroll_px / max_scroll;
        let knob = (view_h * (view_h / total)).max(20.0);
        draw_rectangle(area.x + area.w - 5.0, area.y + frac * (view_h - knob), 4.0, knob, p.dim);
    }
    // roaming mascot at bottom edge of the world
    let mxp = area.x + 20.0 + ((t * 22.0) % ((area.w - 60.0) * 2.0) - (area.w - 60.0)).abs();
    mascot(p, mxp, area.y + area.h - 40.0, 1.9, t);
}

fn chip(p: &Pal, x: f32, y: f32, label: &str, val: &str, col: Color) -> f32 {
    let s = format!("{} {}", val, label);
    let w = text_w(&s, 18) + 24.0;
    draw_rectangle(x, y, w, 32.0, p.panel2);
    draw_rectangle_lines(x, y, w, 32.0, 1.0, p.dim);
    text(val, x + 9.0, y + 6.0, 18, col);
    text(label, x + 9.0 + text_w(val, 18) + 5.0, y + 7.0, 16, p.dim);
    w + 10.0
}
const QUOTES: [&str; 20] = [
    "I pixel sognano in verde.", "Ogni sessione è una piccola galassia.",
    "Il fosforo non dimentica mai.", "Scansiono, dunque sono.",
    "I bit più felici sono quelli riletti.", "La cache è nostalgia ottimizzata.",
    "Un terminale, mille mondi.", "Le sessioni dormono, i log no.",
    "Anche i robot prendono il caffè.", "Token oggi, saggezza domani.",
    "Premi un tasto qualsiasi per esistere.", "Sotto ogni prompt, un universo.",
    "I fantasmi del CRT ti salutano.", "Verde fosforo, cuore caldo.",
    "La notte i bit brillano di più.", "Ogni errore è un haiku non richiesto.",
    "Il cursore lampeggia, quindi spera.", "Niente panico: è solo un altro .jsonl.",
    "Phosphor veglia mentre tu dormi.", "Retro è futuro con pazienza.",
];

const TAGLINES: [&str; 6] = [
    "made to monitor your harness", "the persistence of vision",
    "where every run leaves a glow", "watch your harness glow",
    "harnessing the afterglow", "a quiet light over your sessions",
];

fn draw_topbar(p: &Pal, st: &Stats, a: Rect) {
    draw_rectangle(a.x, a.y, a.w, a.h, p.panel);
    draw_rectangle(a.x, a.y + a.h - 2.0, a.w, 2.0, p.accent);
    text("PHOSPHOR", 18.0, a.y + 10.0, 28, p.accent);
    let ti = (now_ms() / 20000) as usize % TAGLINES.len();
    let tg: String = TAGLINES[ti].chars().take(30).collect();
    text(&tg, 20.0, a.y + 40.0, 16, p.dim);
    let qi = (now_ms() / 30000) as usize % QUOTES.len();
    text(&format!("“{}”", QUOTES[qi]), 20.0, a.y + a.h - 22.0, 15, p.idle);
    let clk = chrono::Local::now().format("⌚ %d/%m/%Y  %H:%M:%S").to_string();
    text(&clk, a.x + a.w - text_w(&clk, 18) - 16.0, a.y + a.h - 24.0, 18, p.run);
    let mut x = 250.0; let y = a.y + 18.0;
    x += chip(p, x, y, "sessioni", &st.n.to_string(), p.ink);
    x += chip(p, x, y, "progetti", &st.projects.to_string(), p.ink);
    x += chip(p, x, y, "live", &st.live.to_string(), p.run);
    x += chip(p, x, y, "idle", &st.idle.to_string(), p.idle);
    x += chip(p, x, y, "token", &fmt_tok(st.tok), p.accent);
    let _ = chip(p, x, y, "spesa", &fmt_usd(st.cost), p.idle);
}

fn draw_sidebar(p: &Pal, app: &App, st: &Stats, a: Rect) {
    panel(p, a, p.dim);
    let cx = a.x + 16.0;
    text("PROFILO", cx, a.y + 14.0, 24, p.accent);
    text("il tuo universo di sessioni", cx, a.y + 42.0, 14, p.dim);
    let mut y = a.y + 68.0;
    let kv = |k: &str, v: &str, col: Color, y: &mut f32| { text(k, cx, *y, 16, p.dim); let vw = text_w(v, 18); text(v, a.x + a.w - 16.0 - vw, *y - 1.0, 18, col); *y += 28.0; };
    let mut yy = y;
    kv("sessioni", &st.n.to_string(), p.ink, &mut yy);
    kv("progetti", &st.projects.to_string(), p.ink, &mut yy);
    kv("live adesso", &st.live.to_string(), p.run, &mut yy);
    kv("token totali", &fmt_tok(st.tok), p.accent, &mut yy);
    kv("spesa stimata", &fmt_usd(st.cost), p.idle, &mut yy);
    kv("modello pref.", &st.fav, p.ink, &mut yy);
    y = yy + 8.0;
    text("ultima attività", cx, y, 15, p.dim); y += 20.0;
    text(&clip(&st.last, 24), cx, y, 16, p.ink); y += 28.0;
    draw_rectangle(cx, y, a.w - 32.0, 1.0, p.panel_lt); y += 14.0;

    text("TOP PROGETTI · costo", cx, y, 16, p.accent); y += 26.0;
    let mut top: Vec<&Band> = app.bands.iter().collect();
    top.sort_by(|x, z| z.cost.partial_cmp(&x.cost).unwrap_or(std::cmp::Ordering::Equal));
    let maxc = top.first().map(|b| b.cost).unwrap_or(1.0).max(1e-9);
    for b in top.into_iter().take(8) {
        if y > a.y + a.h - 28.0 { break; }
        draw_rectangle(cx, y + 3.0, 10.0, 10.0, proj_color(&b.proj));
        text(&clip(&b.proj, 16), cx + 18.0, y, 16, p.ink);
        let cs = fmt_usd(b.cost);
        text(&cs, a.x + a.w - 16.0 - text_w(&cs, 15), y + 1.0, 15, p.idle);
        bar(p, cx, y + 20.0, a.w - 32.0, 5.0, (b.cost / maxc) as f32, proj_color(&b.proj));
        y += 32.0;
    }
}

fn draw_tooltip(p: &Pal, app: &App, idx: usize) {
    let s = match app.sessions.get(idx) { Some(s) => s, None => return };
    let (mx, my) = mouse_position();
    let lines = [
        clip(&s.title.replace('\n', " "), 46),
        format!("{} · {} · {}", s.live, s.last_state(), rel(s.mtime_ms)),
        format!("{} msg · {} tok · {}", s.message_count, fmt_tok(s.input_tokens + s.output_tokens), fmt_usd(cost(s, &app.prices))),
        format!("{} sub-agenti · modello {}", s.subagents + s.workflows, s.models.first().map(|m| m.replace("claude-", "")).unwrap_or_default()),
    ];
    let w = lines.iter().map(|l| text_w(l, 17)).fold(0.0_f32, f32::max) + 24.0;
    let h = lines.len() as f32 * 23.0 + 14.0;
    let x = (mx + 16.0).min(screen_width() - w - 6.0);
    let y = (my + 16.0).min(screen_height() - h - 6.0);
    draw_rectangle(x + 3.0, y + 3.0, w, h, p.shadow);
    draw_rectangle(x, y, w, h, p.panel);
    draw_rectangle_lines(x, y, w, h, 2.0, p.accent);
    let mut yy = y + 7.0;
    for (i, l) in lines.iter().enumerate() { text(l, x + 12.0, yy, 17, if i == 0 { p.accent } else { p.ink }); yy += 23.0; }
}

fn draw_hud(p: &Pal, app: &mut App, a: Rect) -> Option<usize> {
    draw_rectangle(a.x, a.y, a.w, a.h, p.panel);
    draw_rectangle(a.x, a.y, a.w, 3.0, p.accent);
    let (name, summary, models, tools, files) = match app.selected() {
        Some(s) => (s.project_name.clone(),
            format!("{} · {} · {} msg · {} tok (in {} / out {}) · {} · {} agenti · {}",
                s.live, s.last_state(), s.message_count, fmt_tok(s.input_tokens + s.output_tokens),
                fmt_tok(s.input_tokens), fmt_tok(s.output_tokens), fmt_usd(cost(s, &app.prices)), s.subagents + s.workflows, rel(s.mtime_ms)),
            s.models.iter().map(|m| m.replace("claude-", "")).collect::<Vec<_>>().join(", "),
            s.tools.iter().take(8).map(|(n, cc)| format!("{}·{}", n, cc)).collect::<Vec<_>>().join("  "),
            s.files.len()),
        None => ("—".into(), "nessuna sessione".into(), String::new(), String::new(), 0),
    };
    text(&format!("» {}", app.status), 18.0, a.y + 10.0, 18, p.idle);
    text(&clip(&name, 34), 18.0, a.y + 34.0, 26, p.accent);
    text(&clip(&summary, 108), 18.0, a.y + 66.0, 18, p.ink);
    text(&format!("modelli: {}    file toccati: {}", if models.is_empty() { "—".into() } else { models }, files), 18.0, a.y + 92.0, 16, p.dim);
    text(&format!("tool: {}", if tools.is_empty() { "—".to_string() } else { tools }), 18.0, a.y + 114.0, 16, p.dim);
    text("legenda: corona=opus · antenna=sonnet · puntino=haiku · satelliti=sub-agenti · particelle=token attivi   ·   ←→↑↓ naviga · Invio scheda",
        18.0, a.y + a.h - 24.0, 14, p.dim);

    let bw = 152.0; let bh = 40.0; let gap = 10.0;
    let mut x = a.x + a.w - VERBS.len() as f32 * (bw + gap) - 6.0;
    let y = a.y + 16.0;
    let mut acted = None;
    for (i, label) in VERBS.iter().enumerate() {
        let lbl = if i == 0 { format!("Guarda {}", clip(&name, 6)) } else { label.to_string() };
        if button(p, Rect::new(x, y, bw, bh), &lbl, i == 0 && app.look) { acted = Some(i); }
        x += bw + gap;
    }
    acted
}

fn draw_look(p: &Pal, app: &App, sw: f32, sh: f32) {
    let s = match app.selected() { Some(s) => s, None => return };
    draw_rectangle(0.0, 0.0, sw, sh, Color::from_rgba(0, 0, 0, 165));
    let w = sw * 0.74; let h = sh * 0.82;
    let x = (sw - w) / 2.0; let y = (sh - h) / 2.0;
    panel(p, Rect::new(x, y, w, h), p.accent);
    draw_rectangle(x, y, w, 34.0, p.panel_lt);
    text("SCHEDA SESSIONE   ·   [Esc] chiudi", x + 12.0, y + 8.0, 20, p.accent);
    let t = get_time() as f32;
    let sv = SessView { project: s.project_name.clone(), live: s.live.clone(), tok: s.input_tokens + s.output_tokens,
        agents: (s.subagents + s.workflows) as usize, model: model_kind(s.models.first().map(|x| x.as_str()).unwrap_or("")) };
    draw_bot(p, x + w - 110.0, y + 200.0, 9.0, &sv, t, 0.0);

    let cx = x + 20.0; let tok = s.input_tokens + s.output_tokens;
    text(&clip(&s.title.replace('\n', " "), 56), cx, y + 48.0, 24, p.accent);
    let row = |k: &str, v: &str, cy: &mut f32| { text(k, cx, *cy, 18, p.dim); text(v, cx + 150.0, *cy, 18, p.ink); *cy += 25.0; };
    let mut cy = y + 88.0;
    row("progetto", &s.project_path, &mut cy);
    row("sessionId", &s.id, &mut cy);
    row("stato", &format!("{} · {}", s.live, s.last_state()), &mut cy);
    row("modello", &s.models.iter().map(|m| m.replace("claude-", "")).collect::<Vec<_>>().join(", "), &mut cy);
    row("git branch", if s.git_branch.is_empty() { "—" } else { &s.git_branch }, &mut cy);
    row("versione", &s.version, &mut cy);
    row("messaggi", &s.message_count.to_string(), &mut cy);
    row("ultima att.", &format!("{} ({})", s.modified, rel(s.mtime_ms)), &mut cy);
    row("sub-agenti", &format!("{} subagent · {} workflow", s.subagents, s.workflows), &mut cy);
    row("token", &format!("{} (in {} · out {} · cache {})", fmt_tok(tok), fmt_tok(s.input_tokens), fmt_tok(s.output_tokens), fmt_tok(s.cache_read + s.cache_creation)), &mut cy);
    row("costo", &format!("{} (stima)", fmt_usd(cost(s, &app.prices))), &mut cy);
    cy += 6.0;
    let tools: String = s.tools.iter().map(|(n, cc)| format!("{}·{}", n, cc)).collect::<Vec<_>>().join("   ");
    text("tool usati", cx, cy, 18, p.dim); cy += 22.0;
    for l in wrap(if tools.is_empty() { "—" } else { &tools }, 80).into_iter().take(2) { text(&l, cx, cy, 15, p.ink); cy += 19.0; }
    cy += 4.0;
    text(&format!("file modificati ({})", s.files.len()), cx, cy, 18, p.dim); cy += 22.0;
    for fpath in s.files.iter().take(4) { text(&clip(fpath, 72), cx, cy, 14, p.dim); cy += 18.0; }
    cy += 4.0;
    text("primo prompt", cx, cy, 18, p.dim); cy += 22.0;
    for l in wrap(&s.first_prompt.replace('\n', " "), 76).into_iter().take(3) { text(&l, cx, cy, 15, p.ink); cy += 19.0; }
}

fn wrap(s: &str, n: usize) -> Vec<String> {
    let mut out = Vec::new(); let mut line = String::new();
    for w in s.split_whitespace() {
        if line.chars().count() + w.chars().count() + 1 > n && !line.is_empty() { out.push(std::mem::take(&mut line)); }
        if !line.is_empty() { line.push(' '); } line.push_str(w);
    }
    if !line.is_empty() { out.push(line); }
    if out.is_empty() { out.push(String::new()); }
    out
}
