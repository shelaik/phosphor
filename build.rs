//! Build script: stamp a PRECISE build identity into the binary so the running
//! exe can say exactly which source it was built from (and whether that tree had
//! uncommitted changes). Pure compile-time: it only shells out to `git` while
//! building — the produced binary makes no calls and reaches no network, in line
//! with Phosphor's offline/read-only ethos. If `git` is absent or this isn't a
//! repo, it degrades gracefully to "nogit" instead of failing the build.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn main() {
    // Short commit hash (9 hex chars) — the exact source revision.
    let hash = git(&["rev-parse", "--short=9", "HEAD"]).unwrap_or_else(|| "nogit".into());
    // Commit date (YYYY-MM-DD) — human-readable "which version" anchor.
    let date = git(&["log", "-1", "--date=short", "--format=%cd"]).unwrap_or_default();
    // "+" if the working tree had uncommitted changes when built (so the user
    // knows the binary may not match any committed revision exactly).
    let dirty = match git(&["status", "--porcelain"]) {
        Some(s) if !s.is_empty() => "+",
        _ => "",
    };

    println!("cargo:rustc-env=PHOSPHOR_GIT={hash}");
    println!("cargo:rustc-env=PHOSPHOR_GIT_DIRTY={dirty}");
    println!("cargo:rustc-env=PHOSPHOR_COMMIT_DATE={date}");

    // Re-run when HEAD moves or the index changes, so the stamp stays fresh.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/phosphor.ico");

    embed_icon();
}

/// Give the executable its icon, so Explorer, the taskbar and Alt+Tab show
/// PHOS instead of the generic box.
///
/// Done by hand rather than with a build-dependency crate: adding one would be
/// the project's first dependency outside `ratatui`/`crossterm`, for the sake
/// of shelling out to a tool the Windows SDK already ships. `rc.exe` compiles a
/// two-line `.rc` into a COFF resource and `link.exe` takes it as a plain
/// argument.
///
/// Best-effort on purpose: if the SDK is not installed the build still
/// succeeds, only without an icon. A missing icon is a blemish; a build that
/// refuses to run on a machine without the SDK is a wall.
#[cfg(windows)]
fn embed_icon() {
    use std::path::PathBuf;
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()));
    let ico = root.join("assets").join("phosphor.ico");
    if !ico.exists() {
        // Regenerate it with `phosphor icon` — see `phosphor::icon`.
        println!("cargo:warning=assets/phosphor.ico assente: build senza icona");
        return;
    }
    let rc = match find_rc() {
        Some(p) => p,
        None => {
            println!("cargo:warning=rc.exe (Windows SDK) non trovato: build senza icona");
            return;
        }
    };
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_else(|_| ".".into()));
    let rc_file = out.join("phosphor.rc");
    let res_file = out.join("phosphor.res");
    // Resource id 1: the shell shows the LOWEST-numbered icon as the file's
    // icon, so this must stay 1.
    let script = format!("1 ICON \"{}\"\n", ico.display().to_string().replace('\\', "\\\\"));
    if std::fs::write(&rc_file, script).is_err() {
        return;
    }
    let ok = Command::new(&rc)
        .args(["/nologo", "/fo"])
        .arg(&res_file)
        .arg(&rc_file)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok && res_file.exists() {
        println!("cargo:rustc-link-arg-bins={}", res_file.display());
    } else {
        println!("cargo:warning=rc.exe ha fallito: build senza icona");
    }
}

#[cfg(not(windows))]
fn embed_icon() {}

/// Locate `rc.exe`. It is not on PATH by default — it lives under the Windows
/// SDK, one directory per SDK version — so the newest one is picked.
#[cfg(windows)]
fn find_rc() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if Command::new("rc.exe").arg("/?").output().is_ok() {
        return Some(PathBuf::from("rc.exe"));
    }
    let arch = if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("aarch64") {
        "arm64"
    } else {
        "x64"
    };
    let mut best: Option<(String, PathBuf)> = None;
    for pf in ["ProgramFiles(x86)", "ProgramFiles"] {
        let base = match std::env::var(pf) {
            Ok(v) => PathBuf::from(v).join("Windows Kits").join("10").join("bin"),
            Err(_) => continue,
        };
        let rd = match std::fs::read_dir(&base) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let cand = e.path().join(arch).join("rc.exe");
            // Versions sort lexicographically here because they are all
            // zero-padded 10.0.xxxxx.0 — good enough to mean "the newest".
            if cand.exists() && best.as_ref().map_or(true, |(v, _)| name > *v) {
                best = Some((name, cand));
            }
        }
    }
    best.map(|(_, p)| p)
}
