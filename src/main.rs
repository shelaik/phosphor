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

use phosphor::scan::Session;
use phosphor::server::State;
use phosphor::{cache, config, live, rescan, scan, server, tui};
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
        "", "PROGETTO", "TITOLO", "MSG", "TOKEN", "ULTIMA ATTIVITÀ", "STATO"
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

fn help() {
    println!(
"phosphor — scanner delle sessioni di Claude Code (sola lettura)

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
  .phosphor-cache.v2.jsonl    cache di scansione, rigenerabile (riscritta a ogni scan)
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

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
fn usd(n: f64) -> String {
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
    if wh >= 1000.0 { format!("{:.2} kWh", wh / 1000.0) } else { format!("{:.1} Wh", wh) }
}
fn fmt_ml(ml: f64) -> String {
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
    println!("Spesa stimata (Claude Code):");
    println!("  ultime 5h  : {}   (finestra limiti breve — stima locale)", usd(win(5 * 3_600_000)));
    println!("  ultime 24h : {}", usd(win(d)));
    println!("  ultimi 7g  : {}", usd(win(7 * d)));
    let m = win(30 * d);
    if budget > 0.0 {
        let pct = (m / budget * 100.0).round() as u64;
        let flag = if m > budget { "   ⚠ SOPRA BUDGET" } else { "" };
        println!("  ultimi 30g : {}  ({}% di {} budget){}", usd(m), pct, usd(budget), flag);
    } else {
        println!("  ultimi 30g : {}   (imposta \"budget\" in ~/.claude/phosphor.json per gli alert)", usd(m));
    }
    println!("  totale     : {}", usd(sessions.iter().map(|s| config::cost(s, prices)).sum::<f64>()));

    // Rough energy/water footprint (stima, ±ordine di grandezza).
    let foot = |w: u64| -> (f64, f64) {
        sessions.iter().filter(|s| now.saturating_sub(s.mtime_ms) < w)
            .map(|s| config::footprint(s, cfg.energy_wh_per_token, cfg.water_ml_per_token))
            .fold((0.0, 0.0), |(e, wt), (de, dw)| (e + de, wt + dw))
    };
    let (e24, w24) = foot(d);
    let (e7, w7) = foot(7 * d);
    let (e30, w30) = foot(30 * d);
    let (etot, wtot) = sessions.iter()
        .map(|s| config::footprint(s, cfg.energy_wh_per_token, cfg.water_ml_per_token))
        .fold((0.0, 0.0), |(e, wt), (de, dw)| (e + de, wt + dw));
    println!("\nFootprint stimato (energia · acqua on-site) — STIMA, ±ordine di grandezza:");
    println!("  ultime 24h : {}  ·  {}", fmt_wh(e24), fmt_ml(w24));
    println!("  ultimi 7g  : {}  ·  {}", fmt_wh(e7), fmt_ml(w7));
    println!("  ultimi 30g : {}  ·  {}", fmt_wh(e30), fmt_ml(w30));
    println!("  totale     : {}  ·  {}", fmt_wh(etot), fmt_ml(wtot));
    println!("  (acqua = solo raffreddamento on-site; col footprint completo dell'energia può essere ~100x)");

    let mut by: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for s in sessions {
        *by.entry(s.project_name.clone()).or_insert(0.0) += config::cost(s, prices);
    }
    let mut v: Vec<(String, f64)> = by.into_iter().collect();
    v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    println!("\nTop progetti per costo:");
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
    let mut s = String::from("Piano ");
    s.push_str(if p.tier_label.is_empty() { "?" } else { &p.tier_label });
    if p.extra_usage {
        s.push_str(" (extra usage attivo)");
    }
    if let Some(end) = p.limits_end_ms {
        let abs = chrono::Local
            .timestamp_millis_opt(end as i64)
            .single()
            .map(|d| d.format("%d/%m %H:%M").to_string())
            .unwrap_or_default();
        s.push_str(&format!(" · reset limiti {} ({})", phosphor::plan::reset_in(end, now_ms()), abs));
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
                "\nNota: le percentuali ufficiali 5h/settimanali NON sono salvate in locale\n\
                 (Claude le riceve a runtime). Qui mostriamo solo piano e reset della finestra."
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
    println!("Uso disco transcript: {:.1} MB su {} sessioni\n", total as f64 / 1048576.0, sessions.len());
    println!("Top progetti per spazio:");
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
    println!("\nSessioni vuote (≤1 messaggio e 0 token, escluse le live): {} ({:.1} MB)", empty.len(), esize as f64 / 1048576.0);
    if !delete_empty {
        println!("\nNiente viene eliminato senza la tua conferma. Per ripulire le sessioni vuote:");
        println!("  phosphor clean --delete-empty        → mostra l'elenco e CHIEDE SEMPRE CONFERMA");
        return;
    }
    if empty.is_empty() {
        println!("\nNessuna sessione vuota da eliminare. ✓");
        return;
    }
    println!("\n⚠  STAI PER ELIMINARE {} file di sessione — OPERAZIONE IRREVERSIBILE:", empty.len());
    for s in empty.iter().take(30) {
        println!("     {}", s.path);
    }
    if empty.len() > 30 {
        println!("     … e altri {}", empty.len() - 30);
    }
    let proceed = confirm(&format!("Eliminare definitivamente {} file ({:.1} MB)?", empty.len(), esize as f64 / 1048576.0));
    if !proceed {
        println!("Annullato: NON è stato eliminato nulla.");
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
    println!("Eliminati {} file ({:.1} MB liberati).", removed, freed as f64 / 1048576.0);
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
        println!("Nessun progetto chiamato «{name}».");
        let mut names: Vec<&str> = sessions.iter().map(|s| s.project_name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        if !names.is_empty() {
            println!("Progetti disponibili: {}", names.into_iter().take(20).collect::<Vec<_>>().join(", "));
        }
        return;
    }
    if dirs.len() > 1 {
        println!("Nome «{name}» AMBIGUO: corrisponde a {} progetti distinti:", dirs.len());
        for d in &dirs {
            println!("   {}", d.display());
        }
        println!("Disambigua dalla TUI: seleziona una sessione del progetto giusto e usa «Cancella progetto».");
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

    println!("\nProgetto «{name}»");
    println!("  cartella:  {}", dir.display());
    println!("  sessioni:  {}  ({})", subset.len(), mb(bytes));
    if live > 0 {
        println!("\n⚠  {live} sessione/i LIVE in questo progetto: chiudile prima di cancellare. Annullato.");
        return;
    }
    // Rete di sicurezza: offri un backup .phx (recuperabile) prima della cancellazione.
    if confirm("Esportare prima un backup .phx del progetto (consigliato)?") {
        do_export_bundle(base, &subset, None);
    }
    // Doppia conferma: riscrivere il nome esatto (come la cancellazione repo su GitHub).
    println!("\n⚠  STAI PER CANCELLARE DEFINITIVAMENTE tutto il progetto — OPERAZIONE IRREVERSIBILE.");
    use std::io::Write;
    print!("Per confermare, RISCRIVI il nome del progetto «{name}»: ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let ok = std::io::stdin().read_line(&mut line).is_ok() && line.trim() == name;
    if !ok {
        println!("Il nome non corrisponde: NON è stato cancellato nulla.");
        return;
    }
    match phosphor::delete_project_dir(base, &dir) {
        Ok(()) => println!("✓ Progetto «{name}» cancellato: {} sessioni, {} liberati.", subset.len(), mb(bytes)),
        Err(e) => eprintln!("✗ Cancellazione fallita: {e}"),
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
        println!("Nessun progetto chiamato «{name}».");
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
    println!("\nArchivio «{name}»: {} sessioni ({}) — REVERSIBILE, non distrugge nulla.", subset.len(), mb(bytes));
    if live > 0 {
        println!("⚠  {live} sessione/i LIVE: chiudile prima. Annullato.");
        return;
    }
    if !confirm("Archiviare il progetto (sparisce dalla lista, ripristinabile)?") {
        println!("Annullato.");
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
        println!("Nessun progetto archiviato.");
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
        println!("Nessuna sessione nella finestra «{}». Prova:  phosphor wrapped --window all", card.label);
        return;
    }
    let dir = desktop_dir(base);
    // Nice name first (phosphor-wrapped-2026.svg); if it already exists, fall back
    // to a timestamped name so we never overwrite an existing card.
    let plain = dir.join(format!("phosphor-wrapped-{}.svg", card.label));
    let path = if plain.exists() {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        dir.join(format!("phosphor-wrapped-{}-{}.svg", card.label, stamp))
    } else {
        plain
    };
    match phosphor::write_new(&path, card.svg.as_bytes()) {
        Ok(()) => {
            println!("✓ {}", card.summary);
            println!("    {}", path.display());
            println!("\nÈ un SVG: aprilo nel browser e fai uno screenshot per condividerlo.");
            if opts.anonymous {
                println!("Privacy: mostra solo numeri (nessun nome progetto/percorso). Usa --with-projects per i nomi.");
            }
        }
        Err(e) => eprintln!("✗ Scrittura fallita ({e}). Esiste già un file con quel nome? Riprova."),
    }
}

/// `phosphor export-all` — bundle sessions (after any --project/--running filter)
/// into a single portable `.phx` file for moving to another PC. Writes a NEW
/// timestamped file via `write_new`: it can never overwrite anything.
fn do_export_bundle(base: &std::path::Path, sessions: &[Session], out: Option<PathBuf>) {
    if sessions.is_empty() {
        println!("Nessuna sessione da esportare (controlla i filtri).");
        return;
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let path = out.unwrap_or_else(|| desktop_dir(base).join(format!("phosphor-sessioni-{stamp}.phx")));
    let created = chrono::Local::now().to_rfc3339();
    let bytes = phosphor::bundle::build(base, sessions, &created);
    match phosphor::write_new(&path, &bytes) {
        Ok(()) => {
            println!("✓ Esportate {} sessioni (transcript + sottocartelle) in un file portabile:", sessions.len());
            println!("    {}  ({})", path.display(), mb(bytes.len() as u64));
            println!("\nFile NUOVO: nessun file esistente è stato toccato.");
            println!("Copialo sull'altro PC e usa:  phosphor import \"{}\"", path.display());
        }
        Err(e) => eprintln!("✗ Export fallito ({e}). Esiste già un file con quel nome? Riprova."),
    }
}

/// `phosphor import <file.phx>` — add the sessions from a bundle into this PC's
/// `.claude/projects`. NEVER overwrites (existing files are kept), rejects unsafe
/// paths, and ALWAYS asks for confirmation showing exactly what will be added.
fn do_import(base: &std::path::Path, path: &std::path::Path, cli_remaps: &[(String, String)]) {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Non riesco a leggere «{}»: {e}", path.display());
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
            eprintln!("Bundle non valido: {e}");
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

    println!("Bundle: {} sessioni, {} file.", m.count, parsed.files.len());
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
        println!("\nProgetti nel bundle (cartella di origine → dove finiranno qui):");
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
                        println!("  {proj}   [percorso presente qui: resume diretto]");
                    } else {
                        println!("  {proj}   ⚠ percorso non presente qui — per il resume aggiungi:  --remap \"{proj}=<percorso-locale>\"");
                    }
                }
            }
        }
    }

    println!("\nDestinazione: {}", base.join("projects").display());
    println!("  da AGGIUNGERE          : {} file ({})", plan.added, mb(plan.bytes));
    println!("  già presenti (saltati) : {} file  ← restano intatti, niente sovrascrittura", plan.skipped);
    if plan.rejected > 0 {
        println!("  ⚠ percorsi NON sicuri rifiutati: {}", plan.rejected);
    }

    if plan.added == 0 {
        println!("\nNiente da aggiungere: tutto è già presente. ✓");
        return;
    }
    println!("\nL'import NON modifica né elimina nulla: aggiunge solo i file mancanti.");
    if !confirm(&format!("Aggiungere {} file ({}) in {}?", plan.added, mb(plan.bytes), base.join("projects").display())) {
        println!("Annullato: non è stato scritto nulla.");
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
            println!("  ↳ remap salvato in config (pathRemaps): il resume lo userà da ora in poi.");
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
                    eprintln!("Uso: phosphor remote add <alias-ssh>");
                    return;
                }
            };
            if !phosphor::fleet::valid_alias(a) {
                eprintln!("Alias non valido: ammessi alfanumerici e _ . @ - (iniziale alfanumerica, max 64).");
                return;
            }
            if cfg.remotes.iter().any(|x| x == a) {
                println!("«{a}» è già configurato.");
                return;
            }
            cfg.remotes.push(a.to_string());
            phosphor::config::save(base, &cfg);
            println!("✓ Aggiunto «{a}» (Phosphor salva SOLO l'alias: host e chiavi restano in ~/.ssh/config).");
            println!("  Prova subito:  phosphor fleet");
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
                println!("Nessun PC remoto configurato.");
                println!("Aggiungi:  phosphor remote add <alias-ssh>   (alias di ~/.ssh/config o utente@host)");
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
        println!("Nessun PC remoto configurato.  Aggiungi:  phosphor remote add <alias-ssh>");
        return;
    }
    println!("Flotta: {} host (via ssh, sola lettura)\n", cfg.remotes.len());
    let mut tot = 0usize;
    for alias in &cfg.remotes {
        print!("  {:<24} ", crop(alias, 24));
        let _ = std::io::stdout().flush();
        match phosphor::fleet::fetch_host(alias) {
            Ok(bytes) => {
                let list = phosphor::fleet::parse_sessions_json(&bytes, alias);
                let live = list.iter().filter(|s| s.live == "running" || s.live == "idle").count();
                let sz: u64 = list.iter().map(|s| s.size).sum();
                println!("{:>5} sessioni · {live} live · {}", list.len(), mb(sz));
                tot += list.len();
            }
            Err(e) => println!("✗ {}", tame(&e)),
        }
    }
    println!("\nTotale remoto: {tot} sessioni. Nella TUI premi F per unirle alla lista.");
}

/// `phosphor resume-here <id>` — resume a session with `claude --resume` IN
/// the current terminal (inherited stdio, no new window). It's the remote end
/// of the fleet resume (`ssh -t <alias> phosphor resume-here <id>`): claude
/// runs on THIS machine, in the session's cwd, drawing on the ssh PTY. Handy
/// locally too. Exits with claude's exit code so failures show in the caller.
fn do_resume_here(sessions: &[Session], id: &str, remaps: &[(String, String)]) {
    if !phosphor::valid_session_id(id) {
        eprintln!("Id sessione non valido.");
        std::process::exit(2);
    }
    let s = match sessions.iter().find(|s| s.id == *id) {
        Some(s) => s,
        None => {
            eprintln!("Nessuna sessione con id {id} su questa macchina.");
            std::process::exit(2);
        }
    };
    let recorded = phosphor::resume_cwd_for(&s.path, &s.project_path);
    let (cwd, fork) = match phosphor::resolve_cwd(&recorded, remaps) {
        Some(x) => x,
        None => {
            eprintln!("La cartella del progetto non esiste qui: {}", tame(&recorded));
            eprintln!("Aggiungi un remap in phosphor.json (pathRemaps) o ricrea la cartella.");
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
            eprintln!("Impossibile lanciare claude: {e}");
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
                eprintln!("Uso:  phosphor sync set <cartella-del-repo-git>");
                return;
            }
        };
        let repo = PathBuf::from(p);
        if !repo.is_dir() {
            eprintln!("Non è una cartella: {p}");
            return;
        }
        if git(&repo, &["rev-parse", "--is-inside-work-tree"]).is_err() {
            eprintln!("«{p}» non è un repository git.");
            eprintln!("Prima clona il tuo repo PRIVATO in locale, es.:");
            eprintln!("  git clone <tuo-repo-privato> \"{p}\"");
            return;
        }
        let mut cfg = config::load(base);
        cfg.sync_repo = repo.to_string_lossy().to_string();
        config::save(base, &cfg);
        println!("✓ Repo di sync impostato in phosphor.json:\n    {}", repo.display());
        println!("Ora:  phosphor sync push   (invia)    ·    phosphor sync pull   (recupera)");
        return;
    }

    let cfg = config::load(base);
    if cfg.sync_repo.trim().is_empty() {
        eprintln!("Sync non configurato. Serve un repo git PRIVATO clonato in locale:");
        eprintln!("  1) git clone <tuo-repo-privato>  C:\\percorso\\phosphor-sync");
        eprintln!("  2) phosphor sync set C:\\percorso\\phosphor-sync");
        eprintln!("Poi:  phosphor sync push   /   phosphor sync pull");
        return;
    }
    let repo = PathBuf::from(&cfg.sync_repo);
    if !repo.is_dir() || git(&repo, &["rev-parse", "--is-inside-work-tree"]).is_err() {
        eprintln!("Repo di sync assente o non valido:\n    {}", repo.display());
        eprintln!("Correggi 'syncRepo' in phosphor.json oppure:  phosphor sync set <cartella>");
        return;
    }

    match action {
        "push" => do_sync_push(base, &repo, push_sessions.unwrap_or(&[]), &cfg.sync_encrypt),
        "pull" => do_sync_pull(base, &repo, &cfg.sync_identity),
        _ => do_sync_status(&repo, &cfg.sync_encrypt),
    }
}

fn do_sync_status(repo: &std::path::Path, encrypt_to: &str) {
    println!("Repo di sync:\n    {}", repo.display());
    if let Ok(o) = git(repo, &["remote", "get-url", "origin"]) {
        let url = o.trim();
        if !url.is_empty() {
            println!("Remoto (origin):  {url}");
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
        println!("  (nessun bundle ancora — usa  phosphor sync push)");
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
        println!("Nessuna sessione da sincronizzare (controlla i filtri).");
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
            eprintln!("⚠ Cifratura richiesta (syncEncrypt) ma 'age' non è sul PATH.");
            eprintln!("  Annullo per non inviare in chiaro. Installa age, oppure svuota");
            eprintln!("  syncEncrypt in phosphor.json.  (https://age-encryption.org)");
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
        eprintln!("⚠ Il bundle supera 100 MiB: GitHub potrebbe rifiutare il push.");
        eprintln!("  Spezzalo per progetto:  phosphor sync push --project <nome>");
    }
    if let Err(e) = std::fs::write(repo.join(&fname), &payload) {
        eprintln!("Scrittura di {fname} fallita: {e}");
        return;
    }
    // Keep only the chosen variant for this host (drop the stale plain/enc twin).
    let twin = if fname == enc_name { &plain_name } else { &enc_name };
    let _ = std::fs::remove_file(repo.join(twin));
    let enc_note = if fname == enc_name { " (cifrato)" } else { "" };
    println!("Bundle{enc_note}: {fname}  ({}, {} sessioni)", mb(payload.len() as u64), sessions.len());

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
                println!("Nessuna modifica da inviare: il bundle è identico all'ultimo. ✓");
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
            eprintln!("git push fallito: {e}");
            eprintln!("(controlla remoto e credenziali del repo: il push lo gestisce git, non Phosphor)");
        }
    }
}

fn do_sync_pull(base: &std::path::Path, repo: &std::path::Path, identity: &str) {
    print!("Aggiorno dal remoto… ");
    match git(repo, &["pull", "--ff-only"]) {
        Ok(_) => println!("ok."),
        Err(e) => {
            println!();
            eprintln!("git pull fallito: {e}");
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
        println!("Nessun bundle di altri PC da importare.");
        return;
    }
    println!("Trovati {} bundle da altri PC. Li importo (con conferma, niente sovrascrittura):", files.len());
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
    println!("Phosphor watch — Ctrl+C per uscire (ogni {}s)\n", interval.max(2));
    let projects = base.join("projects");
    let mut prev: HashMap<String, String> = HashMap::new();
    let mut stuck: HashSet<String> = HashSet::new();
    let mut first = true;
    loop {
        let (mut sessions, _) = scan::scan_incremental(&projects, cache_map);
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
    let mut wrapped_no_cost = false;
    let mut base = claude_base();

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
            "--no-cost" => wrapped_no_cost = true,
            "--window" => {
                if let Some(v) = args.get(i + 1) {
                    wrapped_window = Some(v.clone());
                    i += 1;
                }
            }
            "--delete-empty" => delete_empty = true,
            "--delete-project" => {
                delete_project = args.get(i + 1).cloned();
                if delete_project.is_some() {
                    i += 1;
                }
            }
            "--archive-project" => {
                archive_project = args.get(i + 1).cloned();
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
            "export-all" | "export" => export_mode = true,
            "sync" => {
                let sub = args.get(i + 1).map(|s| s.as_str()).unwrap_or("status");
                let known = matches!(sub, "push" | "pull" | "status" | "set");
                sync_action = Some(if known { sub.to_string() } else { "status".to_string() });
                if known {
                    i += 1;
                    if sub == "set" {
                        sync_set_path = args.get(i + 1).cloned();
                        if sync_set_path.is_some() {
                            i += 1;
                        }
                    }
                }
            }
            "import" => {
                import_path = args.get(i + 1).cloned();
                if import_path.is_some() {
                    i += 1;
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
                if resume_here.is_some() {
                    i += 1;
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
            "--out" => {
                if let Some(v) = args.get(i + 1) {
                    out_path = Some(PathBuf::from(v));
                    i += 1;
                }
            }
            "find" | "search" => {
                find_query = args.get(i + 1).map(|s| s.to_lowercase());
                if find_query.is_some() {
                    i += 1;
                }
            }
            "--web" => web_mode = true,
            "--pixel" => pixel_flag = true,
            "--selftest" => selftest_mode = true,
            "--no-open" => open = false,
            "--running" => running_only = true,
            "--project" => {
                if let Some(v) = args.get(i + 1) {
                    project_filter = Some(v.clone());
                    i += 1;
                }
            }
            "--port" => {
                if let Some(v) = args.get(i + 1).and_then(|x| x.parse::<u16>().ok()) {
                    port = v;
                    i += 1;
                }
            }
            "--watch" => {
                if let Some(v) = args.get(i + 1).and_then(|x| x.parse::<u64>().ok()) {
                    watch_opt = Some(v.max(1));
                    i += 1;
                }
            }
            "--dir" => {
                if let Some(v) = args.get(i + 1) {
                    base = PathBuf::from(v);
                    i += 1;
                }
            }
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
    let watch = watch_opt.unwrap_or(cfg.watch);

    if !json_mode {
        eprint!("Scansione di {} … ", base.display());
    }
    let t0 = std::time::Instant::now();
    let mut cache_map = cache::load(&base);
    let projects = base.join("projects");
    let (mut sessions, _) = scan::scan_incremental(&projects, &mut cache_map);
    live::annotate(&base, &mut sessions);
    cache::save(&base, &sessions);
    phosphor::add_recovered(&base, &mut sessions);
    if !json_mode {
        eprintln!(
            "{} sessioni in {:.1}s",
            sessions.len(),
            t0.elapsed().as_secs_f64()
        );
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
