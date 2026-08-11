use std::env;
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn main() {
    let package_version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION is set by Cargo");

    // Re-run when the checked-out commit or working tree changes.
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
    if let Some(tags_path) = git(&["rev-parse", "--git-path", "refs/tags"])
        && !tags_path.is_empty()
    {
        println!("cargo:rerun-if-changed={tags_path}");
    }
    if let Some(packed_refs_path) = git(&["rev-parse", "--git-path", "packed-refs"])
        && !packed_refs_path.is_empty()
    {
        println!("cargo:rerun-if-changed={packed_refs_path}");
    }
    if let Some(git_ref) = git(&["symbolic-ref", "-q", "HEAD"])
        && let Some(ref_path) = git(&["rev-parse", "--git-path", &git_ref])
    {
        println!("cargo:rerun-if-changed={ref_path}");
    }

    let display_version = match git(&["describe", "--tags", "--exact-match", "HEAD"]) {
        Some(_) => package_version,
        None => match git(&["rev-parse", "--short=7", "HEAD"]) {
            Some(commit) => {
                let dirty = git(&["status", "--porcelain", "--untracked-files=normal"])
                    .is_some_and(|status| !status.is_empty());
                if dirty {
                    format!("{package_version} ({commit}, dirty)")
                } else {
                    format!("{package_version} ({commit})")
                }
            }
            None => format!("{package_version} (unknown commit)"),
        },
    };

    println!("cargo:rustc-env=TORRENTUTILSR_VERSION={display_version}");
}
