# Phosphor

**Every conversation you have had with Claude Code and the Codex CLI, in one
list you can actually use.**

Your coding agents already keep a full record of every session — what you asked,
what they did, which files they touched, what it cost. It sits on your disk as
thousands of JSON lines you will never read, in folders you will never open, and
Claude Code deletes it after 30 days.

Phosphor turns that into a screen.

```
●   my-api          Fix the auth middleware        412   1.2M  today 14:22  running
◐   frontend        Rewrite the settings page       89   340k  today 09:10  idle
·   scraper      ◆  Parse the sitemap               23    61k  yesterday    ended
⛁   old-project     Migrate to Postgres            201   890k  3 weeks ago  ended
```

One row per session. Click one to read the whole conversation back. Click
*Resume* and the agent picks it up where you left off — in the right folder,
with its memory intact.

### What it actually does for you

- **Find that conversation again.** Search every prompt, file and tool you have
  ever used, across both agents, in milliseconds. "Where did I fix that CORS
  thing?" is a question with an answer now.
- **Pick up where you stopped.** Resume any session, including on a different
  PC: Phosphor packs one into a single portable file, or asks your other
  machines over ssh and shows their sessions beside yours.
- **See what it costs.** Spend per day, per project, per model — priced model by
  model, with the arithmetic printed out if you want to check it. Plus the
  energy and water behind it, which nothing else will tell you.
- **Stop losing work.** Claude Code deletes its own transcripts after 30 days,
  with no recycle bin. Phosphor asks you about that on the first run, rebuilds
  what was already lost, and can keep everything alive for **zero extra bytes**.

### What it will not do

It **reads**. It never edits a transcript and never sends anything anywhere on
its own — no telemetry, no account, no network at all unless you explicitly run
the optional git sync. Anything that writes a file, contacts another PC or
deletes something tells you first and waits for a yes.

One Rust binary, no installer, no runtime, three dependencies. Windows-ready;
it works in a terminal, in your browser, or as a pixel-art adventure.

---

## Contents

- [Install and run](#install-and-run)
- [Commands](#commands)
- [Options](#options)
- [Shortcuts (TUI)](#shortcuts-tui)
- [What it shows](#what-it-shows)
- [The vault](#the-vault--keep-transcripts-for-zero-extra-bytes)
- [Two agents in one list](#two-agents-in-one-list)
- [Porting between PCs](#porting-between-pcs)
- [Plan limits](#plan-limits)
- [Security](#security)
- [Privacy and files written](#privacy-and-files-written)
- [Configuration](#configuration)
- [Build from source](#build-from-source)
- [Graphical version](#graphical-version)

---

## Install and run

**Windows — 30 seconds, no admin rights, no runtime.**

1. Open the [latest release](../../releases/latest) and download **`phosphor.exe`**.
2. Double-click it.

That is the whole thing. It is a single self-contained file: nothing is
installed, nothing is registered, and deleting the `.exe` uninstalls it
completely.

**Want it on your PATH and on the Desktop?** Download `install.ps1` from the
same release, put it *next to* the `.exe`, then right-click it > *Run with
PowerShell*. It copies Phosphor under `%LOCALAPPDATA%\Phosphor`, adds that to
your user PATH and makes a Desktop shortcut — still no admin rights. The same
release also carries `phosphor-adv.exe` (the pixel-art version) and
`SHA256SUMS.txt` if you want to check what you downloaded.

The first time it opens it will show you three screens explaining the list, ask
whether to stop Claude Code from deleting your history, and get out of the way.

```
phosphor                 native terminal app (default, no browser)
phosphor --pixel         same, with "pixel" graphics
phosphor --web           browser dashboard at http://127.0.0.1:8787
phosphor --dir <path>    use a different .claude directory
```

**Everyone sees their own sessions.** There is nothing baked into the binary:
at every launch it reads `%USERPROFILE%` and opens that user's `.claude`. Hand
the same `.exe` to a colleague and they get their own list — an empty one if
they have never used Claude Code. *Resume* needs the `claude` command on the
PATH; everything else works without it.

To compile it yourself, see [Build from source](#build-from-source).

## Commands

| Command | Description |
|---|---|
| *(none)* | native full-screen app (TUI) |
| `--pixel` | same, pixel graphics (toggle in-app with `p`) |
| `--web` | browser dashboard at `http://127.0.0.1:8787` |
| `ls` | print a table of sessions and exit |
| `json` | print sessions as JSON (scriptable) and exit |
| `find <text>` | search prompts, files and tools across all sessions |
| `cost` | estimated spend 24h / 7d / 30d, budget and top projects. Add `--explain` to print the whole chain — tokens per model → price → Wh → litres — so the estimate can be checked instead of trusted |
| `limits` | plan (e.g. Max 20x) and limit-window reset |
| `wrapped` | a shareable card of your year on the Desktop, **PNG and SVG** — tokens, cost, energy, water, and how many times you had to correct the agent. Anonymous by default: numbers only |
| `mcp` | MCP server (stdio) giving Claude recall over your past sessions |
| `watch` | live monitor: notifies state changes (`Ctrl+C` to quit) |
| `clean` | disk usage and empty sessions (never deletes without confirmation) |
| `export-all` | pack sessions into a single portable `.phx` file |
| `import <file.phx>` | add a bundle's sessions to this PC |
| `remote add/rm/list <alias>` | register your other PCs for the fleet view (ssh alias only) |
| `fleet` | query your other PCs over ssh and summarize their sessions |
| `resume-here <id>` | resume a session in the CURRENT terminal (used by fleet over ssh) |
| `sync set <dir>` | configure a local git repo (private) for bundle sync |
| `sync push` / `pull` | send this PC's bundle / import other PCs' bundles via git |
| `retention [<days>]` | show — and raise — how long Claude Code keeps its own transcripts (30 days by default) |
| `icon [file]` | regenerate the app icon (maintenance: it is already embedded in the binary) |
| `vault [on\|off]` | hard-link vault that keeps transcripts alive after deletion (0 extra bytes); no args = status; `vault restore <id>` puts one back |

## Options

| Option | Effect |
|---|---|
| `--port <n>` | web dashboard port (default 8787) |
| `--watch <sec>` | live re-scan interval (default 5) |
| `--no-open` | with `--web`: do not open the browser |
| `--explain` | with `cost`: write out the arithmetic behind every figure |
| `--running` | with `ls` / `json` / `export-all`: live sessions only |
| `--project <txt>` | with `ls` / `json` / `export-all`: filter by project name |
| `--out <file>` | with `export-all`: path of the `.phx` file to create |
| `--dir <path>` | use a different `.claude` directory |
| `--delete-empty` | with `clean`: delete empty sessions (always asks for confirmation) |
| `--delete-project <name>` | permanently delete ALL transcripts of a project — double confirmation (re-type the name), refuses live/ambiguous, offers a `.phx` backup first |
| `-h`, `--help` | show help in the terminal |

## Shortcuts (TUI)

Press `?` or `F1` in-app for the full help. You never have to learn any of
this: every one of these actions is also a thing you can click — see
[Mouse only](#mouse-only).

| Key | Action | | Key | Action |
|---|---|---|---|---|
| `↑` `↓` / wheel | move the selection | | `r` | resume (`claude --resume`) |
| `Enter` / click | open the detail view | | `v` | read the transcript in-app |
| `Tab` / `1` `2` `3` | switch view | | `e` | export CSV + JSON |
| `/` | full-text search | | `x` | export a `.phx` bundle |
| `f` | status filter | | `i` | import a `.phx` bundle |
| `o` / `s` | sort column / direction | | `t` / `T` | theme forward / back |
| `a` | sub-agents and workflows | | `p` | pixel graphics on/off |
| `R` | immediate rescan | | `m` | chart metric |
| `L` | language: italiano ⇄ English | | 🖰 | mouse-only mode |
| `F` | fleet: merge sessions from your other PCs (ssh) | | `V` | restore a `⛁` session from the vault |
| `W` | Wrapped card (PNG + SVG) on the Desktop | | | |
| `q` / `Ctrl+C` | quit | | | |

Anything that leaves a mark says what it will do and waits for a yes (`s` to
confirm, `Esc` to cancel): *resume*, *export*, *import*, the Wrapped card and
the `.md` summary (they write to your Desktop), restoring from the vault (it
writes back into the agent's own store), deleting, and *fleet* — which lists
the PCs it is about to reach over ssh before contacting any of them.

The tips that scroll in the box at the top right — the help you get without
asking for it — are written as verse. The keys and commands inside them stay
literal (`v`, `/`, `phosphor cost`): a line that hid the key behind a metaphor
would be decoration, not help. A test keeps them within the box and checks that
every key they promise exists.

### Language

Press **`L`**, or click the `L` chip in the bottom bar: Italian ⇄ English,
instantly, anywhere in the app. The choice is remembered in `phosphor.json`
(`lang`), and the terminal side follows it too — `--help`, `ls`, `cost`,
`clean`, `limits`, `retention`, `vault`.

The switch sits on a single key and a visible chip on purpose: whoever needs it
is precisely the person who cannot read what is on screen, and sending them to
find a setting in a config file written in the wrong language would be a joke.

The scrolling tips exist in both: Italian in the *dolce stil novo*, English in
its contemporary — the verse of Shakespeare. Not a line-by-line translation,
which would have produced crooked prose in two languages instead of verse in
one.

### Mouse only

Nothing needs the keyboard. **Click the words, not the letters**: every row of
the help is a command, so `read the transcript in-app` opens the transcript —
you never have to know it was `v`. Click a row to open its detail, a column
header to sort, a tab to switch view; the metric buttons, the bottom bar and
the theme swatches all respond. **Clicking outside a panel closes it**, which
is the one gesture worth remembering, and the only way out you need.

The chip at the bottom right toggles the two modes:

| Mode | The bottom bar reads | For |
|---|---|---|
| `mouse+tasti` | `[⏎ apri] [v leggi]` | knowing the shortcut while you click |
| `solo mouse` | `[apri] [leggi]` | when the letters are just noise |

The keys keep working in both — the mode only decides whether the bar shows
them. The choice is remembered in `phosphor.json` (`mouseOnly`).

The bar wraps to a second row rather than dropping the commands that do not
fit: at 120 columns it used to swallow the whole last group, and a command that
is simply absent is not something anyone notices is missing.

**First run.** Three screens introduce the list, the click-anywhere model and
what Phosphor will and will not do on its own. `AVANTI` / `SALTA`, both
clickable; it does not come back (`tourDone` in `phosphor.json`). If the
retention question is due, that comes first — it is about losing work.

## What it shows

- **Sessions**: live status (running / idle / ended), project, title, message
  count, tokens, estimated cost, sub-agents and workflows, transcript size
  (auto-scaled B/KB/MB/GB), date of the last message, and model. With search,
  filters, sorting and a detail card. The table title also shows the total size
  of the sessions in view.
- **Projects**: aggregates per project (sessions, tokens, cost) with a bar
  chart.
- **Trends**: a chart of cost, tokens and sessions per day.
- **Resume**: opens a new terminal running `claude --resume <id>` in the
  correct working directory (with cross-PC `pathRemaps` support).
- **Read** (`v`): the actual conversation of a session, rendered in-app
  (user prompts, assistant replies, compact tool markers), scrollable.
- **Continuations**: sessions started by `/compact` or resume are detected and
  marked with `↳` (and linked to their parent), so resume chains no longer look
  like confusing duplicates.
- **Both agents** (`◆`): Codex CLI threads are listed next to the Claude Code
  ones — see [Two agents in one list](#two-agents-in-one-list).
- **Vault** (`⛁`): transcripts that outlived deletion because `vault on` had
  hard-linked them — full sessions, restorable with `V`. See
  [The vault](#the-vault--keep-transcripts-for-zero-extra-bytes).
- **Recovered sessions** (`⚱`): Claude Code deletes its own transcripts after
  `cleanupPeriodDays` (30 days by default), so old projects silently disappear
  from the list. Phosphor rebuilds those sittings from `~/.claude/history.jsonl`
  (never pruned) and the surviving `memory/` sidecars: real title, real prompts,
  real dates. The assistant's replies, tokens and cost are gone for good and
  stay at zero, and file-backed actions (resume, `.phx` export, delete) refuse
  to run on them. See "Keeping your history" below.
- **Global content search** (`g`): grep the real conversations across every
  session and jump straight to the matching message in the reader.
- **Organize**: pin favorites (`*`, shown with `★`, filterable), attach a note
  per session (`n`, shown with `📝`), and export a session to Markdown (`M`).

### Keeping your history

Claude Code prunes `~/.claude/projects/<project>/<id>.jsonl` on startup: anything
older than `cleanupPeriodDays` is unlinked (no recycle bin, no backup). The
default is 30 days, so a project you have not touched in a month vanishes from
Phosphor because there is nothing left on disk to scan. Only the top-level
transcripts go — the `subagents/` and `memory/` subfolders stay behind, which is
why an "empty" project folder can still hold gigabytes.

Phosphor asks you about this **on first launch**, because the setting is
invisible until it has already cost you something: one key picks ten years, one
picks a year, one dismisses it for good. It changes that single number and keeps
a copy of the file as `settings.json.phosphor-bak` — every other setting of
yours is left byte for byte as it was.

From the command line:

```
phosphor retention          # what is in force, and what it means
phosphor retention 3650     # keep ten years of history
```

Or by hand in `~/.claude/settings.json`:

```json
{ "cleanupPeriodDays": 3650 }
```

**Codex has no equivalent.** Its rollouts are filed by date and never pruned on
a timer — checked against a real store whose oldest thread was six months old
and untouched. Nothing to set there; what protects it is the vault below.

Do that BEFORE you rely on Phosphor for history. What is already deleted cannot
be restored; the `⚱` recovery above is a skeleton rebuilt from the prompt
history, not the conversation.

### The vault (`⛁`): keep transcripts for zero extra bytes

Raising `cleanupPeriodDays` stops Claude Code. It does not stop a disk cleaner,
a sync tool mirroring a deletion, or Codex pruning its own store. The vault does:

```
phosphor vault on
```

From then on every scan **hard-links** each transcript into
`~/.claude/phosphor-vault/`. A hard link is a second name for the same bytes on
the same volume, so while the original exists the vault costs nothing — on a
real store, 893 MB of conversations came to 0.02 MB of directory entries. When
something deletes the original, that link simply becomes the file's only name:
the data never moves and never doubles. **The vault grows by exactly what you
would otherwise have lost, and by nothing else.**

A transcript that survives this way keeps its `⛁` row in the list, complete —
tokens, tools, the whole conversation, not the `⚱` skeleton. Press `V` (or
`phosphor vault restore <id>`) to link it back into its agent's store, and
`claude --resume` / `codex resume` find it again.

`phosphor vault` reports what is held and, separately, how much of it is
*orphans* — the transcripts whose original is gone. Only those bytes are storage
you are actually paying for.

It is **off by default**: Phosphor is read-only until you say otherwise, and
this is the one feature that creates files. Nothing is linked, read or created
before you turn it on. Hard links cannot cross volumes, so if `CODEX_HOME` sits
on another drive those transcripts are reported as skipped rather than silently
copied — copying is exactly the cost the design exists to avoid.

## Two agents in one list

Phosphor reads the **Codex CLI**'s own store as well
(`$CODEX_HOME`, else `~/.codex`) and turns each of its rollouts into the same
kind of row, so one list, one search and one cost total cover both agents. If
Codex is not installed nothing changes and nothing is looked up.

Codex rows are marked two ways, because colour alone is not a signal everyone
can read: the project name is drawn in a second colour, and the title carries a
`◆`. Filter with `agent:codex` / `agent:claude` (the detail card names the
agent and its CLI version).

What carries over, and what does not:

| | Claude Code | Codex |
|---|---|---|
| Title | `aiTitle`, else first prompt | the thread name Codex itself shows in its picker |
| Tokens / cost | Anthropic list prices | OpenAI list prices (`"gpt"` in `phosphor.json`) |
| Sub-agents | `subagents/` sidecar transcripts | rollouts with a `parent_thread_id` (a `guardian_review` pass, a spawned agent) |
| Live | per-pid session files | a writer lock plus a running `codex` process |
| Resume (`r`) | `claude --resume` | `codex resume` (`codex fork` after a path remap) |
| Read (`v`), search (`g`), Markdown export (`M`) | yes | yes |
| `.phx` bundle, delete/archive project | yes | no — a `.phx` reproduces Claude Code's `projects/<encoded-cwd>` layout, which a Codex thread has no place in; use `codex delete <id>` / `codex archive <id>` |

OpenAI reports `input_tokens` inclusive of the cached prefix and Anthropic
reports it exclusive, so Phosphor subtracts the cached part from the Codex side.
Both agents then cost through the same formula and the totals are comparable.

## Porting between PCs

Move sessions to another computer, carrying the subfolders too (the main
transcript plus the sub-agent and workflow transcripts) in a single portable
`.phx` file.

```
# on the source PC
phosphor export-all                      # -> Desktop\phosphor-sessioni-YYYYMMDD-hhmmss.phx
phosphor export-all --project name       # a single project only
phosphor export-all --out D:\backup.phx  # custom path

# copy the .phx to the other PC, then:
phosphor import D:\backup.phx            # shows what it will add and asks to confirm

# same project lives at a different path on the new PC? remap it on import:
phosphor import D:\backup.phx --remap "C:\Users\me\proj=D:\work\proj"
```

In the TUI: `x` creates a bundle of the sessions in view; `i` opens an in-app
ASCII file picker to browse folders and choose the `.phx` to import.

### Resuming on a different PC / path (and any Claude account)

A transcript is a plain local file — it is **not tied to a Claude account**, so a
session exported on one PC resumes on another regardless of which account is
logged in (the account only decides who pays). The *only* thing that must line up
is the **path**: `claude --resume` looks for the session in the folder encoding
the *current* working directory.

- **Same path** (or same PC): import and resume — it just works.
- **Different path**: pass `--remap "<original>=<local>"` on import. Phosphor
  places the transcript under the folder Claude will look in for the local path,
  so `claude --resume <id>` works natively from `<local>` — no Phosphor needed to
  resume. The remap is also saved to `phosphor.json` (`pathRemaps`) so the in-app
  `r` resume resolves it too (forking with `--fork-session`). `import` lists each
  bundled project's original path and shows exactly where it will land.

You can also pre-seed the mapping once in `phosphor.json` (used by both import and
resume):

```json
"pathRemaps": { "C:\\old\\path\\to\\project": "D:\\new\\local\\path" }
```

**Encryption is optional** — needed only if the `.phx` travels over an untrusted
channel (shared cloud, e-mail, a repo others can read). The bundle carries no
credentials, so a hand-carried USB copy needs none. See `sync` below for the
built-in age option.

### Fleet: your other PCs, live, over ssh

Moving a bundle is the "source PC is off" model. When your other PC is **on**,
you can skip the copy entirely and go to the session instead:

```
phosphor remote add pc-casa      # alias from ~/.ssh/config (or user@host)
phosphor fleet                   # summary per host
```

In the TUI press `F`: each configured host is queried over ssh
(`phosphor json` runs there), and its sessions join the list tagged `[alias]`
— filter with `host:pc-casa` (or `host:qui` for local ones). Selecting a
remote session and pressing `r` opens a terminal running
`ssh -t <alias> phosphor resume-here <id>`: **claude resumes on that PC, in
the right directory**, drawing on your ssh terminal — no copies, no path
remap, no divergence.

Security model: Phosphor stores only the **alias** — host, user and keys live
in `~/.ssh/config` and ssh-agent, the channel is encrypted by SSH itself, and
everything a remote returns is treated as untrusted (ids re-validated,
control characters stripped, sizes capped, fetch under BatchMode with a hard
timeout). Requirements on each remote: sshd reachable, `phosphor` and
`claude` installed and logged in (any account — that machine's account pays).
Local file actions (read, export, delete, archive) stay disabled on remote
rows; resume, detail, notes and favorites work.

### Optional: sync bundles through your own git repo

Phosphor can move bundles between PCs through a private git repository you
control. It only shells out to `git` (no network code, no credentials handled by
Phosphor); the repo and its authentication are yours.

```
git clone <your-private-repo>  C:\path\phosphor-sync   # once, per PC
phosphor sync set C:\path\phosphor-sync                # remember it
phosphor sync push                                     # pack + commit + push (this PC)
phosphor sync pull                                     # pull + import other PCs' bundles
phosphor sync status                                   # repo, git status, bundles
```

Each PC writes its own `phosphor-<host>.phx`, so machines never collide. This is
the only feature that uploads transcripts, and it is never automatic: it runs
only when you invoke `sync push`. Keep the repo **private** and mind GitHub's
100 MiB per-file limit (split with `--project` if needed).

**Optional encryption (recommended).** Because the repo is a third party, you
can encrypt the bundles with [age](https://age-encryption.org) using public-key
recipients, so no secret is ever stored by Phosphor:

```json
"syncEncrypt":  "age1...your-recipient...",   // encrypt on push (public key)
"syncIdentity": "C:\\path\\to\\age-key.txt"   // decrypt on pull (your private key)
```

With `syncEncrypt` set, `sync push` writes `phosphor-<host>.phx.age` and `sync
pull` decrypts before importing (age may prompt for your key passphrase on the
terminal; the private key never passes through Phosphor). If encryption is
configured but `age` is not installed, `sync push` aborts rather than upload
plaintext.

> Import is data-safe: it rejects anomalous paths, **never overwrites** existing
> files (it skips them), and always asks for confirmation. It can only add the
> missing transcripts. The bundle remembers the original working directory: if
> that path does not exist on the new PC, the sessions are still viewable in
> Phosphor (only *Resume* needs a valid directory).

## Recall over past sessions (MCP)

`phosphor mcp` runs a small [Model Context Protocol](https://modelcontextprotocol.io)
server over stdio, so **Claude itself can recall your past Claude Code sessions** —
search them, read a transcript, grep their content — to answer things like *"what
did we decide about X last week?"*. It is 100% offline (JSON-RPC on stdin/stdout,
no network) and read-only.

Register it once with Claude Code:

```
claude mcp add phosphor -- phosphor mcp
```

(use the full path to `phosphor.exe` if it isn't on your `PATH`, then reopen
Claude Code). Both agents are covered: Claude Code sessions, Codex sessions, and
the ones Phosphor recovered after Claude Code had deleted them.

### What to actually ask it

You talk to Claude normally — it picks the tool. These are real questions with
what happens behind them:

> **"What did I decide about the cache invalidation, a few weeks ago?"**
> Claude greps the *text* of every conversation you have had, finds the passages,
> and reads that session back. You do not need to remember which project it was.

> **"Find my sessions that touched scan.rs in September."**
> `file:scan.rs after:2026-09-01` — the filter matches the files the session
> actually touched, not what you happened to type.

> **"Summarise what I did on the frontend project last week."**
> `project:frontend after:2026-09-08`, then Claude reads the sessions it finds
> and writes the summary. Useful on a Monday, or for a standup.

> **"Have I ever used ssh in a session, and how did I set it up?"**
> Content search across every transcript, with the surrounding lines.

> **"Which of my Opus sessions were the longest?"**
> `model:opus` — every result carries the project, the date, the message count
> and the token total, so Claude can rank them for you.

> **"Read back the session where I set up the git sync and tell me the commands."**
> Claude searches, opens the transcript, and pulls the commands out of it —
> instead of you scrolling a terminal you closed two weeks ago.

### The three tools, precisely

| Tool | What it does | Arguments |
|---|---|---|
| `search_sessions` | finds sessions by query and filters; returns title, project, date, messages, tokens and the id | `query`, `limit` (default 20, max 200) |
| `read_session` | reads one full conversation back, turn by turn | `id` (from a search), `max_chars` (default 20 000) |
| `search_content` | greps the TEXT of every transcript, up to 2 passages per session, with the id to read further | `text`, `limit` (default 20, max 100) |

Filters `search_sessions` understands — all of them narrow, several combine:

| Filter | Matches |
|---|---|
| `project:` | the project's name |
| `file:` | a file the session touched |
| `tool:` | a tool it used (`tool:Edit`, `tool:Bash`) |
| `model:` | a model it ran on (`model:opus`) |
| `agent:` | `agent:codex` or `agent:claude` |
| `after:` `before:` | a date, `YYYY-MM-DD` |
| `host:` | another of your PCs, or `host:qui` for this one |
| anything else | free text over prompts, files, tools, titles and project names |

So `project:api tool:Bash after:2026-09-01 deploy` means all four at once: the
api project, sessions that ran Bash, since 1 September, mentioning deploy.

**Which of the two searches to use** — the difference matters. The free text in
`search_sessions` looks at **what you wrote**: your prompts, plus titles, file
names, tool names and projects. `search_content` reads the **whole
conversation**, Claude's answers included. So *"a session where I asked about
retention"* is the first one; *"wherever the word `cleanupPeriodDays` appears,
even if Claude was the one who said it"* is the second.

**What it will not do.** It reads. It cannot resume, edit or delete a session,
and it never leaves your machine — it speaks to Claude over stdin/stdout, with
no network involved.

## Plan limits

`limits` (and the status line at the top of the TUI) shows your plan (e.g.
*Max 20x*) and when the limit window resets, read from `~/.claude.json`.

> The official 5h and weekly window percentages are not stored locally (Claude
> receives them at runtime), so they are not shown: Phosphor reports only what is
> actually available on disk, with no misleading estimates.

## Security

Phosphor is meant to be shared. Before distribution a full review was done and
these defenses applied (verified with tests, a clean build, and a self-test):

- **Loopback-only server with Host check.** The web dashboard listens only on
  `127.0.0.1` and rejects (403) requests with a non-local `Host` header,
  blocking DNS rebinding.
- **POST same-origin actions only.** The acting endpoints (`/api/resume`,
  `/api/refresh`) accept only `POST` with a same-origin `Origin`: no other site
  can trigger them (anti-CSRF).
- **Escaped HTML output + a restrictive Content-Security-Policy** against XSS.
- **CSV formula neutralization.** Values beginning with `= + - @` (or a tab) are
  prefixed so Excel/Sheets do not execute them.
- **Command-injection-proof "Resume".** The session id is validated, the working
  directory is checked and must exist, and it is passed as a separate argument,
  never interpolated into a shell string.
- **Path-traversal-proof import.** Paths with `..`, absolute paths, or drive
  letters are rejected; files are only written inside `projects/` and never
  overwritten.
- **Bounded parser and server** (JSON parser recursion cap, connection cap,
  header reads with size and timeout limits) to resist malformed or malicious
  input.
- **Atomic writes** (temp + rename) for config and cache; exports use
  *create-new* with timestamped names, so they never overwrite anything.
- **Confirmation for every side effect.** Export, resume, import and cleanup
  explain what they will do and ask for confirmation. Deletions are opt-in and
  guarded: `clean --delete-empty` removes only empty `.jsonl` files after a typed
  confirmation, and `--delete-project` (the whole-project delete) requires
  re-typing the exact project name, refuses projects with a live session, offers
  a `.phx` backup first, and is path-confined to a direct child of `projects/`.

## Privacy and files written

Session transcripts are never modified or sent over the network. Phosphor only
writes these files (inside `.claude` and, for exports, on the Desktop):

| File | Contents |
|---|---|
| `phosphor.json` | the configuration (theme, prices, budget…), atomic write |
| `.phosphor-cache.<key>.jsonl` | scan cache, regenerable (rewritten on each scan). The key is a fingerprint of the modules that produce a cached row, so a build that parses differently reads a different file instead of trusting stale results; caches of other builds are cleared on the next scan |
| `phosphor-vault/` | hard links to your transcripts, only if you ran `vault on` (no extra bytes; see [The vault](#the-vault--keep-transcripts-for-zero-extra-bytes)) |
| `Desktop\phosphor-export-*` | CSV/JSON of the current view (timestamped, never overwrites) |
| `Desktop\phosphor-wrapped-*` | the Wrapped card, PNG + SVG (timestamped, never overwrites) |
| `Desktop\phosphor-sessioni-*.phx` | portable `export-all` bundle (timestamped, never overwrites) |

Phosphor also reads `~/.claude.json` read-only for plan and limit reset; it does
not touch the credentials file.

The Codex store (`$CODEX_HOME`, else `~/.codex`) is read-only too: the rollouts
under `sessions/`, the thread names in `session_index.jsonl`, and the lock file
names under `thread-writer-locks/`. Phosphor writes nothing there — everything
it keeps lives in `~/.claude` as above. `auth.json` is never opened.

## Configuration

`~/.claude/phosphor.json` (created on first run): per-model prices (editable
without recompiling — `opus` / `sonnet` / `haiku` for Claude Code, `gpt` for
Codex, `default` for anything else), default `theme` and `pixel`, `mouseOnly` for the mouse-only bottom bar,
`watch` interval, monthly
`budget` for alerts, `pathRemaps` for cross-PC resume, `syncRepo` /
`syncEncrypt` / `syncIdentity` for the optional git sync, your `favorites`
and `notes`, `vault` for the hard-link vault (off by default),
`tourDone` for the first-run introduction, `lang` (`"it"` or `"en"`),
and `energyWhPerOutputToken` / `waterLPerKwh` for the footprint estimate shown
by `cost`, in the detail view and on the Wrapped card.

`pricesAsOf` is the day the price list was last checked (ISO `YYYY-MM-DD`).
After six months Phosphor says so, next to the cost and in the status line,
because a total computed on last year's prices looks exactly like a correct
one — and whoever opens Phosphor to find out what they spent is precisely the
person who does not know the list has moved. Update the prices and the date
together.

Seven retro palettes cycled with `t` / `T`: phosphor (green), amber, ice,
synthwave, matrix, red, blue. The chosen theme is saved.

### How the cost and footprint are worked out

Both are computed **per model and per token kind**, not per session, because a
single session routinely spans two to four models — charging all of it to
whichever model answered first was the largest error these numbers used to
carry.

**Cost** follows the published list prices, with one distinction most tools
miss: a cache write is billed at 1.25x the base input rate for the 5-minute TTL
but **2x** for the 1-hour one, and in real use almost all of it is the 1-hour
kind. Both rates are in `phosphor.json` (`cacheWrite`, `cacheWrite1h`).

**Energy** is anchored to generation. The only published figure anyone has —
roughly 0.24 Wh for a median Gemini prompt — measures producing a reply, so
`energyWhPerOutputToken` is exactly that, and everything else is scaled from it:
prompt tokens and cache writes cost a fraction of a generated token (prefill is
one batched pass; decoding runs the model once per token), cache reads cost
less again but not nothing, and the whole lot scales with the size of the model
that ran. A Haiku session and an Opus session are no longer the same session.

**Water** is derived from that energy through `waterLPerKwh` (WUE, how data
centres actually report it) instead of being counted off tokens a second time,
so the two figures cannot drift apart.

It is still an order-of-magnitude estimate — no vendor publishes per-token
energy — and the card says so. What it is now is *relatively* honest: the
comparison between two sessions, two models or two months means something.

Upgrading from an older version: `energyWhPerToken` and `waterMlPerToken` are
gone. They measured different quantities, so any value you had set is ignored
rather than silently reinterpreted; the new keys start at their defaults.

## Build from source

Requires a Rust toolchain (stable).

```
cargo build --release                                          # -> phosphor.exe (TUI)
cargo build --release --features adventure --bin phosphor-adv  # -> phosphor-adv.exe (GUI)
```

The project is a shared library (`src/lib.rs`) with two binaries: the TUI
(`src/main.rs`) and the graphical version (`src/bin/adventure.rs`). No network,
TLS, or cryptography dependencies.

### Clean / release builds

`build-release.ps1` builds both binaries with the user's home path remapped, so
the distributed `.exe` carries no personal path (no session data is ever
embedded — the exe is compiled code).

`release.ps1` publishes a GitHub release **always aligned to `Cargo.toml`**: it
reads the version from `Cargo.toml`, builds clean, and refuses to publish unless
the compiled binary reports exactly that version and the tag `vX.Y.Z` doesn't
already exist. To cut a release: bump `version` in `Cargo.toml`, commit, then run
`pwsh -File release.ps1` (or `-DryRun` for checks only). This is what keeps the
release/tag number from drifting away from the package version.

## Graphical version

`phosphor-adv.exe` is a separate front-end in a real graphical window (Rust +
macroquad), a point-and-click adventure style: a retro scene with a scanner-bot,
sessions as a clickable inventory, and verbs (Look, Resume, Export, Refresh,
Quit). Same scanning backend as the TUI.

---

The interface and in-app help are in Italian.
