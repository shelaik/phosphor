# Phosphor

A standalone scanner for your local coding-agent sessions — **Claude Code** and
the **Codex CLI**, side by side in one list. A single Rust binary (~1.5 MB, no
external runtime) that reads `~/.claude` and `~/.codex` and shows all your
sessions (where they ran, what they did, where they left off) in a native
full-screen terminal app, a browser dashboard, or a pixel-art graphical
version.

Read-only by default: it never modifies your transcripts and never uploads
anything on its own. The only feature that sends data out is the opt-in
`sync push` (to a private git repo you configure), and it runs only when you
invoke it. Any action that writes to disk is explained first and asks for
confirmation.

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

**Windows (ready to use).** Two ways to get the binary:

- **Download** the `.exe` from the [latest release](../../releases/latest)
  (`phosphor.exe`, plus `phosphor-adv.exe` for the graphical version), or
- grab the prebuilt binaries in the [`dist/`](dist) folder of this repo.

Then just double-click `phosphor.exe`. Optional one-step setup: download
`install.ps1` next to the `.exe` and run it (right-click > Run with PowerShell)
to copy it under `%LOCALAPPDATA%\Phosphor`, add it to your `PATH`, and create a
Desktop shortcut. No admin rights, no runtime needed.

```
phosphor                 native terminal app (default, no browser)
phosphor --pixel         same, with "pixel" graphics
phosphor --web           browser dashboard at http://127.0.0.1:8787
phosphor --dir <path>    use a different .claude directory
```

Everyone uses their own copy: the binary auto-detects the current user's
`%USERPROFILE%\.claude`. The *Resume* action requires the `claude` command to be
on your PATH. To compile from source, see [Build](#build-from-source).

## Commands

| Command | Description |
|---|---|
| *(none)* | native full-screen app (TUI) |
| `--pixel` | same, pixel graphics (toggle in-app with `p`) |
| `--web` | browser dashboard at `http://127.0.0.1:8787` |
| `ls` | print a table of sessions and exit |
| `json` | print sessions as JSON (scriptable) and exit |
| `find <text>` | search prompts, files and tools across all sessions |
| `cost` | estimated spend 24h / 7d / 30d, budget and top projects |
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
| `vault [on\|off]` | hard-link vault that keeps transcripts alive after deletion (0 extra bytes); no args = status; `vault restore <id>` puts one back |

## Options

| Option | Effect |
|---|---|
| `--port <n>` | web dashboard port (default 8787) |
| `--watch <sec>` | live re-scan interval (default 5) |
| `--no-open` | with `--web`: do not open the browser |
| `--running` | with `ls` / `json` / `export-all`: live sessions only |
| `--project <txt>` | with `ls` / `json` / `export-all`: filter by project name |
| `--out <file>` | with `export-all`: path of the `.phx` file to create |
| `--dir <path>` | use a different `.claude` directory |
| `--delete-empty` | with `clean`: delete empty sessions (always asks for confirmation) |
| `--delete-project <name>` | permanently delete ALL transcripts of a project — double confirmation (re-type the name), refuses live/ambiguous, offers a `.phx` backup first |
| `-h`, `--help` | show help in the terminal |

## Shortcuts (TUI)

Press `?` or `F1` in-app for the full help. Everything is clickable with the
mouse too.

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
| `F` | fleet: merge sessions from your other PCs (ssh) | | `V` | restore a `⛁` session from the vault |
| `W` | Wrapped card (PNG + SVG) on the Desktop | | | |
| `q` / `Ctrl+C` | quit | | | |

The *Resume*, *export* and *import* actions show what they will do first and ask
for confirmation (`s` to confirm, `Esc` to cancel).

**Mouse:** click a row to open its detail; click a column header to sort; click
the tabs to switch view; the metric buttons, the bottom bar chips and the theme
swatches in the help are all clickable too.

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

(use the full path to `phosphor.exe` if it isn't on your `PATH`). It exposes three
tools: `search_sessions` (same `project:`/`model:`/`file:`/`after:` filter syntax
as the in-app search), `read_session`, and `search_content`.

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
| `.phosphor-cache.v7.jsonl` | scan cache, regenerable (rewritten on each scan) |
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
Codex, `default` for anything else), default `theme` and `pixel`, `watch` interval, monthly
`budget` for alerts, `pathRemaps` for cross-PC resume, `syncRepo` /
`syncEncrypt` / `syncIdentity` for the optional git sync, your `favorites`
and `notes`, `vault` for the hard-link vault (off by default),
and `energyWhPerOutputToken` / `waterLPerKwh` for the footprint estimate shown
by `cost`, in the detail view and on the Wrapped card.

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
