//! phosphor — a standalone scanner for local Claude Code sessions.
//!
//!   phosphor                 # native full-screen TUI app (no browser)
//!   phosphor --pixel         # TUI with pixelated graphics
//!   phosphor --web           # serve the browser dashboard instead
//!   phosphor --watch 10      # re-scan interval in seconds (default 5)
//!   phosphor ls              # print a table to the terminal and exit
//!   phosphor json            # dump sessions as JSON (scriptable) and exit
//!   phosphor ls --running    # only live sessions
//!   phosphor ls --project x  # filter by project name
//!   phosphor --web --port 9000   # browser dashboard on a custom port
//!   phosphor --dir <path>    # point at a different .claude directory

use phosphor::t;
use phosphor::scan::Session;
use phosphor::server::State;
use phosphor::{cache, config, live, rescan, server, tui};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

fn claude_base() -> PathBuf {
    phosphor::default_base()
}

fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.0}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn print_table(sessions: &[Session]) {
    println!(
        "{:<3} {:<20} {:<34} {:>5} {:>7} {:<16} {}",
        "",
        t!("PROGETTO", "PROJECT"),
        t!("TITOLO", "TITLE"),
        "MSG",
        t!("TOKEN", "TOKENS"),
        t!("ULTIMA ATTIVITÀ", "LAST ACTIVITY"),
        t!("STATO", "STATE")
    );
    for s in sessions {
        let dot = match s.live.as_str() {
            "running" => "●",
            "idle" => "◐",
            _ => "·",
        };
        let proj: String = tame(&s.project_name).chars().take(20).collect();
        let title: String = tame(&s.title).chars().take(34).collect();
        let modified: String = s.modified.chars().take(16).collect();
        let tok = fmt_tokens(s.input_tokens + s.output_tokens);
        println!(
            "{:<3} {:<20} {:<34} {:>5} {:>7} {:<16} {}",
            dot, proj, title, s.message_count, tok, modified, s.live
        );
    }
    let running = sessions.iter().filter(|s| s.live == "running").count();
    let idle = sessions.iter().filter(|s| s.live == "idle").count();
    println!(
        "\n{} sessioni · {} in esecuzione · {} live-idle",
        sessions.len(),
        running,
        idle
    );
}

/// The terminal help, in the language in use.
fn help() {
    if phosphor::lang::is_en() {
        help_en()
    } else {
        help_it()
    }
}

/// L'aiuto da terminale in italiano.
fn help_it() {
    println!(
"phosphor — scanner delle sessioni di Claude Code e Codex (sola lettura)

USO
  phosphor [comando] [opzioni]

COMANDI
  (nessuno)         app nativa a tutto schermo nel terminale (TUI)
  --pixel           come sopra, con grafica a pixel (toggle con 'p')
  --web             dashboard nel browser su http://127.0.0.1:8787
  ls                stampa una tabella delle sessioni ed esce
  json              stampa le sessioni in JSON (scriptabile) ed esce
  find <testo>      cerca in prompt/file/tool di tutte le sessioni
  cost              spesa 24h/7g/30g, budget e top progetti
  wrapped           genera una card condivisibile (SVG) \"Wrapped\": token, costo,
                    energia/acqua, top progetti — 100% offline, sul Desktop
  limits            piano (es. Max 20x) e reset finestra limiti
  mcp               server MCP (stdio): dà a Claude la memoria delle sessioni
                    passate (search_sessions · read_session · search_content)
  watch             monitor live: notifica i cambi di stato (Ctrl+C esce)
  clean             uso disco e sessioni vuote (non elimina senza conferma)
  archived          elenca i progetti archiviati (vedi --archive-project)
  icon [file]      rigenera l'icona dell'app (manutenzione: e' gia' nell'exe)
  retention [giorni] mostra (e alza) ogni quanto Claude Code cancella i suoi
                    transcript: di default dopo 30 giorni, senza cestino
  vault [on|off]    magazzino anti-cancellazione: hard link dei transcript in
                    ~/.claude/phosphor-vault (0 byte in piu'). Senza argomenti
                    mostra lo stato;  vault restore <id>  rimette a posto una
                    sessione che l'agente ha cancellato
  export-all        impacchetta le sessioni (con transcript + sottocartelle) in
                    UN file portabile .phx, per spostarle su un altro PC
  import <file.phx> aggiunge le sessioni di un bundle a questo PC (mai sovrascrive,
                    CHIEDE SEMPRE CONFERMA)
  import <f.phx> --remap \"<orig>=<locale>\"   rimappa un progetto al percorso di
                    QUESTO PC: riprendibile con 'claude --resume' dalla cartella
                    locale (ripetibile; il remap viene salvato per il resume)
  remote add <alias>  registra un altro tuo PC per la flotta (SOLO l'alias ssh:
                    host, utente e chiavi restano in ~/.ssh/config + ssh-agent)
  remote rm|list    rimuovi / elenca i PC remoti configurati
  fleet             interroga i PC remoti via ssh e riassume le loro sessioni
                    (nella TUI: tasto F per unirle alla lista, [alias] accanto)
  resume-here <id>  riprende una sessione NEL terminale corrente (niente nuova
                    finestra) — è ciò che la flotta esegue via ssh sull'altro PC
  sync set <dir>    configura un repo git (privato) clonato in locale per la sync
  sync push         impacchetta le sessioni in un .phx e le invia al repo (git)
  sync pull         aggiorna dal repo e importa i bundle degli altri PC (conferma)
  sync status       mostra repo, cifratura, stato git e bundle presenti
                    (cifratura opzionale: imposta syncEncrypt con un destinatario
                     age in phosphor.json; syncIdentity = chiave per decifrare)

OPZIONI
  --port <n>        porta della dashboard web (default 8787)
  --watch <sec>     intervallo di re-scan live (default 5)
  --no-open         con --web: non aprire il browser automaticamente
  --running         con ls/json/export-all: solo sessioni live
  --project <txt>   con ls/json/export-all: filtra per nome progetto
  --window <w>      con wrapped: 7g | 30g | anno | tutto  (default: anno)
  --with-projects   con wrapped: mostra i nomi dei progetti (default: anonimo)
  --no-cost         con wrapped: nasconde la riga del costo in $
  --out <file>      con export-all: percorso del file .phx da creare
  --dir <path>      usa un'altra cartella .claude
  --delete-empty    con clean: elimina le sessioni vuote (CHIEDE SEMPRE CONFERMA)
  --delete-project <nome>  cancella DEFINITIVAMENTE tutti i transcript di un
                    progetto (doppia conferma: riscrivi il nome; offre backup .phx)
  --archive-project <nome>  archivia un progetto (lo sposta in archived/, sparisce
                    dalla lista ma NON è distrutto — reversibile)
  --unarchive-project <cartella>  ripristina un progetto archiviato
  -V, --version     mostra versione precisa (vX.Y.Z · commit git · data) ed esce
  -h, --help        mostra questo aiuto

SCORCIATOIE (nell'app TUI; premi ? o F1 per l'aiuto completo)
  ↑↓ / rotella  muovi selezione        ⏎ / click   apri dettaglio sessione
  /             cerca full-text        f           filtro stato
  o / s         ordina colonna / dir   a           sub-agenti e workflow
  r             riprendi (conferma)    e           export CSV+JSON (conferma)
  x             esporta bundle .phx    i           importa bundle (conferma)
  t / T         tema avanti/indietro   p           grafica pixel on/off
  m             metrica grafici        R           rescan ora
  Tab / 1 2 3   cambia vista           q / Ctrl+C  esci

PORTARE UNA SESSIONE SU UN ALTRO PC (passo per passo)
  Vale uguale con lo STESSO account Claude o con uno DIVERSO: il file .phx non
  contiene credenziali e l'import lavora solo su file locali.

  Sul PC di partenza:
    1) phosphor export-all          tutte le sessioni → Desktop/phosphor-sessioni-*.phx
                                    (oppure --project <nome> per uno solo,
                                     --out <file.phx> per scegliere dove salvarlo)
    2) copia quel file .phx sull'altro PC (chiavetta, email, ecc.)
  Sul PC di destinazione:
    3) phosphor import <file.phx>   mostra cosa aggiunge e CHIEDE CONFERMA;
                                    non sovrascrive nulla
    4) phosphor                     le sessioni importate sono ora visibili

  Per RIPRENDERE la conversazione (non solo vederla):
    • su quel PC servono Claude Code installato e un login valido — anche un
      account DIVERSO va bene: il transcript è un file locale; i consumi andranno
      sull'account con cui sei loggato lì.
    • serve la cartella di lavoro del progetto, perché `claude --resume` cerca la
      sessione nella cartella che corrisponde al percorso del progetto:
        - STESSO percorso assoluto di prima → in phosphor seleziona la sessione e
          premi `r` (Riprendi), oppure da quella cartella:  claude --resume <id>
        - percorso DIVERSO (utente/drive)   → porta i file del progetto (es. con
          git) e dalla cartella del progetto:  claude --resume <id> --fork-session
    • il CODICE del progetto NON è dentro la sessione: portalo a parte (git/copia)
      se vuoi che la conversazione ripresa lavori sui file giusti.

FILE SCRITTI (solo dentro la cartella .claude e, per export, sul Desktop)
  phosphor.json               la tua configurazione (tema, prezzi, budget…)
  .phosphor-cache.<chiave>.jsonl  cache di scansione, rigenerabile (riscritta a ogni scan)
  Desktop/phosphor-export-*   CSV/JSON dell'export vista: data nel nome, mai sovrascrive
  Desktop/phosphor-sessioni-* bundle .phx di export-all: data nel nome, mai sovrascrive
  Desktop/phosphor-wrapped-*  card SVG di wrapped: mai sovrascrive (nome con data se esiste)

I transcript delle sessioni NON vengono mai modificati né inviati in rete.
`export-all` crea solo un nuovo file .phx (non tocca nulla di esistente).
`import` SOLO AGGIUNGE i file mancanti dentro <dir>/projects: non sovrascrive,
non modifica e non elimina nulla, e chiede sempre conferma prima di scrivere.
L'unica azione che può cancellare file è `clean --delete-empty`, e solo file
.jsonl vuoti dentro <dir>/projects, sempre dopo conferma esplicita digitata.
La voce «Riprendi» apre un terminale e lancia `claude --resume`, previa conferma."
    );
}

/// The terminal help in English.
///
/// A second block rather than a pair per line, for the same reason as the TUI
/// panel: this is a page someone reads top to bottom, and a page is revised as
/// a page. The commands and flags in the left column are identical in both, so
/// the two cannot drift on anything that matters.
fn help_en() {
    println!(
"phosphor — a read-only scanner for your Claude Code and Codex sessions

USAGE
  phosphor [command] [options]

COMMANDS
  (none)            native full-screen app in the terminal (TUI)
  --pixel           the same, with pixel graphics (toggle with 'p')
  --web             browser dashboard at http://127.0.0.1:8787
  ls                print a table of the sessions and exit
  json              print the sessions as JSON (scriptable) and exit
  find <text>       search prompts/files/tools across every session
  cost              spend 24h/7d/30d, budget and top projects
                    (--explain writes out the whole arithmetic)
  wrapped           make a shareable \"Wrapped\" card (PNG + SVG): tokens, cost,
                    energy/water, top projects — 100% offline, on the Desktop
  limits            plan (e.g. Max 20x) and limit-window reset
  mcp               MCP server (stdio): gives Claude recall over your past
                    sessions (search_sessions · read_session · search_content)
  watch             live monitor: reports state changes (Ctrl+C quits)
  clean             disk usage and empty sessions (never deletes unasked)
  archived          list archived projects (see --archive-project)
  icon [file]       regenerate the app icon (maintenance: it is already in the exe)
  retention [days]  show — and raise — how often Claude Code deletes its own
                    transcripts: 30 days by default, with no recycle bin
  vault [on|off]    anti-deletion store: hard links of the transcripts in
                    ~/.claude/phosphor-vault (0 extra bytes). With no argument
                    it shows the status;  vault restore <id>  puts back a
                    session the agent has deleted
  export-all        pack the sessions (transcripts + subfolders) into ONE
                    portable .phx file, to move them to another PC
  import <file.phx> add a bundle's sessions to this PC (never overwrites,
                    ALWAYS ASKS FIRST)
  import <f.phx> --remap \"<orig>=<local>\"   remap a project onto THIS PC's
                    path: resumable with 'claude --resume' from the local
                    folder (repeatable; the remap is saved for the resume)
  remote add <alias>  register another of your PCs for the fleet (the ssh ALIAS
                    only: host, user and keys stay in ~/.ssh/config + ssh-agent)
  remote rm|list    remove / list the configured remote PCs
  fleet             query the remote PCs over ssh and summarise their sessions
                    (in the TUI: key F merges them into the list, [alias] beside)
  resume-here <id>  resume a session IN THE CURRENT terminal (no new window) —
                    this is what the fleet runs over ssh on the other PC
  sync set <dir>    configure a (private) git repo cloned locally for sync
  sync push         pack the sessions into a .phx and send it to the repo (git)
  sync pull         update from the repo and import the other PCs' bundles (asks)
  sync status       show repo, encryption, git state and the bundles present
                    (encryption optional: set syncEncrypt to an age recipient
                     in phosphor.json; syncIdentity = the key to decrypt)

OPTIONS
  --port <n>        web dashboard port (default 8787)
  --watch <sec>     live re-scan interval (default 5)
  --no-open         with --web: do not open the browser automatically
  --running         with ls/json/export-all: live sessions only
  --project <txt>   with ls/json/export-all: filter by project name
  --explain         with cost: write out the arithmetic behind every figure
  --window <w>      with wrapped: 7g | 30g | anno | tutto  (default: anno)
  --with-projects   with wrapped: show the project names (default: anonymous)
  --no-cost         with wrapped: hide the cost line in $
  --out <file>      with export-all: path of the .phx file to create
  --dir <path>      use a different .claude folder
  --delete-empty    with clean: delete empty sessions (ALWAYS ASKS FIRST)
  --delete-project <name>  PERMANENTLY delete every transcript of a project
                    (double confirmation: re-type the name; offers a .phx backup)
  --archive-project <name>  archive a project (moves it to archived/, it leaves
                    the list but is NOT destroyed — reversible)
  --unarchive-project <folder>  restore an archived project
  -V, --version     print the precise version (vX.Y.Z · git commit · date) and exit
  -h, --help        print this help

SHORTCUTS (in the TUI app; press ? or F1 for the full, clickable help)
  ↑↓ / wheel    move the selection      ⏎ / click   open the session detail
  /             full-text search        f           state filter
  o / s         sort column / dir       a           sub-agents and workflows
  r             resume (asks first)     e           export CSV+JSON (asks)
  x             export a .phx bundle    i           import a bundle (asks)
  t / T         theme forward/back      p           pixel graphics on/off
  m             chart metric            R           rescan now
  L             English ⇄ italiano      🖰           mouse-only mode
  Tab / 1 2 3   switch view             q / Ctrl+C  quit

MOVING A SESSION TO ANOTHER PC (step by step)
  It works the same with the SAME Claude account or a DIFFERENT one: a .phx
  holds no credentials and the import only ever touches local files.

  On the PC you are leaving:
    1) phosphor export-all          every session → Desktop/phosphor-sessioni-*.phx
                                    (or --project <name> for just one,
                                     --out <file.phx> to choose where it lands)
    2) copy that .phx to the other PC (USB stick, email, whatever)
  On the destination PC:
    3) phosphor import <file.phx>   shows what it will add and ASKS FIRST;
                                    it overwrites nothing
    4) phosphor                     the imported sessions are now in the list

  To RESUME the conversation (not merely look at it):
    • that PC needs Claude Code installed and a valid login — a DIFFERENT
      account is fine: the transcript is a local file; the usage will go to
      whichever account is logged in there.
    • you need the project's working folder, because `claude --resume` looks for
      the session in the folder matching the project's path:
        - SAME absolute path as before → select the session in phosphor and
          press `r` (Resume), or from that folder:  claude --resume <id>
        - DIFFERENT path (user/drive)  → bring the project files over (git, say)
          and from the project folder:  claude --resume <id> --fork-session
    • the project's CODE is NOT inside the session: move it separately (git or a
      copy) if you want the resumed conversation to work on the right files.

FILES WRITTEN (only inside the .claude folder and, for exports, on the Desktop)
  phosphor.json               your configuration (theme, prices, budget, language…)
  .phosphor-cache.<key>.jsonl scan cache, regenerable (rewritten on every scan)
  Desktop/phosphor-export-*   CSV/JSON of the exported view: dated name, never overwrites
  Desktop/phosphor-sessioni-* .phx bundle from export-all: dated name, never overwrites
  Desktop/phosphor-wrapped-*  the wrapped card: never overwrites (dated name if one exists)

Session transcripts are NEVER modified, and nothing is ever sent over the network.
`export-all` only creates a new .phx file (it touches nothing that exists).
`import` ONLY ADDS the missing files inside <dir>/projects: it does not overwrite,
modify or remove anything, and it always asks before writing.
The only action that can delete files is `clean --delete-empty`, and only empty
.jsonl files inside <dir>/projects, always after an explicitly typed confirmation.
The \"Resume\" action opens a terminal and runs `claude --resume`, after asking."
    );
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
fn usd(n: f64) -> String {
    // `+ 0.0` normalizza lo ZERO NEGATIVO: in Rust la somma di una lista vuota
    // di float e' -0.0 (e' l'identita' che usa `Sum`), e su uno store senza
    // sessioni il totale veniva stampato «$-0.000». Il costo di niente non e'
    // meno di zero.
    let n = n + 0.0;
    if n >= 1.0 { format!("${:.2}", n) } else { format!("${:.3}", n) }
}
fn crop(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
/// Control-char sanitizer for untrusted strings printed to the terminal —
/// shared with the TUI/fleet (see `phosphor::tame`).
fn tame(s: &str) -> String {
    phosphor::tame(s)
}
fn label(s: &Session) -> String {
    format!("{} · {}", crop(&tame(&s.project_name), 16), crop(&tame(&s.title), 32))
}

/// Interactive yes/no confirmation. Returns false on EOF/non-tty (fail-safe:
/// nothing destructive happens unless the user explicitly types 'si'/'y').
fn confirm(msg: &str) -> bool {
    use std::io::Write;
    print!("{}  [scrivi 'si' per confermare, invio per annullare]: ", msg);
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_ok() {
        matches!(line.trim().to_lowercase().as_str(), "si" | "sì" | "s" | "yes" | "y")
    } else {
        false
    }
}

/// Human-readable energy (Wh -> Wh / kWh) and water (mL -> mL / L).
fn fmt_wh(wh: f64) -> String {
    let wh = wh + 0.0; // vedi `usd`: -0.0 si stampa col segno
    if wh >= 1000.0 { format!("{:.2} kWh", wh / 1000.0) } else { format!("{:.1} Wh", wh) }
}
fn fmt_ml(ml: f64) -> String {
    let ml = ml + 0.0;
    if ml >= 1000.0 { format!("{:.2} L", ml / 1000.0) } else { format!("{:.0} mL", ml) }
}

/// `phosphor cost` — spend over 24h / 7d / 30d, budget, top projects, and a
/// rough energy/water footprint estimate.
fn print_cost(base: &std::path::Path, sessions: &[Session], cfg: &config::Config) {
    let prices = &cfg.prices;
    let budget = cfg.budget;
    let now = now_ms();
    let win = |w: u64| -> f64 {
        sessions.iter().filter(|s| now.saturating_sub(s.mtime_ms) < w).map(|s| config::cost(s, prices)).sum()
    };
    let d = 86_400_000u64;
    // The window totals mix agents when both are installed, so name them: the
    // Claude and the Codex halves are priced off different lists.
    let cx = sessions.iter().filter(|s| s.is_codex()).count();
    let agents = if cx > 0 { "Claude Code + Codex" } else { "Claude Code" };
    let asof = if cfg.prices_as_of.trim().is_empty() { "?" } else { cfg.prices_as_of.trim() };
    println!(
        "{}",
        t!(
            format!("Spesa stimata ({agents}, listino del {asof}):"),
            format!("Estimated spend ({agents}, price list of {asof}):"),
        )
    );
    if phosphor::lang::is_en() { println!("  last 5h    : {}   (short limit window — local estimate)", usd(win(5 * 3_600_000))); } else { println!("  ultime 5h  : {}   (finestra limiti breve — stima locale)", usd(win(5 * 3_600_000))); }
    if phosphor::lang::is_en() { println!("  last 24h   : {}", usd(win(d))); } else { println!("  ultime 24h : {}", usd(win(d))); }
    if phosphor::lang::is_en() { println!("  last 7d    : {}", usd(win(7 * d))); } else { println!("  ultimi 7g  : {}", usd(win(7 * d))); }
    let m = win(30 * d);
    if budget > 0.0 {
        let pct = (m / budget * 100.0).round() as u64;
        let flag = if m > budget { "   ⚠ SOPRA BUDGET" } else { "" };
        if phosphor::lang::is_en() { println!("  last 30d   : {}  ({}% of {} budget){}", usd(m), pct, usd(budget), flag); } else { println!("  ultimi 30g : {}  ({}% di {} budget){}", usd(m), pct, usd(budget), flag); }
    } else {
        println!(
            "{}",
            t!(
                format!("  ultimi 30g : {}   (imposta \"budget\" in ~/.claude/phosphor.json per gli alert)", usd(m)),
                format!("  last 30d   : {}   (set \"budget\" in ~/.claude/phosphor.json for alerts)", usd(m)),
            )
        );
    }
    if phosphor::lang::is_en() { println!("  total      : {}", usd(sessions.iter().map(|s| config::cost(s, prices)).sum::<f64>())); } else { println!("  totale     : {}", usd(sessions.iter().map(|s| config::cost(s, prices)).sum::<f64>())); }
    // Un costo calcolato su prezzi vecchi e' indistinguibile da uno giusto: e'
    // l'unico modo in cui questi numeri possono mentire senza che si veda.
    if let Some(w) = config::prices_warning(&cfg.prices_as_of, &config::today_iso()) {
        println!("  ⚠ {w}");
    }

    // Rough energy/water footprint (stima, ±ordine di grandezza).
    let foot = |w: u64| -> (f64, f64) {
        sessions.iter().filter(|s| now.saturating_sub(s.mtime_ms) < w)
            .map(|s| config::footprint(s, cfg.energy_wh_per_output_token, cfg.water_l_per_kwh))
            .fold((0.0, 0.0), |(e, wt), (de, dw)| (e + de, wt + dw))
    };
    let (e24, w24) = foot(d);
    let (e7, w7) = foot(7 * d);
    let (e30, w30) = foot(30 * d);
    let (etot, wtot) = sessions.iter()
        .map(|s| config::footprint(s, cfg.energy_wh_per_output_token, cfg.water_l_per_kwh))
        .fold((0.0, 0.0), |(e, wt), (de, dw)| (e + de, wt + dw));
    println!(
        "\n{}",
        t!(
            "Footprint stimato (energia · acqua on-site) — STIMA, ±ordine di grandezza:",
            "Estimated footprint (energy · on-site water) — ESTIMATE, ±order of magnitude:",
        )
    );
    if phosphor::lang::is_en() { println!("  last 24h   : {}  ·  {}", fmt_wh(e24), fmt_ml(w24)); } else { println!("  ultime 24h : {}  ·  {}", fmt_wh(e24), fmt_ml(w24)); }
    if phosphor::lang::is_en() { println!("  last 7d    : {}  ·  {}", fmt_wh(e7), fmt_ml(w7)); } else { println!("  ultimi 7g  : {}  ·  {}", fmt_wh(e7), fmt_ml(w7)); }
    if phosphor::lang::is_en() { println!("  last 30d   : {}  ·  {}", fmt_wh(e30), fmt_ml(w30)); } else { println!("  ultimi 30g : {}  ·  {}", fmt_wh(e30), fmt_ml(w30)); }
    if phosphor::lang::is_en() { println!("  total      : {}  ·  {}", fmt_wh(etot), fmt_ml(wtot)); } else { println!("  totale     : {}  ·  {}", fmt_wh(etot), fmt_ml(wtot)); }
    if phosphor::lang::is_en() { println!("  (water = on-site cooling only; with the full energy footprint it can be ~100x)"); } else { println!("  (acqua = solo raffreddamento on-site; col footprint completo dell'energia può essere ~100x)"); }

    let mut by: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for s in sessions {
        *by.entry(s.project_name.clone()).or_insert(0.0) += config::cost(s, prices);
    }
    let mut v: Vec<(String, f64)> = by.into_iter().collect();
    v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    println!("\n{}", t!("Top progetti per costo:", "Top projects by cost:"));
    for (p, c) in v.into_iter().take(8) {
        println!("  {:<24} {}", crop(&p, 24), usd(c));
    }
    if let Some(line) = plan_line(base) {
        println!("\n{line}");
    }
}

/// One-line summary of plan + limit-window reset, or None if unavailable.
fn plan_line(base: &std::path::Path) -> Option<String> {
    use chrono::TimeZone;
    let p = phosphor::plan::load(base)?;
    let mut s = String::from(t!("Piano ", "Plan "));
    s.push_str(if p.tier_label.is_empty() { "?" } else { &p.tier_label });
    if p.extra_usage {
        s.push_str(t!(" (extra usage attivo)", " (extra usage on)"));
    }
    if let Some(end) = p.limits_end_ms {
        let abs = chrono::Local
            .timestamp_millis_opt(end as i64)
            .single()
            .map(|d| d.format("%d/%m %H:%M").to_string())
            .unwrap_or_default();
        let r = phosphor::plan::reset_in(end, now_ms());
        s.push_str(&t!(format!(" · reset limiti {r} ({abs})"), format!(" · limits reset {r} ({abs})")));
    }
    Some(s)
}

/// `phosphor limits` — show the locally-known plan and limit-window reset.
/// The official 5h/weekly percentages are NOT stored locally, so we don't guess.
fn print_limits(base: &std::path::Path) {
    match plan_line(base) {
        Some(line) => {
            println!("{line}");
            println!(
                "\n{}",
                t!(
                    "Nota: le percentuali ufficiali 5h/settimanali NON sono salvate in locale\n\
                     (Claude le riceve a runtime). Qui mostriamo solo piano e reset della finestra.",
                    "Note: the official 5h/weekly percentages are NOT stored locally\n\
                     (Claude receives them at runtime). Only the plan and the window reset are shown.",
                )
            );
        }
        None => {
            eprintln!("Info piano non disponibile: manca {}\\..\\.claude.json o i relativi campi.", base.display());
        }
    }
}

/// `phosphor clean` — disk usage and empty sessions. Deletion ALWAYS warns and
/// asks for an explicit typed confirmation (no bypass; fails safe to "no" on a
/// non-interactive terminal), and only ever touches `.jsonl` files inside
/// `<base>/projects`.
fn do_clean(base: &std::path::Path, sessions: &[Session], delete_empty: bool) {
    let mut by: std::collections::HashMap<String, (u64, u64)> = std::collections::HashMap::new();
    for s in sessions {
        let e = by.entry(s.project_name.clone()).or_insert((0, 0));
        e.0 += s.size;
        e.1 += 1;
    }
    let mut v: Vec<(String, (u64, u64))> = by.into_iter().collect();
    v.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    let total: u64 = sessions.iter().map(|s| s.size).sum();
    let (mb, nsess) = (total as f64 / 1048576.0, sessions.len());
    println!(
        "{}\n",
        t!(
            format!("Uso disco transcript: {mb:.1} MB su {nsess} sessioni"),
            format!("Transcript disk usage: {mb:.1} MB across {nsess} sessions"),
        )
    );
    println!("{}", t!("Top progetti per spazio:", "Top projects by size:"));
    for (p, (b, n)) in v.iter().take(10) {
        println!("  {:<24} {:>8.1} MB  ({} sess)", crop(p, 24), *b as f64 / 1048576.0, n);
    }
    // "Vuota" = ≤1 messaggio E 0 token (AND, non OR: un transcript con molti
    // prompt ma 0 token registrati NON è vuoto), e mai una sessione live/idle
    // (una conversazione in corso ha message_count basso e 0 token, ma non va
    // toccata). Difesa contro la perdita di dati: fail-safe verso il non-eliminare.
    let empty: Vec<&Session> = sessions
        .iter()
        .filter(|s| {
            s.live != "running"
                && s.live != "idle"
                && s.message_count <= 1
                && (s.input_tokens + s.output_tokens) == 0
        })
        .collect();
    let esize: u64 = empty.iter().map(|s| s.size).sum();
    let (ne, emb) = (empty.len(), esize as f64 / 1048576.0);
    println!(
        "\n{}",
        t!(
            format!("Sessioni vuote (≤1 messaggio e 0 token, escluse le live): {ne} ({emb:.1} MB)"),
            format!("Empty sessions (≤1 message and 0 tokens, live ones excluded): {ne} ({emb:.1} MB)"),
        )
    );
    if !delete_empty {
        println!(
            "\n{}",
            t!(
                "Niente viene eliminato senza la tua conferma. Per ripulire le sessioni vuote:",
                "Nothing is deleted without your say-so. To clear the empty sessions:",
            )
        );
        if phosphor::lang::is_en() { println!("  phosphor clean --delete-empty        → lists them and ALWAYS ASKS FIRST"); } else { println!("  phosphor clean --delete-empty        → mostra l'elenco e CHIEDE SEMPRE CONFERMA"); }
        return;
    }
    if empty.is_empty() {
        if phosphor::lang::is_en() { println!("\nNo empty sessions to delete. ✓"); } else { println!("\nNessuna sessione vuota da eliminare. ✓"); }
        return;
    }
    if phosphor::lang::is_en() { println!("\n⚠  YOU ARE ABOUT TO DELETE {} session files — THIS CANNOT BE UNDONE:", empty.len()); } else { println!("\n⚠  STAI PER ELIMINARE {} file di sessione — OPERAZIONE IRREVERSIBILE:", empty.len()); }
    for s in empty.iter().take(30) {
        println!("     {}", s.path);
    }
    if empty.len() > 30 {
        if phosphor::lang::is_en() { println!("     … and {} more", empty.len() - 30); } else { println!("     … e altri {}", empty.len() - 30); }
    }
    let proceed = confirm(&format!("Eliminare definitivamente {} file ({:.1} MB)?", empty.len(), esize as f64 / 1048576.0));
    if !proceed {
        if phosphor::lang::is_en() { println!("Cancelled: NOTHING was deleted."); } else { println!("Annullato: NON è stato eliminato nulla."); }
        return;
    }
    // defense in depth: only delete .jsonl files that live *inside* <base>/projects
    // (trailing separator so a sibling like "projects_backup" can't match the prefix)
    let mut projroot = base.join("projects").to_string_lossy().to_string();
    projroot.push(std::path::MAIN_SEPARATOR);
    let mut removed = 0u64;
    let mut freed = 0u64;
    for s in &empty {
        if s.path.starts_with(&projroot) && s.path.ends_with(".jsonl") && std::fs::remove_file(&s.path).is_ok() {
            removed += 1;
            freed += s.size;
        }
    }
    if phosphor::lang::is_en() { println!("Deleted {} files ({:.1} MB freed).", removed, freed as f64 / 1048576.0); } else { println!("Eliminati {} file ({:.1} MB liberati).", removed, freed as f64 / 1048576.0); }
}

/// `phosphor clean --delete-project "<nome>"` — cancella DEFINITIVAMENTE tutti i
/// transcript di un progetto (l'intera cartella `projects/<encoded>/`). Doppia
/// conferma: il nome è già l'argomento e va RISCRITTO per confermare. Rifiuta se
/// il nome è ambiguo o se il progetto ha sessioni live; offre un backup .phx
/// prima di cancellare (i transcript sono l'unica copia). La rimozione fisica è
/// confinata da `phosphor::delete_project_dir`.
fn do_delete_project(base: &std::path::Path, sessions: &[Session], name: &str) {
    use std::collections::BTreeSet;
    let dir_of = |s: &Session| std::path::Path::new(&s.path).parent().map(|p| p.to_path_buf());
    let dirs: BTreeSet<PathBuf> = sessions
        .iter()
        .filter(|s| s.project_name == name)
        .filter_map(dir_of)
        .collect();
    if dirs.is_empty() {
        if phosphor::lang::is_en() { println!("No project called «{name}»."); } else { println!("Nessun progetto chiamato «{name}»."); }
        let mut names: Vec<&str> = sessions.iter().map(|s| s.project_name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        if !names.is_empty() {
            if phosphor::lang::is_en() { println!("Projects available: {}", names.into_iter().take(20).collect::<Vec<_>>().join(", ")); } else { println!("Progetti disponibili: {}", names.into_iter().take(20).collect::<Vec<_>>().join(", ")); }
        }
        return;
    }
    if dirs.len() > 1 {
        if phosphor::lang::is_en() { println!("The name «{name}» is AMBIGUOUS: it matches {} distinct projects:", dirs.len()); } else { println!("Nome «{name}» AMBIGUO: corrisponde a {} progetti distinti:", dirs.len()); }
        for d in &dirs {
            println!("   {}", d.display());
        }
        if phosphor::lang::is_en() { println!("Disambiguate from the TUI: select a session of the right project and use «Delete project»."); } else { println!("Disambigua dalla TUI: seleziona una sessione del progetto giusto e usa «Cancella progetto»."); }
        return;
    }
    let dir = dirs.into_iter().next().unwrap();
    let subset: Vec<Session> = sessions
        .iter()
        .filter(|s| dir_of(s).as_deref() == Some(dir.as_path()))
        .cloned()
        .collect();
    let bytes: u64 = subset.iter().map(|s| s.size).sum();
    let live = subset.iter().filter(|s| s.live == "running" || s.live == "idle").count();

    if phosphor::lang::is_en() { println!("\nProject «{name}»"); } else { println!("\nProgetto «{name}»"); }
    if phosphor::lang::is_en() { println!("  folder:    {}", dir.display()); } else { println!("  cartella:  {}", dir.display()); }
    if phosphor::lang::is_en() { println!("  sessions:  {}  ({})", subset.len(), mb(bytes)); } else { println!("  sessioni:  {}  ({})", subset.len(), mb(bytes)); }
    if live > 0 {
        if phosphor::lang::is_en() { println!("\n⚠  {live} LIVE session(s) in this project: close them before deleting. Cancelled."); } else { println!("\n⚠  {live} sessione/i LIVE in questo progetto: chiudile prima di cancellare. Annullato."); }
        return;
    }
    // Rete di sicurezza: offri un backup .phx (recuperabile) prima della cancellazione.
    if confirm("Esportare prima un backup .phx del progetto (consigliato)?") {
        do_export_bundle(base, &subset, None);
    }
    // Doppia conferma: riscrivere il nome esatto (come la cancellazione repo su GitHub).
    if phosphor::lang::is_en() { println!("\n⚠  YOU ARE ABOUT TO PERMANENTLY DELETE the whole project — THIS CANNOT BE UNDONE."); } else { println!("\n⚠  STAI PER CANCELLARE DEFINITIVAMENTE tutto il progetto — OPERAZIONE IRREVERSIBILE."); }
    use std::io::Write;
    if phosphor::lang::is_en() { print!("To confirm, RE-TYPE the project name «{name}»: "); } else { print!("Per confermare, RISCRIVI il nome del progetto «{name}»: "); }
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let ok = std::io::stdin().read_line(&mut line).is_ok() && line.trim() == name;
    if !ok {
        if phosphor::lang::is_en() { println!("The name does not match: NOTHING was deleted."); } else { println!("Il nome non corrisponde: NON è stato cancellato nulla."); }
        return;
    }
    match phosphor::delete_project_dir(base, &dir) {
        Ok(()) => println!("✓ Progetto «{name}» cancellato: {} sessioni, {} liberati.", subset.len(), mb(bytes)),
        Err(e) => eprintln!("✗ Cancellazione fallita: {e}"),
    }
}

/// `phosphor retention [<giorni>]` — read, and optionally raise, the setting
/// that decides how long Claude Code keeps its own transcripts. See
/// `phosphor::retention`.
fn do_retention_cmd(base: &std::path::Path, arg: Option<&str>) {
    use phosphor::retention as ret;
    match arg {
        None => {
            println!("{}", ret::summary(base));
            if phosphor::lang::is_en() { println!("  files      : {}", ret::settings_path(base).display()); } else { println!("  file       : {}", ret::settings_path(base).display()); }
            if phosphor::lang::is_en() { println!("  in force   : {} days", ret::effective(base)); } else { println!("  in vigore  : {} giorni", ret::effective(base)); }
            if ret::at_risk(base) {
                println!();
                if phosphor::lang::is_en() { println!("⚠ Claude Code deletes transcripts older than this by itself, at startup."); } else { println!("⚠ Claude Code cancella da solo i transcript piu' vecchi di cosi', all'avvio."); }
                if phosphor::lang::is_en() { println!("  No recycle bin, no backup: a project idle for a month disappears."); } else { println!("  Niente cestino, niente backup: un progetto fermo da un mese sparisce."); }
                println!();
                if phosphor::lang::is_en() { println!("  phosphor retention {}    to keep them ten years", ret::RECOMMENDED_DAYS); } else { println!("  phosphor retention {}    per tenerli 10 anni", ret::RECOMMENDED_DAYS); }
                if phosphor::lang::is_en() { println!("  phosphor retention 365     to keep them one year"); } else { println!("  phosphor retention 365     per tenerli un anno"); }
            }
            println!();
            if phosphor::lang::is_en() { println!("Codex has no equivalent setting: it does not prune by date, it keeps everything."); } else { println!("Codex non ha un'impostazione equivalente: non pota per data, tiene tutto."); }
            if phosphor::lang::is_en() { println!("Against EVERYTHING else (disk cleaners, sync, manual deletion):"); } else { println!("Contro TUTTO il resto (pulitori disco, sync, cancellazioni a mano):"); }
            println!("  phosphor vault on");
        }
        Some(a) => {
            let days: u64 = match a.parse() {
                Ok(d) => d,
                Err(_) => {
                    eprintln!("Uso: phosphor retention [<giorni>]   (es. phosphor retention 3650)");
                    return;
                }
            };
            let before = ret::effective(base);
            match ret::set(base, days) {
                Ok(p) => {
                    println!("✓ cleanupPeriodDays: {before} -> {days} giorni");
                    println!("  {}", p.display());
                    if phosphor::lang::is_en() { println!("  a copy of the previous file in settings.json.phosphor-bak"); } else { println!("  copia del precedente in settings.json.phosphor-bak"); }
                    println!();
                    println!("Vale da qui in avanti. Quello gia' cancellato non torna:");
                    println!("Phosphor lo ricostruisce come puo' dai prompt (righe ⚱).");
                }
                Err(e) => eprintln!("✗ non ho modificato settings.json: {e}"),
            }
        }
    }
}

/// `phosphor icon [--out <file.ico>]` — regenerate the application icon.
///
/// A maintenance command, not an everyday one: the icon is already embedded in
/// this executable. It exists so the `.ico` in the repo stays a BUILD ARTIFACT
/// with source behind it (see `phosphor::icon`) rather than an opaque binary
/// nobody can reproduce or review in a diff.
fn do_icon_cmd(arg: Option<&str>) {
    let path = std::path::PathBuf::from(arg.unwrap_or("assets/phosphor.ico"));
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let data = phosphor::icon::ico();
    match std::fs::write(&path, &data) {
        Ok(()) => {
            println!("✓ icona scritta: {}  ({:.1} KB)", path.display(), data.len() as f64 / 1024.0);
            println!(
                "  {} dimensioni: {}",
                phosphor::icon::SIZES.len(),
                phosphor::icon::SIZES.map(|s| s.to_string()).join(", ")
            );
            println!();
            println!("Per vederla nell'exe serve ricompilare: build.rs la incorpora come");
            println!("risorsa Windows quando rc.exe del Windows SDK e' disponibile.");
        }
        Err(e) => eprintln!("✗ scrittura fallita: {e}"),
    }
}
/// `phosphor vault [status|on|off|restore <id>]` — the hard-link vault that
/// keeps transcripts alive after an agent (or a disk cleaner) deletes them.
/// See `phosphor::vault`.
fn do_vault_cmd(base: &std::path::Path, verb: &str, arg: Option<&str>) {
    let mut cfg = config::load(base);
    match verb {
        "on" | "off" => {
            cfg.vault = verb == "on";
            config::save(base, &cfg);
            if cfg.vault {
                // Scan once with the flag already saved: scan_all does the
                // linking itself, so this protects what is on disk NOW instead
                // of only what arrives later.
                let mut cache = cache::load(base);
                let (sessions, _) = phosphor::scan_all(base, &mut cache);
                let failed = phosphor::vault::link_all(base, &sessions).1;
                let (n, bytes, _, _) = phosphor::vault::stats(base);
                let d = phosphor::vault::dir(base).display().to_string();
                println!("{}", t!(format!("Vault ACCESO — {d}"), format!("Vault ON — {d}")));
                println!(
                    "  {n} transcript al sicuro ({:.1} MB di conversazioni) per 0 byte in piu': sono hard link.",
                    bytes as f64 / 1_048_576.0
                );
                if failed > 0 {
                    if phosphor::lang::is_en() { println!("  ⚠ {failed} could not be linked: hard links do not cross volumes."); } else { println!("  ⚠ {failed} non collegabili: gli hard link non attraversano i volumi."); }
                    println!("    (succede se CODEX_HOME sta su un altro disco rispetto a ~/.claude)");
                }
                println!("  Da ora ogni scansione collega i nuovi. Quando un agente cancella un");
                println!("  transcript, Phosphor continua a mostrarlo: ⛁ in lista, R per rimetterlo a posto.");
            } else {
                println!("Vault SPENTO: non collego piu' nulla.");
                println!("  Quello gia' nel vault resta dov'e' — {}", phosphor::vault::dir(base).display());
                if phosphor::lang::is_en() { println!("  Delete that folder by hand to free the orphans' space."); } else { println!("  Cancella quella cartella a mano per liberare lo spazio degli orfani."); }
            }
        }
        "restore" => {
            let id = match arg {
                Some(x) if !x.is_empty() => x,
                _ => {
                    eprintln!("Uso: phosphor vault restore <id-sessione>");
                    return;
                }
            };
            let mut cache = cache::load(base);
            let (sessions, _) = phosphor::scan_all(base, &mut cache);
            let hit = sessions.iter().find(|s| s.is_vaulted() && s.id.starts_with(id));
            match hit {
                None => eprintln!("Nessuna sessione nel vault con id che inizia per «{id}»."),
                Some(s) => match phosphor::vault::restore(base, s) {
                    Ok(p) => {
                        println!("✓ rimessa a posto: {}", p.display());
                        let cmd = if s.is_codex() { "codex resume" } else { "claude --resume" };
                        println!("  ora e' riprendibile:  {cmd} {}", s.id);
                    }
                    Err(e) => eprintln!("✗ ripristino fallito: {e}"),
                },
            }
        }
        _ => {
            let (n, bytes, orphans, obytes) = phosphor::vault::stats(base);
            let mb = |b: u64| format!("{:.1} MB", b as f64 / 1_048_576.0);
            println!("Vault: {}", if cfg.vault { t!("ACCESO", "ON") } else { t!("spento", "off") });
            if phosphor::lang::is_en() { println!("  folder     : {}", phosphor::vault::dir(base).display()); } else { println!("  cartella   : {}", phosphor::vault::dir(base).display()); }
            if phosphor::lang::is_en() { println!("  transcripts: {n}  ({} in total)", mb(bytes)); } else { println!("  transcript : {n}  ({} in totale)", mb(bytes)); }
            if phosphor::lang::is_en() { println!("  orphans    : {orphans}  ({})  <- the only bytes you really pay for", mb(obytes)); } else { println!("  orfani     : {orphans}  ({})  <- i soli byte che paghi davvero", mb(obytes)); }
            println!();
            if phosphor::lang::is_en() { println!("A hard link is a second name for the same bytes: while the original"); } else { println!("Un hard link e' un secondo nome per gli stessi byte: finche' l'originale"); }
            if phosphor::lang::is_en() { println!("exists the vault costs nothing. It owns only what somebody else has"); } else { println!("esiste il vault non occupa nulla. Diventa proprietario solo di cio' che"); }
            if phosphor::lang::is_en() { println!("deleted — which is exactly what you would have lost."); } else { println!("qualcun altro ha cancellato, cioe' esattamente cio' che avresti perso."); }
            if !cfg.vault {
                println!();
                if phosphor::lang::is_en() { println!("  phosphor vault on    to turn it on"); } else { println!("  phosphor vault on    per accenderlo"); }
            }
        }
    }
}
/// `phosphor clean --archive-project "<nome>"` — sposta un progetto in `archived/`
/// (fuori da `projects/`): sparisce dalla lista e dai `--resume` di Claude ma NON
/// viene distrutto (reversibile con `--unarchive-project`). Singola conferma.
fn do_archive_project(base: &std::path::Path, sessions: &[Session], name: &str) {
    use std::collections::BTreeSet;
    let dir_of = |s: &Session| std::path::Path::new(&s.path).parent().map(|p| p.to_path_buf());
    let dirs: BTreeSet<PathBuf> = sessions
        .iter()
        .filter(|s| s.project_name == name)
        .filter_map(dir_of)
        .collect();
    if dirs.is_empty() {
        if phosphor::lang::is_en() { println!("No project called «{name}»."); } else { println!("Nessun progetto chiamato «{name}»."); }
        return;
    }
    if dirs.len() > 1 {
        println!("Nome «{name}» AMBIGUO: {} progetti distinti — archivia dalla TUI.", dirs.len());
        for d in &dirs {
            println!("   {}", d.display());
        }
        return;
    }
    let dir = dirs.into_iter().next().unwrap();
    let subset: Vec<&Session> = sessions.iter().filter(|s| dir_of(s).as_deref() == Some(dir.as_path())).collect();
    let bytes: u64 = subset.iter().map(|s| s.size).sum();
    let live = subset.iter().filter(|s| s.live == "running" || s.live == "idle").count();
    if phosphor::lang::is_en() { println!("\nArchiving «{name}»: {} sessions ({}) — REVERSIBLE, nothing is destroyed.", subset.len(), mb(bytes)); } else { println!("\nArchivio «{name}»: {} sessioni ({}) — REVERSIBILE, non distrugge nulla.", subset.len(), mb(bytes)); }
    if live > 0 {
        if phosphor::lang::is_en() { println!("⚠  {live} LIVE session(s): close them first. Cancelled."); } else { println!("⚠  {live} sessione/i LIVE: chiudile prima. Annullato."); }
        return;
    }
    if !confirm("Archiviare il progetto (sparisce dalla lista, ripristinabile)?") {
        if phosphor::lang::is_en() { println!("Cancelled."); } else { println!("Annullato."); }
        return;
    }
    match phosphor::archive_project_dir(base, &dir) {
        Ok(dest) => {
            let folder = dir.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
            println!("✓ Archiviato in {}", dest.display());
            println!("  Ripristina con:  phosphor clean --unarchive-project \"{folder}\"");
        }
        Err(e) => eprintln!("✗ Archiviazione fallita: {e}"),
    }
}

/// `phosphor clean --unarchive-project "<cartella>"` — ripristina un progetto
/// archiviato (nome cartella come mostrato da `phosphor archived`).
fn do_unarchive_project(base: &std::path::Path, name: &str) {
    match phosphor::unarchive_project_dir(base, name) {
        Ok(dest) => println!("✓ Ripristinato in {}", dest.display()),
        Err(e) => eprintln!("✗ Ripristino fallito: {e}"),
    }
}

/// `phosphor archived` — elenca i progetti archiviati (in `archived/`).
fn do_archived_list(base: &std::path::Path) {
    let items = phosphor::list_archived(base);
    if items.is_empty() {
        if phosphor::lang::is_en() { println!("No archived projects."); } else { println!("Nessun progetto archiviato."); }
        return;
    }
    println!("Progetti archiviati ({}):", items.len());
    for (name, bytes) in &items {
        println!("  {:<44} {}", crop(name, 44), mb(*bytes));
    }
    println!("\nRipristina:  phosphor clean --unarchive-project \"<cartella>\"");
}

fn desktop_dir(base: &std::path::Path) -> PathBuf {
    std::env::var("USERPROFILE")
        .map(|h| PathBuf::from(h).join("Desktop"))
        .ok()
        .filter(|d| d.is_dir())
        .unwrap_or_else(|| base.to_path_buf())
}
fn mb(n: u64) -> String {
    format!("{:.1} MB", n as f64 / 1_048_576.0)
}

/// `phosphor wrapped` — render a single shareable "AI coding receipt" SVG card
/// from the already-scanned sessions and write it to the Desktop (never-overwrite,
/// like the other exports). 100% offline, read-only; the user shares it manually.
fn do_wrapped(base: &std::path::Path, sessions: &[Session], cfg: &config::Config, window: Option<&str>, with_projects: bool, show_cost: bool) {
    use phosphor::wrapped;
    let now = now_ms();
    let opts = wrapped::Opts {
        window: wrapped::parse_window(window.unwrap_or("year"), now),
        anonymous: !with_projects,
        show_cost,
    };
    let card = wrapped::render(sessions, cfg, now, &opts);
    if card.sessions_count == 0 {
        if phosphor::lang::is_en() { println!("No sessions in the «{}» window. Try:  phosphor wrapped --window all", card.label); } else { println!("Nessuna sessione nella finestra «{}». Prova:  phosphor wrapped --window all", card.label); }
        return;
    }
    let dir = desktop_dir(base);
    // Nice name first (phosphor-wrapped-2026.png); if it already exists, fall back
    // to a timestamped name so we never overwrite an existing card. The stamp is
    // shared by both files so the pair stays recognisable.
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let name = |ext: &str| {
        let plain = dir.join(format!("phosphor-wrapped-{}.{ext}", card.label));
        if plain.exists() {
            dir.join(format!("phosphor-wrapped-{}-{}.{ext}", card.label, stamp))
        } else {
            plain
        }
    };
    // PNG first: it is the one that actually travels. The SVG stays as the
    // lossless original for anyone who wants to print or edit it.
    let (png_path, svg_path) = (name("png"), name("svg"));
    let png_ok = phosphor::write_new(&png_path, &card.png);
    let svg_ok = phosphor::write_new(&svg_path, card.svg.as_bytes());
    match (&png_ok, &svg_ok) {
        (Err(e), Err(_)) => {
            eprintln!("✗ Scrittura fallita ({e}). Esiste già un file con quel nome? Riprova.")
        }
        _ => {
            println!("✓ {}", card.summary);
            if png_ok.is_ok() {
                println!("    {}", png_path.display());
            }
            if svg_ok.is_ok() {
                if phosphor::lang::is_en() { println!("    {}   (vector, for printing or touch-ups)", svg_path.display()); } else { println!("    {}   (vettoriale, per stampa o ritocchi)", svg_path.display()); }
            }
            if phosphor::lang::is_en() { println!("\nThe PNG pastes straight into X, Reddit or Slack: none of them"); } else { println!("\nIl PNG si incolla direttamente su X, Reddit, Slack: nessuno di loro"); }
            if phosphor::lang::is_en() { println!("renders an SVG, which is why the card never travelled."); } else { println!("renderizza un SVG, ed era il motivo per cui la card non circolava."); }
            if opts.anonymous {
                if phosphor::lang::is_en() { println!("Privacy: numbers only (no project names or paths). Use --with-projects for the names."); } else { println!("Privacy: mostra solo numeri (nessun nome progetto/percorso). Usa --with-projects per i nomi."); }
            }
        }
    }
}

/// `phosphor export-all` — bundle sessions (after any --project/--running filter)
/// into a single portable `.phx` file for moving to another PC. Writes a NEW
/// timestamped file via `write_new`: it can never overwrite anything.
fn do_export_bundle(base: &std::path::Path, sessions: &[Session], out: Option<PathBuf>) {
    if sessions.is_empty() {
        if phosphor::lang::is_en() { println!("No sessions to export (check your filters)."); } else { println!("Nessuna sessione da esportare (controlla i filtri)."); }
        return;
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let path = out.unwrap_or_else(|| desktop_dir(base).join(format!("phosphor-sessioni-{stamp}.phx")));
    let created = chrono::Local::now().to_rfc3339();
    let bytes = phosphor::bundle::build(base, sessions, &created);
    match phosphor::write_new(&path, &bytes) {
        Ok(()) => {
            if phosphor::lang::is_en() { println!("✓ Exported {} sessions (transcripts + subfolders) into one portable file:", sessions.len()); } else { println!("✓ Esportate {} sessioni (transcript + sottocartelle) in un file portabile:", sessions.len()); }
            println!("    {}  ({})", path.display(), mb(bytes.len() as u64));
            if phosphor::lang::is_en() { println!("\nA NEW file: nothing existing was touched."); } else { println!("\nFile NUOVO: nessun file esistente è stato toccato."); }
            println!("Copialo sull'altro PC e usa:  phosphor import \"{}\"", path.display());
        }
        Err(e) => eprintln!("✗ Export fallito ({e}). Esiste già un file con quel nome? Riprova."),
    }
}

/// `phosphor import <file.phx>` — add the sessions from a bundle into this PC's
/// `.claude/projects`. NEVER overwrites (existing files are kept), rejects unsafe
/// paths, and ALWAYS asks for confirmation showing exactly what will be added.
fn do_import(base: &std::path::Path, path: &std::path::Path, cli_remaps: &[(String, String)]) {
    let data = match phosphor::bundle::read_file(path) {
        Ok(d) => d,
        Err(e) => {
            if phosphor::lang::is_en() { eprintln!("Cannot read «{}»: {e}", path.display()); } else { eprintln!("Non riesco a leggere «{}»: {e}", path.display()); }
            return;
        }
    };
    do_import_data(base, &data, cli_remaps);
}

/// Import a bundle already in memory (used after decrypting a `.phx.age`).
///
/// `cli_remaps` are `(source_cwd, target_cwd)` pairs from `--remap`. They are
/// merged with the persistent `pathRemaps` in the config (CLI wins) so a session
/// exported on another PC is placed under THIS machine's path — becoming natively
/// resumable with `claude --resume` from the target directory.
fn do_import_data(base: &std::path::Path, data: &[u8], cli_remaps: &[(String, String)]) {
    let parsed = match phosphor::bundle::inspect(data) {
        Ok(p) => p,
        Err(e) => {
            if phosphor::lang::is_en() { eprintln!("Invalid bundle: {e}"); } else { eprintln!("Bundle non valido: {e}"); }
            return;
        }
    };
    let m = &parsed.manifest;

    // Merge remaps: explicit --remap first (precedence), then persistent config.
    let cfg = phosphor::config::load(base);
    let mut remaps: Vec<(String, String)> = cli_remaps.to_vec();
    for r in &cfg.path_remaps {
        if !remaps.iter().any(|(f, _)| f == &r.0) {
            remaps.push(r.clone());
        }
    }

    let plan = phosphor::bundle::apply_remapped(base, data, &parsed, true, &remaps); // dry-run

    if phosphor::lang::is_en() { println!("Bundle: {} sessions, {} files.", m.count, parsed.files.len()); } else { println!("Bundle: {} sessioni, {} file.", m.count, parsed.files.len()); }
    if !m.created.is_empty() || !m.source_host.is_empty() {
        println!(
            "  creato: {}   origine: {} ({})",
            if m.created.is_empty() { "?" } else { &m.created },
            if m.source_host.is_empty() { "?" } else { &m.source_host },
            if m.source_os.is_empty() { "?" } else { &m.source_os }
        );
    }

    // Show each source project and how it resolves on THIS machine.
    if !m.projects.is_empty() {
        let find = |src: &str| remaps.iter().find(|(f, _)| f == src).map(|(_, t)| t.clone());
        if phosphor::lang::is_en() { println!("\nProjects in the bundle (source folder → where they will land here):"); } else { println!("\nProgetti nel bundle (cartella di origine → dove finiranno qui):"); }
        for proj in &m.projects {
            match find(proj) {
                Some(target) => {
                    let exists = std::path::Path::new(&target).is_dir();
                    println!("  {proj}");
                    println!("     ↳ remap → {target}  [{}]  (cartella projects/{})",
                        if exists { "cartella presente" } else { "cartella ASSENTE: creala per il resume" },
                        phosphor::encode_cwd(&target));
                }
                None => {
                    let here = std::path::Path::new(proj).is_dir();
                    if here {
                        if phosphor::lang::is_en() { println!("  {proj}   [path exists here: resume works directly]"); } else { println!("  {proj}   [percorso presente qui: resume diretto]"); }
                    } else {
                        println!("  {proj}   ⚠ percorso non presente qui — per il resume aggiungi:  --remap \"{proj}=<percorso-locale>\"");
                    }
                }
            }
        }
    }

    if phosphor::lang::is_en() { println!("\nDestination: {}", base.join("projects").display()); } else { println!("\nDestinazione: {}", base.join("projects").display()); }
    if phosphor::lang::is_en() { println!("  to ADD                 : {} files ({})", plan.added, mb(plan.bytes)); } else { println!("  da AGGIUNGERE          : {} file ({})", plan.added, mb(plan.bytes)); }
    if phosphor::lang::is_en() { println!("  already here (skipped) : {} files  ← left untouched, nothing overwritten", plan.skipped); } else { println!("  già presenti (saltati) : {} file  ← restano intatti, niente sovrascrittura", plan.skipped); }
    if plan.rejected > 0 {
        if phosphor::lang::is_en() { println!("  ⚠ unsafe paths refused: {}", plan.rejected); } else { println!("  ⚠ percorsi NON sicuri rifiutati: {}", plan.rejected); }
    }

    if plan.added == 0 {
        if phosphor::lang::is_en() { println!("\nNothing to add: it is all here already. ✓"); } else { println!("\nNiente da aggiungere: tutto è già presente. ✓"); }
        return;
    }
    if phosphor::lang::is_en() { println!("\nImport changes and deletes NOTHING: it only adds the missing files."); } else { println!("\nL'import NON modifica né elimina nulla: aggiunge solo i file mancanti."); }
    if !confirm(&format!("Aggiungere {} file ({}) in {}?", plan.added, mb(plan.bytes), base.join("projects").display())) {
        if phosphor::lang::is_en() { println!("Cancelled: nothing was written."); } else { println!("Annullato: non è stato scritto nulla."); }
        return;
    }
    let _ = std::fs::create_dir_all(base.join("projects"));
    let rep = phosphor::bundle::apply_remapped(base, data, &parsed, false, &remaps);
    println!(
        "✓ Aggiunti {} file ({}). Saltati {} già presenti.",
        rep.added,
        mb(rep.bytes),
        rep.skipped
    );

    // Persist the explicit --remap rules so both `claude --resume` (native) and
    // Phosphor's own resume (`r`) keep working next time, without re-typing them.
    if !cli_remaps.is_empty() {
        let mut cfg2 = phosphor::config::load(base);
        let mut changed = false;
        for (from, to) in cli_remaps {
            if from.is_empty() || to.is_empty() {
                continue;
            }
            if !cfg2.path_remaps.iter().any(|(f, t)| f == from && t == to) {
                cfg2.path_remaps.push((from.clone(), to.clone()));
                changed = true;
            }
        }
        if changed {
            phosphor::config::save(base, &cfg2);
            if phosphor::lang::is_en() { println!("  ↳ remap saved in the config (pathRemaps): resume will use it from now on."); } else { println!("  ↳ remap salvato in config (pathRemaps): il resume lo userà da ora in poi."); }
        }
    }
}

// ---------------------------------------------------------------------------
// `phosphor remote` / `fleet` / `resume-here` — fleet view over ssh. Phosphor
// stores only the ssh ALIASES (hosts/keys live in ~/.ssh/config + ssh-agent),
// shells out to ssh, and treats everything a remote returns as untrusted (see
// src/fleet.rs).
// ---------------------------------------------------------------------------

/// `phosphor remote add|rm|list <alias>` — manage the fleet ssh aliases.
fn do_remote_cmd(base: &std::path::Path, verb: &str, alias: Option<&str>) {
    let mut cfg = phosphor::config::load(base);
    match verb {
        "add" => {
            let a = match alias {
                Some(a) => a,
                None => {
                    if phosphor::lang::is_en() { eprintln!("Usage: phosphor remote add <ssh-alias>"); } else { eprintln!("Uso: phosphor remote add <alias-ssh>"); }
                    return;
                }
            };
            if !phosphor::fleet::valid_alias(a) {
                if phosphor::lang::is_en() { eprintln!("Invalid alias: alphanumerics and _ . @ - only (must start alphanumeric, max 64)."); } else { eprintln!("Alias non valido: ammessi alfanumerici e _ . @ - (iniziale alfanumerica, max 64)."); }
                return;
            }
            if cfg.remotes.iter().any(|x| x == a) {
                if phosphor::lang::is_en() { println!("«{a}» is already configured."); } else { println!("«{a}» è già configurato."); }
                return;
            }
            cfg.remotes.push(a.to_string());
            phosphor::config::save(base, &cfg);
            if phosphor::lang::is_en() { println!("✓ Added «{a}» (Phosphor stores ONLY the alias: hosts and keys stay in ~/.ssh/config)."); } else { println!("✓ Aggiunto «{a}» (Phosphor salva SOLO l'alias: host e chiavi restano in ~/.ssh/config)."); }
            if phosphor::lang::is_en() { println!("  Try it now:  phosphor fleet"); } else { println!("  Prova subito:  phosphor fleet"); }
        }
        "rm" | "remove" => {
            let a = match alias {
                Some(a) => a,
                None => {
                    eprintln!("Uso: phosphor remote rm <alias>");
                    return;
                }
            };
            let before = cfg.remotes.len();
            cfg.remotes.retain(|x| x != a);
            if cfg.remotes.len() == before {
                println!("«{a}» non era configurato.");
            } else {
                phosphor::config::save(base, &cfg);
                println!("✓ Rimosso «{a}».");
            }
        }
        _ => {
            if cfg.remotes.is_empty() {
                if phosphor::lang::is_en() { println!("No remote PC configured."); } else { println!("Nessun PC remoto configurato."); }
                println!("{}", if phosphor::lang::is_en() { "Add one:  phosphor remote add <ssh-alias>   (an alias from ~/.ssh/config, or user@host)" } else { "Aggiungi:  phosphor remote add <alias-ssh>   (alias di ~/.ssh/config o utente@host)" });
            } else {
                println!("PC remoti ({}):", cfg.remotes.len());
                for a in &cfg.remotes {
                    println!("  {}", crop(a, 64));
                }
            }
        }
    }
}

/// `phosphor fleet` — fetch every configured remote's sessions over ssh and
/// print a per-host summary. Read-only everywhere; nothing is stored locally.
fn do_fleet(base: &std::path::Path) {
    use std::io::Write as _;
    let cfg = phosphor::config::load(base);
    if cfg.remotes.is_empty() {
        if phosphor::lang::is_en() { println!("No remote PC configured.  Add one:  phosphor remote add <ssh-alias>"); } else { println!("Nessun PC remoto configurato.  Aggiungi:  phosphor remote add <alias-ssh>"); }
        return;
    }
    if phosphor::lang::is_en() { println!("Fleet: {} hosts (over ssh, read-only)\n", cfg.remotes.len()); } else { println!("Flotta: {} host (via ssh, sola lettura)\n", cfg.remotes.len()); }
    let mut tot = 0usize;
    for alias in &cfg.remotes {
        print!("  {:<24} ", crop(alias, 24));
        let _ = std::io::stdout().flush();
        match phosphor::fleet::fetch_host(alias) {
            Ok(bytes) => {
                let list = phosphor::fleet::parse_sessions_json(&bytes, alias);
                let live = list.iter().filter(|s| s.live == "running" || s.live == "idle").count();
                let sz: u64 = list.iter().map(|s| s.size).sum();
                if phosphor::lang::is_en() { println!("{:>5} sessions · {live} live · {}", list.len(), mb(sz)); } else { println!("{:>5} sessioni · {live} live · {}", list.len(), mb(sz)); }
                tot += list.len();
            }
            Err(e) => println!("✗ {}", tame(&e)),
        }
    }
    if phosphor::lang::is_en() { println!("\nRemote total: {tot} sessions. In the TUI press F to merge them into the list."); } else { println!("\nTotale remoto: {tot} sessioni. Nella TUI premi F per unirle alla lista."); }
}

/// `phosphor resume-here <id>` — resume a session with `claude --resume` IN
/// the current terminal (inherited stdio, no new window). It's the remote end
/// of the fleet resume (`ssh -t <alias> phosphor resume-here <id>`): claude
/// runs on THIS machine, in the session's cwd, drawing on the ssh PTY. Handy
/// locally too. Exits with claude's exit code so failures show in the caller.
fn do_resume_here(sessions: &[Session], id: &str, remaps: &[(String, String)]) {
    if !phosphor::valid_session_id(id) {
        if phosphor::lang::is_en() { eprintln!("Invalid session id."); } else { eprintln!("Id sessione non valido."); }
        std::process::exit(2);
    }
    let s = match sessions.iter().find(|s| s.id == *id) {
        Some(s) => s,
        None => {
            if phosphor::lang::is_en() { eprintln!("No session with id {id} on this machine."); } else { eprintln!("Nessuna sessione con id {id} su questa macchina."); }
            std::process::exit(2);
        }
    };
    let recorded = phosphor::resume_cwd_for(&s.path, &s.project_path);
    let (cwd, fork) = match phosphor::resolve_cwd(&recorded, remaps) {
        Some(x) => x,
        None => {
            if phosphor::lang::is_en() { eprintln!("The project folder does not exist here: {}", tame(&recorded)); } else { eprintln!("La cartella del progetto non esiste qui: {}", tame(&recorded)); }
            if phosphor::lang::is_en() { eprintln!("Add a remap in phosphor.json (pathRemaps), or recreate the folder."); } else { eprintln!("Aggiungi un remap in phosphor.json (pathRemaps) o ricrea la cartella."); }
            std::process::exit(2);
        }
    };
    eprintln!(
        "Riprendo {} in {}{}",
        crop(id, 8),
        tame(&cwd),
        if fork { "  (percorso rimappato: fork)" } else { "" }
    );
    // `claude` on Windows is a .cmd shim CreateProcess won't resolve: go
    // through `cmd /C`. Arguments are validated (id is hex+dash only).
    let mut cmd = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.arg("/C").arg("claude").arg("--resume").arg(id);
        c
    } else {
        let mut c = std::process::Command::new("claude");
        c.arg("--resume").arg(id);
        c
    };
    if fork {
        cmd.arg("--fork-session");
    }
    cmd.current_dir(&cwd);
    match cmd.status() {
        Ok(st) => std::process::exit(st.code().unwrap_or(1)),
        Err(e) => {
            if phosphor::lang::is_en() { eprintln!("Cannot launch claude: {e}"); } else { eprintln!("Impossibile lanciare claude: {e}"); }
            std::process::exit(2);
        }
    }
}

// ---------------------------------------------------------------------------
// `phosphor sync` — move .phx bundles between PCs through a git repo the user
// controls. Phosphor only shells out to `git` (no new deps, no credentials
// handled here); the repo is a private one the user has cloned locally.
// ---------------------------------------------------------------------------

/// Run `git -C <repo> <args>`, returning stdout on success or a message on error.
fn git(repo: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| format!("git non disponibile sul PATH: {e}"))?;
    let so = String::from_utf8_lossy(&out.stdout).to_string();
    let se = String::from_utf8_lossy(&out.stderr).to_string();
    if out.status.success() {
        Ok(so)
    } else {
        Err(if se.trim().is_empty() { so } else { se })
    }
}

/// True if `name` is an executable on PATH (probed with `name --version`).
fn tool_exists(name: &str) -> bool {
    std::process::Command::new(name)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run `cmd args…` feeding `input` on stdin and returning its stdout. stdin is
/// written on a thread so a large pipe can't deadlock against stdout.
fn run_filter(cmd: &str, args: &[&str], input: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let mut child = std::process::Command::new(cmd)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("{cmd} non disponibile sul PATH: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("stdin")?;
    let buf = input.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&buf);
    });
    let out = child.wait_with_output().map_err(|e| format!("{cmd}: {e}"))?;
    let _ = writer.join();
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Encrypt `bytes` to an age recipient (literal `age1…` or a recipients file).
fn age_encrypt(recipient: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
    let args: Vec<&str> = if std::path::Path::new(recipient).is_file() {
        vec!["-R", recipient]
    } else {
        vec!["-r", recipient]
    };
    run_filter("age", &args, bytes)
}

/// Decrypt an age ciphertext using an identity file (the private key never
/// passes through Phosphor; age may prompt on the terminal for its passphrase).
fn age_decrypt(identity: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
    if identity.is_empty() {
        return Err("serve 'syncIdentity' in phosphor.json (file chiave age) per decifrare".into());
    }
    run_filter("age", &["-d", "-i", identity], bytes)
}

/// A filesystem-safe, lowercase tag identifying this machine (for the per-PC
/// bundle filename, so two PCs never fight over the same file).
fn host_tag() -> String {
    let raw = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "pc".into());
    let t: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let t = t.trim_matches('-').to_string();
    if t.is_empty() { "pc".into() } else { t }
}

/// Dispatch `phosphor sync <action>`. `push_sessions` is Some only for "push".
fn do_sync(base: &std::path::Path, action: &str, set_path: Option<&str>, push_sessions: Option<&[Session]>) {
    if action == "set" {
        let p = match set_path {
            Some(p) => p,
            None => {
                if phosphor::lang::is_en() { eprintln!("Usage:  phosphor sync set <git-repo-folder>"); } else { eprintln!("Uso:  phosphor sync set <cartella-del-repo-git>"); }
                return;
            }
        };
        let repo = PathBuf::from(p);
        if !repo.is_dir() {
            if phosphor::lang::is_en() { eprintln!("Not a folder: {p}"); } else { eprintln!("Non è una cartella: {p}"); }
            return;
        }
        if git(&repo, &["rev-parse", "--is-inside-work-tree"]).is_err() {
            if phosphor::lang::is_en() { eprintln!("«{p}» is not a git repository."); } else { eprintln!("«{p}» non è un repository git."); }
            if phosphor::lang::is_en() { eprintln!("Clone your PRIVATE repo locally first, e.g.:"); } else { eprintln!("Prima clona il tuo repo PRIVATO in locale, es.:"); }
            eprintln!("  git clone {} \"{p}\"", if phosphor::lang::is_en() { "<your-private-repo>" } else { "<tuo-repo-privato>" });
            return;
        }
        let mut cfg = config::load(base);
        cfg.sync_repo = repo.to_string_lossy().to_string();
        config::save(base, &cfg);
        if phosphor::lang::is_en() { println!("✓ Sync repo set in phosphor.json:\n    {}", repo.display()); } else { println!("✓ Repo di sync impostato in phosphor.json:\n    {}", repo.display()); }
        if phosphor::lang::is_en() { println!("Now:  phosphor sync push   (send)    ·    phosphor sync pull   (fetch)"); } else { println!("Ora:  phosphor sync push   (invia)    ·    phosphor sync pull   (recupera)"); }
        return;
    }

    let cfg = config::load(base);
    if cfg.sync_repo.trim().is_empty() {
        if phosphor::lang::is_en() { eprintln!("Sync is not configured. It needs a PRIVATE git repo cloned locally:"); } else { eprintln!("Sync non configurato. Serve un repo git PRIVATO clonato in locale:"); }
        if phosphor::lang::is_en() {
            eprintln!("  1) git clone <your-private-repo>  C:\\path\\phosphor-sync");
        } else {
            eprintln!("  1) git clone <tuo-repo-privato>  C:\\percorso\\phosphor-sync");
        }
        if phosphor::lang::is_en() {
            eprintln!("  2) phosphor sync set C:\\path\\phosphor-sync");
        } else {
            eprintln!("  2) phosphor sync set C:\\percorso\\phosphor-sync");
        }
        if phosphor::lang::is_en() { eprintln!("Then:  phosphor sync push   /   phosphor sync pull"); } else { eprintln!("Poi:  phosphor sync push   /   phosphor sync pull"); }
        return;
    }
    let repo = PathBuf::from(&cfg.sync_repo);
    if !repo.is_dir() || git(&repo, &["rev-parse", "--is-inside-work-tree"]).is_err() {
        if phosphor::lang::is_en() { eprintln!("Sync repo missing or invalid:\n    {}", repo.display()); } else { eprintln!("Repo di sync assente o non valido:\n    {}", repo.display()); }
        if phosphor::lang::is_en() { eprintln!("Fix 'syncRepo' in phosphor.json, or:  phosphor sync set <folder>"); } else { eprintln!("Correggi 'syncRepo' in phosphor.json oppure:  phosphor sync set <cartella>"); }
        return;
    }

    match action {
        "push" => do_sync_push(base, &repo, push_sessions.unwrap_or(&[]), &cfg.sync_encrypt),
        "pull" => do_sync_pull(base, &repo, &cfg.sync_identity),
        _ => do_sync_status(&repo, &cfg.sync_encrypt),
    }
}

fn do_sync_status(repo: &std::path::Path, encrypt_to: &str) {
    if phosphor::lang::is_en() { println!("Sync repo:\n    {}", repo.display()); } else { println!("Repo di sync:\n    {}", repo.display()); }
    if let Ok(o) = git(repo, &["remote", "get-url", "origin"]) {
        let url = o.trim();
        if !url.is_empty() {
            if phosphor::lang::is_en() { println!("Remote (origin):  {url}"); } else { println!("Remoto (origin):  {url}"); }
        }
    }
    if encrypt_to.trim().is_empty() {
        println!("Cifratura: OFF  (i .phx sono in chiaro — imposta 'syncEncrypt' per cifrarli)");
    } else {
        println!("Cifratura: ON  → age recipient: {}{}", clip_str(encrypt_to.trim(), 28), if tool_exists("age") { "" } else { "  ⚠ 'age' NON sul PATH" });
    }
    match git(repo, &["status", "--short", "--branch"]) {
        Ok(o) if !o.trim().is_empty() => println!("Stato git:\n{}", o.trim_end()),
        Ok(_) => println!("Stato git: pulito."),
        Err(e) => eprintln!("git status: {e}"),
    }
    let mut found = false;
    if let Ok(rd) = std::fs::read_dir(repo) {
        for e in rd.flatten() {
            let p = e.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            if name.ends_with(".phx") || name.ends_with(".phx.age") {
                let sz = e.metadata().map(|m| m.len()).unwrap_or(0);
                let tag = if name.ends_with(".age") { " 🔒" } else { "" };
                println!("  · {}  ({}){}", name, mb(sz), tag);
                found = true;
            }
        }
    }
    if !found {
        if phosphor::lang::is_en() { println!("  (no bundle yet — use  phosphor sync push)"); } else { println!("  (nessun bundle ancora — usa  phosphor sync push)"); }
    }
}

/// Truncate a plain string to `n` chars with an ellipsis (CLI-side helper).
fn clip_str(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut o: String = s.chars().take(n.saturating_sub(1)).collect();
        o.push('…');
        o
    }
}

fn do_sync_push(base: &std::path::Path, repo: &std::path::Path, sessions: &[Session], encrypt_to: &str) {
    if sessions.is_empty() {
        if phosphor::lang::is_en() { println!("No sessions to sync (check your filters)."); } else { println!("Nessuna sessione da sincronizzare (controlla i filtri)."); }
        return;
    }
    let host = host_tag();
    let plain_name = format!("phosphor-{host}.phx");
    let enc_name = format!("{plain_name}.age");
    let created = chrono::Local::now().to_rfc3339();
    let bytes = phosphor::bundle::build(base, sessions, &created);

    // Optional encryption: if configured but age is missing, ABORT rather than
    // leak plaintext. Public-key (recipient) based — no secret is stored here.
    let (fname, payload) = if !encrypt_to.trim().is_empty() {
        if !tool_exists("age") {
            if phosphor::lang::is_en() { eprintln!("⚠ Encryption requested (syncEncrypt) but 'age' is not on the PATH."); } else { eprintln!("⚠ Cifratura richiesta (syncEncrypt) ma 'age' non è sul PATH."); }
            if phosphor::lang::is_en() { eprintln!("  Cancelling rather than sending in the clear. Install age, or empty"); } else { eprintln!("  Annullo per non inviare in chiaro. Installa age, oppure svuota"); }
            if phosphor::lang::is_en() { eprintln!("  syncEncrypt in phosphor.json.  (https://age-encryption.org)"); } else { eprintln!("  syncEncrypt in phosphor.json.  (https://age-encryption.org)"); }
            return;
        }
        match age_encrypt(encrypt_to.trim(), &bytes) {
            Ok(ct) => (enc_name.clone(), ct),
            Err(e) => { eprintln!("age (cifratura) fallita: {e}"); return; }
        }
    } else {
        (plain_name.clone(), bytes)
    };

    if payload.len() as u64 > 100 * 1024 * 1024 {
        if phosphor::lang::is_en() { eprintln!("⚠ The bundle is over 100 MiB: GitHub may refuse the push."); } else { eprintln!("⚠ Il bundle supera 100 MiB: GitHub potrebbe rifiutare il push."); }
        if phosphor::lang::is_en() { eprintln!("  Split it by project:  phosphor sync push --project <name>"); } else { eprintln!("  Spezzalo per progetto:  phosphor sync push --project <nome>"); }
    }
    if let Err(e) = std::fs::write(repo.join(&fname), &payload) {
        if phosphor::lang::is_en() { eprintln!("Writing {fname} failed: {e}"); } else { eprintln!("Scrittura di {fname} fallita: {e}"); }
        return;
    }
    // Keep only the chosen variant for this host (drop the stale plain/enc twin).
    let twin = if fname == enc_name { &plain_name } else { &enc_name };
    let _ = std::fs::remove_file(repo.join(twin));
    let enc_note = if fname == enc_name { " (cifrato)" } else { "" };
    if phosphor::lang::is_en() { println!("Bundle{enc_note}: {fname}  ({}, {} sessions)", mb(payload.len() as u64), sessions.len()); } else { println!("Bundle{enc_note}: {fname}  ({}, {} sessioni)", mb(payload.len() as u64), sessions.len()); }

    // Stage everything (handles the new file plus the removed twin).
    if let Err(e) = git(repo, &["add", "-A"]) {
        eprintln!("git add: {e}");
        return;
    }
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
    let msg = format!("phosphor: {host} {stamp}");
    match git(repo, &["commit", "-m", &msg]) {
        Ok(_) => {}
        Err(e) => {
            if e.to_lowercase().contains("nothing to commit") {
                if phosphor::lang::is_en() { println!("Nothing to send: the bundle is identical to the last one. ✓"); } else { println!("Nessuna modifica da inviare: il bundle è identico all'ultimo. ✓"); }
                return;
            }
            eprintln!("git commit: {e}");
            return;
        }
    }
    print!("Invio al remoto… ");
    match git(repo, &["push"]) {
        Ok(_) => println!("✓ inviato."),
        Err(e) => {
            println!();
            if phosphor::lang::is_en() { eprintln!("git push failed: {e}"); } else { eprintln!("git push fallito: {e}"); }
            if phosphor::lang::is_en() { eprintln!("(check the repo remote and credentials: git handles the push, not Phosphor)"); } else { eprintln!("(controlla remoto e credenziali del repo: il push lo gestisce git, non Phosphor)"); }
        }
    }
}

fn do_sync_pull(base: &std::path::Path, repo: &std::path::Path, identity: &str) {
    print!("Aggiorno dal remoto… ");
    match git(repo, &["pull", "--ff-only"]) {
        Ok(_) => println!("ok."),
        Err(e) => {
            println!();
            if phosphor::lang::is_en() { eprintln!("git pull failed: {e}"); } else { eprintln!("git pull fallito: {e}"); }
            return;
        }
    }
    let host = host_tag();
    let me_plain = format!("phosphor-{host}.phx");
    let me_enc = format!("phosphor-{host}.phx.age");
    // Collect this-host-excluded bundles: plain `.phx` and encrypted `.phx.age`.
    let mut files: Vec<PathBuf> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(repo) {
        for e in rd.flatten() {
            let p = e.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            if name == me_plain || name == me_enc {
                continue;
            }
            if name.ends_with(".phx") || name.ends_with(".phx.age") {
                files.push(p);
            }
        }
    }
    files.sort();
    if files.is_empty() {
        if phosphor::lang::is_en() { println!("No bundles from other PCs to import."); } else { println!("Nessun bundle di altri PC da importare."); }
        return;
    }
    if phosphor::lang::is_en() { println!("Found {} bundles from other PCs. Importing them (with confirmation, nothing overwritten):", files.len()); } else { println!("Trovati {} bundle da altri PC. Li importo (con conferma, niente sovrascrittura):", files.len()); }
    for f in &files {
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        println!("\n── {name}");
        if name.ends_with(".age") {
            let ct = match std::fs::read(f) { Ok(d) => d, Err(e) => { eprintln!("lettura fallita: {e}"); continue; } };
            match age_decrypt(identity, &ct) {
                Ok(plain) => do_import_data(base, &plain, &[]),
                Err(e) => eprintln!("decifratura fallita: {e}"),
            }
        } else {
            do_import(base, f, &[]);
        }
    }
}

/// `phosphor watch` — poll and print live state transitions (Ctrl+C to stop).
fn do_watch(base: &std::path::Path, cache_map: &mut std::collections::HashMap<String, Session>, prices: &config::Prices, interval: u64) {
    use std::collections::{HashMap, HashSet};
    if phosphor::lang::is_en() { println!("Phosphor watch — Ctrl+C to quit (every {}s)\n", interval.max(2)); } else { println!("Phosphor watch — Ctrl+C per uscire (ogni {}s)\n", interval.max(2)); }
    let mut prev: HashMap<String, String> = HashMap::new();
    let mut stuck: HashSet<String> = HashSet::new();
    let mut first = true;
    loop {
        let (mut sessions, _) = phosphor::scan_all(base, cache_map);
        live::annotate(base, &mut sessions);
        phosphor::add_recovered(base, &mut sessions);
        let now = now_ms();
        for s in &sessions {
            let was = prev.get(&s.id).cloned();
            if !first {
                match (was.as_deref(), s.live.as_str()) {
                    (None, "running") | (Some("ended"), "running") | (Some("idle"), "running") => println!("🟢 {}  avviata", label(s)),
                    (Some("running"), "ended") | (Some("idle"), "ended") => println!("⚪ {}  terminata  ({})", label(s), usd(config::cost(s, prices))),
                    (Some("running"), "idle") => println!("🟡 {}  in pausa", label(s)),
                    _ => {}
                }
            }
            prev.insert(s.id.clone(), s.live.clone());
            // stuck: running but no transcript activity for >15 min (warn once)
            let mins = now.saturating_sub(s.mtime_ms) / 60_000;
            if s.live == "running" && mins > 15 {
                if stuck.insert(s.id.clone()) {
                    println!("⏳ {}  ferma da {} min", label(s), mins);
                }
            } else {
                stuck.remove(&s.id);
            }
        }
        first = false;
        std::thread::sleep(Duration::from_secs(interval.max(2)));
    }
}

fn main() {
    // La lingua PRIMA di leggere gli argomenti: `--help` stampa ed esce dentro
    // il ciclo di parsing, quindi caricarla piu' tardi la lascerebbe fuori
    // proprio dalla pagina che serve a chi non sa ancora come si usa.
    // Con --dir viene riletta dopo, dalla configurazione di QUELLA cartella.
    phosphor::lang::set_from_code(&config::load(&claude_base()).lang);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut port: u16 = 8787;
    let mut watch_opt: Option<u64> = None;
    let mut open = true;
    let mut list_mode = false;
    let mut json_mode = false;
    let mut web_mode = false;
    let mut selftest_mode = false;
    let mut pixel_flag = false;
    let mut running_only = false;
    let mut cost_mode = false;
    let mut limits_mode = false;
    let mut mcp_mode = false;
    let mut watch_mode = false;
    let mut clean_mode = false;
    let mut delete_empty = false;
    let mut delete_project: Option<String> = None;
    let mut archive_project: Option<String> = None;
    let mut unarchive_project: Option<String> = None;
    let mut archived_mode = false;
    let mut vault_action: Option<(String, Option<String>)> = None;
    let mut retention_action: Option<Option<String>> = None;
    let mut icon_out: Option<Option<String>> = None;
    let mut export_mode = false;
    let mut import_path: Option<String> = None;
    let mut import_remaps: Vec<(String, String)> = Vec::new();
    let mut out_path: Option<PathBuf> = None;
    let mut find_query: Option<String> = None;
    let mut project_filter: Option<String> = None;
    let mut sync_action: Option<String> = None;
    let mut sync_set_path: Option<String> = None;
    let mut remote_action: Option<(String, Option<String>)> = None;
    let mut fleet_mode = false;
    let mut resume_here: Option<String> = None;
    let mut wrapped_mode = false;
    let mut wrapped_window: Option<String> = None;
    let mut wrapped_with_projects = false;
    // `cost --explain`: scrive per esteso la catena token → prezzo → Wh → litri.
    let mut explain = false;
    let mut wrapped_no_cost = false;
    let mut base = claude_base();

    // Opzioni e comandi a cui manca il loro argomento. Senza questo elenco
    // finivano in `None` in silenzio e il programma cadeva fino in fondo, cioe'
    // APRIVA LA TUI: chi scriveva `phosphor find` dimenticando il testo si
    // trovava l'applicazione intera al posto di una ricerca, e in uno script si
    // trovava un processo che non finiva piu'.
    let mut missing: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "ls" | "list" | "--list" => list_mode = true,
            "json" | "--json" => json_mode = true,
            "cost" | "costs" => cost_mode = true,
            "limits" | "limiti" => limits_mode = true,
            "mcp" => mcp_mode = true,
            "watch" => watch_mode = true,
            "clean" => clean_mode = true,
            "wrapped" => wrapped_mode = true,
            "--with-projects" => wrapped_with_projects = true,
            "--explain" | "--spiega" => explain = true,
            "--no-cost" => wrapped_no_cost = true,
            "--window" => {
                if args.get(i + 1).is_none() { missing.push("--window <periodo>".into()); }
                if let Some(v) = args.get(i + 1) {
                    wrapped_window = Some(v.clone());
                    i += 1;
                }
            }
            "--delete-empty" => delete_empty = true,
            "--delete-project" => {
                delete_project = args.get(i + 1).cloned();
                match delete_project.is_some() {
                    true => i += 1,
                    false => missing.push("--delete-project <nome>".into()),
                }
            }
            "--archive-project" => {
                archive_project = args.get(i + 1).cloned();
                if archive_project.is_none() { missing.push("--archive-project <nome>".into()); }
                if archive_project.is_some() {
                    i += 1;
                }
            }
            "--unarchive-project" => {
                unarchive_project = args.get(i + 1).cloned();
                if unarchive_project.is_some() {
                    i += 1;
                }
            }
            "archived" => archived_mode = true,
            "icon" | "icona" => {
                let n = args.get(i + 1).cloned().filter(|x| !x.starts_with('-'));
                if n.is_some() {
                    i += 1;
                }
                icon_out = Some(n);
            }
            "retention" | "retenzione" => {
                let n = args.get(i + 1).cloned().filter(|x| x.parse::<u64>().is_ok());
                if n.is_some() {
                    i += 1;
                }
                retention_action = Some(n);
            }
            "vault" => {
                let sub = args.get(i + 1).map(|s| s.as_str()).unwrap_or("status");
                let known = matches!(sub, "on" | "off" | "status" | "restore");
                let verb = if known { sub.to_string() } else { "status".to_string() };
                let mut arg = None;
                if known {
                    i += 1;
                    if sub == "restore" {
                        arg = args.get(i + 1).cloned();
                        if arg.is_some() {
                            i += 1;
                        }
                    }
                }
                vault_action = Some((verb, arg));
            }
            "export-all" | "export" => export_mode = true,
            "sync" => {
                let sub = args.get(i + 1).map(|s| s.as_str()).unwrap_or("status");
                let known = matches!(sub, "push" | "pull" | "status" | "set");
                sync_action = Some(if known { sub.to_string() } else { "status".to_string() });
                if known {
                    i += 1;
                    if sub == "set" {
                        sync_set_path = args.get(i + 1).cloned();
                        if sync_set_path.is_none() { missing.push("sync set <cartella>".into()); }
                        if sync_set_path.is_some() {
                            i += 1;
                        }
                    }
                }
            }
            "import" => {
                import_path = args.get(i + 1).cloned();
                match import_path.is_some() {
                    true => i += 1,
                    false => missing.push("import <file.phx>".into()),
                }
            }
            "remote" => {
                // `remote add|rm|list [<alias>]` — mirrors the `sync` sub-verb style.
                let sub = args.get(i + 1).map(|s| s.as_str()).unwrap_or("list");
                let known = matches!(sub, "add" | "rm" | "remove" | "list");
                let verb = if known { sub.to_string() } else { "list".to_string() };
                let mut alias = None;
                if known {
                    i += 1;
                    if sub != "list" {
                        alias = args.get(i + 1).cloned();
                        if alias.is_some() {
                            i += 1;
                        }
                    }
                }
                remote_action = Some((verb, alias));
            }
            "fleet" => fleet_mode = true,
            "resume-here" => {
                resume_here = args.get(i + 1).cloned();
                match resume_here.is_some() {
                    true => i += 1,
                    false => missing.push("resume-here <id>".into()),
                }
            }
            "--remap" => {
                // `--remap "<percorso-origine>=<percorso-locale>"` (ripetibile):
                // rimappa la cartella di un progetto importato al path di QUESTO PC.
                if let Some(v) = args.get(i + 1) {
                    if let Some(eq) = v.find('=') {
                        let from = v[..eq].trim().to_string();
                        let to = v[eq + 1..].trim().to_string();
                        if !from.is_empty() && !to.is_empty() {
                            import_remaps.push((from, to));
                        }
                    }
                    i += 1;
                }
            }
            "--out" => match args.get(i + 1) {
                Some(v) => { out_path = Some(PathBuf::from(v)); i += 1; }
                None => missing.push("--out <file>".into()),
            },
            "find" | "search" => {
                find_query = args.get(i + 1).map(|s| s.to_lowercase());
                match find_query.is_some() {
                    true => i += 1,
                    false => missing.push("find <testo>".into()),
                }
            }
            "--web" => web_mode = true,
            "--pixel" => pixel_flag = true,
            "--selftest" => selftest_mode = true,
            "--no-open" => open = false,
            "--running" => running_only = true,
            "--project" => match args.get(i + 1) {
                Some(v) => { project_filter = Some(v.clone()); i += 1; }
                None => missing.push("--project <testo>".into()),
            },
            "--port" => match args.get(i + 1).and_then(|x| x.parse::<u16>().ok()) {
                Some(v) => { port = v; i += 1; }
                None => missing.push("--port <numero>".into()),
            },
            "--watch" => match args.get(i + 1).and_then(|x| x.parse::<u64>().ok()) {
                Some(v) => { watch_opt = Some(v.max(1)); i += 1; }
                None => missing.push("--watch <secondi>".into()),
            },
            "--dir" => match args.get(i + 1) {
                Some(v) => { base = PathBuf::from(v); i += 1; }
                None => missing.push("--dir <cartella>".into()),
            },
            "-h" | "--help" => {
                help();
                return;
            }
            "-V" | "--version" | "version" => {
                println!("phosphor {}", phosphor::version_line());
                return;
            }
            _ => {}
        }
        i += 1;
    }

    // Un argomento che manca va detto, non ignorato. Prima cadeva in `None` in
    // silenzio e il programma proseguiva fino ad APRIRE LA TUI: chi scriveva
    // `phosphor find` senza il testo si trovava l'applicazione intera al posto
    // di una ricerca, e in uno script un processo che non finiva piu'.
    if !missing.is_empty() {
        for m in &missing {
            eprintln!("Argomento mancante o non valido:  {m}");
        }
        eprintln!("\n  phosphor --help   per l'elenco completo");
        std::process::exit(2);
    }

    // Import works even on a fresh machine (no existing sessions yet), so handle
    // it before the "projects must exist" guard.
    if let Some(p) = &import_path {
        do_import(&base, std::path::Path::new(p), &import_remaps);
        return;
    }
    // sync: set/status/pull need no scan (work on a fresh machine too); push is
    // handled after the scan so it can respect --project / --running filters.
    if let Some(act) = &sync_action {
        if act != "push" {
            do_sync(&base, act, sync_set_path.as_deref(), None);
            return;
        }
    }
    if limits_mode {
        print_limits(&base);
        return;
    }
    // Archived-project management needs no scan (it works on `archived/`, outside
    // the scanned `projects/`), so it runs before the "projects must exist" guard.
    if archived_mode {
        do_archived_list(&base);
        return;
    }
    if let Some((verb, arg)) = &vault_action {
        do_vault_cmd(&base, verb, arg.as_deref());
        return;
    }
    if let Some(arg) = &retention_action {
        do_retention_cmd(&base, arg.as_deref());
        return;
    }
    if let Some(out) = &icon_out {
        do_icon_cmd(out.as_deref());
        return;
    }
    if let Some(name) = &unarchive_project {
        do_unarchive_project(&base, name);
        return;
    }
    // MCP server: speaks JSON-RPC on stdout (logs on stderr), so it must run
    // before any stdout chatter and works even with no sessions yet.
    if mcp_mode {
        if let Err(e) = phosphor::mcp::run(base) {
            eprintln!("Errore MCP: {e}");
        }
        return;
    }
    // Fleet management needs no local scan (it talks to OTHER machines).
    if let Some((verb, alias)) = &remote_action {
        do_remote_cmd(&base, verb, alias.as_deref());
        return;
    }
    if fleet_mode {
        do_fleet(&base);
        return;
    }

    if !base.join("projects").is_dir() {
        eprintln!(
            "Non trovo {}\\projects — è la cartella .claude giusta?",
            base.display()
        );
        eprintln!("Usa:  phosphor --dir C:\\percorso\\a\\.claude");
        return;
    }

    let cfg = config::load(&base);
    // La lingua prima di qualunque stampa: la riga «Scansione di …» e' gia'
    // interfaccia, e uscirebbe in italiano a chi ha scelto l'inglese.
    phosphor::lang::set_from_code(&cfg.lang);
    let watch = watch_opt.unwrap_or(cfg.watch);

    if !json_mode {
        eprint!(
            "{}",
            t!(
                format!("Scansione di {} … ", base.display()),
                format!("Scanning {} … ", base.display()),
            )
        );
    }
    let t0 = std::time::Instant::now();
    let mut cache_map = cache::load(&base);
    let (mut sessions, _) = phosphor::scan_all(&base, &mut cache_map);
    live::annotate(&base, &mut sessions);
    cache::save(&base, &sessions);
    phosphor::add_recovered(&base, &mut sessions);
    if !json_mode {
        let (n, secs) = (sessions.len(), t0.elapsed().as_secs_f64());
        eprintln!("{}", t!(format!("{n} sessioni in {secs:.1}s"), format!("{n} sessions in {secs:.1}s")));
    }

    // CLI filters (apply to ls/json output).
    if running_only {
        sessions.retain(|s| s.live != "ended");
    }
    if let Some(pf) = &project_filter {
        let pf = pf.to_lowercase();
        sessions.retain(|s| s.project_name.to_lowercase().contains(&pf));
    }

    if cost_mode {
        print_cost(&base, &sessions, &cfg);
        if explain {
            println!();
            for l in config::explain(&sessions, &cfg) {
                println!("{l}");
            }
        }
        return;
    }
    if wrapped_mode {
        do_wrapped(&base, &sessions, &cfg, wrapped_window.as_deref(), wrapped_with_projects, !wrapped_no_cost);
        return;
    }
    if let Some(q) = &find_query {
        sessions.retain(|s| {
            s.search_text.contains(q) || s.project_name.to_lowercase().contains(q) || s.title.to_lowercase().contains(q)
        });
        print_table(&sessions);
        eprintln!("\n{} risultati per «{}»", sessions.len(), q);
        return;
    }
    // Checked BEFORE clean so `phosphor clean --delete-project X` reaches it.
    if let Some(name) = &delete_project {
        do_delete_project(&base, &sessions, name);
        return;
    }
    if let Some(name) = &archive_project {
        do_archive_project(&base, &sessions, name);
        return;
    }
    if clean_mode {
        do_clean(&base, &sessions, delete_empty);
        return;
    }
    if export_mode {
        do_export_bundle(&base, &sessions, out_path);
        return;
    }
    if sync_action.as_deref() == Some("push") {
        do_sync(&base, "push", None, Some(&sessions));
        return;
    }
    if watch_mode {
        do_watch(&base, &mut cache_map, &cfg.prices, watch);
        return;
    }

    if json_mode {
        println!("{}", server::sessions_json(&sessions));
        return;
    }
    // `resume-here <id>`: run `claude --resume` IN THIS terminal (no new window).
    // It's the remote end of the fleet resume (`ssh -t <alias> phosphor
    // resume-here <id>`), and works locally too.
    if let Some(id) = &resume_here {
        do_resume_here(&sessions, id, &cfg.path_remaps);
        return;
    }
    if list_mode {
        print_table(&sessions);
        return;
    }
    if selftest_mode {
        let cache = Arc::new(Mutex::new(cache_map));
        let ok = tui::selftest(base, sessions, cache);
        println!("selftest TUI: {}", if ok { "OK" } else { "FAIL" });
        std::process::exit(if ok { 0 } else { 1 });
    }

    if web_mode {
        let state = Arc::new(State {
            base,
            sessions: RwLock::new(sessions),
            cache: Mutex::new(cache_map),
        });
        // Background watcher: periodic incremental re-scan + liveness refresh.
        {
            let st = state.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(watch));
                rescan(&st);
            });
        }
        server::serve(state, port, open);
        return;
    }

    // Default: native full-screen TUI (its own watcher runs inside).
    let cache = Arc::new(Mutex::new(cache_map));
    let theme_idx = tui::theme_index(&cfg.theme);
    let pixel = pixel_flag || cfg.pixel;
    if let Err(e) = tui::run(base, sessions, cache, cfg.prices, cfg.budget, cfg.path_remaps, cfg.sync_repo, theme_idx, pixel, watch) {
        eprintln!("Errore TUI: {e}");
    }
}
