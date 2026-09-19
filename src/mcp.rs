//! `phosphor mcp` — a minimal Model Context Protocol server over stdio, so
//! Claude itself can recall your PAST Claude Code sessions: search them, read a
//! transcript, grep their content. 100% offline: JSON-RPC over stdin/stdout, no
//! network, read-only. Logs go to stderr; stdout carries only MCP messages.
//!
//! Spec: messages are newline-delimited JSON-RPC 2.0 (no embedded newlines).
//! Lifecycle: initialize -> initialized (notification) -> tools/list, tools/call.

use crate::json::{escape, P};
use crate::scan::{self, Session};
use crate::tui::Query;
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

const PROTOCOL_VERSION: &str = "2025-06-18";

// ---- tiny JSON value tree (parse only what the client sends) ---------------

enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr, // present for shape only: the server never reads an array param
    Obj(Vec<(String, Json)>),
}

/// Max JSON nesting we parse from an (untrusted) stdin message. `parse_value` is
/// recursive-descent; without a cap a line with tens of thousands of nested
/// brackets would overflow the stack and abort the process (the MCP main loop
/// has no catch_unwind). Mirrors `json::P::skip`'s own 512 guard.
const MAX_DEPTH: u32 = 512;

fn parse_value(p: &mut P) -> Json {
    parse_value_depth(p, 0)
}

fn parse_value_depth(p: &mut P, depth: u32) -> Json {
    if depth >= MAX_DEPTH {
        let _ = p.skip(); // consume and discard the over-deep subtree (bounded)
        return Json::Null;
    }
    match p.peek_ws() {
        b'{' => {
            let mut o = Vec::new();
            if p.obj_begin() {
                loop {
                    match p.obj_key() {
                        Some(k) => o.push((k, parse_value_depth(p, depth + 1))),
                        None => break,
                    }
                    if !p.obj_sep() { break; }
                }
            }
            Json::Obj(o)
        }
        b'[' => {
            // A syntactically valid array, but no MCP request field is ever read
            // as one — consume and discard it (json::P::skip has its own 512-deep
            // guard, so this stays bounded without threading `depth` through).
            let _ = p.skip();
            Json::Arr
        }
        b'"' => Json::Str(p.take_string().unwrap_or_default()),
        b't' | b'f' => Json::Bool(p.take_bool()),
        b'-' | b'0'..=b'9' => Json::Num(p.take_number()),
        _ => { let _ = p.skip(); Json::Null }
    }
}

impl Json {
    fn get(&self, k: &str) -> Option<&Json> {
        match self { Json::Obj(o) => o.iter().find(|(kk, _)| kk == k).map(|(_, v)| v), _ => None }
    }
    fn as_str(&self) -> Option<&str> {
        match self { Json::Str(s) => Some(s), _ => None }
    }
    fn as_usize(&self) -> Option<usize> {
        match self { Json::Num(n) if *n >= 0.0 => Some(*n as usize), _ => None }
    }
    /// Re-serialize (used only to echo the request id verbatim).
    fn to_json(&self) -> String {
        match self {
            Json::Null => "null".into(),
            Json::Bool(b) => b.to_string(),
            Json::Num(n) => if n.fract() == 0.0 && n.is_finite() { format!("{}", *n as i64) } else { n.to_string() },
            Json::Str(s) => format!("\"{}\"", escape(s)),
            Json::Arr | Json::Obj(_) => "null".into(), // ids are never composite
        }
    }
}

// ---- server ---------------------------------------------------------------

pub fn run(base: PathBuf) -> io::Result<()> {
    // Index the user's sessions once at startup (the client launches one server
    // per session). Read-only; nothing is written.
    let mut cache: HashMap<String, Session> = HashMap::new();
    let (mut sessions, _) = crate::scan_all(&base, &mut cache);
    crate::add_recovered(&base, &mut sessions);
    eprintln!("phosphor mcp: {} sessioni indicizzate (sola lettura, offline)", sessions.len());

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut out = io::stdout();
    let mut buf = String::new();
    loop {
        buf.clear();
        let n = reader.read_line(&mut buf)?;
        if n == 0 { break; } // stdin closed -> shut down
        let line = buf.trim();
        if line.is_empty() { continue; }
        if let Some(resp) = handle(line, &base, &sessions) {
            out.write_all(resp.as_bytes())?;
            out.write_all(b"\n")?;
            out.flush()?;
        }
    }
    Ok(())
}

/// Handle one JSON-RPC message. Returns Some(response line) for requests, None
/// for notifications (which take no reply).
fn handle(line: &str, base: &Path, sessions: &[Session]) -> Option<String> {
    let mut p = P::new(line.as_bytes());
    let msg = parse_value(&mut p);
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = msg.get("id"); // absent => notification
    let id_str = id.map(|j| j.to_json());

    // Notifications (no id) get no response.
    let id_str = match id_str { Some(s) => s, None => return None };

    match method {
        "initialize" => {
            let pv = msg.get("params").and_then(|p| p.get("protocolVersion")).and_then(|v| v.as_str()).unwrap_or(PROTOCOL_VERSION);
            let result = format!(
                concat!(
                    "{{\"protocolVersion\":\"{}\",\"capabilities\":{{\"tools\":{{}}}},",
                    "\"serverInfo\":{{\"name\":\"phosphor\",\"version\":\"{}\"}},",
                    "\"instructions\":\"Memoria delle sessioni passate dell'utente (Claude Code e Codex) (sola lettura, locale). Usa search_sessions per trovarle, read_session per leggerne una, search_content per cercare nel testo delle conversazioni.\"}}"
                ),
                escape(pv), crate::VERSION
            );
            Some(rpc_result(&id_str, &result))
        }
        "tools/list" => Some(rpc_result(&id_str, TOOLS_LIST)),
        "tools/call" => {
            let params = msg.get("params");
            let name = params.and_then(|p| p.get("name")).and_then(|v| v.as_str()).unwrap_or("");
            let args = params.and_then(|p| p.get("arguments"));
            Some(call_tool(&id_str, name, args, base, sessions))
        }
        "ping" => Some(rpc_result(&id_str, "{}")),
        _ => Some(rpc_error(&id_str, -32601, &format!("metodo non supportato: {method}"))),
    }
}

fn call_tool(id: &str, name: &str, args: Option<&Json>, base: &Path, sessions: &[Session]) -> String {
    match name {
        "search_sessions" => {
            let q = args.and_then(|a| a.get("query")).and_then(|v| v.as_str()).unwrap_or("");
            let limit = args.and_then(|a| a.get("limit")).and_then(|v| v.as_usize()).unwrap_or(20).clamp(1, 200);
            text_result(id, &tool_search_sessions(q, limit, sessions), false)
        }
        "read_session" => match args.and_then(|a| a.get("id")).and_then(|v| v.as_str()) {
            Some(sid) => {
                let max = args.and_then(|a| a.get("max_chars")).and_then(|v| v.as_usize()).unwrap_or(20000).clamp(500, 200_000);
                match tool_read_session(sid, max, base, sessions) {
                    Some(t) => text_result(id, &t, false),
                    None => text_result(id, &format!("nessuna sessione con id '{sid}'"), true),
                }
            }
            None => text_result(id, "manca l'argomento 'id'", true),
        },
        "search_content" => match args.and_then(|a| a.get("text")).and_then(|v| v.as_str()) {
            Some(text) if !text.trim().is_empty() => {
                let limit = args.and_then(|a| a.get("limit")).and_then(|v| v.as_usize()).unwrap_or(20).clamp(1, 100);
                text_result(id, &tool_search_content(text, limit, base, sessions), false)
            }
            _ => text_result(id, "manca l'argomento 'text'", true),
        },
        other => text_result(id, &format!("tool sconosciuto: {other}"), true),
    }
}

// ---- tools ----------------------------------------------------------------

fn fmt_session_line(s: &Session) -> String {
    let day = s.modified.get(..10).unwrap_or("");
    let tok = s.input_tokens + s.output_tokens;
    let title = s.title.replace('\n', " ");
    format!("• {title}\n    progetto: {} · {} · {} msg · {} tok · id: {}",
        s.project_name, day, s.message_count, tok, s.id)
}

fn tool_search_sessions(query: &str, limit: usize, sessions: &[Session]) -> String {
    let q = Query::parse(query);
    let mut hits: Vec<&Session> = sessions.iter().filter(|s| q.matches(s)).collect();
    hits.sort_by(|a, b| b.mtime_ms.cmp(&a.mtime_ms));
    if hits.is_empty() {
        return format!("Nessuna sessione per la query '{query}'.");
    }
    let total = hits.len();
    let shown: Vec<String> = hits.iter().take(limit).map(|s| fmt_session_line(s)).collect();
    let head = if total > limit {
        format!("{total} sessioni trovate (prime {limit}):\n\n")
    } else {
        format!("{total} sessioni trovate:\n\n")
    };
    format!("{head}{}", shown.join("\n\n"))
}

fn tool_read_session(id: &str, max_chars: usize, base: &Path, sessions: &[Session]) -> Option<String> {
    let s = sessions.iter().find(|s| s.id == id)?;
    // `turns_of` e non `scan::read_transcript`: quest'ultimo conosce solo il
    // formato di Claude Code, quindi una sessione Codex tornava VUOTA e una
    // recuperata provava ad aprire un file che non esiste. Cioe' la memoria
    // che diamo a Claude aveva due buchi silenziosi.
    let turns = crate::turns_of(base, s);
    let mut out = format!("# {} ({})\n\n", s.title.replace('\n', " "), s.project_name);
    for t in turns {
        let who = match t.role { 0 => "Utente", 1 => "Claude", _ => "·" };
        out.push_str(who);
        out.push_str(": ");
        out.push_str(t.text.trim());
        out.push_str("\n\n");
        if out.len() >= max_chars { out.push_str("… [troncato]"); break; }
    }
    Some(out)
}

fn tool_search_content(text: &str, limit: usize, base: &Path, sessions: &[Session]) -> String {
    let needle = text.to_lowercase();
    let mut out: Vec<String> = Vec::new();
    for s in sessions {
        if out.len() >= limit { break; }
        // `grep_of` e non `grep_transcript`: lo stesso buco gia' chiuso in
        // `read_session`. Quest'ultimo conosce solo il formato di Claude Code,
        // quindi le sessioni Codex e quelle recuperate non venivano MAI
        // trovate — e senza dirlo: zero risultati sembra «non c'e'», non
        // «non ho guardato».
        let hits = crate::grep_of(base, s, &needle, 2);
        for h in hits {
            if out.len() >= limit { break; }
            let who = match h.role { 0 => "utente", 1 => "claude", _ => "·" };
            out.push(format!("• {} [{}] (id: {})\n    {}",
                s.title.replace('\n', " "), who, s.id, h.snippet.replace('\n', " ")));
        }
    }
    if out.is_empty() {
        format!("Nessun contenuto contiene '{text}'.")
    } else {
        format!("{} risultati per '{text}':\n\n{}", out.len(), out.join("\n\n"))
    }
}

// ---- JSON-RPC response builders -------------------------------------------

fn rpc_result(id: &str, result: &str) -> String {
    format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{result}}}")
}
fn rpc_error(id: &str, code: i32, message: &str) -> String {
    format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"error\":{{\"code\":{code},\"message\":\"{}\"}}}}", escape(message))
}
/// A tools/call result carrying a single text block.
fn text_result(id: &str, text: &str, is_error: bool) -> String {
    let result = format!("{{\"content\":[{{\"type\":\"text\",\"text\":\"{}\"}}],\"isError\":{}}}", escape(text), is_error);
    rpc_result(id, &result)
}

const TOOLS_LIST: &str = r#"{"tools":[
{"name":"search_sessions","description":"Cerca tra le sessioni passate dell'utente. Supporta testo libero e filtri inline: project: model: file: tool: agent: after:YYYY-MM-DD before:YYYY-MM-DD (es. \"parser project:phosphor after:2026-06-01\"). Restituisce titolo, progetto, data, conteggi e id.","inputSchema":{"type":"object","properties":{"query":{"type":"string","description":"Testo + filtri opzionali. Vuoto = sessioni piu' recenti."},"limit":{"type":"integer","description":"Max risultati (default 20)."}}}},
{"name":"read_session","description":"Legge il transcript (conversazione) di una sessione dato il suo id (ottenuto da search_sessions).","inputSchema":{"type":"object","properties":{"id":{"type":"string","description":"L'id della sessione."},"max_chars":{"type":"integer","description":"Limite caratteri (default 20000)."}},"required":["id"]}},
{"name":"search_content","description":"Cerca una stringa NEL CONTENUTO di tutte le conversazioni passate e restituisce frammenti con la sessione di origine.","inputSchema":{"type":"object","properties":{"text":{"type":"string","description":"Testo da cercare nel contenuto."},"limit":{"type":"integer","description":"Max frammenti (default 20)."}},"required":["text"]}}
]}"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn sess(id: &str, proj: &str, title: &str, mtime: u64) -> Session {
        Session { id: id.into(), project_name: proj.into(), title: title.into(), mtime_ms: mtime,
            modified: "2026-06-20T10:00:00".into(), search_text: title.to_lowercase(), ..Default::default() }
    }

    #[test]
    fn the_memory_we_give_claude_covers_both_agents() {
        // Due volte lo stesso difetto: `read_session` prima, `search_content`
        // poi. Tutti e due leggevano i file col parser di Claude Code, quindi
        // una sessione Codex tornava vuota e una recuperata provava ad aprire
        // un file inesistente — senza dirlo. Zero risultati sembra «non c'e'»,
        // non «non ho guardato», ed e' il tipo di bugia che nessuno verifica.
        let dir = std::env::temp_dir().join(format!("phosphor-mcp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // Un rollout Codex vero: il formato e' suo, non quello di Claude Code.
        let roll = dir.join("rollout-2026-09-10T08-00-00-16b42417.jsonl");
        std::fs::write(
            &roll,
            concat!(
                r#"{"type":"session_meta","payload":{"id":"16b42417","cwd":"C:\\p","timestamp":"2026-09-10T08:00:00.000Z"}}"#,
                "\n",
                r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"come si configura il parallaxe"}]}}"#,
                "\n",
            ),
        )
        .unwrap();
        let mut cx = sess("16b42417", "p", "sessione codex", 1000);
        cx.path = roll.to_string_lossy().into_owned();
        cx.agent = "codex".into();

        let sessions = vec![cx];
        let call = r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"search_content","arguments":{"text":"parallaxe"}}}"#;
        let r = handle(call, &dir, &sessions).expect("risposta");
        // NON basta cercare la parola: quando non trova nulla la risposta e'
        // «Nessun contenuto contiene 'parallaxe'», che la parola ce l'ha
        // dentro. Si controlla che ci sia un RISULTATO: il pallino e l'id.
        assert!(!r.contains("Nessun contenuto"), "nessun risultato: {r}");
        assert!(r.contains("16b42417"), "il risultato deve citare la sessione: {r}");
        assert!(r.contains("parallaxe"), "e mostrare il frammento: {r}");

        // E la stessa sessione si deve poter leggere per intero.
        let read = r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"read_session","arguments":{"id":"16b42417"}}}"#;
        let r = handle(read, &dir, &sessions).expect("risposta");
        assert!(!r.contains("nessuna sessione"), "sessione non trovata: {r}");
        assert!(r.contains("parallaxe"), "read_session su Codex deve restituire il testo: {r}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn initialize_echoes_protocol_and_advertises_tools() {
        let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"c","version":"1"}}}"#;
        let r = handle(init, Path::new("."), &[]).expect("init has a response");
        assert!(r.contains("\"id\":1"));
        assert!(r.contains("\"protocolVersion\":\"2025-06-18\""));
        assert!(r.contains("\"serverInfo\""));
        // tools/list
        let tl = handle(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, Path::new("."), &[]).unwrap();
        assert!(tl.contains("search_sessions") && tl.contains("read_session") && tl.contains("search_content"));
    }

    #[test]
    fn notification_gets_no_response() {
        assert!(handle(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, Path::new("."), &[]).is_none());
    }

    #[test]
    fn deeply_nested_params_do_not_overflow() {
        // A hostile JSON-RPC line with tens of thousands of nested arrays must be
        // parsed without a stack overflow (depth-capped recursion). Simply reaching
        // this assertion means the process didn't abort.
        let deep = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\",\"params\":{}{}}}",
            "[".repeat(20000),
            "]".repeat(20000)
        );
        let _ = handle(&deep, Path::new("."), &[]);
    }

    #[test]
    fn unknown_method_is_jsonrpc_error() {
        let r = handle(r#"{"jsonrpc":"2.0","id":7,"method":"bogus"}"#, Path::new("."), &[]).unwrap();
        assert!(r.contains("\"error\"") && r.contains("-32601") && r.contains("\"id\":7"));
    }

    #[test]
    fn search_sessions_tool_filters_and_reports() {
        let s = vec![
            sess("aaa", "phosphor", "Build the parser", 3000),
            sess("bbb", "other", "Fix tests", 4000),
        ];
        let call = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"search_sessions","arguments":{"query":"project:phosphor"}}}"#;
        let r = handle(call, Path::new("."), &s).unwrap();
        assert!(r.contains("Build the parser"), "trova la sessione del progetto phosphor");
        assert!(!r.contains("Fix tests"), "esclude l'altro progetto");
        assert!(r.contains("id: aaa"));
        assert!(r.contains("\"isError\":false"));
    }

    #[test]
    fn read_session_missing_id_is_error() {
        let r = handle(r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"read_session","arguments":{"id":"nope"}}}"#, Path::new("."), &[]).unwrap();
        assert!(r.contains("\"isError\":true"));
    }
}
