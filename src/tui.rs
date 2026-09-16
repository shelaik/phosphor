//! Native terminal UI (ratatui + crossterm). No browser, no webview — a single
//! standalone binary that runs full-screen in its own console window.
//! Fully mouse-driven (clickable tabs, headers, shortcut bar, buttons, themes)
//! with a persistent shortcut legend and a polished retro look.

use crate::t;
use crate::config::{cost, Prices};
use crate::json::P;
use crate::scan::Session;
use crate::{cache, live, scan};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEventKind,
};
use ratatui::prelude::*;
use ratatui::widgets::{
    Axis, Block, BorderType, Borders, Cell, Chart, Clear, Dataset, GraphType, HighlightSpacing,
    Paragraph, Row, Table, TableState, Tabs, Wrap,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// Banner letterforms — fixed-width block grid (39 cols/row, verified aligned).
// Rendered with a vertical phosphor-glow gradient in ui().
const LOGO: [&str; 5] = [
    "███ █ █ ███ ███ ███ █ █ ███ ███",
    "█ █ █ █ █ █ █   █ █ █ █ █ █ █ █",
    "███ ███ █ █ ███ ███ ███ █ █ ███",
    "█   █ █ █ █   █ █   █ █ █ █ █ █",
    "█   █ █ ███ ███ █   █ █ ███ █ █",
];
const PIXEL_LOGO: [&str; 5] = LOGO;

// Elegant, subtle taglines (rotate; no overt product reference).
const TAGLINES: [&str; 6] = [
    "made to monitor your harness",
    "the persistence of vision",
    "where every run leaves a glow",
    "watch your harness glow",
    "harnessing the afterglow",
    "a quiet light over your sessions",
];

// The scene beside the banner stars PHOS — the phosphor sprite that lives in
// the CRT and keeps watch over your sessions (its lore rotates in the captions).
// PHOS shows different faces by time of day; sometimes the sky or a faraway
// landmark he has wandered to appears instead. Time-of-day is REAL (local clock):
// night scenes only at night, day only by day, the rest any time; eligible
// scenes rotate every ~20s. Weather/landmarks are decorative — Phosphor has no
// network by design, so no live forecast. ASCII-only, so it stays aligned.
struct Scene {
    art: [&'static str; 5],
    when: u8, // 0 = any, 1 = day (06-19), 2 = night
}
const fn sc(art: [&'static str; 5], when: u8) -> Scene {
    Scene { art, when }
}
const SCENES: &[Scene] = &[
    // --- PHOS, lo spirito del CRT (personaggio ricorrente) ---
    sc([" .---------. ", " |  o   o  | ", " |    -    | ", " |  \\___/  | ", " '---------' "], 1), // sveglio
    sc([" .---------. ", " |  ^   ^  | ", " |         | ", " |  \\___/  | ", " '---------' "], 1), // felice
    sc([" .---------. ", " |  -   -  | ", " |   z z   | ", " |  (___)  | ", " '---------' "], 2), // assonnato
    sc([" .---------. ", " |  o   o  | ", " | [=====] | ", " |  .....  | ", " '---------' "], 0), // scansione
    sc([" .---------. ", " |  O   o  | ", " |  ~~~~~  | ", " |  ? ? ?  | ", " '---------' "], 0), // glitch
    // --- il cielo di PHOS (meteo decorativo) ---
    sc(["  \\   |   /  ", "   .-----.   ", " -(       )- ", "   '-----'   ", "  /   |   \\  "], 1), // sole
    sc(["   .--.      ", "  /    \\   * ", " |      |    ", "  \\    /     ", "   '--'      "], 2), // luna
    sc(["  *    .   * ", "    .    *   ", " *    .    . ", "   *    .  * ", "  .   *   *  "], 2), // stelle
    // --- i viaggi di PHOS (luoghi famosi) ---
    sc(["     /\\      ", "    /  \\     ", "   / || \\    ", "  /__||__\\   ", "    ||||     "], 0), // Parigi
    sc(["      O      ", "     /\\      ", "    /  \\     ", "   / || \\    ", "  /______\\   "], 1), // Egitto
    sc(["  .-------.  ", "  |n n n n|  ", "  |n n n n|  ", "  |_______|  ", "  /       \\  "], 0), // Roma
    sc(["    .  *  .  ", "   .-===-.   ", "  ( o   O )  ", "   '-===-'   ", "  *   .   *  "], 2), // spazio
];

/// Pick a scene for the current hour, rotating among the eligible ones.
fn pick_scene(hour: u32, t: u64) -> &'static [&'static str; 5] {
    let day = (6..=19).contains(&hour);
    let mut elig = [0usize; 64];
    let mut n = 0;
    let mut i = 0;
    while i < SCENES.len() {
        let w = SCENES[i].when;
        if w == 0 || (w == 1 && day) || (w == 2 && !day) {
            elig[n] = i;
            n += 1;
        }
        i += 1;
    }
    if n == 0 {
        return &SCENES[0].art;
    }
    &SCENES[elig[((t / 20000) as usize) % n]].art
}
// Ever-changing quotes (a new one every ~30s and each launch): house flavour
// plus famous movie one-liners. A slice so the count stays automatic.
const QUOTES: &[&str] = &[
    // — flavour —
    "I pixel sognano in verde.",
    "Ogni sessione è una piccola galassia.",
    "Il fosforo non dimentica mai.",
    "Scansiono, dunque sono.",
    "I bit più felici sono quelli riletti.",
    "La cache è nostalgia ottimizzata.",
    "Un terminale, mille mondi.",
    "Le sessioni dormono, i log no.",
    "Anche i robot prendono il caffè.",
    "Token oggi, saggezza domani.",
    "Il vero costo è il tempo non scansionato.",
    "Premi un tasto qualsiasi per esistere.",
    "Sotto ogni prompt, un universo.",
    "I fantasmi del CRT ti salutano.",
    "Verde fosforo, cuore caldo.",
    "Conta i token, non le ore.",
    "La notte i bit brillano di più.",
    "Ogni errore è un haiku non richiesto.",
    "Il cursore lampeggia, quindi spera.",
    "Le idee migliori arrivano in pixel.",
    "Niente panico: è solo un altro .jsonl.",
    "Phosphor veglia mentre tu dormi.",
    "Un byte alla volta si conquista il mondo.",
    "Retro è futuro con pazienza.",
    // — Star Wars —
    "May the Force be with you.",
    "Do or do not. There is no try.",
    "No, I am your father.",
    "It's a trap!",
    "The Force will be with you. Always.",
    "I've got a bad feeling about this.",
    "Help me, Obi-Wan. You're my only hope.",
    // — Matrix —
    "There is no spoon.",
    "Follow the white rabbit.",
    "Red pill, or blue pill?",
    "I know kung fu.",
    "Welcome to the real world.",
    // — altri film —
    "I'll be back.",
    "Hasta la vista, baby.",
    "Houston, we have a problem.",
    "To infinity and beyond!",
    "Why so serious?",
    "I see dead people.",
    "There's no place like home.",
    "Just keep swimming.",
    "E.T. phone home.",
    "You shall not pass!",
    "One does not simply walk in.",
    "My precious.",
    "Live long and prosper.",
    "Bond. James Bond.",
    "Here's looking at you, kid.",
    "Show me the money!",
    "Wax on, wax off.",
    "Roads? We don't need roads.",
    "Elementary, my dear Watson.",
    // — la lore di PHOS —
    "PHOS vive nel fosforo del tuo schermo.",
    "Nato da un CRT dimenticato, PHOS veglia le tue sessioni.",
    "Di notte PHOS conta le stelle dei tuoi log.",
    "PHOS ha visto Parigi, l'Egitto e lo spazio: torna sempre qui.",
    "Mentre dormi, PHOS riordina i token.",
    "Il verde di PHOS non si spegne mai del tutto.",
    "Ogni sessione lascia un'impronta che PHOS ricorda.",
    "PHOS non scrive mai nulla senza chiedertelo.",
];
// Le pillole che scorrono nel riquadro in alto a destra: l'aiuto che si vede
// senza chiedere niente. Sono in DOLCE STIL NOVO — endecasillabi, o quasi —
// perché un programma che uno apre ogni giorno può permettersi di essere anche
// bello, e perché una cosa scritta in versi si rilegge invece di scorrere via.
//
// La regola che le tiene oneste: il TASTO e il COMANDO restano letterali. Un
// verso che nasconde  v  dietro una metafora smette di essere aiuto e diventa
// decorazione. Qui la forma è antica, l'istruzione è esatta.
//
// Corte per forza: vengono tagliate a ~50 colonne (vedi `clip`), che è poi la
// misura di un endecasillabo. Un test controlla che nessuna sfori.
const TIPS: &[&str] = &[
    "Un tocco in su la riga, ed ella s'apre.",
    "Chi preme  v  legge ciò che fu detto.",
    "↳ : questa nacque d'altra conversazione.",
    "●  vive,  ◐  posa,  ·  è già compiuta.",
    "⏎  è Invio, e dischiude lo dettaglio.",
    "Con  /  cerchi ne' prompt, ne' file, ne' tool.",
    "Con  f  riman soltanto chi ha quel stato.",
    "Con  o  muta la colonna che dà ordine.",
    "Con  s  l'ordine si volge al suo contrario.",
    "Tocca l'intestazione: ordina lei.",
    "Tab, o  1 2 3 : e si cangia la veduta.",
    "Veduta  2 : ogni progetto in una somma.",
    "Veduta  3 : come il tempo li dispone.",
    "Con  r  la conversazione si riprende.",
    "Con  a  vedi li agenti che servirono.",
    "Con  e  si scrive in CSV e in JSON.",
    "Con  x  fai un  .phx  che viaggia teco.",
    "Con  i  accogli un  .phx  che altri ti diè.",
    "Con  t  e  T  si cangiano i colori.",
    "Sette tavolozze: verde, ambra, ghiaccio…",
    "Con  p  il disegno si fa di quadretti.",
    "Con  m  muti la misura de' grafici.",
    "Con  R  si torna a legger ogni cosa.",
    "Con  ?  s'apre l'aiuto, e tutto dice.",
    "Con  q  ti parti, e Phosphor si congeda.",
    "DIMENS. : quanto pesa la memoria.",
    "DATA : l'ora de l'ultima parola.",
    "Il titolo dice il peso di tutte insieme.",
    "L'import non cancella: solamente aggiunge.",
    "Il  .phx  porta seco ogni sottocartella.",
    "Ogni cosa si tocca: nulla è sol dipinta.",
    "Le riprese si stringon sotto  [+] / [-] .",
    "Su quella riga:  +  apre,  -  raccoglie.",
    "Con  A  tutte s'aprono, o tutte si serrano.",
    "Il tasto destro chiama il menù intero.",
    "Anche il punto  .  chiama quello stesso menù.",
    "La rotella fa scorrere la lista.",
    "Nel dettaglio: RIPRENDI · LEGGI · AGENTI.",
    "Chi riprende, ha un terminale tutto suo.",
    "Altro cammino? pathRemaps in phosphor.json.",
    "Col remap la sessione si biforca.",
    "phosphor cost : quanto t'è costato il dire.",
    "phosphor cost --explain : ti mostra il conto.",
    "phosphor limits : il piano, e quando torna.",
    "phosphor watch : veglia, e poi t'avvisa.",
    "phosphor clean : lo spazio, e le vuote.",
    "phosphor find <testo> : cerca dal terminale.",
    "phosphor --web : la stessa in un browser.",
    "phosphor sync push : manda i tuoi fardelli.",
    "phosphor sync pull : e li ritrova altrove.",
    "Nulla si move se tu non lo comandi.",
    "Phosphor legge e non tocca: solo mira.",
    "Nulla esce di qui: ogni cosa sta in casa.",
    "Poni un budget in phosphor.json, e ti guarda.",
    "I prezzi son tuoi: mutali in phosphor.json.",
    "Ogni scrittura chiede prima il tuo sì.",
    "Ogni export ha la sua data: non si perde.",
    "Home  e  End : al principio, e a la fine.",
    "PgUp  e  PgDn : dieci righe per volta.",
    "Nel lettore:  ↑↓  e rotella per scorrere.",
    "AG : quanti agenti servirono in silenzio.",
    "Chi clicca fuori d'una finestra, la serra.",
    "phosphor --dir <via> : un'altra  .claude .",
    "Per riprender altrove: il  .phx  e 'l codice.",
    // Le cose nuove, che l'aiuto d'avvio ancora taceva.
    "🖰 qui sotto: o sol mouse, o mouse e tasti.",
    "Ogni riga de l'aiuto  ?  è comando: toccala.",
    "Con  W  la tua annata si fa immagine.",
    "Con  V  torna in vita ciò ch'era nel vault.",
    "Con  F  domandi a li altri tuoi computer.",
    "Con  H  la sessione si pone da parte.",
    "vault on : e nulla più ti sarà tolto.",
    "Claude cancella a trenta giorni: díglielo tu.",
    "Le sessioni Codex portano il segno  ◆ .",
    "⛁ sta nel vault;  ⚱ fu ritrovata dopo.",
    "Con  L  si muta la lingua: it ⇄ en.",
];

/// Le stesse pillole in inglese.
///
/// Non una traduzione riga per riga: l'italiano è in dolce stil novo, e a un
/// endecasillabo trecentesco corrisponde, in inglese, il verso di Shakespeare —
/// stessa età, stessa musica, stessa promessa di dire una cosa sola per riga.
/// Tradurre alla lettera avrebbe dato prosa storta in due lingue invece di
/// versi in una.
///
/// La regola resta: il tasto e il comando sono letterali.
const TIPS_EN: &[&str] = &[
    "One touch upon the row, and it doth open.",
    "Who presseth  v  shall read what once was said.",
    "↳ : this one was born of another talk.",
    "●  lives,  ◐  rests,  ·  is already done.",
    "⏎  is Enter, and it opens the detail.",
    "With  /  thou searchest prompts, files and tools.",
    "With  f  there stayeth only that estate.",
    "With  o  the ordering column is changed.",
    "With  s  the order turneth on its head.",
    "Touch thou the header: it will sort for thee.",
    "Tab, or  1 2 3 : and the view is changed.",
    "View  2 : every project gathered in a sum.",
    "View  3 : how the days dispose of them.",
    "With  r  the conversation is resumed.",
    "With  a  behold the agents that did serve.",
    "With  e  it writes in CSV and in JSON.",
    "With  x  make thou a  .phx  to travel with.",
    "With  i  receive a  .phx  another gave.",
    "With  t  and  T  the colours are exchanged.",
    "Seven palettes: green, amber, ice and more…",
    "With  p  the drawing turns to little squares.",
    "With  m  thou changest what the charts measure.",
    "With  R  it goes to read the whole again.",
    "With  ?  the help unfolds, and tells thee all.",
    "With  q  thou partest; Phosphor takes its leave.",
    "SIZE : how heavy is the memory.",
    "DATE : the hour of the final word.",
    "The title tells their weight when taken all.",
    "Import destroyeth not: it only adds.",
    "The  .phx  beareth every subfolder with it.",
    "All things are touched: none is but painted.",
    "Resumes are gathered under  [+] / [-] .",
    "Upon that row:  +  opens,  -  gathers in.",
    "With  A  all open, or else all are closed.",
    "The right-hand button calls the whole menu.",
    "The point  .  doth call that selfsame menu.",
    "The wheel will make the list to scroll.",
    "In the detail: RESUME · READ · AGENTS.",
    "Who doth resume shall have a console his own.",
    "Another path? pathRemaps in phosphor.json.",
    "With a remap the session forks in two.",
    "phosphor cost : what all thy speech hath cost.",
    "phosphor cost --explain : it shows the reckoning.",
    "phosphor limits : the plan, and when it turns.",
    "phosphor watch : it keeps watch, and warns thee.",
    "phosphor clean : the room, and the empty ones.",
    "phosphor find <text> : search from the console.",
    "phosphor --web : the same within a browser.",
    "phosphor sync push : it sends thy bundles forth.",
    "phosphor sync pull : and finds them otherwhere.",
    "Nothing doth move unless thou bid it move.",
    "Phosphor doth read and touch not: only look.",
    "Naught leaveth here: the whole of it stays home.",
    "Set thou a budget in phosphor.json; it watches.",
    "The prices are thine: change them, phosphor.json.",
    "Each writing asketh first thy yea or nay.",
    "Each export bears its date: none is undone.",
    "Home  and  End : to the beginning, and the end.",
    "PgUp  and  PgDn : ten rows at every turn.",
    "In the reader:  ↑↓  and wheel will scroll.",
    "AG : how many agents served without a word.",
    "Who clicks without a window, shuts it so.",
    "phosphor --dir <way> : another  .claude .",
    "To take it hence: the  .phx  and the code.",
    "🖰 below: or mouse alone, or mouse and keys.",
    "Each row of help  ?  is a command: touch it.",
    "With  W  thy year is turned into a picture.",
    "With  V  returns to life what lay in vault.",
    "With  F  thou askest of thine other machines.",
    "With  H  the project is set gently aside.",
    "vault on : and nothing more shall be taken.",
    "Claude clears at thirty days: go tell it not to.",
    "The Codex sessions bear the mark  ◆ .",
    "⛁ lies in vault;  ⚱ was found again after.",
    "With  L  the tongue is changed: it ⇄ en.",
];

/// Le pillole nella lingua scelta.
fn tips() -> &'static [&'static str] {
    t!(TIPS, TIPS_EN)
}

/// I nomi delle colonne d'ordinamento, nella lingua scelta. Una funzione e non
/// una costante: la lingua si cambia a programma acceso.
fn sorts() -> [&'static str; 9] {
    t!(
        ["attività", "progetto", "titolo", "msg", "token", "costo", "agenti", "stato", "dimens"],
        ["activity", "project", "title", "msg", "tokens", "cost", "agents", "state", "size"],
    )
}

// Action codes dispatched by clickable shortcut chips.
const A_UP: u16 = 1;
const A_DOWN: u16 = 2;
const A_SEARCH: u16 = 3;
const A_FILTER: u16 = 4;
const A_SORTCOL: u16 = 5;
const A_SORTDIR: u16 = 6;
const A_METRIC: u16 = 7;
const A_THEME_NEXT: u16 = 8;
const A_THEME_PREV: u16 = 9;
const A_RESUME: u16 = 10;
const A_AGENTS: u16 = 11;
const A_RESCAN: u16 = 14;
const A_DETAIL: u16 = 15;
const A_HELP: u16 = 16;
const A_QUIT: u16 = 17;
const A_TAB: u16 = 18;
const A_EXPORT: u16 = 13;
const A_PIXEL: u16 = 19;
const A_EXPBUNDLE: u16 = 20;
const A_IMPORT: u16 = 21;
const A_READ: u16 = 22;
const A_GSEARCH: u16 = 23;
const A_MARKDOWN: u16 = 24;
const A_FAVORITE: u16 = 25;
const A_NOTE: u16 = 26;
const A_EXPAND_ALL: u16 = 27;
const A_DELPROJECT: u16 = 28;
const A_ALIAS: u16 = 29;
const A_OPENFOLDER: u16 = 30;
const A_COPYPATH: u16 = 31;
const A_ARCHIVE: u16 = 32;
const A_FLEET: u16 = 33;
const A_VAULT_RESTORE: u16 = 34;
const A_WRAPPED: u16 = 35;
const A_MOUSE_MODE: u16 = 36;
const A_LANG: u16 = 37;

// I temi vivono in un file loro: una tavolozza non tocca lo stato
// dell'applicazione, ed e' uno dei pochi punti di questo file con una
// giuntura vera.
mod bar;
mod help;
mod theme;
pub use theme::{theme_index, THEME_COUNT, THEME_NAMES};
use bar::{render_shortcut_bar, shortcut_bar_height};
// Usati solo dai test che controllano che ogni voce disegnata sia cliccabile.
#[cfg(test)]
use bar::{place_chips, shortcut_chips, BAR_MAX_LINES};
use theme::{theme, Theme};


fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}
fn rel(ms: u64, now: u64) -> String {
    if ms == 0 {
        return "—".into();
    }
    let d = now.saturating_sub(ms) / 1000;
    if d < 60 {
        "ora".into()
    } else if d < 3600 {
        format!("{}m fa", d / 60)
    } else if d < 86400 {
        format!("{}h fa", d / 3600)
    } else {
        format!("{}g fa", d / 86400)
    }
}
fn fmt_tok(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1_000 {
        format!("{:.0}k", n as f64 / 1e3)
    } else {
        n.to_string()
    }
}
fn fmt_usd(n: f64) -> String {
    // `+ 0.0` normalizza lo ZERO NEGATIVO: in Rust la somma di una lista vuota
    // di float e' -0.0, ed e' esattamente il caso del primo avvio — la riga in
    // fondo diceva «$-0.000 costo» a chi non aveva ancora nessuna sessione.
    let n = n + 0.0;
    if n >= 1.0 {
        format!("${:.2}", n)
    } else {
        format!("${:.3}", n)
    }
}
fn tok_of(s: &Session) -> u64 {
    s.input_tokens + s.output_tokens
}
/// Human-readable byte size, scaled to B / KB / MB / GB as appropriate.
fn fmt_size(bytes: u64) -> String {
    let b = bytes as f64;
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if b < 1_048_576.0 {
        format!("{:.0} KB", b / 1024.0)
    } else if b < 1_073_741_824.0 {
        format!("{:.1} MB", b / 1_048_576.0)
    } else {
        format!("{:.2} GB", b / 1_073_741_824.0)
    }
}
fn fmt_wh(wh: f64) -> String {
    if wh >= 1000.0 { format!("{:.2} kWh", wh / 1000.0) } else { format!("{:.1} Wh", wh) }
}
fn fmt_ml(ml: f64) -> String {
    if ml >= 1000.0 { format!("{:.2} L", ml / 1000.0) } else { format!("{:.0} mL", ml) }
}
/// Absolute local date+time of an event (last message), e.g. "23/06/26 14:32".
fn abs_date(ms: u64) -> String {
    if ms == 0 {
        return "—".into();
    }
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms as i64)
        .single()
        .map(|t| t.format("%d/%m/%y %H:%M").to_string())
        .unwrap_or_else(|| "—".into())
}
fn clip(s: &str, w: usize) -> String {
    if s.chars().count() <= w {
        s.to_string()
    } else {
        let mut o: String = s.chars().take(w.saturating_sub(1)).collect();
        o.push('…');
        o
    }
}
fn file_name(p: &Path) -> String {
    p.file_name().and_then(|x| x.to_str()).unwrap_or("").to_string()
}
/// A filename-safe slug from a title (lowercase ascii-alnum + dashes, ≤40).
fn slugify(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if matches!(c, ' ' | '-' | '_') && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(40).collect();
    if out.is_empty() { "sessione".into() } else { out }
}

/// Neutralize untrusted transcript text before writing it into a Markdown file:
/// escape the HTML-active trio (`& < >`) so an imported (hostile) title/message
/// can't smuggle raw HTML (`<img onerror=…>`, `<script>`) that a permissive
/// Markdown renderer would execute, and drop control bytes. Consistent with the
/// escaping the CSV/JSON/SVG/HTML exporters already apply.
fn md_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\t' => out.push('\t'),
            c if c.is_control() => {} // drop ESC/CR/BEL/etc.
            c => out.push(c),
        }
    }
    out
}

/// List a directory for the file picker: a `[..]` row (if there's a parent),
/// then sub-directories (alphabetical), then `.phx` files (newest first).
fn read_dir_entries(dir: &Path, dirs_only: bool) -> Vec<PickEntry> {
    use chrono::TimeZone;
    let mut dirs: Vec<PickEntry> = Vec::new();
    let mut files: Vec<(u64, PickEntry)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let path = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') && path.is_dir() {
                continue; // skip hidden dirs to keep the list tidy
            }
            let ft = match e.file_type() { Ok(f) => f, Err(_) => continue };
            if ft.is_dir() {
                dirs.push(PickEntry { label: format!("[ {} ]", name), path, is_dir: true, select_here: false });
            } else if !dirs_only && path.extension().and_then(|x| x.to_str()) == Some("phx") {
                let md = e.metadata().ok();
                let sz = md.as_ref().map(|m| m.len()).unwrap_or(0);
                let mt = md.as_ref().and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64).unwrap_or(0);
                let when = chrono::Local.timestamp_millis_opt(mt as i64).single()
                    .map(|d| d.format("%d/%m %H:%M").to_string()).unwrap_or_default();
                let label = format!("{:<26} {:>7.1} MB  {}", clip(&name, 26), sz as f64 / 1_048_576.0, when);
                files.push((mt, PickEntry { label, path, is_dir: false, select_here: false }));
            }
        }
    }
    dirs.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    files.sort_by(|a, b| b.0.cmp(&a.0)); // newest first
    let mut out = Vec::new();
    if let Some(parent) = dir.parent() {
        out.push(PickEntry { label: "[..]".into(), path: parent.to_path_buf(), is_dir: true, select_here: false });
    }
    out.extend(dirs);
    out.extend(files.into_iter().map(|(_, e)| e));
    out
}
fn minibar(v: f64, max: f64, cells: usize) -> String {
    if max <= 0.0 || v <= 0.0 {
        return " ".repeat(cells);
    }
    let units = (v / max * cells as f64 * 8.0).round() as usize;
    let full = units / 8;
    let rem = units % 8;
    let mut s = "█".repeat(full.min(cells));
    if full < cells && rem > 0 {
        s.push(['▏', '▎', '▍', '▌', '▋', '▊', '▉'][rem - 1]);
    }
    let len = s.chars().count();
    if len < cells {
        s.push_str(&" ".repeat(cells - len));
    }
    s
}
fn hit(r: Rect, col: u16, row: u16) -> bool {
    row == r.y && col >= r.x && col < r.x + r.width
}
/// Whole-area hit test. [`hit`] deliberately checks a single ROW because the
/// things it tests are one-line chips; using it on a panel would make every
/// click below the top border count as a click *outside* the panel.
fn within(r: Rect, col: u16, row: u16) -> bool {
    col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
}

/// The clickable regions that run a command: a rectangle and an action code.
///
/// One type, because the defect worth closing is that every surface used to
/// register its own way and the mouse handler had to remember how. The bar was
/// the proof: it kept its rectangles correctly and was then read behind a
/// `row == bar.y` guard, so the second row of chips was drawn, registered and
/// unreachable. Here the rectangles ARE the guard.
#[derive(Default)]
struct Hotspots(Vec<(Rect, u16)>);

impl Hotspots {
    fn clear(&mut self) {
        self.0.clear();
    }
    /// Register a region. Action 0 means "no command" — registering it would
    /// make a thing that looks clickable, clicks, and does nothing.
    fn push(&mut self, r: Rect, action: u16) {
        if action != 0 && r.width > 0 && r.height > 0 {
            self.0.push((r, action));
        }
    }
    fn at(&self, col: u16, row: u16) -> Option<u16> {
        self.0.iter().find(|(r, _)| within(*r, col, row)).map(|(_, a)| *a)
    }
    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Where a given action can be clicked. Used by the tests that check that
    /// everything drawn can in fact be reached.
    #[cfg(test)]
    fn rect_of(&self, action: u16) -> Option<Rect> {
        self.0.iter().find(|(_, a)| *a == action).map(|(r, _)| *r)
    }
}
/// Border style: chunky quadrant blocks in pixel mode, double lines otherwise.
fn btype(pixel: bool) -> BorderType {
    if pixel { BorderType::QuadrantOutside } else { BorderType::Double }
}

fn sess_widths() -> [Constraint; 12] {
    [
        Constraint::Length(2),
        Constraint::Length(18),
        Constraint::Min(16),
        Constraint::Length(6),
        Constraint::Length(7),
        Constraint::Length(7),
        Constraint::Length(8),  // ENERGIA
        Constraint::Length(8),  // ACQUA
        Constraint::Length(3),  // AG
        Constraint::Length(8),  // DIMENS.
        Constraint::Length(14), // DATA (ultimo messaggio)
        Constraint::Length(12), // MODELLO
    ]
}
// column index -> sort index in SORTS (None = not sortable)
const COL_SORT: [Option<usize>; 12] = [
    Some(7), Some(1), Some(2), Some(3), Some(4), Some(5), None, None, Some(6), Some(8), Some(0), None,
];

struct AgentInfo {
    kind: String,
    desc: String,
    msgs: u64,
    tok: u64,
    cost: f64,
}

/// One fleet-fetch result: (generation, ssh alias, sessions or error).
/// The generation lets the UI drop results of a superseded fetch (double-F).
type FleetMsg = (u64, String, Result<Vec<Session>, String>);

struct App {
    base: PathBuf,
    cache: Arc<Mutex<HashMap<String, Session>>>,
    prices: Prices,
    budget: f64,
    remaps: Vec<(String, String)>, // cross-PC cwd remaps for Resume (from phosphor.json)
    remotes: Vec<String>, // ssh aliases of the user's other PCs (fleet view, key F)
    fleet: Vec<Session>,  // remote sessions from the last fleet fetch (host != "")
    fleet_tx: Option<mpsc::Sender<FleetMsg>>, // handed to fetch threads by run()
    /// Where a re-scan sends its result: the same channel the periodic watcher
    /// uses, so both arrive by the same road. Handed over by `run()`.
    scan_tx: Option<mpsc::Sender<Vec<Session>>>,
    /// Il giro di presentazione non e' ancora stato visto ne' saltato.
    tour_todo: bool,
    /// A re-scan is in flight. Only to avoid piling up threads when someone
    /// leans on `R`, and to have something honest to put in the status line.
    scanning: bool,
    fleet_gen: u64,       // fetch generation (stale results are dropped)
    fleet_expect: usize,  // hosts queried in the current generation
    fleet_got: usize,     // hosts answered (ok or error) so far
    sync_repo: String, // preserved across config saves (used by the CLI `sync`)
    watch: u64,        // live re-scan interval (kept so we can persist config mid-session)
    favorites: HashSet<String>,    // pinned session ids (★), from phosphor.json
    marked: HashSet<String>,       // multi-selected session ids (◉) for bulk actions
    notes: HashMap<String, String>, // per-session free-text notes
    aliases: HashMap<String, String>, // per-session custom title shown in the list
    note_editing: bool, // editing the note (or alias) of the selected session
    note_is_alias: bool, // the shared text editor is editing an ALIAS, not a note
    note_buf: String,   // in-progress note/alias text
    energy_wh_per_output_token: f64, // footprint rates (display-only; edited in phosphor.json)
    water_l_per_kwh: f64,
    all: Vec<Session>,
    view: Vec<usize>,
    tab: usize,
    ts: TableState,
    search: String,
    searching: bool,
    state_filter: u8,
    metric: u8,
    sort_col: usize,
    sort_desc: bool,
    theme_idx: usize,
    pixel: bool,
    detail: bool,
    help: bool,
    show_agents: bool,
    agents: Vec<AgentInfo>,
    status: String,
    blink: bool,
    dry: bool, // test mode: suppress side effects (resume/export/rescan)
    // hit-test regions, refreshed every draw
    rect_tabs: Rect,
    rect_table: Rect,
    rect_shortcut: Rect,
    /// Il riquadro dell'overlay attualmente disegnato. Una sola definizione di
    /// "dentro la finestra": chi disegna un pannello lo registra qui, e il
    /// gestore del mouse sa cosa vuol dire cliccare FUORI senza doverlo
    /// ricalcolare per ognuno.
    rect_overlay: Rect,
    /// Le righe dell'aiuto che sono anche comandi: riquadro e azione. Vuoto
    /// per le righe di sola legenda, che non fanno nulla se cliccate.
    rect_help_actions: Hotspots,
    /// Vedi `config::Config::mouse_only`.
    mouse_only: bool,
    shortcut_groups: Hotspots,
    rect_sort_headers: Vec<(Rect, usize)>,
    rect_metric_buttons: Vec<Rect>,
    rect_detail_buttons: Vec<Rect>,
    rect_agents_close: Rect,
    rect_theme_buttons: Vec<Rect>,
    confirm: Option<Confirm>,
    rect_confirm_buttons: Vec<Rect>,
    detail_opened_ms: u64, // when detail opened via mouse — debounces a double-click's 2nd press
    plan: Option<crate::plan::PlanInfo>, // subscription tier + limit-window reset (read-only)
    help_scroll: u16, // vertical scroll offset of the (scrollable) help overlay
    picker: Option<Picker>, // ASCII file browser for choosing a .phx to import
    rect_picker_list: Rect, // hit-test region of the picker's file list
    reader: Option<Reader>, // in-app transcript reader overlay
    rect_reader: Rect,      // hit-test/scroll region of the reader body
    gsearch: Option<GSearch>, // global content search overlay
    rect_gsearch_list: Rect,  // hit-test region of the results list
    menu: Option<Menu>,       // right-click context menu over a session
    rect_menu_items: Vec<Rect>, // hit-test region of each menu row
    delproj: Option<DelProj>, // "delete entire project" two-phase modal
    // Collapsible resume-chains inside the detailed Sessioni list. `view_all` is
    // the full filtered+sorted set (used for export/totals); `view` is the
    // currently VISIBLE subset (chain heads + the children of expanded chains);
    // `row_meta` is aligned with `view` and carries the +/- marker + indent.
    view_all: Vec<usize>,                // every filtered session (pre-collapse)
    row_meta: Vec<RowMeta>,              // aligned with `view`
    expanded_chains: HashSet<String>,    // chain heads the user opened (collapsed by default)
    rect_title_col: Rect,                // x-range of the TITLE column (for +/- click)
}

/// In-app transcript reader: the human-readable conversation of one session.
struct Reader {
    title: String,
    turns: Vec<crate::scan::Turn>,
    scroll: u16,
    target_turn: Option<usize>, // jump here on first render (from global search)
}

/// Global content search across all transcripts (key `g`): grep the real
/// conversations, not just the per-session index, then jump into the reader.
struct GSearch {
    query: String,
    typing: bool, // true = editing the query, false = browsing results
    results: Vec<GHit>,
    sel: usize,
    scroll: usize,
}

struct GHit {
    sess: usize, // index into app.all
    turn: usize,
    role: u8,
    title: String,
    project: String,
    snippet: String,
}

/// One row in the ASCII file picker (a sub-directory or a `.phx` file).
struct PickEntry {
    label: String,      // shown text, e.g. "[..]", "[ dir ]", "file.phx  1.2 MB"
    path: PathBuf,      // target path
    is_dir: bool,       // directory (navigate) vs file (select)
    select_here: bool,  // synthetic "✓ use THIS directory" action (remap mode)
}

/// What the file browser is for: importing a `.phx`, or choosing the local
/// folder to remap a session's working directory to (cross-PC resume).
#[derive(Clone)]
enum PickPurpose {
    Import,
    Remap { recorded: String },
}

/// A minimal ASCII file browser to choose a `.phx` bundle, or a directory.
struct Picker {
    dir: PathBuf,
    entries: Vec<PickEntry>,
    sel: usize,
    scroll: usize,
    purpose: PickPurpose,
}

/// A side effect that has been REQUESTED but is waiting for the user's explicit
/// confirmation before it touches the disk or launches a process.
enum Pending {
    Export { csv: PathBuf, json: PathBuf },
    Resume { id: String, cwd: String, fork: bool, codex: bool },
    /// Alza cleanupPeriodDays di Claude Code. $days = 0 significa "lascia
    /// com'e'": non tocca nulla, ma smette di chiedere.
    SetRetention { days: u64 },
    /// Scrive la card sul Desktop.
    Wrapped,
    /// Contatta gli altri PC via ssh: esce dalla macchina, quindi si chiede.
    Fleet,
    /// Rimette un transcript nel magazzino del suo agente.
    VaultRestore,
    /// Mostra la pagina `n` del giro di presentazione, o lo chiude quando le
    /// pagine sono finite.
    Tour(u8),
    /// Il giro e' stato visto o saltato: non si ripresenta.
    TourDone,
    ExportBundle { out: PathBuf },
    ImportBundle { src: PathBuf },
    ArchiveProject { dir: PathBuf, name: String },
    DeleteMarked { paths: Vec<String> },
    RemoteResume { host: String, id: String },
}

/// A modal confirmation: a clear explanation of exactly what will happen, plus
/// the pending action to run only if the user confirms.
struct Confirm {
    title: String,
    lines: Vec<String>,
    action: Pending,
    /// Risposte alternative a tasto singolo, oltre al solito sì/no:
    /// `(tasto, azione)`. Servono a far scegliere un VALORE invece di un
    /// binario — la prima domanda che Phosphor fa all'utente è "per quanti
    /// giorni?", e ridurla a sì/no la renderebbe un'altra domanda.
    alts: Vec<(char, Pending)>,
    /// Etichette dei due bottoni, quando «CONFERMA / ANNULLA» non descrive la
    /// scelta. Il giro di presentazione le usa per «AVANTI / SALTA»: chiedere
    /// «conferma» a chi sta leggendo una spiegazione non vuol dire niente.
    buttons: Option<(&'static str, &'static str)>,
    /// Cosa fare sul secondo bottone. Di norma niente — si annulla e basta —
    /// ma «salta il giro» deve comunque ricordarsi di essere stato saltato.
    cancel: Option<Pending>,
}

/// The "delete an entire project" flow — the most destructive action in the app,
/// so it is guarded hardest. Two phases: `phase == 0` shows the impact and offers
/// a `.phx` backup; `phase == 1` requires the user to RE-TYPE the exact project
/// name before anything is removed (GitHub-style type-to-confirm). Refused up
/// front if the project has any live session.
struct DelProj {
    dir: PathBuf,   // the project directory to remove (confined at delete time)
    name: String,   // display name the user must re-type to confirm
    count: usize,   // sessions in the project
    bytes: u64,     // total size on disk
    typed: String,  // what the user has typed so far (phase 1)
    phase: u8,      // 0 = warn + offer backup, 1 = type-to-confirm
    exported: bool, // a backup .phx has been written this session
}

/// One entry of the right-click context menu: a label, its keyboard hint, and
/// the `dispatch()` action code it triggers.
struct MenuItem {
    key: &'static str,
    label: &'static str,
    action: u16,
}

/// A right-click context menu over a session — every per-session action in one
/// place, anchored where the user clicked. Closes after any choice.
struct Menu {
    title: String,        // the session's title, shown as the menu header
    items: Vec<MenuItem>,
    sel: usize,
    col: u16,             // anchor (the click position), clamped on render
    row: u16,
}

/// Per-visible-row metadata for the collapsible Sessioni list, aligned with
/// `view`. A chain HEAD (the most recent session of a resume/compact chain) is
/// rendered with a +/- marker and `children` > 0; its older siblings are shown
/// indented (depth 1) only while the chain is expanded.
#[derive(Clone, Copy)]
struct RowMeta {
    depth: u8,       // 0 = head/standalone, 1 = a chain child
    children: u32,   // >0 only on a collapsible head
    expanded: bool,  // head open?
    root: usize,     // all-index of the chain ROOT (oldest): the STABLE identity
                     // used to key expand state and to label a collapsed head;
                     // on a child/standalone it is the row's own index.
    certain: bool,   // head: chain has >=1 proven (shared-uuid) link;
                     // child: THIS row is linked to its chain by proof, not heuristic.
}

impl App {
    fn new(base: PathBuf, cache: Arc<Mutex<HashMap<String, Session>>>, prices: Prices, budget: f64, remaps: Vec<(String, String)>, sync_repo: String, watch: u64, theme_idx: usize, pixel: bool, all: Vec<Session>) -> Self {
        let plan = crate::plan::load(&base);
        let fav_cfg = crate::config::load(&base);
        let favorites: HashSet<String> = fav_cfg.favorites.into_iter().collect();
        let notes: HashMap<String, String> = fav_cfg.notes.into_iter().collect();
        let aliases: HashMap<String, String> = fav_cfg.aliases.into_iter().collect();
        let energy_wh_per_output_token = fav_cfg.energy_wh_per_output_token;
        let mouse_only_cfg = fav_cfg.mouse_only;
        let water_l_per_kwh = fav_cfg.water_l_per_kwh;
        // Un listino vecchio non si vede: i costi restano plausibili e sono
        // sbagliati. Se e' il caso, lo dice la prima riga di stato, una volta.
        let prices_stale =
            crate::config::prices_warning(&fav_cfg.prices_as_of, &crate::config::today_iso());
        let mut a = App {
            base, cache, prices, budget, remaps, sync_repo, watch,
            remotes: fav_cfg.remotes, fleet: vec![], fleet_tx: None, scan_tx: None, scanning: false, tour_todo: !fav_cfg.tour_done, fleet_gen: 0, fleet_expect: 0, fleet_got: 0,
            favorites, marked: HashSet::new(), notes, aliases, note_editing: false, note_is_alias: false, note_buf: String::new(),
            energy_wh_per_output_token, water_l_per_kwh, all,
            view: vec![], tab: 0, ts: TableState::default(),
            search: String::new(), searching: false, state_filter: 0, metric: 0,
            sort_col: 0, sort_desc: true, theme_idx, pixel,
            detail: false, help: false, show_agents: false, agents: vec![],
            status: match &prices_stale {
                Some(w) => format!("⚠ {w}"),
                None => t!("pronto · ? aiuto", "ready · ? help").to_string(),
            },
            blink: true, dry: false,
            rect_tabs: Rect::default(), rect_table: Rect::default(), rect_shortcut: Rect::default(),
            rect_overlay: Rect { x: 0, y: 0, width: 0, height: 0 },
            rect_help_actions: Hotspots::default(),
            mouse_only: mouse_only_cfg,
            shortcut_groups: Hotspots::default(), rect_sort_headers: vec![], rect_metric_buttons: vec![],
            rect_detail_buttons: vec![], rect_agents_close: Rect::default(), rect_theme_buttons: vec![],
            confirm: None, rect_confirm_buttons: vec![], detail_opened_ms: 0, plan, help_scroll: 0,
            picker: None, rect_picker_list: Rect::default(),
            reader: None, rect_reader: Rect::default(),
            gsearch: None, rect_gsearch_list: Rect::default(),
            menu: None, rect_menu_items: vec![],
            delproj: None,
            view_all: vec![], row_meta: vec![], expanded_chains: HashSet::new(),
            rect_title_col: Rect::default(),
        };
        link_continuations(&mut a.all);
        link_kin(&mut a.all);
        a.apply_filter();
        a.ts.select(if a.view.is_empty() { None } else { Some(0) });
        a
    }
    fn selected(&self) -> Option<&Session> {
        // .get (not indexing): `view` can be transiently stale vs `all` while a
        // merge rebuild is in progress (e.g. refresh_with_fleet takes `all`
        // before set_sessions recomputes the view).
        self.ts.selected().and_then(|p| self.view.get(p)).and_then(|&i| self.all.get(i))
    }
    fn selected_id(&self) -> Option<String> {
        self.selected().map(|s| s.id.clone())
    }
    fn is_favorite(&self, id: &str) -> bool {
        self.favorites.contains(id)
    }
    /// Guard for actions that need the transcript ON DISK here: true (and a
    /// status hint) when the selection is a fleet session living on another PC.
    fn selected_is_remote(&mut self, action: &str) -> bool {
        let remote = self.selected().map(|s| !s.host.is_empty()).unwrap_or(false);
        if remote {
            self.status = if crate::lang::is_en() { format!("remote session: {action} not available (only r = resume there, d = detail)") } else { format!("sessione remota: {action} non disponibile (solo r = riprendi là, d = dettaglio)") };
        }
        remote
    }
    /// Guard for actions that need a transcript FILE: true (and a status hint)
    /// when the selection was reconstructed from `history.jsonl` because Claude
    /// Code's retention deleted the original (see `crate::recover`).
    fn selected_is_ghost(&mut self, action: &str) -> bool {
        let ghost = self.selected().map(|s| s.is_ghost()).unwrap_or(false);
        if ghost {
            self.status = if crate::lang::is_en() { format!("recovered session (transcript deleted by Claude Code): {action} not available") } else { format!("sessione recuperata (transcript cancellato da Claude Code): {action} non disponibile") };
        }
        ghost
    }
    /// Guard for actions that only make sense inside Claude Code's `projects/`
    /// tree: true (and a status hint) when the selection is a Codex session,
    /// which lives in `~/.codex/sessions` and is laid out per date, not per
    /// project folder.
    fn selected_is_codex(&mut self, action: &str) -> bool {
        let codex = self.selected().map(|s| s.is_codex()).unwrap_or(false);
        if codex {
            self.status = if crate::lang::is_en() { format!("Codex session (~/.codex): {action} not available") } else { format!("sessione Codex (~/.codex): {action} non disponibile") };
        }
        codex
    }
    /// Open the right-click context menu over the session at view-index `idx`,
    /// anchored at the click position. Selects that row first so every action
    /// targets it (the menu items reuse the same `dispatch()` codes as the keys).
    fn open_session_menu(&mut self, col: u16, row: u16, idx: usize) {
        if self.tab != 0 || idx >= self.view.len() { return; }
        self.ts.select(Some(idx));
        let title = self.selected().map(|s| clip(&self.display_title(s).replace('\n', " "), 40)).unwrap_or_default();
        let fav = self.selected_id().map(|id| self.is_favorite(&id)).unwrap_or(false);
        let items = vec![
            MenuItem { key: "d",   label: "Apri dettaglio",        action: A_DETAIL },
            MenuItem { key: "v",   label: "Leggi transcript",      action: A_READ },
            MenuItem { key: "r",   label: "Riprendi sessione",     action: A_RESUME },
            MenuItem { key: "a",   label: "Sub-agenti / workflow", action: A_AGENTS },
            MenuItem { key: "*",   label: if fav { "Togli dai preferiti" } else { "Aggiungi ai preferiti" }, action: A_FAVORITE },
            MenuItem { key: "n",   label: "Nota…",                 action: A_NOTE },
            MenuItem { key: "N",   label: "Rinomina (alias)…",     action: A_ALIAS },
            MenuItem { key: "M",   label: "Esporta Markdown",      action: A_MARKDOWN },
            MenuItem { key: "e",   label: "Esporta CSV+JSON",      action: A_EXPORT },
            MenuItem { key: "x",   label: "Esporta bundle .phx",   action: A_EXPBUNDLE },
            MenuItem { key: "O",   label: "Apri cartella",         action: A_OPENFOLDER },
            MenuItem { key: "y",   label: "Copia percorso",        action: A_COPYPATH },
            MenuItem { key: "H",   label: "Archivia progetto",     action: A_ARCHIVE },
            MenuItem { key: "V",   label: "Ripristina dal vault",  action: A_VAULT_RESTORE },
            MenuItem { key: "W",   label: "Card Wrapped (PNG)",   action: A_WRAPPED },
            MenuItem { key: "D",   label: "Cancella progetto…",    action: A_DELPROJECT },
        ];
        self.menu = Some(Menu { title, items, sel: 0, col, row });
    }
    /// Move the menu highlight by `d` rows (wraps around).
    fn menu_move(&mut self, d: i64) {
        if let Some(m) = &mut self.menu {
            let n = m.items.len() as i64;
            if n > 0 { m.sel = (m.sel as i64 + d).rem_euclid(n) as usize; }
        }
    }
    /// Run the highlighted menu item's action, then close the menu. Returns true
    /// if the app should quit (none of the session actions do, but stay uniform).
    fn menu_activate(&mut self) -> bool {
        let action = self.menu.as_ref().and_then(|m| m.items.get(m.sel)).map(|it| it.action);
        self.menu = None;
        if let Some(a) = action { return dispatch(self, a); }
        false
    }

    // ---- Inline resume-chain collapse (head + folded riprese) ---------------

    /// Derive the VISIBLE list `view` (+ `row_meta`) from the full filtered set
    /// `view_all`, folding each project's resume/compact chain under its most
    /// recent session: that head row carries the +/- marker, and the older
    /// siblings are emitted (indented) only while the chain is expanded.
    fn collapse(&mut self) {
        // Bucket by (host, project) — a remote project with the same name must
        // never fold into a local chain (fleet rows are snapshots of another PC).
        let mut proj: HashMap<(String, String), Vec<usize>> = HashMap::new();
        for &ai in &self.view_all {
            let s = &self.all[ai];
            proj.entry((s.host.clone(), s.project_name.clone())).or_default().push(ai);
        }
        // head_of[member] = the chain's head (latest) session; kids[head] = the
        // rest (newest first); root_of[head] = the chain ROOT (oldest). The root
        // is the STABLE chain identity: it never changes when a newer resume
        // joins, so expand state survives a live resume and the collapsed row can
        // show the original conversation title. head_certain[head] = the chain has
        // at least one PROVEN (shared-uuid) link, not just the title heuristic.
        let mut head_of: HashMap<usize, usize> = HashMap::new();
        let mut kids: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut root_of: HashMap<usize, usize> = HashMap::new();
        let mut head_certain: HashMap<usize, bool> = HashMap::new();
        for members in proj.values() {
            let mut ord = members.clone();
            ord.sort_by_key(|&ai| self.all[ai].mtime_ms); // oldest → newest
            let n = ord.len();
            // Union-find merges two signals into one chain: (a) the title/compact
            // heuristic folds a continuation into the session right before it, and
            // (b) PROVEN kin (sessions that share message uuids) merge regardless
            // of titles.
            let mut uf: Vec<usize> = (0..n).collect();
            for i in 1..n {
                let s = &self.all[ord[i]];
                if s.is_continuation || title_is_resume(&s.title) {
                    uf_union(&mut uf, i, i - 1);
                }
            }
            let mut first_of_group: HashMap<u32, usize> = HashMap::new();
            for i in 0..n {
                let g = self.all[ord[i]].kin_group;
                if g != 0 {
                    match first_of_group.get(&g) {
                        Some(&j) => uf_union(&mut uf, i, j),
                        None => { first_of_group.insert(g, i); }
                    }
                }
            }
            let mut comp: HashMap<usize, Vec<usize>> = HashMap::new();
            for i in 0..n {
                let r = uf_find(&mut uf, i);
                comp.entry(r).or_default().push(ord[i]);
            }
            for (_r, ch) in comp {
                let head = *ch.iter().max_by_key(|&&ai| self.all[ai].mtime_ms).unwrap();
                let root = *ch.iter().min_by_key(|&&ai| self.all[ai].mtime_ms).unwrap();
                let mut others: Vec<usize> = ch.iter().cloned().filter(|&ai| ai != head).collect();
                others.sort_by_key(|&ai| std::cmp::Reverse(self.all[ai].mtime_ms)); // newest first
                let certain = ch.iter().any(|&ai| self.all[ai].kin_group != 0);
                for &m in &ch { head_of.insert(m, head); }
                kids.insert(head, others);
                root_of.insert(head, root);
                head_certain.insert(head, certain);
            }
        }
        // Walk `view_all` in its sorted order; emit heads (with marker), and the
        // children of expanded chains right after their head. Children are skipped
        // in the main walk (they appear under their head).
        let mut view = Vec::with_capacity(self.view_all.len());
        let mut meta = Vec::with_capacity(self.view_all.len());
        for &ai in &self.view_all {
            if head_of.get(&ai) != Some(&ai) { continue; }
            let kid = kids.get(&ai).cloned().unwrap_or_default();
            let root = *root_of.get(&ai).unwrap_or(&ai);
            let exp = self.expanded_chains.contains(&self.all[root].id);
            let certain = *head_certain.get(&ai).unwrap_or(&false);
            view.push(ai);
            meta.push(RowMeta { depth: 0, children: kid.len() as u32, expanded: exp, root, certain });
            if exp {
                for &k in &kid {
                    let kc = self.all[k].kin_group != 0; // this child is proven kin
                    view.push(k);
                    meta.push(RowMeta { depth: 1, children: 0, expanded: false, root: k, certain: kc });
                }
            }
        }
        self.view = view;
        self.row_meta = meta;
    }

    /// The visible-row index of the chain head governing row `pos` (itself if it
    /// is a head, else the nearest head above it).
    fn head_row_of(&self, pos: usize) -> usize {
        if self.row_meta.get(pos).map_or(false, |m| m.depth == 0) { return pos; }
        let mut i = pos;
        while i > 0 {
            i -= 1;
            if self.row_meta.get(i).map_or(false, |m| m.depth == 0) { return i; }
        }
        pos
    }
    /// Expand/collapse the chain owning the current selection, keeping the cursor
    /// on the head. `want`: Some(true)=expand, Some(false)=collapse, None=toggle.
    fn set_chain_expanded(&mut self, want: Option<bool>) {
        let pos = match self.ts.selected() { Some(p) => p, None => return };
        let hp = self.head_row_of(pos);
        let meta = match self.row_meta.get(hp) { Some(m) => *m, None => return };
        if meta.children == 0 { return; } // not collapsible
        let key = self.all[meta.root].id.clone();        // stable chain identity
        let head_id = self.all[self.view[hp]].id.clone(); // always-visible cursor anchor
        let is_exp = self.expanded_chains.contains(&key);
        let target = want.unwrap_or(!is_exp);
        if target == is_exp { return; }
        if target { self.expanded_chains.insert(key); } else { self.expanded_chains.remove(&key); }
        self.collapse();
        if let Some(p) = self.view.iter().position(|&i| self.all[i].id == head_id) { self.ts.select(Some(p)); }
    }
    /// `A`: expand every chain if any is collapsed, else collapse them all.
    fn toggle_all_chains(&mut self) {
        let keep = self.selected_id();
        // If a child (ripresa) is selected, collapsing all hides it — fall back to
        // the chain head, which is always visible, so the cursor never strands.
        let keep_head = self.ts.selected().map(|p| self.head_row_of(p))
            .and_then(|hp| self.view.get(hp)).map(|&i| self.all[i].id.clone());
        let any_collapsed = self.row_meta.iter()
            .any(|m| m.children > 0 && !self.expanded_chains.contains(&self.all[m.root].id));
        if any_collapsed {
            for m in self.row_meta.clone() {
                if m.children > 0 { self.expanded_chains.insert(self.all[m.root].id.clone()); }
            }
            self.status = t!("riprese: tutte espanse", "resumes: all expanded").into();
        } else {
            self.expanded_chains.clear();
            self.status = t!("riprese: tutte compresse", "resumes: all collapsed").into();
        }
        self.collapse();
        // Restore by the kept id if it is still visible, else by its chain head.
        let want = keep.filter(|id| self.view.iter().any(|&i| &self.all[i].id == id)).or(keep_head);
        if let Some(id) = want {
            if let Some(p) = self.view.iter().position(|&i| self.all[i].id == id) { self.ts.select(Some(p)); }
        }
    }

    fn toggle_favorite(&mut self) {
        // With a multi-selection active, toggle the whole set; else the current row.
        if !self.marked.is_empty() {
            let ids: Vec<String> = self.marked.iter().cloned().collect();
            for id in ids {
                if !self.favorites.remove(&id) { self.favorites.insert(id); }
            }
            self.status = if crate::lang::is_en() { format!("★ favorites updated for {} selected", self.marked.len()) } else { format!("★ preferiti aggiornati per {} selezionate", self.marked.len()) };
        } else {
            let id = match self.selected_id() { Some(i) => i, None => return };
            if self.favorites.remove(&id) {
                self.status = t!("☆ rimosso dai preferiti", "☆ removed from favorites").into();
            } else {
                self.favorites.insert(id);
                self.status = t!("★ aggiunto ai preferiti", "★ added to favorites").into();
            }
        }
        if !self.dry { self.persist_config(); }
        if self.state_filter == 4 { self.apply_filter(); } // favorites view may shrink
    }
    /// Toggle the multi-select mark on the current row, then advance one row so a
    /// run of sessions can be marked with repeated Space presses.
    fn toggle_mark(&mut self) {
        if self.selected_is_remote("selezione bulk") { return; }
        if let Some(id) = self.selected_id() {
            if !self.marked.remove(&id) { self.marked.insert(id); }
            self.status = if crate::lang::is_en() { format!("{} selected (space marks · Esc clears · x bundle · X delete · * favorites)", self.marked.len()) } else { format!("{} selezionate (spazio marca · Esc azzera · x bundle · X cancella · * preferiti)", self.marked.len()) };
            self.move_sel(1);
        }
    }
    /// Ask before permanently deleting the MARKED sessions (bulk). Refused if any
    /// marked session is live. Deletes only the chosen transcripts + their sidecars.
    fn request_delete_marked(&mut self) {
        if self.marked.is_empty() {
            self.status = t!("nessuna sessione selezionata (spazio per marcare)", "no session selected (space to mark)").into();
            return;
        }
        let sel: Vec<&Session> = self.all.iter().filter(|s| self.marked.contains(&s.id)).collect();
        if sel.iter().any(|s| s.is_ghost()) {
            self.status = t!("⚠ una selezionata è recuperata: non ha un file da cancellare", "⚠ one selected is recovered: it has no file to delete").into();
            return;
        }
        if sel.iter().any(|s| s.is_codex()) {
            // delete_session_file is confined to projects/ and would refuse
            // anyway; say so up front instead of reporting a silent 0 deleted.
            self.status = t!("⚠ una selezionata è Codex: cancellala con  codex delete <id>", "⚠ one selected is Codex: delete it with  codex delete <id>").into();
            return;
        }
        if sel.iter().any(|s| s.live == "running" || s.live == "idle") {
            self.status = t!("⚠ una selezionata è live: deselezionala prima", "⚠ one selected is live: deselect it first").into();
            return;
        }
        let bytes: u64 = sel.iter().map(|s| s.size).sum();
        let paths: Vec<String> = sel.iter().map(|s| s.path.clone()).collect();
        let (n, mb) = (paths.len(), bytes as f64 / 1_048_576.0);
        let lines = t!(
            vec![
                format!("Cancello {n} sessioni selezionate  ({mb:.1} MB) — IRREVERSIBILE."),
                "Solo i transcript scelti (+ le loro sottocartelle),".into(),
                "confinati a projects/. Nessun altro file toccato.".into(),
            ],
            vec![
                format!("Deleting {n} selected sessions  ({mb:.1} MB) — NO UNDO."),
                "Only the chosen transcripts (+ their subfolders),".into(),
                "confined to projects/. No other file is touched.".into(),
            ],
        );
        self.confirm = Some(Confirm { title: t!(" CONFERMA CANCELLA SELEZIONE ", " CONFIRM DELETE SELECTION ").into(), lines, action: Pending::DeleteMarked { paths }, alts: Vec::new(), buttons: None, cancel: None });
    }
    /// Perform the bulk delete (already confirmed): remove each file confined to
    /// projects/, drop them from the view, clear the selection.
    fn do_delete_marked(&mut self, paths: Vec<String>) {
        let mut n = 0usize;
        for p in &paths {
            if crate::delete_session_file(&self.base, Path::new(p)).is_ok() { n += 1; }
        }
        let gone: HashSet<&str> = paths.iter().map(|s| s.as_str()).collect();
        self.all.retain(|s| !gone.contains(s.path.as_str()));
        self.marked.clear();
        self.apply_filter();
        let v = self.view.len();
        self.ts.select(if v > 0 { Some(0) } else { None });
        self.status = t!(format!("✓ {n} sessioni cancellate"), format!("✓ {n} sessions deleted"));
    }
    fn start_note_edit(&mut self) {
        let id = match self.selected_id() { Some(i) => i, None => return };
        self.note_buf = self.notes.get(&id).cloned().unwrap_or_default();
        self.note_is_alias = false;
        self.note_editing = true;
    }
    /// Reuse the shared text editor to set a custom TITLE (alias) for the session,
    /// shown in the list in place of the (often greeting-like) auto-title.
    fn start_alias_edit(&mut self) {
        let id = match self.selected_id() { Some(i) => i, None => return };
        self.note_buf = self.aliases.get(&id).cloned().unwrap_or_default();
        self.note_is_alias = true;
        self.note_editing = true;
    }
    fn save_note(&mut self) {
        if let Some(id) = self.selected_id() {
            let text = self.note_buf.trim().to_string();
            let (map, kind) = if self.note_is_alias {
                (&mut self.aliases, "alias")
            } else {
                (&mut self.notes, "nota")
            };
            if text.is_empty() {
                map.remove(&id);
                self.status = if crate::lang::is_en() { format!("{kind} removed") } else { format!("{kind} rimosso") };
            } else {
                map.insert(id, text);
                self.status = if crate::lang::is_en() { format!("{kind} saved") } else { format!("{kind} salvato") };
            }
            if !self.dry { self.persist_config(); }
        }
        self.note_editing = false;
        self.note_is_alias = false;
        self.note_buf.clear();
    }
    /// The title to show for a session: the user's alias if set, else the
    /// auto-generated title (newlines flattened at the call site as needed).
    fn display_title<'a>(&'a self, s: &'a Session) -> &'a str {
        self.aliases.get(&s.id).map(|a| a.as_str()).unwrap_or(&s.title)
    }
    /// Open the selected session's PROJECT folder in the OS file manager.
    fn open_session_folder(&mut self) {
        if self.selected_is_remote("apri cartella") { return; }
        let dir = match self.selected().and_then(|s| Path::new(&s.path).parent().map(|p| p.to_string_lossy().into_owned())) {
            Some(d) => d,
            None => return,
        };
        if !self.dry {
            crate::open_folder(&dir);
        }
        self.status = if crate::lang::is_en() { format!("opening  {}", clip(&dir, 44)) } else { format!("apro  {}", clip(&dir, 44)) };
    }
    /// Ask before the fleet fetch: it is the only action that leaves this
    /// machine. It opens an ssh connection to every registered PC and runs a
    /// command there — harmless, but the user should know it is happening
    /// rather than discover it from their firewall.
    fn request_fleet(&mut self) {
        if self.dry {
            return;
        }
        let aliases = crate::config::load(&self.base).remotes;
        if aliases.is_empty() {
            self.status = t!("nessun PC registrato:  phosphor remote add <alias>", "no PC registered:  phosphor remote add <alias>").into();
            return;
        }
        let mut lines = vec![
            "Mi collego via ssh a questi PC e chiedo le loro sessioni:".into(),
            String::new(),
        ];
        for a in aliases.iter().take(8) {
            lines.push(format!("  ssh {a} — phosphor json"));
        }
        lines.push(String::new());
        lines.push("È l'unica cosa che esce da questa macchina. Sola lettura:".into());
        lines.push("niente viene scritto sugli altri PC.".into());
        self.confirm = Some(Confirm {
            title: t!(" CONFERMA INTERROGA FLOTTA ", " CONFIRM QUERY FLEET ").into(),
            lines,
            action: Pending::Fleet,
            alts: Vec::new(), buttons: None, cancel: None,
        });
    }

    /// Ask before restoring from the vault: it writes into the agent's own
    /// store, which is the one place Phosphor otherwise never touches.
    fn request_vault_restore(&mut self) {
        if self.dry {
            return;
        }
        let s = match self.selected() {
            Some(s) if s.is_vaulted() => s.clone(),
            Some(_) => {
                self.status = t!("V vale solo sulle righe ⛁ (quelle salvate dal vault)", "V only works on ⛁ rows (the ones saved by the vault)").into();
                return;
            }
            None => return,
        };
        let agent = if s.is_codex() { "Codex" } else { "Claude Code" };
        let cmd = if s.is_codex() { "codex resume" } else { "claude --resume" };
        self.confirm = Some(Confirm {
            title: t!(" CONFERMA RIPRISTINA DAL VAULT ", " CONFIRM RESTORE FROM VAULT ").into(),
            lines: {
                let title = clip(&s.title.replace('\n', " "), 56);
                t!(
                    vec![
                        format!("Rimetto questo transcript nel magazzino di {agent},"),
                        "da cui era stato cancellato:".into(),
                        String::new(),
                        format!("  {title}"),
                        String::new(),
                        "È un hard link, non una copia: zero byte in più, e il vault".into(),
                        "tiene comunque il suo. Non sovrascrive mai nulla.".into(),
                        String::new(),
                        format!("Dopo, {cmd} la ritrova."),
                    ],
                    vec![
                        format!("I will put this transcript back into {agent}'s own store,"),
                        "which is where it was deleted from:".into(),
                        String::new(),
                        format!("  {title}"),
                        String::new(),
                        "It is a hard link, not a copy: zero extra bytes, and the".into(),
                        "vault keeps its own. Nothing is ever overwritten.".into(),
                        String::new(),
                        format!("After this, {cmd} will find it again."),
                    ],
                )
            },
            action: Pending::VaultRestore,
            alts: Vec::new(), buttons: None, cancel: None,
        });
    }

    /// Switch between "mouse + tasti" and "solo mouse", and remember it.
    ///
    /// The two are not the same interface with a setting flipped. In mouse-only
    /// the bar stops leading with the key and leads with the WORD, because a
    /// letter you are not going to press is noise; the help becomes the place
    /// commands are run from rather than a list to memorise. The keys keep
    /// working either way — taking them away from someone who knows them would
    /// be a loss, not a simplification.
    /// Cambia lingua all'istante: italiano ⇄ inglese.
    ///
    /// Sta su un tasto solo e su un chip visibile perche' chi ne ha bisogno e'
    /// proprio chi non capisce quello che sta leggendo — mandarlo a cercare la
    /// voce in un file di configurazione scritto nella lingua sbagliata
    /// sarebbe una barzelletta.
    fn toggle_lang(&mut self) {
        crate::lang::set_en(!crate::lang::is_en());
        if !self.dry {
            let mut cfg = crate::config::load(&self.base);
            cfg.lang = crate::lang::code().to_string();
            crate::config::save(&self.base, &cfg);
        }
        self.status = t!(
            "lingua: italiano  ·  L per English".to_string(),
            "language: English  ·  L for italiano".to_string(),
        );
    }

    fn toggle_mouse_mode(&mut self) {
        self.mouse_only = !self.mouse_only;
        if !self.dry {
            let mut cfg = crate::config::load(&self.base);
            cfg.mouse_only = self.mouse_only;
            crate::config::save(&self.base, &cfg);
        }
        self.status = if self.mouse_only {
            "solo mouse: clicca le voci in basso, o ? per l'elenco completo cliccabile".into()
        } else {
            "mouse + tasti: le lettere sono di nuovo in evidenza".into()
        };
    }

    /// Ask before writing the Wrapped card: it drops two files on the Desktop,
    /// and every other action that writes there already asks.
    fn request_wrapped(&mut self) {
        if self.dry {
            return;
        }
        self.confirm = Some(Confirm {
            title: t!(" CONFERMA CARD WRAPPED ", " CONFIRM WRAPPED CARD ").into(),
            lines: t!(
                vec![
                    "Scrivo sul Desktop due file NUOVI (mai sovrascritti):".into(),
                    String::new(),
                    "  phosphor-wrapped-<anno>.png   da condividere".into(),
                    "  phosphor-wrapped-<anno>.svg   vettoriale".into(),
                    String::new(),
                    "La card è anonima: solo numeri, nessun nome di progetto".into(),
                    "né percorso né testo dei prompt.".into(),
                ],
                vec![
                    "I will write TWO NEW files to your Desktop (never overwritten):".into(),
                    String::new(),
                    "  phosphor-wrapped-<year>.png   to share".into(),
                    "  phosphor-wrapped-<year>.svg   vector".into(),
                    String::new(),
                    "The card is anonymous: numbers only, no project name,".into(),
                    "no path, no prompt text.".into(),
                ],
            ),
            action: Pending::Wrapped,
            alts: Vec::new(), buttons: None, cancel: None,
        });
    }

    /// Write the Wrapped card to the Desktop, without leaving the list.
    ///
    /// It used to be CLI-only, which meant quitting the app to get the one
    /// thing in it you might want to show someone. The card is built from the
    /// sessions already on screen, so it follows nothing but the current scan —
    /// and it is anonymous, so it can be posted without a second thought.
    fn make_wrapped(&mut self) {
        if self.dry {
            return;
        }
        let cfg = crate::config::load(&self.base);
        let opts = crate::wrapped::Opts {
            window: crate::wrapped::parse_window("anno", now_ms()),
            anonymous: true,
            show_cost: true,
        };
        let card = crate::wrapped::render(&self.all, &cfg, now_ms(), &opts);
        if card.sessions_count == 0 {
            self.status = t!("nessuna sessione quest'anno: niente card", "no sessions this year: no card").into();
            return;
        }
        let dir = std::env::var("USERPROFILE")
            .map(|h| PathBuf::from(h).join("Desktop"))
            .unwrap_or_else(|_| self.base.clone());
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let png = dir.join(format!("phosphor-wrapped-{}-{stamp}.png", card.label));
        let svg = dir.join(format!("phosphor-wrapped-{}-{stamp}.svg", card.label));
        let ok_png = crate::write_new(&png, &card.png).is_ok();
        let _ = crate::write_new(&svg, card.svg.as_bytes());
        self.status = if ok_png {
            format!("✓ {} → {}", card.summary, clip(&png.to_string_lossy(), 42))
        } else {
            "✗ non ho potuto scrivere la card sul Desktop".into()
        };
    }

    /// Ask, once, whether to stop Claude Code from deleting the history.
    ///
    /// Shown on the first run that finds a retention short enough to lose work
    /// (see `crate::retention`). Answering either way is remembered, so the
    /// question is asked once and never nags: the point is to make an invisible
    /// setting visible at the one moment the user can act on it.
    fn maybe_ask_retention(&mut self) {
        if self.confirm.is_some() {
            return;
        }
        let cfg = crate::config::load(&self.base);
        if cfg.retention_asked || !crate::retention::at_risk(&self.base) {
            return;
        }
        let days = crate::retention::effective(&self.base);
        let lines_en = vec![
            format!("Claude Code DELETES its transcripts after {days} days."),
            "It does it by itself at startup: no recycle bin, no backup.".into(),
            "That is why a project untouched for a month vanishes from here.".into(),
            String::new(),
            "I can raise  cleanupPeriodDays  in ~/.claude/settings.json.".into(),
            "I change ONE number: your other settings stay untouched,".into(),
            "and I keep a copy in settings.json.phosphor-bak.".into(),
            String::new(),
            "   1   10 years (3650 days) — recommended".into(),
            "   2   1 year   (365 days)".into(),
            "   3   leave it as it is, stop asking".into(),
            String::new(),
            "What is already deleted does not come back. To lose nothing".into(),
            "else, beyond this setting too:  phosphor vault on".into(),
        ];
        let lines = vec![
            format!("Claude Code CANCELLA i transcript dopo {days} giorni."),
            "Lo fa da solo all'avvio: niente cestino, niente backup.".into(),
            "È il motivo per cui un progetto fermo da un mese sparisce da qui.".into(),
            String::new(),
            "Posso alzare  cleanupPeriodDays  in ~/.claude/settings.json.".into(),
            "Cambio UN numero: le altre tue impostazioni restano intatte,".into(),
            "e ne tengo una copia in settings.json.phosphor-bak.".into(),
            String::new(),
            "   1   10 anni  (3650 giorni) — consigliato".into(),
            "   2   1 anno   (365 giorni)".into(),
            "   3   lascia com'è, non chiedermelo più".into(),
            String::new(),
            "Quello che è già stato cancellato non torna. Per non perdere".into(),
            "altro anche fuori da questa impostazione:  phosphor vault on".into(),
        ];
        self.confirm = Some(Confirm {
            title: t!(" LA TUA CRONOLOGIA SI STA CANCELLANDO ", " YOUR HISTORY IS BEING DELETED ").into(),
            lines: t!(lines, lines_en),
            // Enter/s = la scelta consigliata, così la via rapida è quella giusta.
            action: Pending::SetRetention { days: crate::retention::RECOMMENDED_DAYS },
            alts: vec![
                ('1', Pending::SetRetention { days: crate::retention::RECOMMENDED_DAYS }),
                ('2', Pending::SetRetention { days: 365 }),
                ('3', Pending::SetRetention { days: 0 }),
            ],
            buttons: None,
            cancel: None,
        });
    }

    /// Le pagine del giro di presentazione. Tre, e si chiudono cliccando.
    ///
    /// Serve a una cosa sola: che un collega apra Phosphor per la prima volta e
    /// sappia cosa fare senza leggere il README. Per questo e' scritto in
    /// termini di cose da CLICCARE, non di tasti — i tasti li trova dopo, se li
    /// vuole, e intanto il programma e' gia' utilizzabile.
    const TOUR_PAGES: usize = 3;

    fn tour_page(&mut self, page: u8) {
        let (title, lines): (&str, Vec<String>) = if crate::lang::is_en() {
            match page {
                0 => (
                    " WELCOME — 1 of 3: THE LIST ",
                    vec![
                        "Every row is a working session with an agent:".into(),
                        "Claude Code and the Codex CLI, in one list.".into(),
                        String::new(),
                        "  • click a row          → open the session".into(),
                        "  • click a header       → sort by that column".into(),
                        "  • right-click a row    → every action on it".into(),
                        String::new(),
                        "The dots on the left say how it ended: ● running,".into(),
                        "◐ idle, · done. ⛁ is safe in the vault, ⚱ was".into(),
                        "recovered after the agent had already deleted it.".into(),
                    ],
                ),
                1 => (
                    " WELCOME — 2 of 3: IT ALL WORKS BY MOUSE ",
                    vec![
                        "You do not have to learn a single shortcut.".into(),
                        String::new(),
                        "  • the full list of commands is the  ?  panel,".into(),
                        "    and every row of it IS the command: click the".into(),
                        "    description and it runs. No need to know it was «v».".into(),
                        String::new(),
                        "  • to close any window: CLICK OUTSIDE IT.".into(),
                        "    That is the one gesture worth remembering.".into(),
                        String::new(),
                        "At the bottom there is the switch between the two modes:".into(),
                        "«mouse+keys» also shows the letters while you click,".into(),
                        "«mouse only» gets them out of the way. The keys work".into(),
                        "in both. Beside it,  L  switches language.".into(),
                    ],
                ),
                _ => (
                    " WELCOME — 3 of 3: NOTHING BEHIND YOUR BACK ",
                    vec![
                        "Phosphor only reads. It does not modify transcripts".into(),
                        "and sends nothing over the network on its own.".into(),
                        String::new(),
                        "Anything that writes a file, contacts another PC or".into(),
                        "deletes something tells you FIRST and waits for a yes.".into(),
                        String::new(),
                        "One thing worth knowing right away: Claude Code".into(),
                        "deletes its own transcripts after 30 days, by itself".into(),
                        "and with no recycle bin. To keep them alive at no".into(),
                        "cost in space:".into(),
                        "    phosphor vault on".into(),
                        String::new(),
                        "Enjoy. This tour does not come back; the  ?  help".into(),
                        "is always there.".into(),
                    ],
                ),
            }
        } else {
            match page {
                0 => (
                    " BENVENUTO — 1 di 3: LA LISTA ",
                    vec![
                        "Ogni riga è una sessione di lavoro con un agente:".into(),
                        "Claude Code e Codex CLI, nella stessa lista.".into(),
                        String::new(),
                        "  • clicca una riga        → apri la sessione".into(),
                        "  • clicca un'intestazione → ordina per quella colonna".into(),
                        "  • tasto destro su una riga → tutte le azioni su quella".into(),
                        String::new(),
                        "I pallini a sinistra dicono com'è finita: ● viva, ◐ ferma,".into(),
                        "· conclusa. ⛁ è al sicuro nel vault, ⚱ è stata recuperata".into(),
                        "dopo che l'agente l'aveva già cancellata.".into(),
                    ],
                ),
                1 => (
                    " BENVENUTO — 2 di 3: SI FA TUTTO COL MOUSE ",
                    vec![
                        "Non serve imparare nessuna scorciatoia.".into(),
                        String::new(),
                        "  • l'elenco completo dei comandi è il pannello  ?".into(),
                        "    e ogni sua riga È il comando: clicchi la descrizione".into(),
                        "    e parte. Non devi sapere che era la lettera «v».".into(),
                        String::new(),
                        "  • per chiudere qualunque finestra: CLICCA FUORI.".into(),
                        "    È l'unico gesto che vale la pena ricordare.".into(),
                        String::new(),
                        "In basso c'è l'interruttore fra le due modalità:".into(),
                        "«mouse+tasti» mostra anche le lettere mentre clicchi,".into(),
                        "«solo mouse» le toglie di mezzo. I tasti funzionano".into(),
                        "comunque in entrambe.".into(),
                    ],
                ),
                _ => (
                    " BENVENUTO — 3 di 3: NIENTE ALLE TUE SPALLE ",
                    vec![
                        "Phosphor legge e basta. Non modifica i transcript e non".into(),
                        "manda niente in rete di sua iniziativa.".into(),
                        String::new(),
                        "Tutto ciò che scrive un file, contatta un altro PC o".into(),
                        "cancella qualcosa te lo dice PRIMA e aspetta un sì.".into(),
                        String::new(),
                        "Una cosa che vale la pena sapere subito: Claude Code".into(),
                        "cancella i suoi transcript dopo 30 giorni, da solo e".into(),
                        "senza cestino. Per tenerli vivi senza occupare spazio:".into(),
                        "    phosphor vault on".into(),
                        String::new(),
                        "Buon lavoro. Questo giro non si ripresenta; l'aiuto  ?".into(),
                        "c'è sempre.".into(),
                    ],
                ),
            }
        };
        let last = page as usize + 1 >= Self::TOUR_PAGES;
        self.confirm = Some(Confirm {
            title: title.into(),
            lines,
            action: if last { Pending::TourDone } else { Pending::Tour(page + 1) },
            alts: Vec::new(),
            buttons: Some(match (last, crate::lang::is_en()) {
                (true, true) => ("START", "CLOSE"),
                (true, false) => ("INIZIA", "CHIUDI"),
                (false, true) => ("NEXT", "SKIP"),
                (false, false) => ("AVANTI", "SALTA"),
            }),
            cancel: Some(Pending::TourDone),
        });
    }

    /// Mostra il giro se non è ancora stato visto. Chiamata a ogni giro del
    /// ciclo principale, ma costa due `bool`: la domanda sulla retention arriva
    /// prima e occupa la modale, quindi il giro deve poter aspettare il suo
    /// turno invece di essere saltato per sempre.
    fn maybe_show_tour(&mut self) {
        if !self.tour_todo || self.dry {
            return;
        }
        // Aspetta che lo schermo sia libero. Senza questo controllo bastava
        // aprire l'aiuto o cominciare a scrivere nella ricerca nei primi
        // istanti perche' il benvenuto ci saltasse sopra — e un benvenuto che
        // interrompe e' peggio che nessun benvenuto.
        let busy = self.confirm.is_some()
            || self.delproj.is_some()
            || self.menu.is_some()
            || self.picker.is_some()
            || self.reader.is_some()
            || self.gsearch.is_some()
            || self.help
            || self.detail
            || self.show_agents
            || self.searching
            || self.note_editing;
        if busy {
            return;
        }
        self.tour_page(0);
    }

    fn finish_tour(&mut self) {
        self.tour_todo = false;
        self.confirm = None;
        if !self.dry {
            let mut cfg = crate::config::load(&self.base);
            cfg.tour_done = true;
            crate::config::save(&self.base, &cfg);
        }
        self.status = t!("pronto · ? per l'elenco completo, cliccabile", "ready · ? for the full, clickable list").into();
    }

    /// Apply the answer to [`maybe_ask_retention`]. `days == 0` means "leave it
    /// alone" — nothing is written to Claude Code's settings, but the answer is
    /// recorded so the question does not come back.
    fn do_set_retention(&mut self, days: u64) {
        if self.dry {
            return;
        }
        let mut cfg = crate::config::load(&self.base);
        cfg.retention_asked = true;
        crate::config::save(&self.base, &cfg);
        if days == 0 {
            self.status = format!(
                "lasciato com'è ({} giorni) — puoi cambiarlo con  phosphor retention <giorni>",
                crate::retention::effective(&self.base)
            );
            return;
        }
        self.status = match crate::retention::set(&self.base, days) {
            Ok(_) => format!("✓ Claude Code ora conserva i transcript {days} giorni"),
            Err(e) => format!("✗ non ho potuto modificare settings.json: {e}"),
        };
    }

    /// Put the selected vaulted transcript back into its agent's store, so the
    /// agent can find and resume it again. Another hard link — nothing is
    /// copied and the vault keeps its own name for the same bytes — so this is
    /// safe to do and cheap to undo (delete the restored file).
    fn restore_from_vault(&mut self) {
        if self.dry {
            return;
        }
        let s = match self.selected() {
            Some(s) if s.is_vaulted() => s.clone(),
            Some(_) => {
                self.status = t!("V vale solo sulle righe ⛁ (quelle salvate dal vault)", "V only works on ⛁ rows (the ones saved by the vault)").into();
                return;
            }
            None => return,
        };
        match crate::vault::restore(&self.base, &s) {
            Ok(p) => {
                let cmd = if s.is_codex() { "codex resume" } else { "claude --resume" };
                self.status = if crate::lang::is_en() { format!("✓ put back ({}) — now  {cmd}  finds it again", clip(&p.to_string_lossy(), 40)) } else { format!("✓ rimessa a posto ({}) — ora  {cmd}  la ritrova", clip(&p.to_string_lossy(), 40)) };
                // It is a normal session again: rescan so the row loses its ⛁
                // and becomes resumable in place.
                self.rescan_now();
            }
            Err(e) => {
                self.status = t!(
                    format!("✗ ripristino fallito: {e}"),
                    format!("✗ restore failed: {e}"),
                )
            }
        }
    }
    /// Copy the selected session's transcript path to the system clipboard.
    fn copy_session_path(&mut self) {
        if self.selected_is_remote("copia percorso") { return; }
        if self.selected_is_ghost("copia percorso") { return; }
        let path = match self.selected() { Some(s) => s.path.clone(), None => return };
        let ok = if self.dry { true } else { crate::copy_to_clipboard(&path) };
        self.status = if ok { format!("📋 copiato: {}", clip(&path, 44)) } else { "✗ copia negli appunti fallita".into() };
    }
    /// Ask before ARCHIVING the selected session's project (reversible hide).
    /// Refused if the project has a live session. A single confirm is enough —
    /// nothing is destroyed, so no type-to-confirm (unlike delete).
    fn request_archive_project(&mut self) {
        if self.selected_is_remote("archivia") { return; }
        if self.selected_is_codex("archivia progetto") { return; }
        let sel = match self.selected() { Some(s) => s, None => return };
        let dir = match Path::new(&sel.path).parent() { Some(p) => p.to_path_buf(), None => return };
        let name = if sel.project_name.is_empty() {
            dir.file_name().and_then(|x| x.to_str()).unwrap_or("progetto").to_string()
        } else {
            sel.project_name.clone()
        };
        let (mut count, mut bytes, mut live) = (0usize, 0u64, 0usize);
        for s in &self.all {
            if Path::new(&s.path).parent() == Some(dir.as_path()) {
                count += 1;
                bytes += s.size;
                if s.live == "running" || s.live == "idle" { live += 1; }
            }
        }
        if live > 0 {
            self.status = if crate::lang::is_en() { format!("⚠ {live} live session(s): close them before archiving") } else { format!("⚠ {live} sessione/i live: chiudile prima di archiviare") };
            return;
        }
        let lines = vec![
            format!("Archivio il progetto «{}»  ({} sessioni · {:.1} MB).", name, count, bytes as f64 / 1_048_576.0),
            "Sparisce dalla lista e dai --resume di Claude,".into(),
            "ma NON viene distrutto: è ripristinabile.".into(),
            String::new(),
            format!("  {}  →  archived/", clip(&dir.display().to_string(), 52)),
        ];
        self.confirm = Some(Confirm { title: t!(" CONFERMA ARCHIVIA ", " CONFIRM ARCHIVE ").into(), lines, action: Pending::ArchiveProject { dir, name }, alts: Vec::new(), buttons: None, cancel: None });
    }
    /// Perform the archive (already confirmed), then drop the project's sessions
    /// from the live view.
    fn do_archive_now(&mut self, dir: PathBuf, name: String) {
        match crate::archive_project_dir(&self.base, &dir) {
            Ok(_) => {
                self.all.retain(|s| Path::new(&s.path).parent() != Some(dir.as_path()));
                self.apply_filter();
                let n = self.view.len();
                self.ts.select(if n > 0 { Some(0) } else { None });
                self.status = if crate::lang::is_en() { format!("✓ «{}» archived (restore from the CLI: --unarchive-project)", clip(&name, 24)) } else { format!("✓ «{}» archiviato (ripristina da CLI: --unarchive-project)", clip(&name, 24)) };
            }
            Err(e) => {
                self.status = t!(
                    format!("✗ archiviazione fallita: {e}"),
                    format!("✗ archiving failed: {e}"),
                )
            }
        }
    }
    fn cmp(&self, a: usize, b: usize) -> std::cmp::Ordering {
        let (x, y) = (&self.all[a], &self.all[b]);
        let o = match self.sort_col {
            1 => x.project_name.to_lowercase().cmp(&y.project_name.to_lowercase()),
            2 => x.title.to_lowercase().cmp(&y.title.to_lowercase()),
            3 => x.message_count.cmp(&y.message_count),
            4 => tok_of(x).cmp(&tok_of(y)),
            5 => cost(x, &self.prices).total_cmp(&cost(y, &self.prices)),
            6 => (x.subagents + x.workflows).cmp(&(y.subagents + y.workflows)),
            7 => live_rank(x).cmp(&live_rank(y)),
            8 => x.size.cmp(&y.size),
            _ => x.mtime_ms.cmp(&y.mtime_ms),
        };
        let o = o.then_with(|| x.id.cmp(&y.id));
        if self.sort_desc { o.reverse() } else { o }
    }
    fn apply_filter(&mut self) {
        let q = Query::parse(&self.search);
        let sf = self.state_filter;
        let mut v: Vec<usize> = self.all.iter().enumerate()
            .filter(|(_, s)| {
                let st_ok = match sf { 1 => s.live == "running", 2 => s.live == "idle", 3 => s.live == "ended", 4 => self.favorites.contains(&s.id), _ => true };
                st_ok && q.matches(s)
            })
            .map(|(i, _)| i).collect();
        v.sort_by(|&a, &b| self.cmp(a, b));
        self.view_all = v;
        self.collapse(); // derive the visible (chain-collapsed) `view` + row_meta
        let n = self.view.len();
        match self.ts.selected() {
            Some(p) if p >= n => self.ts.select(if n == 0 { None } else { Some(n - 1) }),
            None if n > 0 => self.ts.select(Some(0)),
            _ => {}
        }
    }
    /// Replace the LOCAL sessions with a fresh scan and rebuild the merged view.
    /// Fleet (remote) rows are re-appended here so every caller — watch tick,
    /// manual rescan, fleet arrival — rebuilds identically. Dedup by id: a
    /// session synced via .phx exists on several PCs; the LOCAL copy wins (it
    /// is richer and the only actionable one), and across hosts the first wins.
    fn set_sessions(&mut self, mut v: Vec<Session>) {
        if !self.fleet.is_empty() {
            let mut seen: HashSet<String> = v.iter().map(|s| s.id.clone()).collect();
            for s in &self.fleet {
                if seen.insert(s.id.clone()) {
                    v.push(s.clone());
                }
            }
        }
        link_continuations(&mut v);
        link_kin(&mut v);
        let keep = self.selected_id();
        self.all = v;
        self.apply_filter();
        if let Some(id) = keep {
            if let Some(p) = self.view.iter().position(|&i| self.all[i].id == id) {
                self.ts.select(Some(p));
            }
        }
    }
    /// Rebuild the merged list after `self.fleet` changed (no local rescan).
    fn refresh_with_fleet(&mut self) {
        let local: Vec<Session> = std::mem::take(&mut self.all).into_iter().filter(|s| s.host.is_empty()).collect();
        self.set_sessions(local);
    }
    /// Key F: query every configured remote over ssh, in a background thread
    /// (sequential; results stream in via the fleet channel). A new press
    /// supersedes the previous fetch via the generation counter.
    fn start_fleet_fetch(&mut self) {
        if self.remotes.is_empty() {
            self.status = t!("nessun PC remoto: configura con  phosphor remote add <alias-ssh>", "no remote PC: set one up with  phosphor remote add <ssh-alias>").into();
            return;
        }
        if self.dry { return; }
        let tx = match &self.fleet_tx { Some(t) => t.clone(), None => return };
        self.fleet_gen += 1;
        self.fleet_expect = self.remotes.len();
        self.fleet_got = 0;
        let gen = self.fleet_gen;
        let aliases = self.remotes.clone();
        std::thread::spawn(move || {
            for a in aliases {
                let res = crate::fleet::fetch_host(&a).map(|b| crate::fleet::parse_sessions_json(&b, &a));
                if tx.send((gen, a, res)).is_err() {
                    break; // UI gone
                }
            }
        });
        self.status = if crate::lang::is_en() { format!("fleet: querying {} hosts over ssh…", self.fleet_expect) } else { format!("flotta: interrogo {} host via ssh…", self.fleet_expect) };
    }
    /// A fleet result arrived on the channel (drained by `run()` when no
    /// overlay is open). Stale generations (superseded by a newer F) are dropped.
    fn on_fleet_msg(&mut self, gen: u64, alias: String, res: Result<Vec<Session>, String>) {
        if gen != self.fleet_gen {
            return;
        }
        self.fleet_got += 1;
        let progress = format!("flotta {}/{}", self.fleet_got, self.fleet_expect);
        match res {
            Ok(list) => {
                let n = list.len();
                self.fleet.retain(|s| s.host != alias);
                self.fleet.extend(list);
                self.refresh_with_fleet();
                self.status = format!("{progress}: {} +{} sessioni (filtro host:{})", clip(&alias, 24), n, clip(&alias, 24));
            }
            Err(e) => {
                self.status = format!("{progress}: {} ✗ {}", clip(&alias, 24), clip(&e, 80));
            }
        }
    }
    fn move_sel(&mut self, delta: i64) {
        let n = self.view.len() as i64;
        if n == 0 { return; }
        let cur = self.ts.selected().unwrap_or(0) as i64;
        self.ts.select(Some((cur + delta).clamp(0, n - 1) as usize));
    }
    /// Re-scan on demand (`R`), off the drawing thread.
    ///
    /// It used to run right here, which meant the window stopped answering —
    /// including the click-outside gesture — for as long as the store took to
    /// read, with nothing on screen to say why. The periodic watcher had been
    /// on its own thread all along; this just sends `R` down the same road,
    /// and the result arrives on the same channel.
    fn rescan_now(&mut self) {
        if self.dry { return; }
        if self.scanning {
            self.status = t!("sto gia' rileggendo…", "already re-reading…").into();
            return;
        }
        let tx = match self.scan_tx.clone() {
            Some(tx) => tx,
            // No channel means nobody is listening (tests, or a caller that
            // drives the App directly): do it here rather than silently not
            // at all.
            None => {
                let (mut s, changed) = { let mut c = self.cache.lock().unwrap(); crate::scan_all(&self.base, &mut c) };
                live::annotate(&self.base, &mut s);
                if changed { cache::save(&self.base, &s); }
                crate::add_recovered(&self.base, &mut s);
                self.set_sessions(s);
                self.status = t!("scan completato", "scan complete").into();
                return;
            }
        };
        let base = self.base.clone();
        let cache = self.cache.clone();
        self.scanning = true;
        self.status = t!("rilettura in corso… (la finestra resta viva)", "re-reading… (the window stays alive)").into();
        std::thread::spawn(move || {
            let (mut s, changed) = { let mut c = cache.lock().unwrap(); crate::scan_all(&base, &mut c) };
            live::annotate(&base, &mut s);
            if changed { cache::save(&base, &s); }
            crate::add_recovered(&base, &mut s);
            let _ = tx.send(s);
        });
    }
    /// Ask before exporting: build unique timestamped paths (so nothing existing
    /// is ever overwritten) and show a confirmation that names both files.
    fn request_export(&mut self) {
        if self.dry { return; }
        let dir = std::env::var("USERPROFILE").map(|h| PathBuf::from(h).join("Desktop")).unwrap_or_else(|_| self.base.clone());
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f").to_string();
        let csv = dir.join(format!("phosphor-export-{stamp}.csv"));
        let json = dir.join(format!("phosphor-export-{stamp}.json"));
        let lines = vec![
            format!("Esporto {} sessioni (vista corrente) in 2 NUOVI file:", self.view_all.len()),
            String::new(),
            format!("  {}", csv.display()),
            format!("  {}", json.display()),
            String::new(),
            "Nome con data/ora: nessun file esistente verrà sovrascritto.".into(),
        ];
        self.confirm = Some(Confirm { title: t!(" CONFERMA EXPORT ", " CONFIRM EXPORT ").into(), lines, action: Pending::Export { csv, json }, alts: Vec::new(), buttons: None, cancel: None });
    }
    /// Actually write the export to the (already confirmed) paths.
    fn do_export(&mut self, csv_path: PathBuf, json_path: PathBuf) {
        let sel: Vec<&Session> = self.view_all.iter().map(|&i| &self.all[i]).collect();
        let mut csv = String::from("project,title,live,messages,input,output,cache,cost_usd,models,gitBranch,version,created,modified,subagents,workflows,path\n");
        for s in &sel {
            let f = |x: &str| crate::csv_field(x);
            csv.push_str(&format!(
                "{},{},{},{},{},{},{},{:.4},{},{},{},{},{},{},{},{}\n",
                f(&s.project_name), f(&s.title), f(&s.live), s.message_count, s.input_tokens, s.output_tokens,
                s.cache_read + s.cache_creation, cost(s, &self.prices), f(&s.models.join(" ")), f(&s.git_branch),
                f(&s.version), f(&s.created), f(&s.modified), s.subagents, s.workflows, f(&s.path)
            ));
        }
        let owned: Vec<Session> = sel.iter().map(|s| (*s).clone()).collect();
        let json = crate::server::sessions_json(&owned);
        let ok = crate::write_new(&csv_path, csv.as_bytes()).is_ok() && crate::write_new(&json_path, json.as_bytes()).is_ok();
        self.status = if ok { format!("✓ esportato: {} (+ .json)", csv_path.display()) } else { "✗ export fallito (file già esistente?)".into() };
    }
    /// Export the selected session's conversation as a readable Markdown file on
    /// the Desktop (timestamped, never overwrites). Pasteable into issues/docs.
    fn export_markdown(&mut self) {
        if self.dry { return; }
        if self.selected_is_remote("export markdown") { return; }
        let s = match self.selected() { Some(s) => s.clone(), None => return };
        let turns = crate::turns_of(&self.base, &s);
        let mut md = String::new();
        // Every field below is derived from the (untrusted) transcript — title,
        // path, id, timestamps, models — so ALL of them go through md_escape, not
        // just the title/body. The backtick code-spans are dropped: a backtick in
        // an escaped value could otherwise break out of the span (md_escape keeps
        // backticks so the body can render code). Numeric fields are inherently safe.
        md.push_str(&format!("# {}\n\n", md_escape(&s.title)));
        md.push_str(&format!("- Progetto: {}\n", md_escape(&s.project_path)));
        md.push_str(&format!("- Sessione: {}\n", md_escape(&s.id)));
        if !s.created.is_empty() { md.push_str(&format!("- Creata: {}\n", md_escape(&s.created))); }
        if !s.modified.is_empty() { md.push_str(&format!("- Ultimo messaggio: {}\n", md_escape(&s.modified))); }
        md.push_str(&format!("- Messaggi: {} · Token: {} · Costo stimato: {}\n", s.message_count, fmt_tok(tok_of(&s)), fmt_usd(cost(&s, &self.prices))));
        if !s.models.is_empty() { md.push_str(&format!("- Modello: {}\n", md_escape(&s.models.join(", ")))); }
        md.push_str("\n---\n\n");
        if turns.is_empty() {
            md.push_str("_(nessun messaggio leggibile)_\n");
        }
        for t in &turns {
            let who = match t.role { 0 => "🧑 tu", 1 => "🤖 claude", _ => "·" };
            md.push_str(&format!("**{}**\n\n", who));
            for line in t.text.split('\n') {
                let safe = md_escape(line);
                if line.starts_with("· tool:") {
                    md.push_str(&format!("> {}\n", safe));
                } else {
                    md.push_str(&safe);
                    md.push('\n');
                }
            }
            md.push('\n');
        }
        let dir = std::env::var("USERPROFILE").map(|h| PathBuf::from(h).join("Desktop")).unwrap_or_else(|_| self.base.clone());
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let path = dir.join(format!("phosphor-{}-{stamp}.md", slugify(&s.title)));
        self.status = match crate::write_new(&path, md.as_bytes()) {
            Ok(()) => format!("✓ Markdown salvato: {}", path.display()),
            Err(_) => "✗ export markdown fallito (file già esistente?)".into(),
        };
    }
    /// Ask before resuming: surface the exact command and working directory.
    fn request_resume(&mut self) {
        if self.dry { return; }
        // Nothing to resume: claude --resume needs the transcript it deleted.
        if self.selected_is_ghost("riprendi") { return; }
        // A fleet session resumes on ITS machine, over ssh — no local cwd involved.
        if let Some(s) = self.selected() {
            if !s.host.is_empty() {
                let (host, id) = (s.host.clone(), s.id.clone());
                let lines = vec![
                    format!("La sessione vive su «{}»: aprirò un NUOVO terminale con:", clip(&host, 24)),
                    String::new(),
                    format!("  ssh -t {} phosphor resume-here {}", clip(&host, 24), clip(&id, 12)),
                    String::new(),
                    "claude riprenderà LÀ, nella cartella giusta di quel PC.".into(),
                    "(serve phosphor+claude installati e ssh raggiungibile)".into(),
                ];
                self.confirm = Some(Confirm { title: t!(" CONFERMA RIPRENDI REMOTO ", " CONFIRM REMOTE RESUME ").into(), lines, action: Pending::RemoteResume { host, id }, alts: Vec::new(), buttons: None, cancel: None });
                return;
            }
        }
        let (id, path, recorded, codex) = match self.selected() {
            Some(s) => (s.id.clone(), s.path.clone(), s.project_path.clone(), s.is_codex()),
            None => return,
        };
        if recorded.is_empty() { self.status = t!("cwd mancante", "working directory missing").into(); return; }
        // The recorded cwd may be a subdir the user cd'd into; claude resumes
        // under the STARTUP cwd's folder (= the transcript's parent dir), so
        // correct it before resolving, else claude can't find the session.
        // Codex indexes threads by id instead of by folder, so it needs none of
        // this — its rollouts live in a date tree, not an encoded-cwd one.
        let recorded = if codex { recorded } else { crate::resume_cwd_for(&path, &recorded) };
        // Resolve the working directory: as-is if it exists here, else via a
        // user-configured cross-PC remap (pathRemaps in phosphor.json).
        let (cwd, fork) = match crate::resolve_cwd(&recorded, &self.remaps) {
            Some(r) => r,
            None => {
                // Path missing and no remap resolves it: let the user pick the
                // local folder interactively (saved as a remap, then resume).
                self.status = if crate::lang::is_en() { format!("folder «{}» not found: pick the local one…", clip(&recorded, 36)) } else { format!("cartella «{}» non trovata: scegli quella locale…", clip(&recorded, 36)) };
                self.open_remap_picker(recorded);
                return;
            }
        };
        let cmdline = if codex {
            format!("  codex {} {}", if fork { "fork" } else { "resume" }, clip(&id, 12))
        } else {
            format!("  claude --resume {}{}", clip(&id, 12), if fork { " --fork-session" } else { "" })
        };
        let mut lines = vec![
            "Aprirò un NUOVO terminale ed eseguirò:".into(),
            String::new(),
            cmdline,
            String::new(),
            "nella cartella di lavoro:".into(),
            format!("  {}", cwd),
        ];
        if fork {
            lines.push(String::new());
            lines.push(format!("(percorso originale non presente: «{}»)", clip(&recorded, 44)));
            lines.push(if codex {
                "remap applicato → uso  codex fork  (sessione derivata).".into()
            } else {
                "remap applicato → uso --fork-session (sessione derivata).".to_string()
            });
        }
        self.confirm = Some(Confirm { title: t!(" CONFERMA RIPRENDI ", " CONFIRM RESUME ").into(), lines, action: Pending::Resume { id, cwd, fork, codex }, alts: Vec::new(), buttons: None, cancel: None });
    }
    /// Actually spawn the resume terminal (already confirmed).
    fn do_resume_now(&mut self, id: String, cwd: String, fork: bool, codex: bool) {
        let ok = if codex {
            crate::resume_codex_session(&cwd, &id, fork)
        } else if fork {
            crate::resume_session_fork(&cwd, &id)
        } else {
            crate::resume_session(&cwd, &id)
        };
        let agent = if codex { "codex" } else { "claude" };
        self.status = if ok { format!("▶ riprendo {} …", clip(&id, 8)) } else { format!("✗ impossibile avviare {agent}") };
        // Belt-and-suspenders: re-assert mouse capture in case spawning the child
        // process perturbed this console's input mode, so clicks keep working.
        if !self.dry {
            let _ = crossterm::execute!(std::io::stdout(), EnableMouseCapture);
        }
    }
    /// Ask before exporting a PORTABLE bundle (.phx) of every session in the
    /// current view: one file you can copy to another PC. Creates a NEW
    /// timestamped file, never overwrites.
    fn request_export_bundle(&mut self) {
        if self.dry { return; }
        let dir = std::env::var("USERPROFILE").map(|h| PathBuf::from(h).join("Desktop")).ok().filter(|d| d.is_dir()).unwrap_or_else(|| self.base.clone());
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let out = dir.join(format!("phosphor-sessioni-{stamp}.phx"));
        let n = if self.marked.is_empty() { self.view_all.len() } else { self.marked.len() };
        let what = if self.marked.is_empty() { "sessioni (vista corrente)" } else { "sessioni SELEZIONATE" };
        let lines = vec![
            format!("Impacchetto {n} {what} (transcript + sottocartelle)"),
            "in UN file portabile, da copiare su un altro PC:".into(),
            String::new(),
            format!("  {}", out.display()),
            String::new(),
            "File NUOVO con data nel nome: non sovrascrive nulla.".into(),
        ];
        self.confirm = Some(Confirm { title: t!(" CONFERMA EXPORT PORTABILE ", " CONFIRM PORTABLE EXPORT ").into(), lines, action: Pending::ExportBundle { out }, alts: Vec::new(), buttons: None, cancel: None });
    }
    /// Actually build and write the bundle (already confirmed). With a
    /// multi-selection active it bundles just those; else the whole current view.
    fn do_export_bundle(&mut self, out: PathBuf) {
        // Fleet rows are excluded: their transcripts live on another PC (no
        // local path), so they'd only add empty manifest entries. Recovered
        // rows are excluded for the same reason — their transcript no longer
        // exists anywhere.
        // Codex rollouts are excluded too: a `.phx` reproduces the
        // `projects/<encoded-cwd>/…` layout Claude Code resumes from, and a
        // Codex thread has no place in it — importing one would drop a file
        // Claude Code can never open.
        let keep = |s: &Session| s.host.is_empty() && !s.is_ghost() && !s.is_codex();
        let owned: Vec<Session> = if self.marked.is_empty() {
            self.view_all.iter().map(|&i| self.all[i].clone()).filter(|s| keep(s)).collect()
        } else {
            self.all.iter().filter(|s| self.marked.contains(&s.id) && keep(s)).cloned().collect()
        };
        let created = chrono::Local::now().to_rfc3339();
        let bytes = crate::bundle::build(&self.base, &owned, &created);
        let mb = bytes.len() as f64 / 1_048_576.0;
        self.status = match crate::write_new(&out, &bytes) {
            Ok(()) => format!("✓ bundle: {} ({:.1} MB)", out.display(), mb),
            Err(_) => "✗ export bundle fallito (file già esistente?)".into(),
        };
    }
    /// Open the "delete entire project" modal for the SELECTED session's project.
    /// Computes the impact from `all`; refuses up front if any session in the
    /// project is live. Nothing is touched here — the modal drives the rest.
    fn request_delete_project(&mut self) {
        if self.selected_is_remote("cancella progetto") { return; }
        if self.selected_is_codex("cancella progetto") { return; }
        let sel = match self.selected() { Some(s) => s, None => return };
        let dir = match Path::new(&sel.path).parent() { Some(p) => p.to_path_buf(), None => return };
        let name = if sel.project_name.is_empty() {
            dir.file_name().and_then(|x| x.to_str()).unwrap_or("progetto").to_string()
        } else {
            sel.project_name.clone()
        };
        let (mut count, mut bytes, mut live) = (0usize, 0u64, 0usize);
        for s in &self.all {
            if Path::new(&s.path).parent() == Some(dir.as_path()) {
                count += 1;
                bytes += s.size;
                if s.live == "running" || s.live == "idle" { live += 1; }
            }
        }
        if live > 0 {
            self.status = if crate::lang::is_en() { format!("⚠ {live} live session(s) in the project: close them before deleting") } else { format!("⚠ {live} sessione/i live nel progetto: chiudile prima di cancellare") };
            return;
        }
        self.delproj = Some(DelProj { dir, name, count, bytes, typed: String::new(), phase: 0, exported: false });
    }
    /// Phase 0: write a portable `.phx` backup of the project before deletion
    /// (the transcripts are the only copy). Never overwrites.
    fn delproj_export(&mut self) {
        if self.dry { return; }
        let (dir, name) = match &self.delproj { Some(d) => (d.dir.clone(), d.name.clone()), None => return };
        let subset: Vec<Session> = self.all.iter().filter(|s| Path::new(&s.path).parent() == Some(dir.as_path())).cloned().collect();
        let outdir = std::env::var("USERPROFILE").map(|h| PathBuf::from(h).join("Desktop")).ok().filter(|d| d.is_dir()).unwrap_or_else(|| self.base.clone());
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let out = outdir.join(format!("phosphor-{}-{stamp}.phx", slugify(&name)));
        let created = chrono::Local::now().to_rfc3339();
        let bytes = crate::bundle::build(&self.base, &subset, &created);
        match crate::write_new(&out, &bytes) {
            Ok(()) => {
                if let Some(d) = &mut self.delproj { d.exported = true; }
                self.status = if crate::lang::is_en() { format!("✓ backup: {}", out.display()) } else { format!("✓ backup: {}", out.display()) };
            }
            Err(_) => self.status = t!("✗ backup fallito (file già esistente?)", "✗ backup failed (file already there?)").into(),
        }
    }
    /// Phase 1: the user typed a name and pressed Enter. Delete the project ONLY
    /// if it matches exactly and (re-checked) no session went live meanwhile.
    fn delproj_confirm(&mut self) {
        let (dir, name) = match &self.delproj { Some(d) => (d.dir.clone(), d.name.clone()), None => return };
        let typed = self.delproj.as_ref().map(|d| d.typed.trim().to_string()).unwrap_or_default();
        if typed != name {
            self.status = t!("il nome non corrisponde: nulla è stato cancellato", "the name does not match: nothing was deleted").into();
            return;
        }
        let live = self.all.iter().any(|s| Path::new(&s.path).parent() == Some(dir.as_path()) && (s.live == "running" || s.live == "idle"));
        if live {
            self.delproj = None;
            self.status = t!("⚠ una sessione è diventata live: annullato", "⚠ a session went live: cancelled").into();
            return;
        }
        if self.dry {
            self.delproj = None;
            self.status = t!("(dry) progetto non cancellato", "(dry) project not deleted").into();
            return;
        }
        match crate::delete_project_dir(&self.base, &dir) {
            Ok(()) => {
                self.all.retain(|s| Path::new(&s.path).parent() != Some(dir.as_path()));
                self.apply_filter();
                let n = self.view.len();
                self.ts.select(if n > 0 { Some(0) } else { None });
                self.status = if crate::lang::is_en() { format!("✓ project «{}» deleted", clip(&name, 30)) } else { format!("✓ progetto «{}» cancellato", clip(&name, 30)) };
            }
            Err(e) => {
                self.status = t!(
                    format!("✗ cancellazione fallita: {e}"),
                    format!("✗ deletion failed: {e}"),
                )
            }
        }
        self.delproj = None;
    }
    /// Open the in-app transcript reader for the selected session.
    fn open_reader(&mut self) {
        if self.selected_is_remote(t!("lettura transcript", "transcript reading")) { return; }
        let s = match self.selected() { Some(s) => s, None => return };
        let title = s.title.replace('\n', " ");
        let s = s.clone();
        let turns = crate::turns_of(&self.base, &s);
        if turns.is_empty() {
            self.status = t!("nessun messaggio leggibile in questa sessione", "no readable message in this session").into();
            return;
        }
        let n = turns.len();
        self.reader = Some(Reader { title, turns, scroll: 0, target_turn: None });
        self.status = if crate::lang::is_en() { format!("reading: {} messages", n) } else { format!("lettura: {} messaggi", n) };
    }
    /// Open the reader for a session by index, positioned at a given turn (used
    /// by global content search to jump straight to the match).
    fn open_reader_at(&mut self, sess: usize, turn: usize) {
        let s = match self.all.get(sess) { Some(s) => s, None => return };
        let title = s.title.replace('\n', " ");
        let s = s.clone();
        let turns = crate::turns_of(&self.base, &s);
        if turns.is_empty() {
            self.status = t!("nessun messaggio leggibile in questa sessione", "no readable message in this session").into();
            return;
        }
        self.reader = Some(Reader { title, turns, scroll: 0, target_turn: Some(turn) });
    }
    fn reader_scroll(&mut self, delta: i32) {
        if let Some(r) = &mut self.reader {
            r.scroll = (r.scroll as i32 + delta).max(0) as u16;
        }
    }
    /// Open the global content-search overlay (empty, in typing mode).
    fn open_gsearch(&mut self) {
        self.gsearch = Some(GSearch { query: String::new(), typing: true, results: Vec::new(), sel: 0, scroll: 0 });
    }
    /// Run the grep across every session's real transcript and collect hits.
    fn run_gsearch(&mut self) {
        let needle = match &self.gsearch {
            Some(g) => g.query.trim().to_lowercase(),
            None => return,
        };
        let mut results: Vec<GHit> = Vec::new();
        if needle.len() >= 2 {
            const PER_SESSION: usize = 4;
            const TOTAL: usize = 300;
            // newest-first (app.all is sorted by mtime desc)
            for (idx, s) in self.all.iter().enumerate() {
                if results.len() >= TOTAL {
                    break;
                }
                // Fleet rows have no transcript on this disk — and with identical
                // directory layouts across PCs a remote path could even WRONGLY
                // match a local file. Skip them.
                if !s.host.is_empty() {
                    continue;
                }
                for h in crate::grep_of(&self.base, s, &needle, PER_SESSION) {
                    results.push(GHit {
                        sess: idx,
                        turn: h.turn,
                        role: h.role,
                        title: s.title.replace('\n', " "),
                        project: s.project_name.clone(),
                        snippet: h.snippet,
                    });
                    if results.len() >= TOTAL {
                        break;
                    }
                }
            }
        }
        if let Some(g) = &mut self.gsearch {
            self.status = if crate::lang::is_en() { format!("«{}»: {} results", g.query.trim(), results.len()) } else { format!("«{}»: {} risultati", g.query.trim(), results.len()) };
            g.results = results;
            g.sel = 0;
            g.scroll = 0;
            g.typing = false;
        }
    }
    fn gsearch_move(&mut self, delta: i64) {
        if let Some(g) = &mut self.gsearch {
            let n = g.results.len() as i64;
            if n == 0 { return; }
            g.sel = (g.sel as i64 + delta).clamp(0, n - 1) as usize;
        }
    }
    /// Open the selected hit in the reader, positioned at its turn.
    fn gsearch_open(&mut self) {
        let (sess, turn) = match self.gsearch.as_ref().and_then(|g| g.results.get(g.sel)) {
            Some(h) => (h.sess, h.turn),
            None => return,
        };
        self.gsearch = None;
        self.open_reader_at(sess, turn);
    }
    fn desktop_or_base(&self) -> PathBuf {
        std::env::var("USERPROFILE")
            .map(|h| PathBuf::from(h).join("Desktop"))
            .ok()
            .filter(|d| d.is_dir())
            .unwrap_or_else(|| self.base.clone())
    }
    /// Open the ASCII file picker to choose a `.phx` bundle to import. Starts on
    /// the Desktop (where exports land), then on the `.claude` dir.
    fn open_picker(&mut self) {
        let start = self.desktop_or_base();
        self.picker_show(start, PickPurpose::Import);
    }
    /// Open the browser in DIRECTORY mode to pick the local folder a session's
    /// recorded working directory should map to (cross-PC resume).
    fn open_remap_picker(&mut self, recorded: String) {
        let start = self.desktop_or_base();
        self.picker_show(start, PickPurpose::Remap { recorded });
    }
    fn picker_show(&mut self, dir: PathBuf, purpose: PickPurpose) {
        let dirs_only = matches!(purpose, PickPurpose::Remap { .. });
        let mut entries = read_dir_entries(&dir, dirs_only);
        if dirs_only {
            entries.insert(0, PickEntry {
                label: format!("✓ usa QUESTA cartella  →  {}", clip(&dir.to_string_lossy(), 40)),
                path: dir.clone(),
                is_dir: false,
                select_here: true,
            });
        }
        self.picker = Some(Picker { dir, entries, sel: 0, scroll: 0, purpose });
    }
    fn picker_move(&mut self, delta: i64) {
        if let Some(p) = &mut self.picker {
            let n = p.entries.len() as i64;
            if n == 0 { return; }
            p.sel = (p.sel as i64 + delta).clamp(0, n - 1) as usize;
        }
    }
    fn picker_to(&mut self, idx: usize) {
        if let Some(p) = &mut self.picker {
            if idx < p.entries.len() { p.sel = idx; }
        }
    }
    fn picker_parent(&mut self) {
        let nav = self.picker.as_ref().and_then(|p| p.dir.parent().map(|x| (x.to_path_buf(), p.purpose.clone())));
        if let Some((par, purpose)) = nav { self.picker_show(par, purpose); }
    }
    /// Act on the selected entry: enter a directory, pick a file to import, or
    /// (remap mode) select the current directory as the resume folder.
    fn picker_enter(&mut self) {
        let chosen = self.picker.as_ref().and_then(|p| {
            p.entries.get(p.sel).map(|e| (e.path.clone(), e.is_dir, e.select_here, p.purpose.clone()))
        });
        let (path, is_dir, select_here, purpose) = match chosen { Some(x) => x, None => return };
        if select_here {
            self.remap_select(path);
        } else if is_dir {
            self.picker_show(path, purpose);
        } else if matches!(purpose, PickPurpose::Import) {
            self.picker = None;
            self.request_import_path(path);
        }
    }
    /// Save the chosen folder as the remap for the pending session, persist it,
    /// then re-trigger Resume (which now resolves and forks).
    fn remap_select(&mut self, dir: PathBuf) {
        let recorded = match self.picker.as_ref().map(|p| &p.purpose) {
            Some(PickPurpose::Remap { recorded, .. }) => recorded.clone(),
            _ => { self.picker = None; return; }
        };
        let dir_s = dir.to_string_lossy().to_string();
        self.remaps.retain(|(f, _)| f != &recorded); // upsert
        self.remaps.push((recorded.clone(), dir_s.clone()));
        self.persist_config();
        self.picker = None;
        self.status = if crate::lang::is_en() { format!("remap saved: {} → {}", clip(&recorded, 22), clip(&dir_s, 22)) } else { format!("remap salvato: {} → {}", clip(&recorded, 22), clip(&dir_s, 22)) };
        self.request_resume();
    }
    fn persist_config(&self) {
        // Start from disk to keep hand-edited keys we don't model in App
        // (sync encryption, manual price edits), then overlay the live state.
        let mut cfg = crate::config::load(&self.base);
        cfg.prices = self.prices.clone();
        cfg.theme = THEME_NAMES[self.theme_idx % THEME_COUNT].to_string();
        cfg.pixel = self.pixel;
        cfg.watch = self.watch;
        cfg.budget = self.budget;
        cfg.path_remaps = self.remaps.clone();
        cfg.sync_repo = self.sync_repo.clone();
        cfg.favorites = self.favorites.iter().cloned().collect();
        cfg.notes = self.notes.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        cfg.aliases = self.aliases.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        crate::config::save(&self.base, &cfg);
    }
    /// Inspect a chosen bundle and stage the import behind a confirmation.
    /// The import only ADDS missing files — it never overwrites or deletes.
    fn request_import_path(&mut self, src: PathBuf) {
        if self.dry { return; }
        let data = match std::fs::read(&src) { Ok(d) => d, Err(_) => { self.status = t!("impossibile leggere il .phx", "cannot read the .phx").into(); return; } };
        let parsed = match crate::bundle::inspect(&data) { Ok(p) => p, Err(e) => { self.status = if crate::lang::is_en() { format!("invalid bundle: {e}") } else { format!("bundle non valido: {e}") }; return; } };
        // Apply the persistent pathRemaps so a bundle from another PC lands under
        // THIS machine's project paths (natively resumable). Same remaps the `r`
        // resume uses.
        let plan = crate::bundle::apply_remapped(&self.base, &data, &parsed, true, &self.remaps);
        if plan.added == 0 {
            self.status = if crate::lang::is_en() { format!("{}: nothing to add (already there)", file_name(&src)) } else { format!("{}: niente da aggiungere (già presente)", file_name(&src)) };
            return;
        }
        let mb = plan.bytes as f64 / 1_048_576.0;
        // Note any source project that is neither present here nor covered by a
        // remap: it will import but won't be resumable until the path exists.
        let unmapped: Vec<&String> = parsed.manifest.projects.iter()
            .filter(|p| !self.remaps.iter().any(|(f, _)| &f == p) && !std::path::Path::new(p).is_dir())
            .collect();
        let mut lines = vec![
            format!("File: {}", file_name(&src)),
            format!("Origine: {} ({})", if parsed.manifest.source_host.is_empty() { "?" } else { &parsed.manifest.source_host }, if parsed.manifest.source_os.is_empty() { "?" } else { &parsed.manifest.source_os }),
            String::new(),
            format!("Aggiungo {} file ({:.1} MB) in:", plan.added, mb),
            format!("  {}", self.base.join("projects").display()),
            format!("Già presenti (saltati): {}", plan.skipped),
        ];
        if plan.rejected > 0 { lines.push(format!("⚠ percorsi non sicuri rifiutati: {}", plan.rejected)); }
        if !unmapped.is_empty() {
            lines.push(String::new());
            lines.push(format!("⚠ {} progetto/i con percorso non locale: per il resume", unmapped.len()));
            lines.push("   servirà un remap (CLI: import --remap \"orig=locale\").".into());
        }
        lines.push(String::new());
        lines.push("Solo aggiunta: non sovrascrive, non modifica, non elimina.".into());
        self.confirm = Some(Confirm { title: t!(" CONFERMA IMPORT ", " CONFIRM IMPORT ").into(), lines, action: Pending::ImportBundle { src }, alts: Vec::new(), buttons: None, cancel: None });
    }
    /// Actually import the bundle (already confirmed).
    fn do_import_bundle(&mut self, src: PathBuf) {
        let data = match std::fs::read(&src) { Ok(d) => d, Err(_) => { self.status = t!("impossibile leggere il .phx", "cannot read the .phx").into(); return; } };
        let parsed = match crate::bundle::inspect(&data) { Ok(p) => p, Err(e) => { self.status = if crate::lang::is_en() { format!("invalid bundle: {e}") } else { format!("bundle non valido: {e}") }; return; } };
        let _ = std::fs::create_dir_all(self.base.join("projects"));
        let rep = crate::bundle::apply_remapped(&self.base, &data, &parsed, false, &self.remaps);
        self.status = if crate::lang::is_en() { format!("✓ imported {} files, {} skipped", rep.added, rep.skipped) } else { format!("✓ importati {} file, {} saltati", rep.added, rep.skipped) };
        self.rescan_now();
    }
    /// Run a confirmed pending action.
    fn run_pending(&mut self, action: Pending) {
        if let Pending::Tour(n) = action {
            if (n as usize) < Self::TOUR_PAGES { self.tour_page(n); } else { self.finish_tour(); }
            return;
        }
        if let Pending::TourDone = action {
            self.finish_tour();
            return;
        }
        if self.dry { return; } // test mode: never touch disk / spawn
        match action {
            // Gia' gestite sopra: passano prima del controllo `dry` perche' non
            // toccano nulla, se non per ricordare che il giro e' stato visto.
            Pending::Tour(_) | Pending::TourDone => {}
            Pending::Export { csv, json } => self.do_export(csv, json),
            Pending::Resume { id, cwd, fork, codex } => self.do_resume_now(id, cwd, fork, codex),
            Pending::SetRetention { days } => self.do_set_retention(days),
            Pending::Wrapped => self.make_wrapped(),
            Pending::Fleet => self.start_fleet_fetch(),
            Pending::VaultRestore => self.restore_from_vault(),
            Pending::ExportBundle { out } => self.do_export_bundle(out),
            Pending::ImportBundle { src } => self.do_import_bundle(src),
            Pending::ArchiveProject { dir, name } => self.do_archive_now(dir, name),
            Pending::DeleteMarked { paths } => self.do_delete_marked(paths),
            Pending::RemoteResume { host, id } => self.do_remote_resume_now(host, id),
        }
    }
    /// Actually open the ssh terminal towards the remote session (confirmed).
    fn do_remote_resume_now(&mut self, host: String, id: String) {
        let ok = crate::resume_remote_session(&host, &id);
        self.status = if ok {
            format!("▶ riprendo {} su {} …", clip(&id, 8), clip(&host, 24))
        } else {
            "✗ impossibile avviare ssh".into()
        };
        let _ = crossterm::execute!(std::io::stdout(), EnableMouseCapture);
    }
    fn open_agents(&mut self) {
        if self.dry { return; }
        if let Some(s) = self.selected() {
            self.agents = load_agents(&s.path, &self.prices);
            self.show_agents = true;
        }
    }
}

fn live_rank(s: &Session) -> u8 {
    match s.live.as_str() { "running" => 3, "idle" => 2, _ => 1 }
}

fn read_meta(path: &Path) -> (String, String) {
    let (mut kind, mut desc) = (String::new(), String::new());
    if let Ok(b) = std::fs::read(path) {
        let mut p = P::new(&b);
        if p.obj_begin() {
            loop {
                let k = match p.obj_key() { Some(k) => k, None => break };
                match k.as_str() {
                    "agentType" => kind = p.take_string().unwrap_or_default(),
                    "description" => desc = p.take_string().unwrap_or_default(),
                    _ => { let _ = p.skip(); }
                }
                if !p.obj_sep() { break; }
            }
        }
    }
    (kind, desc)
}
fn collect_agents(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                collect_agents(&p, out);
            } else if let Some(name) = p.file_name().and_then(|x| x.to_str()) {
                if name.starts_with("agent-") && name.ends_with(".jsonl") {
                    out.push(p);
                }
            }
        }
    }
}
fn load_agents(session_jsonl: &str, prices: &Prices) -> Vec<AgentInfo> {
    let dir = Path::new(session_jsonl).with_extension("");
    let mut files = Vec::new();
    collect_agents(&dir, &mut files);
    let mut out = Vec::new();
    for f in files {
        let size = std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
        if let Some(s) = scan::parse_one(&f, size) {
            let (kind, desc) = read_meta(&f.with_extension("meta.json"));
            out.push(AgentInfo {
                kind: if kind.is_empty() { "agent".into() } else { kind },
                desc, msgs: s.message_count, tok: tok_of(&s), cost: cost(&s, prices),
            });
        }
    }
    out.sort_by(|a, b| b.tok.cmp(&a.tok));
    out
}

pub fn selftest(base: PathBuf, all: Vec<Session>, cache: Arc<Mutex<HashMap<String, Session>>>) -> bool {
    use ratatui::backend::TestBackend;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let r = catch_unwind(AssertUnwindSafe(|| {
        let mut app = App::new(base, cache, Prices::default(), 0.0, Vec::new(), String::new(), 5, 0, false, all);
        app.dry = true;
        for sz in [(140u16, 42u16), (80, 24), (40, 12)] {
            let mut term = Terminal::new(TestBackend::new(sz.0, sz.1)).expect("backend");
            for tab in 0..3 {
                app.tab = tab;
                app.metric = tab as u8;
                term.draw(|f| ui(f, &mut app)).expect("draw");
                // click every cell across all interactive rows to flex hit-testing
                for r in 0..sz.1 {
                    for c in (0..sz.0).step_by(2) {
                        let _ = handle_mouse(&mut app, mk_click(c, r));
                    }
                }
            }
            app.tab = 0;
            app.detail = true;
            term.draw(|f| ui(f, &mut app)).expect("draw detail");
            for r in 0..sz.1 { for c in (0..sz.0).step_by(3) { let _ = handle_mouse(&mut app, mk_click(c, r)); } app.detail = true; }
            app.show_agents = true;
            app.agents = vec![AgentInfo { kind: "Explore".into(), desc: "x".into(), msgs: 3, tok: 9, cost: 0.1 }];
            term.draw(|f| ui(f, &mut app)).expect("draw agents");
            app.show_agents = false;
            app.detail = false;
            app.help = true;
            term.draw(|f| ui(f, &mut app)).expect("draw help");
            for r in 0..sz.1 { for c in (0..sz.0).step_by(3) { let _ = handle_mouse(&mut app, mk_click(c, r)); } app.help = true; }
            app.help = false;
            app.searching = true;
            app.search = "x".into();
            term.draw(|f| ui(f, &mut app)).expect("draw search");
            app.searching = false;
            // confirmation modal: render + flex its key/mouse handlers (dry: no I/O)
            app.confirm = Some(Confirm {
                title: t!(" CONFERMA EXPORT ", " CONFIRM EXPORT ").into(),
                lines: vec!["riga di prova".into(), "C:/un/percorso/molto/lungo/file.csv".into()],
                action: Pending::Export { csv: PathBuf::from("x.csv"), json: PathBuf::from("x.json") },
                alts: Vec::new(), buttons: None, cancel: None,
            });
            term.draw(|f| ui(f, &mut app)).expect("draw confirm");
            for r in 0..sz.1 { for c in (0..sz.0).step_by(3) { let _ = handle_mouse(&mut app, mk_click(c, r)); } }
            app.confirm = Some(Confirm { title: " C ".into(), lines: vec![], action: Pending::Resume { id: "0".into(), cwd: "C:/Windows".into(), fork: false, codex: false }, alts: Vec::new(), buttons: None, cancel: None });
            let _ = handle_key(&mut app, KeyCode::Esc, KeyModifiers::empty());
            // portable bundle export + import confirm modals (dry: no I/O)
            app.confirm = Some(Confirm { title: " EXP ".into(), lines: vec!["bundle".into()], action: Pending::ExportBundle { out: PathBuf::from("x.phx") }, alts: Vec::new(), buttons: None, cancel: None });
            term.draw(|f| ui(f, &mut app)).expect("draw expbundle");
            let _ = handle_key(&mut app, KeyCode::Enter, KeyModifiers::empty());
            app.confirm = Some(Confirm { title: " IMP ".into(), lines: vec!["import".into()], action: Pending::ImportBundle { src: PathBuf::from("x.phx") }, alts: Vec::new(), buttons: None, cancel: None });
            term.draw(|f| ui(f, &mut app)).expect("draw import");
            let _ = handle_key(&mut app, KeyCode::Esc, KeyModifiers::empty());
            // request_* paths (dry mode short-circuits before any disk access)
            app.request_export_bundle();
            app.confirm = None;
            // file picker: open, navigate, render, parent, then cancel (no I/O)
            app.open_picker();
            term.draw(|f| ui(f, &mut app)).expect("draw picker");
            let _ = handle_key(&mut app, KeyCode::Down, KeyModifiers::empty());
            let _ = handle_key(&mut app, KeyCode::PageDown, KeyModifiers::empty());
            let _ = handle_key(&mut app, KeyCode::Enter, KeyModifiers::empty());
            term.draw(|f| ui(f, &mut app)).expect("draw picker 2");
            let _ = handle_key(&mut app, KeyCode::Left, KeyModifiers::empty());
            for r in 0..sz.1 { let _ = handle_mouse(&mut app, mk_click(sz.0 / 2, r)); }
            app.picker = None;
            // The picker click-sweep above may have selected a real .phx on the
            // Desktop and opened a confirm modal; clear leftover overlay state so
            // it can't swallow the reader-close click below.
            app.confirm = None; app.detail = false; app.show_agents = false; app.help = false; app.gsearch = None;

            // transcript reader overlay: open, scroll, render, close (in-memory).
            app.reader = Some(Reader {
                title: "demo".into(),
                turns: vec![
                    crate::scan::Turn { role: 0, text: "ciao come va".into() },
                    crate::scan::Turn { role: 1, text: "tutto bene, ecco una riga lunghissima ".repeat(20) },
                    crate::scan::Turn { role: 2, text: "· tool: Read".into() },
                ],
                scroll: 0,
                target_turn: Some(1),
            });
            term.draw(|f| ui(f, &mut app)).expect("draw reader");
            let _ = handle_key(&mut app, KeyCode::Down, KeyModifiers::empty());
            let _ = handle_key(&mut app, KeyCode::PageDown, KeyModifiers::empty());
            let _ = handle_key(&mut app, KeyCode::End, KeyModifiers::empty());
            term.draw(|f| ui(f, &mut app)).expect("draw reader 2");
            // Il contratto e' cambiato, e questo selftest teneva ancora il
            // vecchio: si clicca DENTRO per usare il lettore — prima un click
            // qualunque lo chiudeva, quindi non ci si poteva fare niente — e
            // FUORI per chiuderlo.
            let mid = (sz.0 / 2, sz.1 / 2);
            let _ = handle_mouse(&mut app, mk_click(mid.0, mid.1));
            assert!(app.reader.is_some(), "un click dentro NON deve chiudere il lettore");
            let out = app.rect_overlay;
            assert!(out.width > 0, "il lettore deve registrare il suo riquadro");
            let _ = handle_mouse(&mut app, mk_click(out.x.saturating_sub(1), out.y));
            assert!(app.reader.is_none(), "un click fuori deve chiudere il lettore");

            // global content search: open, type (short → no heavy grep), render,
            // browse, close. open_reader_at jump path is rendered too.
            app.open_gsearch();
            let _ = handle_key(&mut app, KeyCode::Char('z'), KeyModifiers::empty());
            term.draw(|f| ui(f, &mut app)).expect("draw gsearch typing");
            let _ = handle_key(&mut app, KeyCode::Enter, KeyModifiers::empty());
            term.draw(|f| ui(f, &mut app)).expect("draw gsearch results");
            let _ = handle_key(&mut app, KeyCode::Down, KeyModifiers::empty());
            let _ = handle_mouse(&mut app, mk_click(sz.0 / 2, sz.1 - 2));
            let _ = handle_key(&mut app, KeyCode::Esc, KeyModifiers::empty());
            app.gsearch = None;
            if !app.all.is_empty() {
                app.open_reader_at(0, 0);
                term.draw(|f| ui(f, &mut app)).expect("draw reader jump");
                app.reader = None;
            }

            // favorites + notes (dry: no disk persistence) and the ★ filter.
            app.tab = 0;
            if !app.view.is_empty() { app.ts.select(Some(0)); }
            app.toggle_favorite();
            app.start_note_edit();
            let _ = handle_key(&mut app, KeyCode::Char('o'), KeyModifiers::empty());
            let _ = handle_key(&mut app, KeyCode::Char('k'), KeyModifiers::empty());
            let _ = handle_key(&mut app, KeyCode::Enter, KeyModifiers::empty());
            term.draw(|f| ui(f, &mut app)).expect("draw note");
            app.state_filter = 4; app.apply_filter();
            term.draw(|f| ui(f, &mut app)).expect("draw fav filter");
            app.state_filter = 0; app.apply_filter();

            // Regression guard for the reported mouse bug: a single click on a
            // row (even one that isn't already selected) must open its detail,
            // and after closing, clicking ANOTHER row must reopen on the first
            // click. The row-open path is intentionally debounce-free.
            app.detail = false; app.help = false; app.show_agents = false;
            app.searching = false; app.search.clear(); app.apply_filter();
            app.tab = 0; app.ts.select(Some(0));
            term.draw(|f| ui(f, &mut app)).expect("draw list");
            let t = app.rect_table;
            if app.view.len() >= 2 && t.height >= 4 && app.ts.offset() == 0 {
                let (cx, first) = (t.x + 2, t.y + 2);
                // click row 1 (NOT the selected row 0) → must open its detail
                let _ = handle_mouse(&mut app, mk_click(cx, first + 1));
                assert!(app.detail, "click su riga non selezionata deve aprire il dettaglio");
                assert_eq!(app.ts.selected(), Some(1), "il click deve selezionare la riga cliccata");
                // close, then click a DIFFERENT row → must reopen on first click
                let _ = handle_key(&mut app, KeyCode::Esc, KeyModifiers::empty());
                assert!(!app.detail, "Esc deve chiudere il dettaglio");
                let _ = handle_mouse(&mut app, mk_click(cx, first));
                assert!(app.detail, "dopo la chiusura, click su altra riga deve riaprire al primo click");
                assert_eq!(app.ts.selected(), Some(0));
                app.detail = false; app.detail_opened_ms = 0;

                // Right-click context menu: a right-click on a row opens it and
                // selects that row; rendering + key/mouse handlers are flexed; a
                // chosen item runs and closes the menu; a stray click dismisses it.
                let _ = handle_mouse(&mut app, mk_rclick(cx, first + 1));
                assert!(app.menu.is_some(), "il tasto destro su una riga deve aprire il menù");
                assert_eq!(app.ts.selected(), Some(1), "il tasto destro deve selezionare la riga sotto il cursore");
                term.draw(|f| ui(f, &mut app)).expect("draw menu");
                let _ = handle_key(&mut app, KeyCode::Down, KeyModifiers::empty());
                let _ = handle_key(&mut app, KeyCode::Up, KeyModifiers::empty());
                // sweep clicks over the menu rows (hit-tests every item rect)
                for r in 0..sz.1 { for c in (0..sz.0).step_by(2) { if app.menu.is_some() { let _ = handle_mouse(&mut app, mk_click(c, r)); } } }
                app.menu = None; app.detail = false; app.confirm = None; app.note_editing = false;
                app.reader = None; app.show_agents = false; app.help = false; app.picker = None; app.gsearch = None;
                // Esc closes the menu without side effects.
                let _ = handle_mouse(&mut app, mk_rclick(cx, first));
                assert!(app.menu.is_some(), "menù riaperto col tasto destro");
                let _ = handle_key(&mut app, KeyCode::Esc, KeyModifiers::empty());
                assert!(app.menu.is_none(), "Esc deve chiudere il menù");
                // Keyboard opener '.' then activate the first item (Apri dettaglio).
                let _ = handle_key(&mut app, KeyCode::Char('.'), KeyModifiers::empty());
                assert!(app.menu.is_some(), "il tasto . deve aprire il menù sulla riga selezionata");
                let _ = handle_key(&mut app, KeyCode::Enter, KeyModifiers::empty());
                assert!(app.menu.is_none(), "scegliere una voce deve chiudere il menù");
                assert!(app.detail, "la prima voce del menù apre il dettaglio");
                app.detail = false; app.detail_opened_ms = 0; app.confirm = None;

                // Accelerator key: open, press 'd' (Apri dettaglio) → runs + closes.
                let _ = handle_key(&mut app, KeyCode::Char('.'), KeyModifiers::empty());
                let _ = handle_key(&mut app, KeyCode::Char('d'), KeyModifiers::empty());
                assert!(app.menu.is_none() && app.detail, "l'accelerator 'd' apre il dettaglio e chiude il menù");
                app.detail = false; app.detail_opened_ms = 0; app.confirm = None;

                // While editing a note, a right-click must NOT open the menu.
                app.start_note_edit();
                let _ = handle_mouse(&mut app, mk_rclick(cx, first));
                assert!(app.menu.is_none(), "niente menù sopra l'editor di nota");
                app.note_editing = false; app.note_buf.clear();

                // Collapsible chains: expand all, collapse all, +/-/Space on a row,
                // and a click sweep (some rows carry the +/- marker), no panics.
                app.menu = None; app.detail = false; app.confirm = None;
                app.toggle_all_chains();
                term.draw(|f| ui(f, &mut app)).expect("draw chains expanded");
                app.toggle_all_chains();
                term.draw(|f| ui(f, &mut app)).expect("draw chains collapsed");
                app.ts.select(Some(0));
                let _ = handle_key(&mut app, KeyCode::Char('+'), KeyModifiers::empty());
                let _ = handle_key(&mut app, KeyCode::Char('-'), KeyModifiers::empty());
                let _ = handle_key(&mut app, KeyCode::Char(' '), KeyModifiers::empty());
                let _ = handle_key(&mut app, KeyCode::Char(' '), KeyModifiers::empty());
                term.draw(|f| ui(f, &mut app)).expect("draw after +/-");
                // click sweep over the rows (may hit a +/- marker → toggles)
                for r in 0..sz.1 { for c in (0..sz.0).step_by(3) { let _ = handle_mouse(&mut app, mk_click(c, r)); app.detail = false; app.confirm = None; } }
                if !app.view.is_empty() { app.ts.select(Some(0)); assert!(app.selected().is_some()); }
            }
        }
        // Right-click menu on a very short terminal: the scroll window keeps the
        // selection visible (real hit-rect) and never panics.
        {
            let mut term = Terminal::new(TestBackend::new(50, 8)).expect("backend");
            app.tab = 0; app.detail = false; app.help = false; app.show_agents = false;
            app.confirm = None; app.menu = None; app.note_editing = false; app.searching = false;
            if !app.view.is_empty() {
                app.ts.select(Some(0));
                app.open_session_menu(2, 3, 0);
                term.draw(|f| ui(f, &mut app)).expect("draw menu short");
                for _ in 0..8 { let _ = handle_key(&mut app, KeyCode::Down, KeyModifiers::empty()); } // to last item
                term.draw(|f| ui(f, &mut app)).expect("draw menu short scrolled");
                if let Some(sel) = app.menu.as_ref().map(|m| m.sel) {
                    assert!(app.rect_menu_items.get(sel).map(|r| r.width > 0).unwrap_or(false),
                        "la voce selezionata resta visibile (scroll) anche su terminale corto");
                }
                app.menu = None; app.detail = false; app.confirm = None;
            }
        }
        let mut term = Terminal::new(TestBackend::new(120, 40)).expect("backend");
        for ti in 0..THEME_COUNT {
            app.theme_idx = ti;
            app.help = true;
            term.draw(|f| ui(f, &mut app)).expect("draw theme");
            app.help = false;
        }
        // scrollable help: exercise scroll keys + clamp, then close
        app.help = true; app.help_scroll = 0;
        for _ in 0..40 { let _ = handle_key(&mut app, KeyCode::Down, KeyModifiers::empty()); }
        term.draw(|f| ui(f, &mut app)).expect("draw help scrolled");
        let _ = handle_key(&mut app, KeyCode::PageUp, KeyModifiers::empty());
        let _ = handle_key(&mut app, KeyCode::Home, KeyModifiers::empty());
        let _ = handle_key(&mut app, KeyCode::Esc, KeyModifiers::empty());
        // pixel mode across all tabs + popups
        app.pixel = true;
        for tab in 0..3 {
            app.tab = tab;
            term.draw(|f| ui(f, &mut app)).expect("draw pixel");
        }
        app.tab = 0;
        app.detail = true;
        term.draw(|f| ui(f, &mut app)).expect("draw pixel detail");
        app.detail = false;
        true
    }));
    r.unwrap_or(false)
}
#[allow(dead_code)]
fn mk_click(col: u16, row: u16) -> event::MouseEvent {
    event::MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: col, row, modifiers: KeyModifiers::empty() }
}
#[allow(dead_code)]
fn mk_rclick(col: u16, row: u16) -> event::MouseEvent {
    event::MouseEvent { kind: MouseEventKind::Down(MouseButton::Right), column: col, row, modifiers: KeyModifiers::empty() }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    base: PathBuf,
    all: Vec<Session>,
    cache: Arc<Mutex<HashMap<String, Session>>>,
    prices: Prices,
    budget: f64,
    remaps: Vec<(String, String)>,
    sync_repo: String,
    theme_idx: usize,
    pixel: bool,
    watch: u64,
) -> std::io::Result<()> {
    let (tx, rx) = mpsc::channel::<Vec<Session>>();
    // A clone for the on-demand re-scan (`R`), so it lands the same way the
    // watcher does instead of blocking the draw.
    let scan_tx = tx.clone();
    {
        let base = base.clone();
        let cache = cache.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(watch));
            let (mut s, changed) = { let mut c = cache.lock().unwrap(); crate::scan_all(&base, &mut c) };
            live::annotate(&base, &mut s);
            if changed { cache::save(&base, &s); }
            crate::add_recovered(&base, &mut s);
            if tx.send(s).is_err() { break; }
        });
    }
    // Fleet results (key F) stream in on their own channel; the fetch threads
    // are spawned on demand by `start_fleet_fetch` with a clone of `ftx`.
    let (ftx, frx) = mpsc::channel::<FleetMsg>();

    let mut term = ratatui::init();
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::SetTitle("Phosphor — session scanner"),
        EnableMouseCapture
    );
    let mut app = App::new(base, cache, prices, budget, remaps, sync_repo, watch, theme_idx, pixel, all);
    app.fleet_tx = Some(ftx);
    app.scan_tx = Some(scan_tx);
    // Prima cosa che si vede, se serve: la retention di Claude Code sta
    // cancellando la cronologia che l'utente e' appena venuto a guardare.
    app.maybe_ask_retention();
    let mut last_blink = Instant::now();
    let res = (|| -> std::io::Result<()> {
        loop {
            // Buffer list updates while any overlay is open: replacing `all`
            // mid-interaction would invalidate the indices behind the reader,
            // gsearch results, menus and confirmations. mpsc queues them; we
            // drain as soon as the overlay closes.
            let overlay = app.confirm.is_some() || app.menu.is_some() || app.reader.is_some()
                || app.gsearch.is_some() || app.picker.is_some() || app.delproj.is_some()
                || app.note_editing;
            if !overlay {
                while let Ok(v) = rx.try_recv() {
                    app.set_sessions(v);
                    // Whoever sent it, the window is current again.
                    if app.scanning {
                        app.scanning = false;
                        app.status = t!("scan completato", "scan complete").into();
                    }
                }
                while let Ok((gen, alias, res)) = frx.try_recv() {
                    app.on_fleet_msg(gen, alias, res);
                }
            }
            // Il giro di presentazione aspetta che la modale sia libera: la
            // domanda sulla retention viene prima, e ha ragione lei.
            app.maybe_show_tour();
            if last_blink.elapsed() >= Duration::from_millis(550) {
                app.blink = !app.blink;
                last_blink = Instant::now();
            }
            term.draw(|f| ui(f, &mut app))?;
            if event::poll(Duration::from_millis(200))? {
                match event::read()? {
                    Event::Key(k) if k.kind == KeyEventKind::Press => {
                        if handle_key(&mut app, k.code, k.modifiers) { break; }
                    }
                    Event::Mouse(m) => {
                        if handle_mouse(&mut app, m) { break; }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    })();
    let _ = crossterm::execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    app.persist_config();
    res
}

fn set_theme(app: &mut App, idx: usize) {
    app.theme_idx = idx % THEME_COUNT;
    app.status = if crate::lang::is_en() { format!("theme: {}", THEME_NAMES[app.theme_idx]) } else { format!("tema: {}", THEME_NAMES[app.theme_idx]) };
}

/// Dispatch a shortcut/chip action. Returns true if the app should quit.
fn dispatch(app: &mut App, code: u16) -> bool {
    match code {
        A_UP => app.move_sel(-3),
        A_DOWN => app.move_sel(3),
        A_SEARCH => { if app.tab == 0 { app.searching = true; } }
        A_FILTER => { app.state_filter = (app.state_filter + 1) % 5; app.apply_filter(); }
        A_SORTCOL => { app.sort_col = (app.sort_col + 1) % sorts().len(); app.apply_filter(); }
        A_SORTDIR => { app.sort_desc = !app.sort_desc; app.apply_filter(); }
        A_METRIC => app.metric = (app.metric + 1) % 3,
        A_THEME_NEXT => set_theme(app, app.theme_idx + 1),
        A_THEME_PREV => set_theme(app, app.theme_idx + THEME_COUNT - 1),
        A_RESUME => app.request_resume(),
        A_AGENTS => { if app.tab == 0 { app.open_agents(); } }
        A_RESCAN => app.rescan_now(),
        A_EXPORT => app.request_export(),
        A_EXPBUNDLE => app.request_export_bundle(),
        A_IMPORT => app.open_picker(),
        A_DETAIL => { if app.tab == 0 && app.selected().is_some() { app.detail = true; } }
        A_READ => { if app.tab == 0 && app.selected().is_some() { app.open_reader(); } }
        A_GSEARCH => app.open_gsearch(),
        A_MARKDOWN => { if app.tab == 0 && app.selected().is_some() { app.export_markdown(); } }
        A_FAVORITE => { if app.tab == 0 && app.selected().is_some() { app.toggle_favorite(); } }
        A_NOTE => { if app.tab == 0 && app.selected().is_some() { app.start_note_edit(); } }
        A_EXPAND_ALL => { if app.tab == 0 { app.toggle_all_chains(); } }
        A_DELPROJECT => { if app.tab == 0 && app.selected().is_some() { app.request_delete_project(); } }
        A_ALIAS => { if app.tab == 0 && app.selected().is_some() { app.start_alias_edit(); } }
        A_OPENFOLDER => { if app.tab == 0 && app.selected().is_some() { app.open_session_folder(); } }
        A_COPYPATH => { if app.tab == 0 && app.selected().is_some() { app.copy_session_path(); } }
        A_ARCHIVE => { if app.tab == 0 && app.selected().is_some() { app.request_archive_project(); } }
        A_FLEET => app.request_fleet(),
        A_VAULT_RESTORE => app.request_vault_restore(),
        A_WRAPPED => app.request_wrapped(),
        A_MOUSE_MODE => app.toggle_mouse_mode(),
        A_LANG => app.toggle_lang(),
        A_HELP => { app.help = true; app.help_scroll = 0; }
        A_TAB => app.tab = (app.tab + 1) % 3,
        A_PIXEL => { app.pixel = !app.pixel; app.status = if app.pixel { "pixel ON".into() } else { "pixel OFF".into() }; }
        A_QUIT => return true,
        _ => {}
    }
    false
}

fn handle_key(app: &mut App, code: KeyCode, mods: KeyModifiers) -> bool {
    // A pending confirmation captures all input until answered.
    if app.confirm.is_some() {
        // An alternative answer (a value instead of yes/no) wins over the
        // default keys, so a confirmation that offers choices can bind any
        // character it likes.
        if let KeyCode::Char(ch) = code {
            let alt = app
                .confirm
                .as_ref()
                .and_then(|c| c.alts.iter().position(|(k, _)| *k == ch));
            if let Some(i) = alt {
                if let Some(mut c) = app.confirm.take() {
                    app.run_pending(c.alts.remove(i).1);
                }
                return false;
            }
        }
        match code {
            KeyCode::Char('s') | KeyCode::Char('S') | KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                if let Some(c) = app.confirm.take() { app.run_pending(c.action); }
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Char('q') => {
                app.confirm = None;
                app.status = t!("annullato", "cancelled").into();
            }
            _ => {}
        }
        return false;
    }
    // The right-click context menu captures input while open.
    if app.menu.is_some() {
        match code {
            KeyCode::Up | KeyCode::Char('k') => app.menu_move(-1),
            KeyCode::Down | KeyCode::Char('j') => app.menu_move(1),
            KeyCode::Enter => return app.menu_activate(),
            KeyCode::Esc | KeyCode::Char('q') => { app.menu = None; app.status = t!("menù chiuso", "menu closed").into(); }
            // The letters shown on each row are live accelerators: jump to that
            // item and run it. (No item uses j/k/q, so navigation stays intact.)
            KeyCode::Char(c) => {
                let target = app.menu.as_ref()
                    .and_then(|m| m.items.iter().position(|it| it.key.chars().next() == Some(c)));
                if let Some(i) = target {
                    if let Some(m) = &mut app.menu { m.sel = i; }
                    return app.menu_activate();
                }
            }
            _ => {}
        }
        return false;
    }
    if app.picker.is_some() {
        match code {
            KeyCode::Up | KeyCode::Char('k') => app.picker_move(-1),
            KeyCode::Down | KeyCode::Char('j') => app.picker_move(1),
            KeyCode::PageUp => app.picker_move(-10),
            KeyCode::PageDown => app.picker_move(10),
            KeyCode::Home => app.picker_to(0),
            KeyCode::End => app.picker_move(i64::MAX / 2),
            KeyCode::Enter | KeyCode::Right => app.picker_enter(),
            KeyCode::Left | KeyCode::Backspace => app.picker_parent(),
            KeyCode::Esc | KeyCode::Char('q') => { app.picker = None; app.status = t!("annullato", "cancelled").into(); }
            _ => {}
        }
        return false;
    }
    if app.gsearch.is_some() {
        let typing = app.gsearch.as_ref().map(|g| g.typing).unwrap_or(false);
        if code == KeyCode::Esc {
            app.gsearch = None;
            return false;
        }
        if typing {
            match code {
                KeyCode::Enter => app.run_gsearch(),
                KeyCode::Backspace => { if let Some(g) = &mut app.gsearch { g.query.pop(); } }
                KeyCode::Char(c) => { if let Some(g) = &mut app.gsearch { g.query.push(c); } }
                _ => {}
            }
        } else {
            match code {
                KeyCode::Up | KeyCode::Char('k') => app.gsearch_move(-1),
                KeyCode::Down | KeyCode::Char('j') => app.gsearch_move(1),
                KeyCode::PageUp => app.gsearch_move(-10),
                KeyCode::PageDown => app.gsearch_move(10),
                KeyCode::Home => app.gsearch_move(i64::MIN / 2),
                KeyCode::End => app.gsearch_move(i64::MAX / 2),
                KeyCode::Enter => app.gsearch_open(),
                KeyCode::Char('/') | KeyCode::Char('g') => { if let Some(g) = &mut app.gsearch { g.typing = true; } }
                _ => {}
            }
        }
        return false;
    }
    if app.reader.is_some() {
        match code {
            KeyCode::Up | KeyCode::Char('k') => app.reader_scroll(-1),
            KeyCode::Down | KeyCode::Char('j') => app.reader_scroll(1),
            KeyCode::PageUp => app.reader_scroll(-15),
            KeyCode::PageDown => app.reader_scroll(15),
            KeyCode::Home => app.reader_scroll(i32::MIN / 2),
            KeyCode::End => app.reader_scroll(i32::MAX / 2),
            KeyCode::Char('M') | KeyCode::Char('m') => app.export_markdown(),
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::Char('v') => app.reader = None,
            _ => {}
        }
        return false;
    }
    if app.help {
        match code {
            KeyCode::Up | KeyCode::Char('k') => app.help_scroll = app.help_scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => app.help_scroll = app.help_scroll.saturating_add(1),
            KeyCode::PageUp => app.help_scroll = app.help_scroll.saturating_sub(8),
            KeyCode::PageDown => app.help_scroll = app.help_scroll.saturating_add(8),
            KeyCode::Home => app.help_scroll = 0,
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::Char('?') | KeyCode::Char('h') | KeyCode::F(1) => app.help = false,
            _ => {}
        }
        return false;
    }
    if app.show_agents {
        if matches!(code, KeyCode::Esc | KeyCode::Char('a') | KeyCode::Enter | KeyCode::Char('q')) {
            app.show_agents = false;
        }
        return false;
    }
    if app.detail {
        match code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('d') | KeyCode::Char('q') => { app.detail = false; app.detail_opened_ms = 0; }
            KeyCode::Char('v') => { app.detail = false; app.detail_opened_ms = 0; app.open_reader(); }
            KeyCode::Char('r') => app.request_resume(),
            KeyCode::Char('a') => app.open_agents(),
            KeyCode::Char('*') => app.toggle_favorite(),
            KeyCode::Char('n') => app.start_note_edit(),
            _ => {}
        }
        return false;
    }
    if app.note_editing {
        match code {
            KeyCode::Esc => { app.note_editing = false; app.note_is_alias = false; app.note_buf.clear(); app.status = t!("annullato", "cancelled").into(); }
            KeyCode::Enter => app.save_note(),
            KeyCode::Backspace => { app.note_buf.pop(); }
            KeyCode::Char(c) => { app.note_buf.push(c); }
            _ => {}
        }
        return false;
    }
    // The "delete project" modal captures ALL input while open (destructive).
    if app.delproj.is_some() {
        let phase = app.delproj.as_ref().map(|d| d.phase).unwrap_or(0);
        match code {
            KeyCode::Esc => { app.delproj = None; app.status = t!("cancellazione annullata", "deletion cancelled").into(); }
            _ if phase == 0 => match code {
                // Phase 0: warn + offer a .phx backup, then step to type-to-confirm.
                KeyCode::Char('e') | KeyCode::Char('E') => app.delproj_export(),
                KeyCode::Enter => { if let Some(d) = &mut app.delproj { d.phase = 1; } }
                _ => {}
            },
            _ => match code {
                // Phase 1: the user must re-type the exact project name.
                KeyCode::Enter => app.delproj_confirm(),
                KeyCode::Backspace => { if let Some(d) = &mut app.delproj { d.typed.pop(); } }
                KeyCode::Char(c) => { if let Some(d) = &mut app.delproj { d.typed.push(c); } }
                _ => {}
            },
        }
        return false;
    }
    if app.searching {
        match code {
            KeyCode::Esc => { app.search.clear(); app.searching = false; app.apply_filter(); }
            KeyCode::Enter => app.searching = false,
            KeyCode::Backspace => { app.search.pop(); app.apply_filter(); }
            KeyCode::Char(c) => { app.search.push(c); app.apply_filter(); }
            _ => {}
        }
        return false;
    }
    // Collapse/expand a resume-chain in the Sessioni list with the keys the user
    // expects (+ / - / Space) — applied to the chain owning the current row.
    if app.tab == 0 && !app.searching {
        match code {
            KeyCode::Char('+') => { app.set_chain_expanded(Some(true)); return false; }
            KeyCode::Char('-') => { app.set_chain_expanded(Some(false)); return false; }
            // Space toggles the multi-select mark on the current row (chain
            // expand/collapse stays on + / -). Marking then moves down one row.
            KeyCode::Char(' ') => { app.toggle_mark(); return false; }
            _ => {}
        }
    }
    match code {
        KeyCode::Char('q') => return true,
        KeyCode::Char('c') if mods.contains(KeyModifiers::CONTROL) => return true,
        KeyCode::Char('?') | KeyCode::Char('h') | KeyCode::F(1) => { app.help = true; app.help_scroll = 0; }
        KeyCode::Tab | KeyCode::Right => app.tab = (app.tab + 1) % 3,
        KeyCode::Left => app.tab = (app.tab + 2) % 3,
        KeyCode::Char('1') => app.tab = 0,
        KeyCode::Char('2') => app.tab = 1,
        KeyCode::Char('3') => app.tab = 2,
        KeyCode::Down | KeyCode::Char('j') => app.move_sel(1),
        KeyCode::Up | KeyCode::Char('k') => app.move_sel(-1),
        KeyCode::PageDown => app.move_sel(10),
        KeyCode::PageUp => app.move_sel(-10),
        KeyCode::Home => app.ts.select(Some(0)),
        KeyCode::End => { let n = app.view.len(); if n > 0 { app.ts.select(Some(n - 1)); } }
        KeyCode::Enter | KeyCode::Char('d') => { if app.tab == 0 && app.selected().is_some() { app.detail = true; } }
        KeyCode::Char('v') => { if app.tab == 0 && app.selected().is_some() { app.open_reader(); } }
        KeyCode::Char('M') => { if app.tab == 0 && app.selected().is_some() { app.export_markdown(); } }
        KeyCode::Char('*') => { if app.tab == 0 && app.selected().is_some() { app.toggle_favorite(); } }
        KeyCode::Char('n') => { if app.tab == 0 && app.selected().is_some() { app.start_note_edit(); } }
        KeyCode::Char('N') => { if app.tab == 0 && app.selected().is_some() { app.start_alias_edit(); } }
        KeyCode::Char('O') => { if app.tab == 0 && app.selected().is_some() { app.open_session_folder(); } }
        KeyCode::Char('y') => { if app.tab == 0 && app.selected().is_some() { app.copy_session_path(); } }
        KeyCode::Char('H') => { if app.tab == 0 && app.selected().is_some() { app.request_archive_project(); } }
        KeyCode::Char('F') => app.start_fleet_fetch(),
        KeyCode::Char('V') => { if app.tab == 0 && app.selected().is_some() { app.restore_from_vault(); } }
        KeyCode::Char('W') => app.make_wrapped(),
        KeyCode::Char('L') | KeyCode::Char('l') => app.toggle_lang(),
        KeyCode::Char('X') => { if app.tab == 0 { app.request_delete_marked(); } }
        KeyCode::Esc => { if !app.marked.is_empty() { app.marked.clear(); app.status = t!("selezione azzerata", "selection cleared").into(); } }
        KeyCode::Char('g') => app.open_gsearch(),
        KeyCode::Char('a') => { if app.tab == 0 && app.selected().is_some() { app.open_agents(); } }
        KeyCode::Char('/') => { if app.tab == 0 { app.searching = true; } }
        KeyCode::Char('r') => app.request_resume(),
        KeyCode::Char('e') => app.request_export(),
        KeyCode::Char('x') => app.request_export_bundle(),
        KeyCode::Char('i') => app.open_picker(),
        KeyCode::Char('t') => set_theme(app, app.theme_idx + 1),
        KeyCode::Char('T') => set_theme(app, app.theme_idx + THEME_COUNT - 1),
        KeyCode::Char('p') => { app.pixel = !app.pixel; app.status = if app.pixel { "pixel ON".into() } else { "pixel OFF".into() }; }
        KeyCode::Char('m') => app.metric = (app.metric + 1) % 3,
        KeyCode::Char('f') => { app.state_filter = (app.state_filter + 1) % 5; app.apply_filter(); }
        KeyCode::Char('o') => { app.sort_col = (app.sort_col + 1) % sorts().len(); app.apply_filter(); }
        KeyCode::Char('s') => { app.sort_desc = !app.sort_desc; app.apply_filter(); }
        KeyCode::Char('R') => app.rescan_now(),
        KeyCode::Char('A') => { if app.tab == 0 { app.toggle_all_chains(); } }
        KeyCode::Char('D') => { if app.tab == 0 && app.selected().is_some() { app.request_delete_project(); } }
        // Open the context menu on the selected row (keyboard equiv. of right-click).
        KeyCode::Char('.') | KeyCode::Menu => {
            if app.tab == 0 {
                if let Some(sel) = app.ts.selected() {
                    let t = app.rect_table;
                    let off = app.ts.offset();
                    let row = t.y + 2 + sel.saturating_sub(off) as u16;
                    app.open_session_menu(t.x + 2, row, sel);
                }
            }
        }
        _ => {}
    }
    false
}

/// True when a left click at (col,row) landed OUTSIDE the overlay on screen.
///
/// One rule for every panel: clicking the darkened background dismisses it,
/// clicking inside does not. Before this, each overlay decided for itself — the
/// reader and the help closed on *any* click, so you could not click inside
/// them at all, while the picker and the global search could only be left with
/// the keyboard. Neither is usable with a mouse alone.
fn clicked_outside(app: &App, m: &event::MouseEvent) -> bool {
    matches!(m.kind, MouseEventKind::Down(MouseButton::Left))
        && app.rect_overlay.width > 0
        && !within(app.rect_overlay, m.column, m.row)
}

fn handle_mouse(app: &mut App, m: event::MouseEvent) -> bool {
    let (col, row) = (m.column, m.row);
    // The delete-project modal is keyboard-only: swallow every mouse event so a
    // stray click can never reach the list or a button behind it.
    if app.delproj.is_some() {
        return false;
    }
    // A pending confirmation captures all input: only its buttons are clickable.
    if app.confirm.is_some() {
        if let MouseEventKind::Down(MouseButton::Left) = m.kind {
            let mut act = None;
            for (i, r) in app.rect_confirm_buttons.iter().enumerate() { if hit(*r, col, row) { act = Some(i); break; } }
            match act {
                Some(0) => { if let Some(c) = app.confirm.take() { app.run_pending(c.action); } }
                Some(1) => {
                    // Il secondo bottone di solito annulla e basta; quando
                    // porta un'azione («salta il giro») va eseguita, o la
                    // scelta non verrebbe ricordata.
                    match app.confirm.take().and_then(|c| c.cancel) {
                        Some(p) => app.run_pending(p),
                        None => app.status = t!("annullato", "cancelled").into(),
                    }
                }
                _ => {}
            }
        }
        return false;
    }
    // A right-click context menu captures input while open: pick an item, or
    // click/right-click elsewhere to dismiss.
    if app.menu.is_some() {
        match m.kind {
            MouseEventKind::ScrollDown => app.menu_move(1),
            MouseEventKind::ScrollUp => app.menu_move(-1),
            MouseEventKind::Down(MouseButton::Left) => {
                let mut hit_item = None;
                for (i, r) in app.rect_menu_items.iter().enumerate() { if hit(*r, col, row) { hit_item = Some(i); break; } }
                match hit_item {
                    Some(i) => { if let Some(mm) = &mut app.menu { mm.sel = i; } return app.menu_activate(); }
                    None => { app.menu = None; app.status = t!("menù chiuso", "menu closed").into(); }
                }
            }
            MouseEventKind::Down(MouseButton::Right) => { app.menu = None; app.status = t!("menù chiuso", "menu closed").into(); }
            _ => {}
        }
        return false;
    }
    // File picker captures input while open.
    if app.picker.is_some() {
        if clicked_outside(app, &m) {
            app.picker = None;
            app.status = t!("annullato", "cancelled").into();
            return false;
        }
        match m.kind {
            MouseEventKind::ScrollDown => app.picker_move(3),
            MouseEventKind::ScrollUp => app.picker_move(-3),
            MouseEventKind::Down(MouseButton::Left) => {
                let lr = app.rect_picker_list;
                if row >= lr.y && row < lr.y + lr.height && col >= lr.x && col < lr.x + lr.width {
                    let scroll = app.picker.as_ref().map(|p| p.scroll).unwrap_or(0);
                    let idx = scroll + (row - lr.y) as usize;
                    let valid = app.picker.as_ref().map(|p| idx < p.entries.len()).unwrap_or(false);
                    if valid { app.picker_to(idx); app.picker_enter(); }
                }
            }
            _ => {}
        }
        return false;
    }
    if app.gsearch.is_some() {
        if clicked_outside(app, &m) {
            app.gsearch = None;
            return false;
        }
        match m.kind {
            MouseEventKind::ScrollDown => app.gsearch_move(3),
            MouseEventKind::ScrollUp => app.gsearch_move(-3),
            MouseEventKind::Down(MouseButton::Left) => {
                let lr = app.rect_gsearch_list;
                if row >= lr.y && row < lr.y + lr.height && col >= lr.x && col < lr.x + lr.width {
                    let scroll = app.gsearch.as_ref().map(|g| g.scroll).unwrap_or(0);
                    let idx = scroll + (row - lr.y) as usize;
                    let valid = app.gsearch.as_ref().map(|g| idx < g.results.len()).unwrap_or(false);
                    if valid {
                        if let Some(g) = &mut app.gsearch { g.sel = idx; }
                        app.gsearch_open();
                    }
                }
            }
            _ => {}
        }
        return false;
    }
    // Transcript reader captures input while open.
    if app.reader.is_some() {
        match m.kind {
            MouseEventKind::ScrollDown => app.reader_scroll(3),
            MouseEventKind::ScrollUp => app.reader_scroll(-3),
            // Only a click on the background closes it. Closing on ANY click
            // made the reader impossible to click into — you could not even put
            // the cursor in it without losing your place.
            MouseEventKind::Down(MouseButton::Left) if clicked_outside(app, &m) => app.reader = None,
            _ => {}
        }
        return false;
    }
    match m.kind {
        MouseEventKind::ScrollDown => {
            if app.help { app.help_scroll = app.help_scroll.saturating_add(3); }
            else if app.tab == 0 && !app.detail && !app.show_agents { app.move_sel(3); }
        }
        MouseEventKind::ScrollUp => {
            if app.help { app.help_scroll = app.help_scroll.saturating_sub(3); }
            else if app.tab == 0 && !app.detail && !app.show_agents { app.move_sel(-3); }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            // Help overlay: a theme swatch, a command row, or the background.
            if app.help {
                let mut pick = None;
                for (i, r) in app.rect_theme_buttons.iter().enumerate() { if hit(*r, col, row) { pick = Some(i); break; } }
                if let Some(i) = pick {
                    set_theme(app, i);
                    return false; // stay open: themes are meant to be tried
                }
                // Every command listed in the help is a button. This is what
                // makes the app usable without the keyboard at all: the help
                // stops being a page to read and becomes the place you run
                // things from.
                let act = app.rect_help_actions.at(col, row);
                if let Some(c) = act {
                    app.help = false;
                    return dispatch(app, c);
                }
                if clicked_outside(app, &m) {
                    app.help = false;
                }
                return false;
            }
            if app.show_agents {
                if clicked_outside(app, &m) {
                    app.show_agents = false;
                }
                return false;
            }
            if app.detail {
                let mut act = None;
                for (i, r) in app.rect_detail_buttons.iter().enumerate() { if hit(*r, col, row) { act = Some(i); break; } }
                match act {
                    Some(0) => app.request_resume(),
                    Some(1) => { app.detail = false; app.detail_opened_ms = 0; app.open_reader(); }
                    Some(2) => app.open_agents(),
                    _ => {
                        // A click on the BACKGROUND closes the detail — except
                        // the second press of a double-click that JUST opened it
                        // (within 350ms), which we swallow so the detail doesn't
                        // flash open-then-shut. Buttons above are never
                        // debounced, so RIPRENDI/AGENTI stay responsive
                        // immediately. A click inside the card does nothing: you
                        // are allowed to click on the text you are reading.
                        if clicked_outside(app, &m)
                            && now_ms().saturating_sub(app.detail_opened_ms) >= 350
                        {
                            app.detail = false;
                        }
                        app.detail_opened_ms = 0;
                    }
                }
                return false;
            }
            // Shortcut bar chips. The whole bar, not just its first row: it is
            // two rows tall when the commands do not fit on one.
            if within(app.rect_shortcut, col, row) {
                if let Some(c) = app.shortcut_groups.at(col, row) { return dispatch(app, c); }
            }
            // Tabs.
            if hit(app.rect_tabs, col, row) {
                let third = (app.rect_tabs.width / 3).max(1);
                app.tab = ((col - app.rect_tabs.x) / third).min(2) as usize;
                return false;
            }
            if app.tab == 0 {
                // Sort headers.
                let mut sc = None;
                for (r, s) in &app.rect_sort_headers { if hit(*r, col, row) { sc = Some(*s); break; } }
                if let Some(sc) = sc {
                    if app.sort_col == sc { app.sort_desc = !app.sort_desc; } else { app.sort_col = sc; app.sort_desc = true; }
                    app.apply_filter();
                    return false;
                }
                // Table rows: a single click selects the row AND opens its
                // detail (the natural expectation). `detail_opened_ms` is stamped
                // so a double-click's second press doesn't immediately close it.
                let t = app.rect_table;
                let first = t.y + 2;
                if row >= first && row + 1 < t.y + t.height && col >= t.x && col < t.x + t.width {
                    let idx = app.ts.offset() + (row - first) as usize;
                    if idx < app.view.len() {
                        // Clicking the +/- marker (start of the TITLE column) on a
                        // chain head toggles the chain instead of opening detail.
                        let tc = app.rect_title_col;
                        let on_marker = app.row_meta.get(idx).map_or(false, |m| m.children > 0)
                            && col >= tc.x && col < tc.x + 3;
                        app.ts.select(Some(idx));
                        if on_marker {
                            app.set_chain_expanded(None);
                        } else {
                            app.detail = true;
                            app.detail_opened_ms = now_ms();
                        }
                    }
                }
            } else {
                // Metric toggle buttons.
                for (i, r) in app.rect_metric_buttons.iter().enumerate() {
                    if hit(*r, col, row) { app.metric = i as u8; break; }
                }
            }
        }
        // Right-click on a session row → open its context menu of actions.
        // Skip while a footer input (note edit / search) is active so the popup
        // can't steal keys from an in-progress edit.
        MouseEventKind::Down(MouseButton::Right) => {
            if !app.help && !app.show_agents && !app.detail && !app.note_editing && !app.searching && app.tab == 0 {
                let t = app.rect_table;
                let first = t.y + 2;
                if row >= first && row + 1 < t.y + t.height && col >= t.x && col < t.x + t.width {
                    let idx = app.ts.offset() + (row - first) as usize;
                    if idx < app.view.len() { app.open_session_menu(col, row, idx); }
                }
            }
        }
        _ => {}
    }
    false
}

// ----------------------------------------------------------------------------
// Rendering
// ----------------------------------------------------------------------------

fn ui(f: &mut Frame, app: &mut App) {
    let th = theme(app.theme_idx);
    let area = f.area();
    f.render_widget(Block::default().style(Style::default().bg(th.bg).fg(th.fg)), area);

    // The bar asks for the height it will actually use: one row when the
    // commands fit, two when they do not. Deciding it here is what keeps it
    // from dropping the ones that would have fallen off the right edge.
    let bar_h = shortcut_bar_height(app, area.width);
    let rows = Layout::vertical([
        Constraint::Length(8), // hero: banner + version + clock/scene/caption
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(bar_h),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(area);

    let banner = if app.pixel { &PIXEL_LOGO[..] } else { &LOGO[..] };
    // vertical phosphor-glow gradient: bright at the top, fading down
    let glow = [th.accent, th.run, th.fg, th.dim, th.dim];
    let lcols = Layout::horizontal([Constraint::Length(52), Constraint::Min(14)]).split(rows[0]);
    let mut left: Vec<Line> = banner.iter().enumerate()
        .map(|(i, l)| Line::from(Span::styled(*l, Style::default().fg(glow[i.min(glow.len() - 1)]).add_modifier(Modifier::BOLD))))
        .collect();
    // Precise build stamp right under the logo: which exact version/commit is
    // running (a trailing `+` = built from an uncommitted tree). Offline by
    // design, so it states the build identity, not "is there a newer release".
    left.push(Line::from(Span::styled(format!("  {}", crate::version_line()), Style::default().fg(th.run).add_modifier(Modifier::BOLD))));
    let ti = (now_ms() / 20000) as usize % TAGLINES.len();
    left.push(Line::from(Span::styled(format!("  ✦ {} ✦", TAGLINES[ti]), Style::default().fg(th.idle).add_modifier(Modifier::ITALIC))));
    left.push(Line::from(Span::styled("  con PHOS, lo spirito del fosforo", Style::default().fg(th.dim))));
    f.render_widget(Paragraph::new(left), lcols[0]);
    let t = now_ms();
    let hour = { use chrono::Timelike; chrono::Local::now().hour() };
    let scene = pick_scene(hour, t);
    // The guida phrases (pills + quote) rotate slowly — one change every 90s.
    let qi = (t / 90_000) as usize % QUOTES.len();
    // One box (live clock as its title) holding BOTH the scrolling "how-to"
    // pills (left) and PHOS, the ASCII mascot (right).
    let clock = chrono::Local::now().format(" ⌚ %d/%m %H:%M:%S · guida ").to_string();
    let guidebox = Block::default().borders(Borders::ALL).border_type(btype(app.pixel))
        .border_style(Style::default().fg(th.dim))
        .title(Span::styled(clock, Style::default().fg(th.run).add_modifier(Modifier::BOLD)));
    let ginner = guidebox.inner(lcols[1]);
    f.render_widget(guidebox, lcols[1]);
    let ic = Layout::horizontal([Constraint::Min(18), Constraint::Length(13)]).split(ginner);

    let w = ic[0].width.saturating_sub(2) as usize;
    let h = ic[0].height as usize;
    let off = (t / 90_000) as usize; // advance one pill every 90s
    let tip_rows = h.saturating_sub(1).max(1);
    let mut tlines: Vec<Line> = (0..tip_rows).map(|i| {
        let tips = tips();
        let tip = tips[(off + i) % tips.len()];
        let style = if i == 0 {
            Style::default().fg(th.fg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(th.dim)
        };
        Line::from(Span::styled(format!("› {}", clip(tip, w)), style))
    }).collect();
    if h > tip_rows {
        tlines.push(Line::from(Span::styled(format!("“{}”", clip(QUOTES[qi], w)), Style::default().fg(th.idle).add_modifier(Modifier::ITALIC))));
    }
    f.render_widget(Paragraph::new(tlines), ic[0]);

    // PHOS keeps watch inside the box, on the right. trim:false preserves the
    // art's leading spaces (otherwise the mascot shifts left and misaligns).
    let side: Vec<Line> = scene.iter().map(|l| Line::from(Span::styled(*l, Style::default().fg(th.accent)))).collect();
    f.render_widget(Paragraph::new(side).wrap(Wrap { trim: false }), ic[1]);
    f.render_widget(stats_line(app, &th), rows[1]);

    let tabs = Tabs::new(vec![" 1·SESSIONI ", " 2·PROGETTI ", " 3·ANDAMENTO "])
        .select(app.tab)
        .style(Style::default().fg(th.dim))
        .highlight_style(Style::default().fg(th.bg).bg(th.accent).add_modifier(Modifier::BOLD))
        .divider(Span::styled("│", Style::default().fg(th.dim)));
    f.render_widget(tabs, rows[2]);
    app.rect_tabs = rows[2];

    let bar = render_shortcut_bar(app, &th, rows[3]);
    f.render_widget(bar, rows[3]);
    app.rect_shortcut = rows[3];

    match app.tab {
        0 => render_sessions(f, app, &th, rows[4]),
        1 => render_projects(f, app, &th, rows[4]),
        _ => render_trends(f, app, &th, rows[4]),
    }
    f.render_widget(footer(app, &th), rows[5]);

    if app.detail { render_detail(f, app, &th, area); }
    if app.show_agents { render_agents(f, app, &th, area); }
    if app.help { render_help(f, app, &th, area); }
    if app.picker.is_some() { render_picker(f, app, &th, area); }
    if app.reader.is_some() { render_reader(f, app, &th, area); }
    if app.gsearch.is_some() { render_gsearch(f, app, &th, area); }
    if app.menu.is_some() { render_menu(f, app, &th, area); }
    if app.confirm.is_some() { render_confirm(f, app, &th, area); }
    if app.delproj.is_some() { render_delproj(f, app, &th, area); }
}

/// Render the right-click context menu as a small bordered popup anchored at the
/// click, clamped to stay fully on-screen. Each row is hit-tested individually.
fn render_menu(f: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let (title, items, sel, ax, ay) = match &app.menu {
        Some(m) => (
            m.title.clone(),
            m.items.iter().map(|it| (it.key, it.label)).collect::<Vec<_>>(),
            m.sel, m.col, m.row,
        ),
        None => return,
    };
    if items.is_empty() { return; }
    // Width fits the widest "label  key" plus 2 leading + 1 trailing space; the
    // header title is also taken into account so it isn't clipped too hard.
    let body_w = items.iter().map(|(k, l)| l.chars().count() + 2 + k.chars().count() + 1).max().unwrap_or(12);
    let inner_w = body_w.max(title.chars().count() + 2);
    let w = (inner_w as u16 + 2).min(area.width.max(1)); // +2 for the border
    let h = (items.len() as u16 + 2).min(area.height.max(1)); // +2 for the border
    // Clamp the anchor so the whole popup stays inside the screen.
    let x = ax.min(area.x + area.width.saturating_sub(w));
    let y = ay.min(area.y + area.height.saturating_sub(h));
    let pop = Rect { x, y, width: w, height: h };
    f.render_widget(Clear, pop);
    let block = Block::default().borders(Borders::ALL).border_type(btype(app.pixel))
        .border_style(Style::default().fg(th.accent)).style(Style::default().bg(th.bg))
        .title(Span::styled(format!(" {} ", clip(&title, w.saturating_sub(4) as usize)), Style::default().fg(th.accent)));
    let inner = block.inner(pop);
    f.render_widget(block, pop);

    let iw = inner.width as usize;
    let len = items.len();
    let vis = inner.height as usize; // rows that fit (popup is clamped to screen)
    // Scroll the window so the highlighted item is always visible; on a terminal
    // tall enough for all items this is a no-op (start = 0).
    let start = if vis > 0 && sel >= vis { (sel + 1 - vis).min(len.saturating_sub(vis)) } else { 0 };
    // Rects are indexed by ABSOLUTE item index (off-window items get a 0-width
    // rect, which never hit-tests), so the mouse handler's index == item index.
    let mut rects = vec![Rect::default(); len];
    let mut lines = Vec::with_capacity(vis);
    for row in 0..vis {
        let idx = start + row;
        if idx >= len { break; }
        let (k, l) = items[idx];
        let left = format!("  {}", l);
        let keyhint = format!("{} ", k);
        let gap = iw.saturating_sub(left.chars().count() + keyhint.chars().count());
        let text = format!("{}{}{}", left, " ".repeat(gap), keyhint);
        let style = if idx == sel {
            Style::default().fg(th.bg).bg(th.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(th.fg)
        };
        lines.push(Line::from(Span::styled(clip(&text, iw), style)));
        rects[idx] = Rect { x: inner.x, y: inner.y + row as u16, width: inner.width, height: 1 };
    }
    app.rect_menu_items = rects;
    f.render_widget(Paragraph::new(lines), inner);
}

fn render_confirm(f: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let (title, lines): (String, Vec<String>) = match &app.confirm {
        Some(c) => (c.title.clone(), c.lines.clone()),
        None => return,
    };
    // La finestra prende l'altezza che il testo chiede, invece di un 50% fisso:
    // la domanda sulla retention e il benvenuto hanno quattordici righe, e su
    // un terminale da 24 righe le ultime sparivano senza che niente lo dicesse
    // — comprese quelle che dicono cosa fare. Due per i bordi, una per i
    // bottoni; il tetto e' il 90%, cosi' resta chiaro che e' una finestra.
    let want = (lines.len() as u16 + 3).min(area.height);
    let ph = ((want as u32 * 100 / area.height.max(1) as u32) as u16).clamp(40, 90);
    let pop = centered(area, 70, ph);
    f.render_widget(Clear, pop);
    let block = Block::default().borders(Borders::ALL).border_type(btype(app.pixel))
        .border_style(Style::default().fg(th.accent)).style(Style::default().bg(th.bg))
        .title(Span::styled(title, Style::default().fg(th.accent)));
    let inner = block.inner(pop);
    f.render_widget(block, pop);
    let parts = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);

    let body: Vec<Line> = lines.into_iter()
        .map(|l| Line::from(Span::styled(l, Style::default().fg(th.fg))))
        .collect();
    f.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), parts[0]);

    // Clickable confirm / cancel buttons (index 0 = confirm, 1 = cancel).
    let (mut spans, mut x) = (vec![Span::raw(" ")], parts[1].x + 1);
    let mut rects = Vec::new();
    let labels = app.confirm.as_ref().and_then(|c| c.buttons).unwrap_or(("CONFERMA", "ANNULLA"));
    for (key, label) in [("s", labels.0), ("Esc", labels.1)] {
        let (bs, w) = button(th, key, label);
        spans.extend(bs);
        rects.push(Rect { x, y: parts[1].y, width: w, height: 1 });
        x += w;
    }
    app.rect_confirm_buttons = rects;
    f.render_widget(Paragraph::new(Line::from(spans)), parts[1]);
}

/// The "delete entire project" modal. Phase 0 warns + offers a `.phx` backup;
/// phase 1 is the type-to-confirm box (the CANCELLA hint only lights up once the
/// typed name matches exactly). Rendered in red — it is the one irreversible act.
fn render_delproj(f: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let (dir, name, count, bytes, typed, phase, exported) = match &app.delproj {
        Some(d) => (d.dir.display().to_string(), d.name.clone(), d.count, d.bytes, d.typed.clone(), d.phase, d.exported),
        None => return,
    };
    let pop = centered(area, 72, 46);
    f.render_widget(Clear, pop);
    let block = Block::default().borders(Borders::ALL).border_type(btype(app.pixel))
        .border_style(Style::default().fg(Color::Red)).style(Style::default().bg(th.bg))
        .title(Span::styled(" CANCELLA PROGETTO — IRREVERSIBILE ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)));
    let inner = block.inner(pop);
    f.render_widget(block, pop);
    let parts = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);

    let mb = bytes as f64 / 1_048_576.0;
    let mut body: Vec<Line> = Vec::new();
    body.push(Line::from(Span::styled(format!("Progetto: {name}"), Style::default().fg(th.fg).add_modifier(Modifier::BOLD))));
    body.push(Line::from(Span::styled(format!("Cartella: {}", clip(&dir, inner.width.saturating_sub(12) as usize)), Style::default().fg(th.dim))));
    body.push(Line::from(Span::styled(format!("Sessioni: {count}  ·  {mb:.1} MB — verrà cancellato TUTTO, per sempre."), Style::default().fg(Color::Red))));
    body.push(Line::raw(""));
    if phase == 0 {
        let bk = if exported { "backup .phx creato ✓" } else { "nessun backup ancora (consigliato: [e])" };
        body.push(Line::from(Span::styled(format!("Rete di sicurezza: {bk}"), Style::default().fg(if exported { th.accent } else { th.dim }))));
        body.push(Line::raw(""));
        body.push(Line::from(Span::styled("[e] esporta un backup .phx     [Invio] procedi alla conferma     [Esc] annulla", Style::default().fg(th.fg))));
    } else {
        body.push(Line::from(Span::styled("Per confermare, RISCRIVI il nome esatto del progetto:", Style::default().fg(th.fg))));
        body.push(Line::from(vec![
            Span::styled("   «", Style::default().fg(th.dim)),
            Span::styled(name.clone(), Style::default().fg(th.accent).add_modifier(Modifier::BOLD)),
            Span::styled("»", Style::default().fg(th.dim)),
        ]));
        let matches = typed.trim() == name;
        let cursor = if app.blink { "_" } else { " " };
        let box_style = if matches { Style::default().fg(Color::Red).add_modifier(Modifier::BOLD) } else { Style::default().fg(th.fg) };
        body.push(Line::from(vec![
            Span::styled("   > ", Style::default().fg(th.dim)),
            Span::styled(format!("{typed}{cursor}"), box_style),
        ]));
        body.push(Line::raw(""));
        let hint = if matches { "[Invio] CANCELLA DEFINITIVAMENTE     [Esc] annulla" } else { "(scrivi il nome esatto per abilitare la cancellazione)     [Esc] annulla" };
        body.push(Line::from(Span::styled(hint, Style::default().fg(if matches { Color::Red } else { th.dim }))));
    }
    f.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), parts[0]);
    f.render_widget(Paragraph::new(Line::from(Span::styled("  «progetto» = tutte le sue conversazioni; il codice del progetto NON è qui.", Style::default().fg(th.dim)))), parts[1]);
}

/// ASCII file picker: a bordered overlay listing folders + `.phx` files.
fn render_picker(f: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    // Pull a snapshot so we can mutate scroll/rect after dropping the borrow.
    let (dir_disp, rows, sel, total, remap) = match &app.picker {
        Some(p) => (
            p.dir.display().to_string(),
            p.entries.iter().map(|e| (e.label.clone(), e.is_dir, e.select_here)).collect::<Vec<_>>(),
            p.sel,
            p.entries.len(),
            matches!(p.purpose, PickPurpose::Remap { .. }),
        ),
        None => return,
    };
    let pop = centered(area, 74, 76);
    app.rect_overlay = pop;
    f.render_widget(Clear, pop);
    let title = if remap { " RESUME · scegli la cartella locale del progetto " } else { " IMPORTA · scegli un file .phx " };
    let block = Block::default().borders(Borders::ALL).border_type(btype(app.pixel))
        .border_style(Style::default().fg(th.accent)).style(Style::default().bg(th.bg))
        .title(Span::styled(title, Style::default().fg(th.accent)));
    let inner = block.inner(pop);
    f.render_widget(block, pop);
    let parts = Layout::vertical([Constraint::Length(2), Constraint::Min(1), Constraint::Length(1)]).split(inner);

    // Current directory.
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(format!("  cartella: {}", clip(&dir_disp, parts[0].width.saturating_sub(12) as usize)), Style::default().fg(th.dim))),
            Line::raw(""),
        ]),
        parts[0],
    );

    // List with scroll window kept in sync with the selection.
    let list = parts[1];
    app.rect_picker_list = list;
    let h = list.height.max(1) as usize;
    let mut scroll = app.picker.as_ref().map(|p| p.scroll).unwrap_or(0);
    if sel < scroll { scroll = sel; }
    if sel >= scroll + h { scroll = sel + 1 - h; }
    if let Some(p) = &mut app.picker { p.scroll = scroll; }

    let mut body: Vec<Line> = Vec::new();
    if total == 0 {
        let msg = if remap { "  (nessuna sotto-cartella — usa [..] o l'azione ✓ in alto)" } else { "  (nessun file .phx qui — entra in una cartella o usa [..])" };
        body.push(Line::from(Span::styled(msg, Style::default().fg(th.dim))));
    }
    for i in scroll..(scroll + h).min(total) {
        let (label, is_dir, select_here) = &rows[i];
        let selected = i == sel;
        let base = if *select_here { th.run } else if *is_dir { th.accent } else { th.fg };
        let st = if selected {
            Style::default().fg(th.bg).bg(th.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(base).add_modifier(if *select_here { Modifier::BOLD } else { Modifier::empty() })
        };
        let marker = if selected { "> " } else { "  " };
        body.push(Line::from(Span::styled(format!("{}{}", marker, label), st)));
    }
    f.render_widget(Paragraph::new(body), list);

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "  ↑↓ scegli · Invio apri/seleziona · ← cartella su · Esc annulla",
            Style::default().fg(th.dim),
        ))),
        parts[2],
    );
}

fn stats_line(app: &App, th: &Theme) -> Paragraph<'static> {
    let run = app.all.iter().filter(|s| s.live == "running").count();
    let idle = app.all.iter().filter(|s| s.live == "idle").count();
    let projs = app.all.iter().map(|s| s.project_name.clone()).collect::<std::collections::HashSet<_>>().len();
    let tok: u64 = app.all.iter().map(tok_of).sum();
    let costtot: f64 = app.all.iter().map(|s| cost(s, &app.prices)).sum();
    let mk = |v: String, l: &str| vec![
        Span::styled(v, Style::default().fg(th.accent).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {} ", l), Style::default().fg(th.dim)),
    ];
    let div = || Span::styled("║ ", Style::default().fg(th.dim));
    let mut sp = vec![Span::styled(" ▐ ", Style::default().fg(th.accent))];
    sp.extend(mk(app.all.len().to_string(), t!("sess", "sess"))); sp.push(div());
    sp.extend(mk(projs.to_string(), t!("prog", "proj"))); sp.push(div());
    sp.extend(mk(run.to_string(), "live")); sp.extend(mk(idle.to_string(), "idle")); sp.push(div());
    sp.extend(mk(fmt_tok(tok), "tok")); sp.extend(mk(fmt_usd(costtot), t!("costo", "cost"))); sp.push(div());
    // last-30-days spend + monthly budget
    let now = now_ms();
    let month: f64 = app.all.iter().filter(|s| now.saturating_sub(s.mtime_ms) < 30 * 86_400_000).map(|s| cost(s, &app.prices)).sum();
    sp.extend(mk(fmt_usd(month), t!("30g", "30d")));
    if app.budget > 0.0 {
        let pct = (month / app.budget * 100.0).round() as u64;
        let col = if month > app.budget { th.idle } else { th.run };
        sp.push(Span::styled(format!("{}% di {} ", pct, fmt_usd(app.budget)), Style::default().fg(col)));
    }
    sp.push(div());
    // Rough LOCAL usage gauge for the rolling 5h / 7d windows: token volume of
    // sessions last active in-window. NOT the official limit %, just a feel for
    // "am I heavy this window" (the `~` marks it as an estimate).
    let tok5: u64 = app.all.iter().filter(|s| now.saturating_sub(s.mtime_ms) < 5 * 3_600_000).map(tok_of).sum();
    let tok7: u64 = app.all.iter().filter(|s| now.saturating_sub(s.mtime_ms) < 7 * 86_400_000).map(tok_of).sum();
    sp.extend(mk(format!("~{}", fmt_tok(tok5)), t!("uso 5h", "used 5h")));
    sp.extend(mk(format!("~{}", fmt_tok(tok7)), t!("7g", "7d")));
    sp.push(div());
    // Plan + limit-window reset (read-only, from ~/.claude.json). Official 5h/
    // weekly percentages aren't stored locally, so we only show plan + reset.
    if let Some(pl) = &app.plan {
        if !pl.tier_label.is_empty() {
            let mut t = pl.tier_label.clone();
            if pl.extra_usage { t.push('+'); }
            sp.extend(mk(t, "piano"));
        }
        if let Some(end) = pl.limits_end_ms {
            sp.push(Span::styled(format!("reset {} ", crate::plan::reset_in(end, now)), Style::default().fg(th.idle)));
        }
        sp.push(div());
    }
    sp.push(Span::styled(format!("{} LIVE", if app.blink { "●" } else { "○" }), Style::default().fg(th.run).add_modifier(Modifier::BOLD)));
    Paragraph::new(Line::from(sp))
}


fn footer(app: &App, th: &Theme) -> Paragraph<'static> {
    let states = t!(
        ["tutte", "live", "idle", "fine", "★ preferiti"],
        ["all", "live", "idle", "done", "★ favorites"],
    );
    let metrics = t!(["costo", "token", "sessioni"], ["cost", "tokens", "sessions"]);
    let ctx: String = if app.confirm.is_some() {
        t!("premi  s  per confermare · Esc per annullare", "press  s  to confirm · Esc to cancel").into()
    } else if app.note_editing {
        let field = if app.note_is_alias { t!("alias (titolo mostrato)", "alias (shown title)") } else { t!("nota", "note") };
        t!(
            format!("{field}: {}_   (Invio salva · Esc annulla · vuoto = rimuove)", app.note_buf),
            format!("{field}: {}_   (Enter saves · Esc cancels · empty = removes)", app.note_buf),
        )
    } else if app.searching {
        if app.search.is_empty() {
            t!(
                "cerca: _   (testo libero · filtri: project: model: file: tool: agent: host: after: before:)",
                "search: _   (free text · filters: project: model: file: tool: agent: host: after: before:)",
            ).into()
        } else {
            t!(format!("cerca: {}_", app.search), format!("search: {}_", app.search))
        }
    } else if app.detail || app.show_agents || app.help {
        t!("premi Esc per chiudere", "press Esc to close").into()
    } else if app.tab == 0 {
        t!(
            format!("ordina {}{} · filtro {}", sorts()[app.sort_col], if app.sort_desc { "▼" } else { "▲" }, states[app.state_filter as usize]),
            format!("sort {}{} · filter {}", sorts()[app.sort_col], if app.sort_desc { "▼" } else { "▲" }, states[app.state_filter as usize]),
        )
    } else {
        format!("metrica {}", metrics[app.metric as usize])
    };
    Paragraph::new(Line::from(vec![
        Span::styled(format!(" {} ", ctx), Style::default().fg(th.dim)),
        Span::styled(format!("  «{}»", app.status), Style::default().fg(th.idle)),
    ]))
}

fn render_sessions(f: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    app.rect_table = area;
    let widths = sess_widths();

    let headers = t!(
        ["", "PROGETTO", "TITOLO", "MSG", "TOKEN", "~COSTO", "ENERGIA", "ACQUA", "AG", "DIMENS.", "DATA", "MODELLO"],
        ["", "PROJECT", "TITLE", "MSG", "TOKENS", "~COST", "ENERGY", "WATER", "AG", "SIZE", "DATE", "MODEL"],
    );
    let header = Row::new(headers.iter().enumerate().map(|(i, h)| {
        let mut st = Style::default().fg(th.dim).add_modifier(Modifier::BOLD);
        if COL_SORT[i] == Some(app.sort_col) {
            st = Style::default().fg(th.accent).add_modifier(Modifier::BOLD | Modifier::REVERSED);
        }
        Cell::from(*h).style(st)
    }));

    let rows: Vec<Row> = app.view.iter().enumerate().map(|(ri, &i)| {
        let s = &app.all[i];
        let meta = app.row_meta.get(ri).copied().unwrap_or(RowMeta { depth: 0, children: 0, expanded: false, root: i, certain: false });
        let (dot, dc) = match s.live.as_str() { "running" => ("●", th.run), "idle" => ("◐", th.idle), _ => ("·", th.dim) };
        let ag = s.subagents + s.workflows;
        let c = cost(s, &app.prices);
        let (wh, ml) = crate::config::footprint(s, app.energy_wh_per_output_token, app.water_l_per_kwh);
        Row::new(vec![
            Cell::from(dot).style(Style::default().fg(dc)),
            // Two signals, never one: the hue separates the agents at a glance
            // while scrolling, the `◆` survives a monochrome terminal, a
            // colour-blind reader and a copy-paste of the screen.
            Cell::from(clip(&s.project_name, 18))
                .style(Style::default().fg(if s.is_codex() { th.alt } else { th.accent })),
            Cell::from({
                let mut pre = String::new();
                if meta.depth >= 1 {
                    pre.push_str("  ↳ "); // a resume nested under its conversation
                } else if meta.children > 0 {
                    pre.push_str(if meta.expanded { "[-] " } else { "[+] " }); // collapsible head
                }
                if !s.host.is_empty() { pre.push_str(&format!("[{}] ", clip(&s.host, 12))); }
                if app.marked.contains(&s.id) { pre.push_str("◉ "); }
                if app.is_favorite(&s.id) { pre.push_str("★ "); }
                if app.notes.contains_key(&s.id) { pre.push_str("📝 "); }
                // Rebuilt from history.jsonl: prompts only, no transcript behind it.
                if s.is_ghost() { pre.push_str("⚱ "); }
                if s.is_codex() { pre.push_str("◆ "); }
                // Il transcript vive solo come hard link nel vault: il suo
                // agente non lo vede più, Phosphor sì.
                if s.is_vaulted() { pre.push_str("⛁ "); }
                if s.is_continuation && meta.depth == 0 && meta.children == 0 { pre.push_str("↳ "); }
                // A COLLAPSED head stands for the whole conversation, so it shows
                // the chain ROOT (original) title — not the latest "resume" greeting.
                // Expanded, the head is just the latest session and keeps its own
                // title (the root then appears as the last child below).
                // Use the user's alias (custom title) when set, else the auto-title.
                let label = if meta.children > 0 && !meta.expanded { app.display_title(&app.all[meta.root]) } else { app.display_title(s) };
                let suffix = if meta.depth >= 1 {
                    // child: is this link proven (shared uuids) or just a guess?
                    if meta.certain { "  (certo)".to_string() } else { "  (probabile)".to_string() }
                } else if meta.children > 0 && !meta.expanded {
                    // collapsed head: count + a ✓ when the chain has a proven link
                    format!("  ⟳{} riprese{}", meta.children, if meta.certain { " ✓" } else { "" })
                } else {
                    String::new()
                };
                format!("{}{}{}", pre, label.replace('\n', " "), suffix)
            }),
            Cell::from(s.message_count.to_string()),
            Cell::from(fmt_tok(tok_of(s))),
            Cell::from(fmt_usd(c)),
            Cell::from(fmt_wh(wh)).style(Style::default().fg(th.idle)),
            Cell::from(fmt_ml(ml)).style(Style::default().fg(th.idle)),
            Cell::from(if ag > 0 { ag.to_string() } else { String::new() }),
            Cell::from(fmt_size(s.size)).style(Style::default().fg(th.dim)),
            Cell::from(abs_date(s.mtime_ms)).style(Style::default().fg(th.dim)),
            Cell::from(clip(&s.models.first().map(|m| m.replace("claude-", "")).unwrap_or_default(), 12)),
        ])
    }).collect();

    // Totals span ALL filtered sessions (view_all), not just the visible/collapsed
    // rows, so collapsing a chain doesn't change the reported count/footprint.
    let total_size: u64 = app.view_all.iter().map(|&i| app.all[i].size).sum();
    let (twh, tml) = app.view_all.iter()
        .map(|&i| crate::config::footprint(&app.all[i], app.energy_wh_per_output_token, app.water_l_per_kwh))
        .fold((0.0, 0.0), |(e, w), (de, dw)| (e + de, w + dw));
    let hidden = app.view_all.len().saturating_sub(app.view.len());
    let folded = if hidden > 0 { format!(" · {} riprese compresse", hidden) } else { String::new() };
    let arrow = if app.sort_desc { "▼" } else { "▲" };
    let title = t!(
        format!(" {} sessioni{} · {} · ~{} ~{} · ordina: {}{} ", app.view_all.len(), folded, fmt_size(total_size), fmt_wh(twh), fmt_ml(tml), sorts()[app.sort_col], arrow),
        format!(" {} sessions{} · {} · ~{} ~{} · sort: {}{} ", app.view_all.len(), folded, fmt_size(total_size), fmt_wh(twh), fmt_ml(tml), sorts()[app.sort_col], arrow),
    );
    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(1)
        // No selection symbol → no left margin → header columns align exactly
        // with the Layout::horizontal split used for click-to-sort below.
        .highlight_spacing(HighlightSpacing::Never)
        .row_highlight_style(Style::default().bg(th.accent).fg(th.bg).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::ALL).border_type(btype(app.pixel))
            .border_style(Style::default().fg(th.dim))
            .title(Span::styled(title, Style::default().fg(th.fg))));
    f.render_stateful_widget(table, area, &mut app.ts);

    // Header column rects for click-to-sort (matches the table's own solver).
    let inner = Rect { x: area.x + 1, y: area.y + 1, width: area.width.saturating_sub(2), height: 1 };
    let cols = Layout::horizontal(widths).spacing(1).flex(ratatui::layout::Flex::Start).split(inner);
    let mut hr = Vec::new();
    for (c, sc) in cols.iter().zip(COL_SORT.iter()) {
        if let Some(sc) = sc {
            hr.push((Rect { x: c.x, y: inner.y, width: c.width, height: 1 }, *sc));
        }
    }
    app.rect_sort_headers = hr;
    // The TITLE column spans the whole body height — used to hit-test clicks on
    // the +/- marker (the first few cells of a chain head's title).
    if let Some(tc) = cols.get(2) {
        app.rect_title_col = Rect { x: tc.x, y: area.y, width: tc.width, height: area.height };
    }
}

fn render_metric_row(app: &mut App, th: &Theme, area: Rect) -> Paragraph<'static> {
    let names = ["COSTO", "TOKEN", "SESSIONI"];
    let mut spans = vec![Span::styled("  metrica: ", Style::default().fg(th.dim))];
    let mut rects = Vec::new();
    let mut x = area.x + 11;
    for (i, n) in names.iter().enumerate() {
        let label = format!(" {n} ");
        let w = label.chars().count() as u16 + 2;
        let st = if app.metric as usize == i {
            Style::default().fg(th.bg).bg(th.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(th.dim)
        };
        spans.push(Span::styled("[", Style::default().fg(th.dim)));
        spans.push(Span::styled(label, st));
        spans.push(Span::styled("] ", Style::default().fg(th.dim)));
        rects.push(Rect { x, y: area.y, width: w, height: 1 });
        x += w + 1;
    }
    app.rect_metric_buttons = rects;
    Paragraph::new(Line::from(spans))
}

fn render_projects(f: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let split = Layout::vertical([Constraint::Length(1), Constraint::Min(2)]).split(area);
    f.render_widget(render_metric_row(app, th, split[0]), split[0]);
    let body = split[1];

    let mut ag = agg_projects(&app.all, &app.prices);
    ag.sort_by(|a, b| metric_val(b, app.metric).total_cmp(&metric_val(a, app.metric)).then_with(|| a.project.cmp(&b.project)));
    let halves = Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)]).split(body);

    let inner_w = halves[0].width.saturating_sub(2) as usize;
    let (label_w, val_w) = (16usize, 9usize);
    let bar_w = inner_w.saturating_sub(label_w + val_w + 2).max(4);
    let max = ag.iter().map(|a| metric_val(a, app.metric)).fold(1e-9, f64::max);
    let mut lines: Vec<Line> = Vec::new();
    for a in ag.iter().take(halves[0].height.saturating_sub(2) as usize) {
        let v = metric_val(a, app.metric);
        let filled = ((v / max) * bar_w as f64).round() as usize;
        lines.push(Line::from(vec![
            Span::styled(format!("{:<w$}", clip(&a.project, label_w), w = label_w), Style::default().fg(th.accent)),
            Span::styled("█".repeat(filled), Style::default().fg(th.fg)),
            Span::styled("░".repeat(bar_w - filled), Style::default().fg(th.dim)),
            Span::styled(format!(" {:>w$}", fmt_metric(v, app.metric), w = val_w), Style::default().fg(th.idle)),
        ]));
    }
    f.render_widget(Paragraph::new(lines).block(Block::default().borders(Borders::ALL).border_type(btype(app.pixel))
        .border_style(Style::default().fg(th.dim))
        .title(Span::styled(format!(" {} per progetto ", metric_name(app.metric)), Style::default().fg(th.fg)))), halves[0]);

    let header = Row::new(["PROGETTO", "SESS", "LIVE", "AG", "TOKEN", "~COSTO", "DIMENS.", "ATTIVITÀ"].into_iter()
        .map(|h| Cell::from(h).style(Style::default().fg(th.dim).add_modifier(Modifier::BOLD))));
    let now = now_ms();
    let trows: Vec<Row> = ag.iter().map(|a| Row::new(vec![
        Cell::from(clip(&a.project, 22)).style(Style::default().fg(th.accent)),
        Cell::from(a.sessions.to_string()),
        Cell::from(if a.running > 0 { a.running.to_string() } else { String::new() }).style(Style::default().fg(th.run)),
        Cell::from(if a.sub > 0 { a.sub.to_string() } else { String::new() }),
        Cell::from(fmt_tok(a.tok)),
        Cell::from(fmt_usd(a.cost)),
        Cell::from(fmt_size(a.size)).style(Style::default().fg(th.dim)),
        Cell::from(rel(a.last, now)),
    ])).collect();
    let widths = [Constraint::Min(16), Constraint::Length(5), Constraint::Length(5), Constraint::Length(4), Constraint::Length(8), Constraint::Length(9), Constraint::Length(8), Constraint::Length(9)];
    f.render_widget(Table::new(trows, widths).header(header).block(Block::default().borders(Borders::ALL).border_type(btype(app.pixel))
        .border_style(Style::default().fg(th.dim)).title(Span::styled(" aggregati ", Style::default().fg(th.fg)))), halves[1]);
}

fn render_trends(f: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let split = Layout::vertical([Constraint::Length(1), Constraint::Min(2)]).split(area);
    f.render_widget(render_metric_row(app, th, split[0]), split[0]);
    let body = split[1];

    let mut days: BTreeMap<String, (f64, u64, u64)> = BTreeMap::new();
    for s in &app.all {
        let day = match s.modified.get(..10) { Some(d) => d.to_string(), None => continue };
        if !day.as_bytes().iter().take(4).all(|b| b.is_ascii_digit()) { continue; }
        let e = days.entry(day).or_insert((0.0, 0, 0));
        e.0 += cost(s, &app.prices); e.1 += tok_of(s); e.2 += 1;
    }
    if days.is_empty() {
        f.render_widget(Paragraph::new("nessun dato temporale").block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(th.dim))), body);
        return;
    }
    let labels: Vec<String> = days.keys().cloned().collect();
    let data: Vec<(f64, f64)> = days.values().enumerate().map(|(i, v)| {
        (i as f64, match app.metric { 1 => v.1 as f64, 2 => v.2 as f64, _ => v.0 })
    }).collect();
    let n = data.len();
    let maxy = data.iter().map(|p| p.1).fold(1e-9, f64::max);
    let ds = vec![Dataset::default().marker(symbols::Marker::Braille).graph_type(GraphType::Line).style(Style::default().fg(th.accent)).data(&data)];
    let chart = Chart::new(ds)
        .block(Block::default().borders(Borders::ALL).border_type(btype(app.pixel)).border_style(Style::default().fg(th.dim))
            .title(Span::styled(format!(" {} / giorno ", metric_name(app.metric)), Style::default().fg(th.fg))))
        .x_axis(Axis::default().style(Style::default().fg(th.dim)).bounds([0.0, (n.max(1) - 1) as f64])
            .labels(vec![Span::raw(labels.first().cloned().unwrap_or_default()), Span::raw(labels.last().cloned().unwrap_or_default())]))
        .y_axis(Axis::default().style(Style::default().fg(th.dim)).bounds([0.0, maxy * 1.05])
            .labels(vec![Span::raw("0"), Span::raw(fmt_metric(maxy, app.metric))]));
    f.render_widget(chart, body);
}

/// Heuristic: does this title read like a "resume/continue the previous work"
/// greeting? Plain `--resume` leaves no compact marker, so these short openers
/// (often Claude's own aiTitle) are how we still fold resume siblings into one
/// collapsible chain. Best-effort by design — only affects visual grouping.
fn title_is_resume(t: &str) -> bool {
    let l = t.to_lowercase();
    // Match GREETING PHRASES, not bare fragments: "continua" alone also hits the
    // ordinary word "continuare", which would wrongly fold a brand-new session
    // into the previous chain (and hide it). Phrases keep the false-positive rate
    // low; an unmatched resume just shows as its own leaf under the project.
    const K: &[&str] = &[
        "dove eravamo", "a che punto eravamo", "ripresa della", "riprendere dalla",
        "riprendiamo", "ricominciamo", "ricordare il contesto", "riepilogo della sessione",
        "continua dalla", "continuiamo da", "continua la conversazione", "continua la sessione",
        "where were we", "pick up where", "continue from", "resuming the", "recap of",
    ];
    K.iter().any(|k| l.contains(k))
}

/// A parsed list-search query: free-text terms (ALL must match) plus optional
/// field filters typed inline as `key:value`. Recognised keys: project|proj,
/// model, file, tool, host (fleet: "host:pc-casa", "host:qui" = local), after|since,
/// before|until (dates as YYYY-MM-DD on the last activity). Anything that isn't
/// a recognised `key:value` is free text.
#[derive(Default)]
pub(crate) struct Query {
    text: Vec<String>,
    project: Vec<String>,
    model: Vec<String>,
    file: Vec<String>,
    tool: Vec<String>,
    host: Vec<String>,
    agent: Vec<String>,
    after: Option<String>,
    before: Option<String>,
}
impl Query {
    pub(crate) fn parse(s: &str) -> Query {
        let mut q = Query::default();
        for tok in s.split_whitespace() {
            let lower = tok.to_lowercase();
            if let Some((k, val)) = lower.split_once(':') {
                if !val.is_empty() {
                    match k {
                        "project" | "proj" => { q.project.push(val.to_string()); continue; }
                        "model" => { q.model.push(val.to_string()); continue; }
                        "file" => { q.file.push(val.to_string()); continue; }
                        "tool" => { q.tool.push(val.to_string()); continue; }
                        "host" => { q.host.push(val.to_string()); continue; }
                        "agent" | "cli" => { q.agent.push(val.to_string()); continue; }
                        "after" | "since" => { q.after = Some(val.to_string()); continue; }
                        "before" | "until" => { q.before = Some(val.to_string()); continue; }
                        _ => {}
                    }
                }
            }
            q.text.push(lower);
        }
        q
    }
    pub(crate) fn matches(&self, s: &Session) -> bool {
        for t in &self.text {
            let hit = s.search_text.contains(t)
                || s.project_name.to_lowercase().contains(t)
                || s.title.to_lowercase().contains(t);
            if !hit { return false; }
        }
        for p in &self.project { if !s.project_name.to_lowercase().contains(p) { return false; } }
        for m in &self.model { if !s.models.iter().any(|x| x.to_lowercase().contains(m)) { return false; } }
        for f in &self.file { if !s.files.iter().any(|x| x.to_lowercase().contains(f)) { return false; } }
        for t in &self.tool { if !s.tools.iter().any(|(n, _)| n.to_lowercase().contains(t)) { return false; } }
        for h in &self.host {
            // "host:qui" / "host:local" selects this PC's sessions; any other
            // value substring-matches the fleet alias.
            let ok = if h == "qui" || h == "local" || h == "locale" {
                s.host.is_empty()
            } else {
                s.host.to_lowercase().contains(h)
            };
            if !ok { return false; }
        }
        for a in &self.agent {
            // "agent:codex" / "agent:claude" — a session with no agent recorded
            // predates the field and is Claude Code's.
            let mine = if s.agent.is_empty() { "claude" } else { s.agent.as_str() };
            if !mine.contains(a.as_str()) { return false; }
        }
        if self.after.is_some() || self.before.is_some() {
            let day = session_day(s);
            if day.is_empty() { return false; }
            if let Some(a) = &self.after { if day.as_str() < a.as_str() { return false; } }
            if let Some(b) = &self.before { if day.as_str() > b.as_str() { return false; } }
        }
        true
    }
}
/// The "YYYY-MM-DD" of a session's last activity (falls back to creation), or "".
fn session_day(s: &Session) -> String {
    let src = if !s.modified.is_empty() { &s.modified } else { &s.created };
    src.get(..10).unwrap_or("").to_string()
}

// ---- Union-find (disjoint set) over small index slices -------------------
fn uf_find(uf: &mut [usize], x: usize) -> usize {
    let mut r = x;
    while uf[r] != r { r = uf[r]; }
    let mut c = x;
    while uf[c] != c { let nx = uf[c]; uf[c] = r; c = nx; } // path compression
    r
}
fn uf_union(uf: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (uf_find(uf, a), uf_find(uf, b));
    if ra != rb { uf[ra] = rb; }
}

/// Minimum shared bottom-sketch hashes for two sessions to count as the SAME
/// conversation. >=2 avoids a lone coincidental hash; validated on real
/// transcripts (the true resume/compact families share well over this).
const KIN_MIN_SHARED: usize = 2;

/// PROVEN lineage: assign a non-zero `kin_group` to every session that shares
/// >=KIN_MIN_SHARED message-uuid hashes with another in the SAME project (they
/// replayed the same messages, so one resumes/compacts the other). Unlike the
/// title heuristic this cannot false-positive on same-named but unrelated
/// sessions. Scoped per project on purpose: `collapse()` groups per project, so
/// a kin_group must stay inside one project for "certo" to mean "proven link
/// WITHIN the displayed chain" (a cross-cwd resume can't be shown as one chain
/// anyway). Also sharpens parent_id/parent_title to the proven parent.
fn link_kin(sessions: &mut [Session]) {
    let n = sessions.len();
    for s in sessions.iter_mut() { s.kin_group = 0; }
    // hash -> session indices that carry it in their sketch
    let mut owners: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, s) in sessions.iter().enumerate() {
        for &h in &s.kin_sketch { owners.entry(h).or_default().push(i); }
    }
    // count shared hashes per session pair, then union pairs over the threshold
    let mut shared: HashMap<(usize, usize), usize> = HashMap::new();
    for idxs in owners.values() {
        if idxs.len() < 2 { continue; }
        for a in 0..idxs.len() {
            for b in (a + 1)..idxs.len() {
                let key = (idxs[a].min(idxs[b]), idxs[a].max(idxs[b]));
                *shared.entry(key).or_insert(0) += 1;
            }
        }
    }
    let mut uf: Vec<usize> = (0..n).collect();
    let mut linked = vec![false; n];
    for (&(a, b), &c) in &shared {
        // Same project only — see the per-project note above.
        if c >= KIN_MIN_SHARED && sessions[a].project_name == sessions[b].project_name {
            uf_union(&mut uf, a, b);
            linked[a] = true;
            linked[b] = true;
        }
    }
    // group ids: only for components with >=2 proven members
    let mut comp: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n { if linked[i] { let r = uf_find(&mut uf, i); comp.entry(r).or_default().push(i); } }
    let mut gid: u32 = 0;
    for members in comp.values() {
        if members.len() < 2 { continue; }
        gid += 1;
        // oldest -> newest; each member's proven parent = the previous member.
        let mut ord = members.clone();
        ord.sort_by_key(|&i| sessions[i].mtime_ms);
        for (k, &i) in ord.iter().enumerate() {
            sessions[i].kin_group = gid;
            if k > 0 {
                let p = ord[k - 1];
                sessions[i].parent_id = sessions[p].id.clone();
                sessions[i].parent_title = sessions[p].title.clone();
            }
        }
    }
}

/// Best-effort: link each continuation session (compact/resume child) to the
/// most recent earlier session in the same project. A child always ends after
/// its parent, so the parent is the same-project session with the greatest
/// mtime strictly below the child's. O(n^2) over the small session list.
fn link_continuations(sessions: &mut [Session]) {
    let snap: Vec<(String, String, u64, String, String)> = sessions
        .iter()
        .map(|s| (s.id.clone(), s.project_name.clone(), s.mtime_ms, s.title.clone(), s.host.clone()))
        .collect();
    for s in sessions.iter_mut() {
        s.parent_id.clear();
        s.parent_title.clear();
        if !s.is_continuation {
            continue;
        }
        let mut best: Option<&(String, String, u64, String, String)> = None;
        for cand in &snap {
            // Same-host only: a remote session (fleet) must never become the
            // parent of a local continuation just because the project name matches.
            if cand.0 == s.id || cand.1 != s.project_name || cand.2 >= s.mtime_ms || cand.4 != s.host {
                continue;
            }
            if best.map_or(true, |b| cand.2 > b.2) {
                best = Some(cand);
            }
        }
        if let Some(b) = best {
            s.parent_id = b.0.clone();
            s.parent_title = b.3.clone();
        }
    }
}

struct Agg { project: String, sessions: u64, tok: u64, cost: f64, running: u64, sub: u64, last: u64, size: u64 }
fn agg_projects(all: &[Session], prices: &Prices) -> Vec<Agg> {
    let mut m: BTreeMap<String, Agg> = BTreeMap::new();
    for s in all {
        let a = m.entry(s.project_name.clone()).or_insert(Agg { project: s.project_name.clone(), sessions: 0, tok: 0, cost: 0.0, running: 0, sub: 0, last: 0, size: 0 });
        a.sessions += 1; a.tok += tok_of(s); a.cost += cost(s, prices); a.sub += s.subagents + s.workflows; a.size += s.size;
        if s.live != "ended" { a.running += 1; }
        if s.mtime_ms > a.last { a.last = s.mtime_ms; }
    }
    m.into_values().collect()
}
fn metric_val(a: &Agg, metric: u8) -> f64 { match metric { 1 => a.tok as f64, 2 => a.sessions as f64, _ => a.cost } }
fn metric_name(metric: u8) -> &'static str { match metric { 1 => "token", 2 => "sessioni", _ => "costo" } }
fn fmt_metric(v: f64, metric: u8) -> String { match metric { 1 => fmt_tok(v as u64), 2 => format!("{}", v as u64), _ => fmt_usd(v) } }

fn centered(area: Rect, pw: u16, ph: u16) -> Rect {
    let w = ((area.width as u32 * pw as u32 / 100) as u16).min(area.width);
    let h = ((area.height as u32 * ph as u32 / 100) as u16).min(area.height);
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

// Build a styled "[ key ] LABEL" button; returns (spans, width).
fn button(th: &Theme, key: &str, label: &str) -> (Vec<Span<'static>>, u16) {
    let spans = vec![
        Span::styled("[ ", Style::default().fg(th.accent)),
        Span::styled(key.to_string(), Style::default().fg(th.bg).bg(th.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" ] ", Style::default().fg(th.accent)),
        Span::styled(format!("{label}  "), Style::default().fg(th.fg).add_modifier(Modifier::BOLD)),
    ];
    // rendered width: "[ "(2) + key + " ] "(3) + label + "  "(2) = 7 + key + label
    let w = (7 + key.chars().count() + label.chars().count()) as u16;
    (spans, w)
}

fn render_detail(f: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let s = match app.selected() { Some(s) => s.clone(), None => return };
    let pop = centered(area, 80, 88);
    app.rect_overlay = pop;
    f.render_widget(Clear, pop);
    let block = Block::default().borders(Borders::ALL).border_type(btype(app.pixel))
        .border_style(Style::default().fg(th.accent)).style(Style::default().bg(th.bg))
        .title(Span::styled(" DETTAGLIO SESSIONE ", Style::default().fg(th.accent)));
    let inner = block.inner(pop);
    f.render_widget(block, pop);
    let parts = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);

    let now = now_ms();
    let kv = |k: &str, v: String| Line::from(vec![
        Span::styled(format!("{:<13}", k), Style::default().fg(th.dim)),
        Span::styled(v, Style::default().fg(th.fg)),
    ]);
    let tools: String = s.tools.iter().map(|(n, c)| format!("{}·{}", n, c)).collect::<Vec<_>>().join("  ");
    let fav = app.is_favorite(&s.id);
    let note = app.notes.get(&s.id).cloned();
    let alias = app.aliases.get(&s.id).cloned();
    let mut lines = vec![
        Line::from(vec![
            Span::styled(if fav { "★ " } else { "" }, Style::default().fg(th.run).add_modifier(Modifier::BOLD)),
            Span::styled(app.display_title(&s).replace('\n', " "), Style::default().fg(th.accent).add_modifier(Modifier::BOLD)),
        ]),
        // When an alias is set, still surface the original auto-title beneath it.
        Line::from(vec![
            Span::styled("titolo auto: ", Style::default().fg(th.dim)),
            Span::styled(
                if alias.is_some() { s.title.replace('\n', " ") } else { "— (premi  N  per un titolo tuo)".into() },
                Style::default().fg(th.dim),
            ),
        ]),
        Line::from(vec![Span::styled(format!("[{}]", s.live), Style::default().fg(th.run)), Span::styled(format!("  {}", s.last_state()), Style::default().fg(th.dim))]),
        Line::from(vec![
            Span::styled("nota: ", Style::default().fg(th.dim)),
            Span::styled(note.clone().unwrap_or_else(|| "— (premi  n  per aggiungerla)".into()), Style::default().fg(if note.is_some() { th.idle } else { th.dim })),
        ]),
        Line::raw(""),
        kv("host", if s.host.is_empty() { "questo PC".into() } else { format!("{}  (remoto via ssh — r per riprendere là)", s.host) }),
        kv("agente", if s.is_codex() {
            format!("Codex CLI{}", if s.version.is_empty() { String::new() } else { format!(" v{}", s.version) })
        } else {
            "Claude Code".to_string()
        }),
        kv("origine", if s.is_vaulted() {
            "⛁ NEL VAULT — il magazzino del suo agente non ce l'ha più, ma il transcript è intero (hard link). Premi  V  per rimetterlo al suo posto e tornare a riprenderlo.".into()
        } else if s.is_ghost() {
            "⚱ RECUPERATA da history.jsonl — Claude Code ha cancellato il transcript (cleanupPeriodDays). Solo i tuoi prompt: niente risposte, token o costi.".into()
        } else {
            "transcript su disco".to_string()
        }),
        kv("progetto", s.project_path.clone()),
        kv("sessionId", s.id.clone()),
        kv("git branch", if s.git_branch.is_empty() { "—".into() } else { s.git_branch.clone() }),
        kv("entrypoint", format!("{} · v{}", s.entrypoint, s.version)),
        kv("messaggi", s.message_count.to_string()),
        kv("creata", if s.created.is_empty() { "—".into() } else { s.created.clone() }),
        kv("ultima att.", format!("{}  ({})", s.modified, rel(s.mtime_ms, now))),
        kv("tipo", if !s.parent_title.is_empty() {
            // a known parent: from the compact marker (is_continuation) OR a
            // PROVEN kin link (shared messages) the title heuristic missed.
            let proven = s.kin_group != 0;
            format!("↳ continua da: {}{}", clip(&s.parent_title.replace('\n', " "), 48), if proven { "  (certo)" } else { "" })
        } else if s.is_continuation {
            "↳ ripresa/compact di una sessione precedente".into()
        } else {
            "sessione iniziale".into()
        }),
        kv("sub-agenti", format!("{} subagent · {} workflow", s.subagents, s.workflows)),
        kv("token", format!("{}  (in {} · out {} · cache {})", fmt_tok(tok_of(&s)), fmt_tok(s.input_tokens), fmt_tok(s.output_tokens), fmt_tok(s.cache_read + s.cache_creation))),
        kv("costo", format!("{} (stima)", fmt_usd(cost(&s, &app.prices)))),
        {
            let (wh, ml) = crate::config::footprint(&s, app.energy_wh_per_output_token, app.water_l_per_kwh);
            kv("footprint", format!("{} · {} (stima, ±ordine di grandezza)", fmt_wh(wh), fmt_ml(ml)))
        },
        kv("dimensione", fmt_size(s.size)),
        kv("modelli", s.models.join(", ")),
        Line::raw(""),
        Line::from(Span::styled("tool usati:", Style::default().fg(th.dim))),
        Line::from(Span::styled(if tools.is_empty() { "—".into() } else { tools }, Style::default().fg(th.fg))),
        Line::raw(""),
        Line::from(Span::styled("primo prompt:", Style::default().fg(th.dim))),
        Line::from(Span::raw(clip(&s.first_prompt.replace('\n', " "), 200))),
        Line::raw(""),
        Line::from(Span::styled("ultimo prompt:", Style::default().fg(th.dim))),
        Line::from(Span::raw(clip(&s.last_prompt.replace('\n', " "), 200))),
        Line::raw(""),
        Line::from(Span::styled(format!("file modificati: {}", s.files.len()), Style::default().fg(th.dim))),
    ];
    for fp in s.files.iter().take(5) {
        lines.push(Line::from(Span::styled(clip(fp, 70), Style::default().fg(th.dim))));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), parts[0]);

    // Clickable button bar on a known row.
    let (mut spans, mut x) = (vec![Span::raw(" ")], parts[1].x + 1);
    let mut rects = Vec::new();
    for (key, label) in [("r", "RIPRENDI"), ("v", "LEGGI"), ("a", "AGENTI"), ("Esc", "CHIUDI")] {
        let (bs, w) = button(th, key, label);
        spans.extend(bs);
        rects.push(Rect { x, y: parts[1].y, width: w, height: 1 });
        x += w;
    }
    app.rect_detail_buttons = rects;
    f.render_widget(Paragraph::new(Line::from(spans)), parts[1]);
}

fn render_agents(f: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let pop = centered(area, 84, 82);
    app.rect_overlay = pop;
    f.render_widget(Clear, pop);
    let block = Block::default().borders(Borders::ALL).border_type(btype(app.pixel))
        .border_style(Style::default().fg(th.accent)).style(Style::default().bg(th.bg))
        .title(Span::styled(" SUB-AGENTI / WORKFLOW ", Style::default().fg(th.accent)));
    let inner = block.inner(pop);
    f.render_widget(block, pop);
    let parts = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);

    let mut lines: Vec<Line> = Vec::new();
    if app.agents.is_empty() {
        lines.push(Line::from(Span::styled("nessun sub-agente / workflow per questa sessione", Style::default().fg(th.dim))));
    } else {
        let total_tok: u64 = app.agents.iter().map(|a| a.tok).sum();
        let total_cost: f64 = app.agents.iter().map(|a| a.cost).sum();
        lines.push(Line::from(Span::styled(format!("{} agenti · {} token · {} totali", app.agents.len(), fmt_tok(total_tok), fmt_usd(total_cost)), Style::default().fg(th.fg))));
        lines.push(Line::raw(""));
        let maxtok = app.agents.iter().map(|a| a.tok).max().unwrap_or(1).max(1);
        for a in app.agents.iter().take(parts[0].height.saturating_sub(2) as usize) {
            lines.push(Line::from(vec![
                Span::styled(format!("{:<20}", clip(&a.kind, 20)), Style::default().fg(th.accent)),
                Span::styled(minibar(a.tok as f64, maxtok as f64, 8), Style::default().fg(th.fg)),
                Span::styled(format!(" {:>6} ", fmt_tok(a.tok)), Style::default().fg(th.idle)),
                Span::styled(format!("{:>4}msg ", a.msgs), Style::default().fg(th.dim)),
                Span::styled(format!("{:>7}  ", fmt_usd(a.cost)), Style::default().fg(th.dim)),
                Span::styled(clip(&a.desc.replace('\n', " "), 34), Style::default().fg(th.fg)),
            ]));
        }
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), parts[0]);

    let (bs, w) = button(th, "Esc", "CHIUDI");
    let mut spans = vec![Span::raw(" ")];
    spans.extend(bs);
    app.rect_agents_close = Rect { x: parts[1].x + 1, y: parts[1].y, width: w, height: 1 };
    f.render_widget(Paragraph::new(Line::from(spans)), parts[1]);
}

/// The command a help row runs when clicked, or 0 for a row that only explains
/// something (the symbol legend, the CLI list).
///
/// Keyed on the text shown at the start of the row, so the table and the help
/// cannot drift apart silently: rename a row's key and it simply stops being
/// clickable, which is visible, rather than firing the wrong command.
///
/// A few rows list two related commands (`r · e`). Those run the FIRST one —
/// the row's headline — which is why the important ones each have a row of
/// their own.

fn render_help(f: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    // Opens large enough to read everything at a glance; still scrollable and it
    // follows the terminal size (resizable).
    let pop = centered(area, 82, 92);
    app.rect_overlay = pop;
    f.render_widget(Clear, pop);
    let block = Block::default().borders(Borders::ALL).border_type(btype(app.pixel))
        .border_style(Style::default().fg(th.accent)).style(Style::default().bg(th.bg))
        .title(Span::styled(" AIUTO · ↑↓ scorri · Esc chiudi ", Style::default().fg(th.accent)));
    let inner = block.inner(pop);
    f.render_widget(block, pop);
    let parts = Layout::vertical([Constraint::Min(1), Constraint::Length(1), Constraint::Length(1)]).split(inner);

    let lines = help::lines(th);
    // Scrollable: clamp the offset so you can't scroll past the end.
    let max_scroll = (lines.len() as u16).saturating_sub(parts[0].height);
    if app.help_scroll > max_scroll { app.help_scroll = max_scroll; }
    // Work out which visible rows are commands, so a click can run them. The
    // key shown at the start of a row IS the row's identity, so it is also what
    // the lookup keys on — no parallel list to keep in step with the text.
    app.rect_help_actions.clear();
    for (i, l) in lines.iter().enumerate() {
        let key = l.spans.first().map(|s| s.content.trim()).unwrap_or("");
        let code = help::action(key);
        if code == 0 {
            continue;
        }
        let y = i as i32 - app.help_scroll as i32;
        if y >= 0 && (y as u16) < parts[0].height {
            app.rect_help_actions.push(
                Rect { x: parts[0].x, y: parts[0].y + y as u16, width: parts[0].width, height: 1 },
                code,
            );
        }
    }
    f.render_widget(Paragraph::new(lines).scroll((app.help_scroll, 0)), parts[0]);

    // Clickable theme swatches.
    let mut spans = vec![Span::styled("  tema:  ", Style::default().fg(th.dim))];
    let mut rects = Vec::new();
    let mut x = parts[1].x + 9;
    for (i, n) in THEME_NAMES.iter().enumerate() {
        let label = format!(" {n} ");
        let w = label.chars().count() as u16 + 1; // include the separator space → no dead gap
        let st = if i == app.theme_idx {
            Style::default().fg(th.bg).bg(th.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(th.dim)
        };
        spans.push(Span::styled(label, st));
        spans.push(Span::raw(" "));
        rects.push(Rect { x, y: parts[1].y, width: w, height: 1 });
        x += w;
    }
    app.rect_theme_buttons = rects;
    f.render_widget(Paragraph::new(Line::from(spans)), parts[1]);
    f.render_widget(Paragraph::new(Line::from(Span::styled("  prezzi/tema/budget: ~/.claude/phosphor.json   ·   Esc per chiudere", Style::default().fg(th.dim)))), parts[2]);
}

/// Simple word-wrap to `width` columns, preserving explicit newlines and
/// hard-splitting words longer than the line. Counts chars, not bytes.
fn wrap_text(s: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut out = Vec::new();
    for raw in s.split('\n') {
        let mut cur = String::new();
        let mut cur_n = 0usize;
        for word in raw.split(' ') {
            let wn = word.chars().count();
            if wn > width {
                if cur_n > 0 { out.push(std::mem::take(&mut cur)); }
                let mut w = word;
                while w.chars().count() > width {
                    let idx = w.char_indices().nth(width).map(|(i, _)| i).unwrap_or(w.len());
                    out.push(w[..idx].to_string());
                    w = &w[idx..];
                }
                cur = w.to_string();
                cur_n = cur.chars().count();
                continue;
            }
            let add = if cur_n == 0 { wn } else { cur_n + 1 + wn };
            if add > width {
                out.push(std::mem::take(&mut cur));
                cur = word.to_string();
                cur_n = wn;
            } else {
                if cur_n > 0 { cur.push(' '); cur_n += 1; }
                cur.push_str(word);
                cur_n += wn;
            }
        }
        out.push(cur);
    }
    out
}

fn render_reader(f: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let pop = centered(area, 86, 92);
    app.rect_overlay = pop;
    f.render_widget(Clear, pop);
    let width = pop.width.saturating_sub(4) as usize; // 2 borders + 2 indent
    let (title, lines, turn_starts): (String, Vec<Line>, Vec<usize>) = {
        let r = match &app.reader { Some(r) => r, None => return };
        let mut lines: Vec<Line> = Vec::new();
        let mut turn_starts: Vec<usize> = Vec::with_capacity(r.turns.len());
        for turn in &r.turns {
            turn_starts.push(lines.len()); // line index where this turn begins
            let (label, lstyle) = match turn.role {
                0 => ("tu", Style::default().fg(th.run).add_modifier(Modifier::BOLD)),
                1 => ("claude", Style::default().fg(th.accent).add_modifier(Modifier::BOLD)),
                _ => ("·", Style::default().fg(th.dim)),
            };
            lines.push(Line::from(Span::styled(label, lstyle)));
            for wl in wrap_text(&turn.text, width) {
                let st = if wl.starts_with("· tool:") {
                    Style::default().fg(th.dim)
                } else {
                    Style::default().fg(th.fg)
                };
                lines.push(Line::from(Span::styled(format!("  {wl}"), st)));
            }
            lines.push(Line::raw(""));
        }
        (clip(&r.title, 44), lines, turn_starts)
    };
    let block = Block::default().borders(Borders::ALL).border_type(btype(app.pixel))
        .border_style(Style::default().fg(th.accent)).style(Style::default().bg(th.bg))
        .title(Span::styled(format!(" LETTURA · {title} · ↑↓ scorri · Esc chiudi "), Style::default().fg(th.accent)));
    let inner = block.inner(pop);
    f.render_widget(block, pop);
    app.rect_reader = inner;
    let max_scroll = (lines.len() as u16).saturating_sub(inner.height);
    let scroll = {
        let r = app.reader.as_mut().unwrap();
        // Jump to the searched turn on first render, then clear the target.
        if let Some(tt) = r.target_turn.take() {
            r.scroll = turn_starts.get(tt).copied().unwrap_or(0) as u16;
        }
        if r.scroll > max_scroll { r.scroll = max_scroll; }
        r.scroll
    };
    f.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);
}

fn render_gsearch(f: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let pop = centered(area, 86, 84);
    app.rect_overlay = pop;
    f.render_widget(Clear, pop);
    let block = Block::default().borders(Borders::ALL).border_type(btype(app.pixel))
        .border_style(Style::default().fg(th.accent)).style(Style::default().bg(th.bg))
        .title(Span::styled(" CERCA NEL CONTENUTO · scrivi+Invio · ↑↓ · Invio apri · Esc ", Style::default().fg(th.accent)));
    let inner = block.inner(pop);
    f.render_widget(block, pop);
    let parts = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).split(inner);
    app.rect_gsearch_list = parts[2];
    let h = parts[2].height as usize;
    // Keep the selected result visible.
    if let Some(g) = &mut app.gsearch {
        if g.sel < g.scroll {
            g.scroll = g.sel;
        } else if h > 0 && g.sel >= g.scroll + h {
            g.scroll = g.sel + 1 - h;
        }
    }
    let g = match &app.gsearch { Some(g) => g, None => return };

    let cursor = if g.typing { "_" } else { "" };
    f.render_widget(Paragraph::new(Line::from(vec![
        Span::styled("  cerca: ", Style::default().fg(th.dim)),
        Span::styled(format!("{}{}", g.query, cursor), Style::default().fg(th.fg).add_modifier(Modifier::BOLD)),
    ])), parts[0]);
    let hint = if g.typing {
        "  scrivi (≥2 caratteri) e premi Invio: cerca in tutte le conversazioni".to_string()
    } else {
        format!("  {} risultati · Invio apre al punto · g per modificare la ricerca", g.results.len())
    };
    f.render_widget(Paragraph::new(Line::from(Span::styled(hint, Style::default().fg(th.dim)))), parts[1]);

    let w = parts[2].width as usize;
    let mut lines: Vec<Line> = Vec::new();
    if g.results.is_empty() {
        let msg = if g.query.trim().len() < 2 { "scrivi almeno 2 caratteri e premi Invio" } else if g.typing { "" } else { "nessun risultato" };
        if !msg.is_empty() {
            lines.push(Line::from(Span::styled(format!("  {msg}"), Style::default().fg(th.dim))));
        }
    } else {
        for i in g.scroll..(g.scroll + h).min(g.results.len()) {
            let r = &g.results[i];
            let who = match r.role { 0 => "tu", 1 => "cl", _ => "··" };
            let body = format!("{} › {} [{}] {}", clip(&r.project, 14), clip(&r.title, 22), who, r.snippet);
            let style = if i == g.sel {
                Style::default().bg(th.accent).fg(th.bg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(th.fg)
            };
            lines.push(Line::from(Span::styled(format!(" {}", clip(&body, w.saturating_sub(2))), style)));
        }
    }
    f.render_widget(Paragraph::new(lines), parts[2]);
}

#[cfg(test)]
mod tree_tests {
    use super::*;

    fn sess(id: &str, proj: &str, ppath: &str, title: &str, mtime: u64) -> Session {
        Session {
            id: id.into(), project_name: proj.into(), project_path: ppath.into(),
            title: title.into(), mtime_ms: mtime, live: "ended".into(),
            ..Default::default()
        }
    }
    fn app_with(all: Vec<Session>) -> App {
        let cache = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let mut a = App::new(std::env::temp_dir(), cache, Prices::default(), 0.0, Vec::new(), String::new(), 5, 0, false, all);
        a.dry = true;
        a
    }

    /// La lingua e' un byte globale e i test girano in parallelo: chi la tocca
    /// prende questo lucchetto e la rimette com'era. Senza, un test che passa
    /// all'inglese fa fallire a caso quello che sta controllando un titolo
    /// italiano — il tipo di rottura che si presenta una volta su venti e fa
    /// perdere un pomeriggio.
    static LANG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn lang_guard(en: bool) -> std::sync::MutexGuard<'static, ()> {
        let g = LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::lang::set_en(en);
        g
    }

    // id of the session at visible row `vp`
    fn id_at(app: &App, vp: usize) -> &str { &app.all[app.view[vp]].id }

    /// Disegna davvero l'interfaccia, come fa il programma. Serve perche' i
    /// rettangoli cliccabili NASCONO dal disegno: senza questo passaggio il
    /// mouse non ha nulla su cui cadere, e un test che clicca senza disegnare
    /// proverebbe soltanto se stesso.
    fn draw(app: &mut App) {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let mut term = Terminal::new(TestBackend::new(120, 40)).expect("backend");
        term.draw(|f| ui(f, app)).expect("disegno");
    }

    /// Clicca al centro del rettangolo: il bordo e' proprio il punto in cui un
    /// errore di un pixel non si nota.
    fn click_middle(app: &mut App, r: Rect) {
        let _ = handle_mouse(app, mk_click(r.x + r.width / 2, r.y + r.height / 2));
    }

    fn chip_rect(app: &App, action: u16) -> Rect {
        app.shortcut_groups
            .rect_of(action)
            .unwrap_or_else(|| panic!("nessun chip per l azione {action} nella barra"))
    }

    #[test]
    fn every_chip_drawn_in_the_bar_can_be_clicked() {
        let _lock = lang_guard(false);
        // Il difetto che questo chiude: i rettangoli cliccabili vivono due
        // volte — il disegno li produce, il mouse li rilegge da un campo di
        // App. Se chi aggiunge un bottone si dimentica di registrarlo, il
        // bottone SI VEDE e non risponde, e finora nessun test se ne accorgeva.
        let mut app = app_with(vec![sess("aaa", "p", "C:/p", "titolo", 1000)]);
        for mouse_only in [false, true] {
            app.mouse_only = mouse_only;
            for tab in [0usize, 1] {
                app.tab = tab;
                draw(&mut app);
                let drawn: Vec<u16> = shortcut_chips(&app)
                    .iter()
                    .flatten()
                    .map(|(_, _, a)| *a)
                    .filter(|a| *a != 0)
                    .collect();
                assert!(!drawn.is_empty());
                for a in drawn {
                    let r = chip_rect(&app, a);
                    assert!(r.width > 0 && r.height > 0, "azione {a}: rettangolo vuoto");
                    // e il rettangolo sta DENTRO la barra che l'ha disegnato
                    let bar = app.rect_shortcut;
                    assert!(
                        r.y >= bar.y && r.y < bar.y + bar.height,
                        "azione {a}: fuori dalla barra"
                    );
                }
            }
        }
    }

    #[test]
    fn the_tour_shows_once_clicks_through_and_never_comes_back() {
        let _lock = lang_guard(false);
        let mut app = app_with(vec![sess("aaa", "p", "C:/p", "titolo", 1000)]);
        app.dry = false;
        app.base = std::env::temp_dir().join(format!("phosphor-tour-{}", std::process::id()));
        std::fs::create_dir_all(&app.base).unwrap();
        app.tour_todo = true;

        // Aspetta il suo turno: la domanda sulla retention viene prima.
        app.confirm = Some(Confirm {
            title: " ALTRO ".into(),
            lines: vec![],
            action: Pending::SetRetention { days: 0 },
            alts: Vec::new(),
            buttons: None,
            cancel: None,
        });
        app.maybe_show_tour();
        assert_eq!(app.confirm.as_ref().unwrap().title, " ALTRO ", "non scavalca la retention");
        app.confirm = None;

        // Ne' salta sopra a un pannello aperto.
        app.help = true;
        app.maybe_show_tour();
        assert!(app.confirm.is_none(), "non interrompe l'aiuto");
        app.help = false;
        app.searching = true;
        app.maybe_show_tour();
        assert!(app.confirm.is_none(), "non interrompe chi sta scrivendo");
        app.searching = false;

        // Poi arriva, e si sfoglia col mouse: i bottoni dicono cosa fanno.
        app.maybe_show_tour();
        for page in 0..App::TOUR_PAGES {
            let c = app.confirm.as_ref().expect("pagina {page} del giro");
            assert!(c.title.contains(&format!("{} di 3", page + 1)), "titolo: {}", c.title);
            let (ok, _) = c.buttons.expect("etichette proprie, non CONFERMA/ANNULLA");
            assert!(ok == "AVANTI" || ok == "INIZIA", "bottone: {ok}");
            draw(&mut app);
            let r = app.rect_confirm_buttons[0];
            click_middle(&mut app, r);
        }
        // Finito: la modale si chiude e non si ripresenta.
        assert!(app.confirm.is_none(), "l'ultima pagina chiude");
        assert!(!app.tour_todo);
        app.maybe_show_tour();
        assert!(app.confirm.is_none(), "non torna");
        assert!(crate::config::load(&app.base).tour_done, "e se lo ricorda su disco");
        std::fs::remove_dir_all(&app.base).ok();
    }

    #[test]
    fn skipping_the_tour_also_counts_as_having_seen_it() {
        // Il secondo bottone qui non è «annulla»: è «salta». Se annullasse e
        // basta, il giro tornerebbe al prossimo avvio — cioè la risposta
        // dell'utente verrebbe ignorata.
        let _lock = lang_guard(false);
        let mut app = app_with(vec![sess("aaa", "p", "C:/p", "titolo", 1000)]);
        app.dry = false;
        app.base = std::env::temp_dir().join(format!("phosphor-tour-skip-{}", std::process::id()));
        std::fs::create_dir_all(&app.base).unwrap();
        app.tour_todo = true;
        app.maybe_show_tour();
        draw(&mut app);
        let r = app.rect_confirm_buttons[1];
        click_middle(&mut app, r);
        assert!(app.confirm.is_none());
        assert!(!app.tour_todo, "saltato vuol dire visto");
        assert!(crate::config::load(&app.base).tour_done);
        std::fs::remove_dir_all(&app.base).ok();
    }

    #[test]
    fn a_manual_rescan_leaves_the_window_alive() {
        // `R` rileggeva sul thread del disegno: su uno store grande la finestra
        // smetteva di rispondere — anche al click fuori da un pannello — senza
        // dire che stava lavorando. Era l'unico punto in cui Phosphor sembrava
        // morto mentre stava benissimo.
        let dir = std::env::temp_dir().join(format!("phosphor-rescan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = app_with(vec![sess("aaa", "p", "C:/p", "titolo", 1000)]);
        app.base = dir.clone();
        app.dry = false;
        let (tx, rx) = mpsc::channel::<Vec<Session>>();
        app.scan_tx = Some(tx);

        app.rescan_now();
        assert!(app.scanning, "la rilettura e' partita");
        assert!(app.status.contains("rilettura"), "e lo dice: {}", app.status);

        // Tenere premuto R non accoda una fila di thread.
        app.rescan_now();
        assert!(app.status.contains("gia'"), "stato: {}", app.status);

        // Il risultato arriva sul canale, come quello del watcher periodico.
        let got = rx
            .recv_timeout(Duration::from_secs(20))
            .expect("il thread di scansione risponde");
        app.set_sessions(got);
        app.scanning = false;
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_bar_wraps_instead_of_swallowing_the_commands_on_the_right() {
        // A 120 colonne — un terminale del tutto normale — la barra si fermava
        // al bordo destro e buttava via TUTTO l'ultimo gruppo: aiuto, tema,
        // pixel e l'interruttore del mouse. Un difetto che non si nota perche'
        // la prova e' un'assenza: non c'e' niente di rotto da guardare.
        let _lock = lang_guard(false);
        let mut app = app_with(vec![sess("aaa", "p", "C:/p", "titolo", 1000)]);
        // Tutte e due le lingue: le etichette inglesi hanno lunghezze diverse, e
        // una barra che sta in piedi in italiano puo' perdere una voce in
        // inglese senza che nessuno se ne accorga.
        for en in [false, true] {
        crate::lang::set_en(en);
        for width in [200u16, 160, 120, 100, 80] {
            let (rows, placed) = place_chips(&app, width);
            let wanted = shortcut_chips(&app).iter().flatten().count();
            assert_eq!(placed.len(), wanted, "a {width} colonne manca qualche voce (en={en})");
            assert!(rows <= BAR_MAX_LINES, "a {width} colonne la barra e' alta {rows}");
            // Nessun chip sborda, su nessuna delle righe.
            for p in &placed {
                assert!(p.x + p.w < width, "a {width} colonne un chip sborda");
            }
        }
        // Stretto sul serio: non si pretende che ci stia tutto, si pretende
        // che non esploda e che quello che resta sia raggiungibile.
        let (rows, placed) = place_chips(&app, 40);
        assert!(rows <= BAR_MAX_LINES);
        for p in &placed {
            assert!(p.x + p.w < 40);
        }
        }
        crate::lang::set_en(false);
    }

    #[test]
    fn clicking_the_words_in_the_bar_does_the_thing() {
        let _lock = lang_guard(false);
        let mut app = app_with(vec![sess("aaa", "p", "C:/p", "titolo", 1000)]);
        app.tab = 0;
        draw(&mut app);

        // «/ cerca» apre la ricerca: si clicca la scritta, non si preme «/».
        let r = chip_rect(&app, A_SEARCH);
        click_middle(&mut app, r);
        assert!(app.searching, "il chip della ricerca deve aprirla");
        app.searching = false;

        // «o ord» cambia colonna di ordinamento.
        draw(&mut app);
        let before = app.sort_col;
        let r = chip_rect(&app, A_SORTCOL);
        click_middle(&mut app, r);
        assert_ne!(app.sort_col, before, "il chip dell'ordine deve cambiarlo");

        // L'interruttore mouse: cambia modalita' e resta cliccabile DOPO,
        // quando la barra si e' ridisegnata senza le lettere.
        draw(&mut app);
        assert!(!app.mouse_only);
        let r = chip_rect(&app, A_MOUSE_MODE);
        click_middle(&mut app, r);
        assert!(app.mouse_only, "primo click: solo mouse");
        draw(&mut app);
        let r = chip_rect(&app, A_MOUSE_MODE);
        click_middle(&mut app, r);
        assert!(!app.mouse_only, "secondo click: si torna indietro");
    }

    #[test]
    fn clicking_a_help_row_runs_it_and_gets_out_of_the_way() {
        let _lock = lang_guard(false);
        let mut app = app_with(vec![sess("aaa", "p", "C:/p", "titolo", 1000)]);
        app.ts.select(Some(0));
        app.help = true;
        app.help_scroll = 0;
        draw(&mut app);
        assert!(!app.rect_help_actions.is_empty(), "le righe dell'aiuto sono comandi");

        let r = app
            .rect_help_actions
            .rect_of(A_GSEARCH)
            .expect("la riga «cerca in tutti i transcript» deve essere cliccabile");
        click_middle(&mut app, r);
        assert!(!app.help, "l'aiuto si toglie di mezzo prima di eseguire");
        assert!(app.gsearch.is_some(), "e l'azione parte davvero");
    }

    #[test]
    fn clicking_a_column_header_sorts_by_that_column() {
        let mut app = app_with(vec![
            sess("aaa", "p", "C:/p", "titolo", 1000),
            sess("bbb", "p", "C:/p", "altro", 2000),
        ]);
        app.tab = 0;
        draw(&mut app);
        let (r, col) = *app.rect_sort_headers.first().expect("intestazioni cliccabili");
        click_middle(&mut app, r);
        assert_eq!(app.sort_col, col, "si ordina per la colonna cliccata");
        // Ri-cliccare la stessa intestazione inverte il verso, come ovunque.
        draw(&mut app);
        let desc = app.sort_desc;
        click_middle(&mut app, r);
        assert_ne!(app.sort_desc, desc, "secondo click: verso invertito");
    }

    #[test]
    fn fleet_merge_dedups_and_stays_after_rescan() {
        let mut app = app_with(vec![
            sess("aaa", "proj", "C:/proj", "locale", 1000),
        ]);
        let mut dup = sess("aaa", "proj", "C:/proj", "copia sincronizzata", 900);
        dup.host = "pc-casa".into();
        let mut fresh = sess("bbb", "proj", "C:/proj", "solo remota", 800);
        fresh.host = "pc-casa".into();
        app.fleet = vec![dup, fresh];
        app.refresh_with_fleet();
        // the duplicated id keeps the LOCAL copy; the new remote id appears
        assert_eq!(app.all.len(), 2);
        let local = app.all.iter().find(|s| s.id == "aaa").unwrap();
        assert!(local.host.is_empty(), "local copy wins on id collision");
        assert_eq!(app.all.iter().find(|s| s.id == "bbb").unwrap().host, "pc-casa");
        // a watch-style rescan replaces the LOCAL set but keeps the fleet rows;
        // with the local "aaa" gone, its remote twin legitimately surfaces
        app.set_sessions(vec![sess("ccc", "proj2", "C:/p2", "nuova locale", 2000)]);
        assert!(app.all.iter().any(|s| s.id == "bbb"), "fleet row survives rescan");
        assert!(app.all.iter().any(|s| s.id == "ccc"));
        let aaa = app.all.iter().find(|s| s.id == "aaa").expect("remote twin surfaces");
        assert_eq!(aaa.host, "pc-casa", "the surviving aaa is the REMOTE copy");
    }

    #[test]
    fn fleet_stale_generation_is_dropped() {
        let mut app = app_with(vec![sess("aaa", "proj", "C:/proj", "locale", 1000)]);
        app.fleet_gen = 2; // a second F superseded the first fetch
        let mut r = sess("old", "proj", "C:/proj", "risultato vecchio", 500);
        r.host = "pc-casa".into();
        app.on_fleet_msg(1, "pc-casa".into(), Ok(vec![r]));
        assert!(app.fleet.is_empty(), "stale generation ignored");
        let mut r2 = sess("new", "proj", "C:/proj", "risultato attuale", 600);
        r2.host = "pc-casa".into();
        app.on_fleet_msg(2, "pc-casa".into(), Ok(vec![r2]));
        assert_eq!(app.fleet.len(), 1);
        assert!(app.all.iter().any(|s| s.id == "new"));
    }

    #[test]
    fn fleet_rows_never_chain_with_local_ones() {
        // Same project name, resume-looking title: without the host partition
        // the remote row would fold under the local chain.
        let mut remote = sess("r-remote", "proj", "C:/proj", "Dove eravamo rimasti", 2000);
        remote.host = "pc-casa".into();
        let mut app = app_with(vec![
            sess("root", "proj", "C:/proj", "Build the thing", 1000),
        ]);
        app.fleet = vec![remote];
        app.refresh_with_fleet();
        // both rows visible as independent heads (no [+] chain folding)
        assert_eq!(app.view.len(), 2, "remote row is its own head, not a hidden child");
        assert!(app.row_meta.iter().all(|m| m.children == 0), "no cross-host chains");
    }

    #[test]
    fn the_retention_question_appears_only_when_it_should() {
        // Una base tutta sua: la domanda dipende da settings.json, non dalle sessioni.
        let base = std::env::temp_dir().join(format!(
            "phosphor-tui-ret-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let mk = |base: &std::path::Path| {
            let cache = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
            let mut a = App::new(base.to_path_buf(), cache, Prices::default(), 0.0, Vec::new(), String::new(), 5, 0, false, Vec::new());
            a.dry = true;
            a
        };

        // chiave assente = 30 giorni = si chiede
        std::fs::write(base.join("settings.json"), "{\n  \"model\": \"opus\"\n}\n").unwrap();
        let mut a = mk(&base);
        a.maybe_ask_retention();
        let c = a.confirm.as_ref().expect("la domanda deve comparire");
        assert_eq!(c.alts.len(), 3, "tre scelte: 10 anni, 1 anno, lascia stare");
        // Invio / s prende la consigliata, non una a caso
        assert!(matches!(c.action, Pending::SetRetention { days } if days == crate::retention::RECOMMENDED_DAYS));
        assert!(matches!(c.alts[2], ('3', Pending::SetRetention { days: 0 })), "il terzo tasto non tocca nulla");

        // retention gia' lunga = non si chiede
        std::fs::write(base.join("settings.json"), "{\n  \"cleanupPeriodDays\": 3650\n}\n").unwrap();
        let mut b = mk(&base);
        b.maybe_ask_retention();
        assert!(b.confirm.is_none(), "niente domanda se la cronologia e' gia' al sicuro");

        // gia' risposto una volta = non si chiede piu', anche se e' corta
        std::fs::write(base.join("settings.json"), "{\n  \"cleanupPeriodDays\": 30\n}\n").unwrap();
        let mut cfg = crate::config::Config::default();
        cfg.retention_asked = true;
        crate::config::save(&base, &cfg);
        let mut c2 = mk(&base);
        c2.maybe_ask_retention();
        assert!(c2.confirm.is_none(), "chiesto una volta, mai piu'");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn clicking_outside_closes_a_panel_and_clicking_inside_does_not() {
        // Il difetto vero che questo previene: prima lettore e aiuto si
        // chiudevano a QUALUNQUE click, quindi non ci si poteva cliccare
        // dentro; picker e ricerca globale invece non si chiudevano affatto col
        // mouse. Due comportamenti opposti, entrambi inutilizzabili senza
        // tastiera.
        let mut app = app_with(vec![sess("aaa", "p", "C:/p", "titolo", 1000)]);
        app.rect_overlay = Rect { x: 10, y: 5, width: 40, height: 20 };
        let click = |c: u16, r: u16| event::MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: c,
            row: r,
            modifiers: KeyModifiers::empty(),
        };
        assert!(!clicked_outside(&app, &click(20, 10)), "dentro il pannello");
        assert!(clicked_outside(&app, &click(2, 2)), "fuori dal pannello");
        assert!(clicked_outside(&app, &click(60, 10)), "a destra del pannello");
        // lo scroll non chiude mai: leggere non è un modo per andarsene
        let scroll = event::MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 2,
            row: 2,
            modifiers: KeyModifiers::empty(),
        };
        assert!(!clicked_outside(&app, &scroll));
        // senza overlay disegnato non esiste un "fuori"
        app.rect_overlay = Rect { x: 0, y: 0, width: 0, height: 0 };
        assert!(!clicked_outside(&app, &click(2, 2)));
    }

    #[test]
    fn the_cost_of_nothing_is_not_minus_zero() {
        // In Rust la somma di una lista vuota di float e' -0.0 (e' l'identita'
        // che usa `Sum`), quindi con nessuna sessione la riga in fondo diceva
        // «$-0.000 costo». Lo vedeva solo chi apre Phosphor per la prima volta,
        // cioe' esattamente la persona che non sa ancora se fidarsi dei numeri.
        let empty: Vec<f64> = vec![];
        let sum: f64 = empty.iter().sum();
        assert!(sum.is_sign_negative(), "e' proprio -0.0: il difetto nasce qui");
        assert_eq!(fmt_usd(sum), "$0.000");
        assert_eq!(fmt_usd(-0.0), "$0.000");
        // E i valori veri non vengono toccati.
        assert_eq!(fmt_usd(0.5), "$0.500");
        assert_eq!(fmt_usd(12.345), "$12.35");
    }

    #[test]
    fn the_language_switch_is_one_key_one_chip_and_it_is_remembered() {
        // Il tasto e il chip stanno bene in vista perche' chi ne ha bisogno e'
        // proprio chi non capisce quello che sta leggendo: mandarlo a cercare
        // una voce in un file di configurazione scritto nella lingua sbagliata
        // sarebbe una barzelletta.
        let _lock = lang_guard(false);
        let mut app = app_with(vec![sess("aaa", "p", "C:/p", "titolo", 1000)]);
        app.dry = false;
        app.base = std::env::temp_dir().join(format!("phosphor-lang-{}", std::process::id()));
        std::fs::create_dir_all(&app.base).unwrap();

        // Il chip c'e' in tutte e due le lingue e in tutte e due le viste.
        for tab in [0usize, 1] {
            app.tab = tab;
            draw(&mut app);
            let r = chip_rect(&app, A_LANG);
            assert!(r.width > 0, "manca il chip della lingua (tab {tab})");
        }
        // Un click lo cambia, e la scelta finisce su disco.
        app.tab = 0;
        draw(&mut app);
        let r = chip_rect(&app, A_LANG);
        click_middle(&mut app, r);
        assert!(crate::lang::is_en(), "il click accende l'inglese");
        assert_eq!(crate::config::load(&app.base).lang, "en", "e se lo ricorda");

        // L'interfaccia cambia davvero, non solo il byte.
        assert!(
            shortcut_chips(&app).iter().flatten().any(|(_, l, _)| *l == "help"),
            "la barra deve parlare inglese"
        );
        let en_help = help::lines(&theme(0)).len();
        assert!(en_help > 50, "l'aiuto inglese esiste ed e' lungo");

        // E il tasto L fa lo stesso, in tutte e due le direzioni.
        let _ = handle_key(&mut app, KeyCode::Char('L'), KeyModifiers::empty());
        assert!(!crate::lang::is_en());
        assert_eq!(crate::config::load(&app.base).lang, "it");
        assert!(shortcut_chips(&app).iter().flatten().any(|(_, l, _)| *l == "aiuto"));

        std::fs::remove_dir_all(&app.base).ok();
        crate::lang::set_en(false);
    }

    #[test]
    fn the_verses_fit_the_box_and_still_name_the_key() {
        // Le pillole sono in versi, ma restano aiuto: se una viene tagliata a
        // metà perde proprio la fine, dove di solito sta la cosa da fare. E un
        // verso che nascondesse il tasto dietro una metafora sarebbe
        // decorazione, non istruzione.
        //
        // 51 colonne e' la larghezza del riquadro su un terminale da 120 (vedi
        // `clip` in render_hero): il logo ne prende 52, la colonna di destra 13.
        const W: usize = 51;
        for t in TIPS.iter().chain(TIPS_EN.iter()) {
            assert!(
                t.chars().count() <= W,
                "«{t}» e' lungo {} caratteri: verrebbe tagliato",
                t.chars().count()
            );
            assert!(!t.trim().is_empty());
        }
        // Le due lingue devono insegnare gli stessi tasti: un verso inglese che
        // dimenticasse  V  lascerebbe un comando senza nessuno che lo nomini.
        for (name, set) in [("it", TIPS), ("en", TIPS_EN)] {
            let all = set.join("\n");
            for token in [
                "  v  ", "  /  ", "  f  ", "  r  ", "  a  ", "  e  ", "  x  ", "  i  ", "  p  ",
                "  m  ", "  R  ", "  ?  ", "  q  ", "  A  ", "  W  ", "  V  ", "  F  ", "  H  ",
                "  L  ", "phosphor cost", "phosphor find", "phosphor --web", "vault on",
            ] {
                assert!(all.contains(token), "[{name}] nessun verso insegna «{}»", token.trim());
            }
            // Nessun doppione: settanta versi si controllano male a occhio.
            let mut seen: Vec<&str> = set.to_vec();
            seen.sort_unstable();
            let n = seen.len();
            seen.dedup();
            assert_eq!(seen.len(), n, "[{name}] c'e' un verso ripetuto");
        }
    }

    #[test]
    fn a_long_confirmation_is_not_cut_off_on_a_short_terminal() {
        // La domanda sulla retention e il benvenuto hanno quattordici righe. Con
        // l'altezza fissa al 50%, su un terminale da 24 righe sparivano le
        // ultime — cioe' proprio quelle che dicono cosa fare — e nulla lo
        // segnalava. I bottoni devono restare dentro in ogni caso.
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let mut app = app_with(vec![sess("aaa", "p", "C:/p", "titolo", 1000)]);
        let lines: Vec<String> = (0..14).map(|i| format!("riga numero {i} del testo")).collect();
        for (w, h) in [(120u16, 40u16), (100, 30), (90, 24), (80, 20)] {
            app.confirm = Some(Confirm {
                title: " PROVA ".into(),
                lines: lines.clone(),
                action: Pending::TourDone,
                alts: Vec::new(),
                buttons: None,
                cancel: None,
            });
            let mut term = Terminal::new(TestBackend::new(w, h)).expect("backend");
            term.draw(|f| ui(f, &mut app)).expect("disegno");
            let b = app.rect_confirm_buttons[0];
            assert!(b.y < h, "a {w}x{h} i bottoni finiscono fuori schermo");
            // Il testo entra: la finestra e' alta almeno quanto serve, finche'
            // il terminale lo consente.
            let needed = (lines.len() as u16 + 3).min(h);
            let got = centered(Rect { x: 0, y: 0, width: w, height: h }, 70, ((needed as u32 * 100 / h as u32) as u16).clamp(40, 90)).height;
            assert!(got + 1 >= needed, "a {w}x{h}: servono {needed} righe, la finestra ne ha {got}");
        }
        app.confirm = None;
    }

    #[test]
    fn no_help_row_becomes_a_command_by_accident() {
        let _lock = lang_guard(false);
        // Il bug che questo chiude: la riga «find <testo>», che documenta il
        // comando DA TERMINALE  phosphor find , cominciava per «f» e la regola
        // starts_with la faceva diventare il filtro di stato. Cliccare una
        // spiegazione cambiava un'impostazione.
        assert_eq!(help::action("find <testo>"), 0, "e' un comando da terminale");
        for cli in ["cost", "limits", "watch", "clean", "export-all", "mcp", "~uso 5h/7g"] {
            assert_eq!(help::action(cli), 0, "«{cli}» non e' un'azione della TUI");
        }

        // E la regola generale, che si controlla da sola quando l'aiuto cresce:
        // una riga e' cliccabile solo se la sua PRIMA PAROLA e' un comando.
        for l in help::lines(&theme(0)) {
            let key = l.spans.first().map(|s| s.content.trim().to_string()).unwrap_or_default();
            if help::action(&key) == 0 {
                continue;
            }
            let first = key.split_whitespace().next().unwrap_or("");
            assert!(
                help::COMMAND_KEYS.contains(&first),
                "la riga «{key}» esegue qualcosa ma «{first}» non e' un comando"
            );
        }
    }

    #[test]
    fn every_help_row_that_looks_like_a_command_is_one() {
        // La tabella e l'aiuto sono due posti diversi: questo test e' cio' che
        // impedisce che si separino in silenzio.
        assert_eq!(help::action("v"), A_READ);
        assert_eq!(help::action("W"), A_WRAPPED);
        assert_eq!(help::action("V"), A_VAULT_RESTORE);
        assert_eq!(help::action("F"), A_FLEET);
        assert_eq!(help::action("D"), A_DELPROJECT);
        // le righe composte eseguono la PRIMA, che e' il titolo della riga
        assert_eq!(help::action("r  ·  e"), A_RESUME);
        assert_eq!(help::action("x  ·  i"), A_EXPBUNDLE);
        // la legenda non e' cliccabile: sono simboli, non comandi
        for k in ["↳", "◆", "⛁", "⚱", "● ◐ ·", "(probabile)", "in blocco"] {
            assert_eq!(help::action(k), 0, "«{k}» non deve essere un comando");
        }
    }

    #[test]
    fn the_actions_that_reach_outside_now_ask_first() {
        let _lock = lang_guard(false);
        // Flotta (ssh verso altri PC), ripristino dal vault (scrive nel
        // magazzino dell'agente) e Wrapped (due file sul Desktop) partivano
        // senza chiedere nulla.
        let mut vaulted = sess("ccc", "p", "C:/p", "sessione salvata", 2000);
        vaulted.path = "C:/base/phosphor-vault/claude/enc/ccc.jsonl".into();
        assert!(vaulted.is_vaulted());
        let mut app = app_with(vec![vaulted]);
        app.dry = false; // le richieste di conferma non fanno I/O
        app.ts.select(Some(0));

        app.request_vault_restore();
        assert!(
            matches!(app.confirm.as_ref().map(|c| &c.action), Some(Pending::VaultRestore)),
            "il ripristino deve chiedere"
        );
        app.confirm = None;

        app.request_wrapped();
        assert!(
            matches!(app.confirm.as_ref().map(|c| &c.action), Some(Pending::Wrapped)),
            "la card deve chiedere: scrive sul Desktop"
        );
        app.confirm = None;

        // Senza PC registrati la flotta non ha nulla da chiedere: dice solo
        // come registrarne uno, invece di aprire una conferma vuota.
        app.request_fleet();
        assert!(app.confirm.is_none());
        assert!(app.status.contains("remote add"), "stato: {}", app.status);
    }

    #[test]
    fn query_agent_filter_and_codex_gating() {
        let mut cx = sess("ccc", "imgnav", "C:/imgnav", "sessione codex", 2000);
        cx.agent = "codex".into();
        // a row written before the field existed is Claude Code's
        let cc = sess("aaa", "imgnav", "C:/imgnav", "sessione claude", 1000);
        assert!(Query::parse("agent:codex").matches(&cx));
        assert!(!Query::parse("agent:codex").matches(&cc));
        assert!(Query::parse("agent:claude").matches(&cc));
        assert!(!Query::parse("agent:claude").matches(&cx));

        // gating: a Codex thread has no project folder under projects/, so the
        // project-level destructive actions must refuse it outright
        let mut app = app_with(vec![cc, cx]);
        let cp = app.view.iter().position(|&i| app.all[i].id == "ccc").unwrap();
        app.ts.select(Some(cp));
        app.request_delete_project();
        assert!(app.confirm.is_none() && app.delproj.is_none(), "niente modale per Codex");
        app.request_archive_project();
        assert!(app.confirm.is_none(), "niente archiviazione per Codex");
        // …and bulk delete refuses the whole selection rather than silently
        // deleting nothing (delete_session_file is confined to projects/)
        app.marked.insert("ccc".to_string());
        app.request_delete_marked();
        assert!(app.confirm.is_none(), "niente cancellazione di massa con dentro Codex");
    }

    #[test]
    fn query_host_filter_and_remote_gating() {
        let mut remote = sess("rrr", "proj", "C:/proj", "remota", 2000);
        remote.host = "pc-casa".into();
        let local = sess("lll", "proj", "C:/proj", "locale", 1000);
        assert!(Query::parse("host:pc-casa").matches(&remote));
        assert!(!Query::parse("host:pc-casa").matches(&local));
        assert!(Query::parse("host:qui").matches(&local));
        assert!(!Query::parse("host:qui").matches(&remote));

        // gating: a remote row can't be bulk-marked (its id would leak into
        // path-based bulk delete) nor archived/deleted/read locally
        let mut app = app_with(vec![local, remote]);
        let rp = app.view.iter().position(|&i| app.all[i].id == "rrr").unwrap();
        app.ts.select(Some(rp));
        app.toggle_mark();
        assert!(app.marked.is_empty(), "remote rows can't be marked");
        app.request_delete_project();
        assert!(app.confirm.is_none() && app.delproj.is_none(), "no delete modal for remote");
        app.request_archive_project();
        assert!(app.confirm.is_none(), "no archive modal for remote");
    }

    #[test]
    fn chain_collapses_to_latest_head_and_expands_to_children() {
        let all = vec![
            sess("root", "proj", "C:/proj", "Build the thing", 1000),
            sess("r1", "proj", "C:/proj", "Dove eravamo rimasti", 2000),
            sess("r2", "proj", "C:/proj", "Ripresa della sessione", 3000),
            sess("solo", "proj", "C:/proj", "Tune hyperparameters", 4000),
        ];
        let mut app = app_with(all);
        // Default sort is by date desc → "solo" (newest), then the chain head "r2".
        // Collapsed by default: chain shows only its latest session as a head.
        assert!(app.view.iter().all(|&i| app.all[i].id != "r1" && app.all[i].id != "root"),
            "le riprese sono nascoste finché la catena è compressa");
        let head_pos = app.view.iter().position(|&i| app.all[i].id == "r2").expect("head r2 visibile");
        assert_eq!(app.row_meta[head_pos].children, 2, "la testa ha 2 riprese sotto");
        assert!(!app.row_meta[head_pos].expanded);
        // "solo" is a standalone row (no children, no marker)
        let solo_pos = app.view.iter().position(|&i| app.all[i].id == "solo").unwrap();
        assert_eq!(app.row_meta[solo_pos].children, 0);

        // Expand the chain head → its older siblings appear right under it (newest first).
        app.ts.select(Some(head_pos));
        app.set_chain_expanded(Some(true));
        let hp = app.view.iter().position(|&i| app.all[i].id == "r2").unwrap();
        assert!(app.row_meta[hp].expanded);
        assert_eq!(id_at(&app, hp + 1), "r1");
        assert_eq!(id_at(&app, hp + 2), "root");
        assert_eq!(app.row_meta[hp + 1].depth, 1, "le riprese sono figlie (indentate)");

        // view_all keeps ALL sessions for export/totals regardless of collapse.
        assert_eq!(app.view_all.len(), 4);
    }

    #[test]
    fn title_resume_precision() {
        assert!(title_is_resume("Dove eravamo rimasti"));
        assert!(title_is_resume("Ripresa della sessione interrotta"));
        assert!(title_is_resume("A che punto eravamo?"));
        // false positives the broad fragments used to catch:
        assert!(!title_is_resume("Come continuare il refactor"));
        assert!(!title_is_resume("Resume parser bug fix"));
        assert!(!title_is_resume("Recapitalize the table headers"));
        assert!(!title_is_resume("Build the thing"));
    }

    #[test]
    fn toggle_all_chains_round_trips() {
        let all = vec![
            sess("root", "proj", "C:/proj", "Build the thing", 1000),
            sess("r1", "proj", "C:/proj", "Dove eravamo rimasti", 2000),
        ];
        let mut app = app_with(all);
        assert_eq!(app.view.len(), 1, "compresso: solo la testa è visibile");
        app.toggle_all_chains();
        assert_eq!(app.view.len(), 2, "espandi tutto: testa + ripresa");
        app.toggle_all_chains();
        assert_eq!(app.view.len(), 1, "comprimi tutto di nuovo");
    }

    #[test]
    fn collapse_selection_preserved_by_id() {
        // two singleton sessions in different projects; selection survives a sort flip
        let all = vec![
            sess("a", "alpha", "C:/alpha", "task one", 100),
            sess("b", "beta", "C:/beta", "task two", 200),
        ];
        let mut app = app_with(all);
        let pos = app.view.iter().position(|&i| app.all[i].id == "a").unwrap();
        app.ts.select(Some(pos));
        // flip sort direction → order changes, selection should still resolve to "a"
        app.sort_desc = !app.sort_desc;
        app.apply_filter();
        if let Some(p) = app.view.iter().position(|&i| app.all[i].id == "a") { app.ts.select(Some(p)); }
        assert_eq!(app.selected().map(|s| s.id.clone()), Some("a".to_string()));
    }

    #[test]
    fn query_text_and_field_filters() {
        let mut a = sess("a", "alpha", "C:/alpha", "Build parser", 3000);
        a.models = vec!["claude-opus-4-8".into()];
        a.files = vec!["C:/alpha/src/parser.rs".into()];
        a.tools = vec![("Edit".into(), 3)];
        a.modified = "2026-06-20T10:00:00".into();
        a.search_text = "build parser nom".into();
        let mut b = sess("b", "beta", "C:/beta", "Fix tests", 4000);
        b.models = vec!["claude-sonnet-4-6".into()];
        b.files = vec!["C:/beta/tests/run.rs".into()];
        b.modified = "2026-06-25T10:00:00".into();
        b.search_text = "fix tests".into();

        assert!(Query::parse("").matches(&a), "query vuota = tutto");
        assert!(Query::parse("parser").matches(&a));
        assert!(!Query::parse("parser").matches(&b));
        assert!(Query::parse("project:beta").matches(&b) && !Query::parse("project:beta").matches(&a));
        assert!(Query::parse("model:opus").matches(&a) && !Query::parse("model:opus").matches(&b));
        assert!(Query::parse("file:parser.rs").matches(&a) && !Query::parse("file:parser.rs").matches(&b));
        assert!(Query::parse("tool:edit").matches(&a));
        assert!(Query::parse("after:2026-06-22").matches(&b) && !Query::parse("after:2026-06-22").matches(&a));
        assert!(Query::parse("before:2026-06-22").matches(&a) && !Query::parse("before:2026-06-22").matches(&b));
        // tutti i termini/filtri in AND
        assert!(Query::parse("parser project:alpha model:opus").matches(&a));
        assert!(!Query::parse("parser project:alpha model:sonnet").matches(&a));
        // un ':' non riconosciuto resta testo libero (non rompe la ricerca)
        assert!(!Query::parse("foo:bar").matches(&a));
    }

    #[test]
    fn collapse_all_keeps_cursor_when_child_selected() {
        // Regression: collapsing everything while a ripresa child is selected must
        // not strand the cursor on a now-hidden row — it falls back to the head.
        let all = vec![
            sess("root", "proj", "C:/proj", "Build the thing", 1000),
            sess("r1", "proj", "C:/proj", "Dove eravamo rimasti", 2000),
            sess("r2", "proj", "C:/proj", "Ripresa della sessione", 3000),
        ];
        let mut app = app_with(all);
        app.toggle_all_chains(); // expand all
        let child = app.row_meta.iter().position(|m| m.depth == 1).expect("una ripresa figlia visibile");
        app.ts.select(Some(child));
        app.toggle_all_chains(); // collapse all → that child is now hidden
        assert!(app.selected().is_some(), "il cursore non resta orfano dopo il collasso");
        assert_eq!(app.selected().map(|s| s.id.clone()), Some("r2".into()), "ricade sulla testa della catena");
    }

    #[test]
    fn expand_state_survives_new_resume() {
        // Regression: expansion is keyed by the STABLE root id, so a fresh resume
        // joining the chain (new head) must not silently re-collapse it.
        let all = vec![
            sess("root", "proj", "C:/proj", "Build the thing", 1000),
            sess("r1", "proj", "C:/proj", "Dove eravamo rimasti", 2000),
        ];
        let mut app = app_with(all);
        let hp = app.view.iter().position(|&i| app.all[i].id == "r1").unwrap();
        app.ts.select(Some(hp));
        app.set_chain_expanded(Some(true));
        assert_eq!(app.view.len(), 2, "catena espansa");
        let next = vec![
            sess("root", "proj", "C:/proj", "Build the thing", 1000),
            sess("r1", "proj", "C:/proj", "Dove eravamo rimasti", 2000),
            sess("r2", "proj", "C:/proj", "Ripresa della sessione", 3000),
        ];
        app.set_sessions(next); // a new resume arrives → new head r2
        assert_eq!(app.view.len(), 3, "la catena resta espansa dopo la nuova ripresa");
    }

    #[test]
    fn kin_sketch_links_sessions_as_certain() {
        // Two sessions sharing >=2 sketch hashes are PROVEN kin and group into one
        // chain even though neither title looks like a resume; both rows are certo.
        let mut a = sess("aaa", "proj", "C:/proj", "Original work", 1000);
        let mut b = sess("bbb", "proj", "C:/proj", "Totally unrelated title", 2000);
        a.kin_sketch = vec![10, 20, 30, 40];
        b.kin_sketch = vec![20, 30, 99, 88]; // shares 20 and 30 (>= KIN_MIN_SHARED)
        let mut app = app_with(vec![a, b]);
        assert_eq!(app.view.len(), 1, "le due sessioni kin si comprimono in una catena");
        assert_eq!(app.row_meta[0].children, 1);
        assert!(app.row_meta[0].certain, "la testa segnala un link provato (✓)");
        app.ts.select(Some(0));
        app.set_chain_expanded(Some(true));
        assert_eq!(app.view.len(), 2);
        let child = app.row_meta.iter().position(|m| m.depth == 1).unwrap();
        assert!(app.row_meta[child].certain, "il figlio kin è (certo)");
    }

    #[test]
    fn kin_single_shared_hash_does_not_link() {
        // A lone coincidental shared hash is below threshold → no false family.
        let mut a = sess("aaa", "proj", "C:/proj", "Work A", 1000);
        let mut b = sess("bbb", "proj", "C:/proj", "Work B", 2000);
        a.kin_sketch = vec![10, 20, 30];
        b.kin_sketch = vec![20, 77, 88]; // shares only 20 (< KIN_MIN_SHARED)
        let app = app_with(vec![a, b]);
        assert_eq!(app.view.len(), 2, "un solo hash condiviso non crea una catena");
        assert!(app.row_meta.iter().all(|m| !m.certain));
    }

    #[test]
    fn kin_does_not_link_across_projects() {
        // Identical sketches but DIFFERENT projects must NOT merge or be marked
        // certain: collapse() groups per project, so a cross-project "proof"
        // can't be shown as one chain — claiming (certo) there would be a lie.
        let mut a = sess("aaa", "alpha", "C:/alpha", "Work", 1000);
        let mut b = sess("bbb", "beta", "C:/beta", "Work", 2000);
        a.kin_sketch = vec![10, 20, 30, 40];
        b.kin_sketch = vec![10, 20, 30, 40];
        let app = app_with(vec![a, b]);
        assert_eq!(app.view.len(), 2, "progetti diversi → niente catena cross-project");
        assert!(app.row_meta.iter().all(|m| !m.certain), "nessun ✓/(certo) cross-project");
    }

    #[test]
    fn collapsed_head_labels_with_root_title() {
        // Regression: the collapsed head row identifies the conversation by its
        // ORIGINAL title, not the latest "resume" greeting.
        let all = vec![
            sess("root", "proj", "C:/proj", "Build the thing", 1000),
            sess("r1", "proj", "C:/proj", "Dove eravamo rimasti", 2000),
        ];
        let app = app_with(all);
        let hp = app.view.iter().position(|&i| app.all[i].id == "r1").expect("head visibile");
        let meta = app.row_meta[hp];
        assert!(meta.children > 0 && !meta.expanded, "testa collassata");
        assert_eq!(app.all[meta.root].id, "root", "il root è la sessione originale");
        assert_eq!(app.all[meta.root].title, "Build the thing");
    }
}




