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
}
