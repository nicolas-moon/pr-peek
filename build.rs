//! Emits build metadata (target, toolchain, git state) for `--version`.

fn git(args: &[&str]) -> Option<std::process::Output> {
    let out = std::process::Command::new("git").args(args).output().ok()?;
    out.status.success().then_some(out)
}

fn main() {
    // The target triple cargo builds for.
    let triple = std::env::var("TARGET").unwrap_or_default();
    println!("cargo:rustc-env=BUILD_TARGET={triple}");

    // The toolchain building this binary; cargo sets RUSTC to its rustc.
    // "rustc 1.95.0 (hash date)" -> "1.95.0".
    let rustc_version = std::env::var_os("RUSTC")
        .as_deref()
        .and_then(|path| {
            std::process::Command::new(path)
                .arg("--version")
                .output()
                .ok()
        })
        .filter(|out| out.status.success())
        .and_then(|out| {
            let line = String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()?
                .to_string();
            Some(line.split(' ').nth(1).unwrap_or("unknown").to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=BUILD_RUSTC={rustc_version}");

    // The commit this tree is at, marked when it has uncommitted changes;
    // "unknown" when there is no repository (e.g. a crates.io build).
    let commit = git(&["rev-parse", "--short", "HEAD"])
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|commit| !commit.is_empty())
        .unwrap_or_else(|| "unknown".into());
    let dirty = git(&["status", "--porcelain"]).is_some_and(|out| !out.stdout.is_empty());
    let commit = if dirty {
        format!("{commit}, modified")
    } else {
        commit
    };
    println!("cargo:rustc-env=BUILD_COMMIT={commit}");

    // Re-run when the sources change so the metadata tracks what the binary
    // actually contains.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/main.rs");
}
