//! Stamps the commit this build came from into the binary.
//!
//! `docs/RISKS.md` R13, second half: the `sim` version says *something* changed
//! and only the commit says *what*, which is the difference between a
//! verification failure somebody can bisect and one they can only file. The
//! manifest carries it, so it has to be a fact about the build rather than a
//! fact about the machine the binary is later run on — which is why it is
//! resolved here, at compile time, and not by shelling out to `git` at run time.
//!
//! Three things this deliberately does:
//!
//! - **It admits ignorance.** A tarball, a vendored build, a container with no
//!   `.git` — every one of those produces no variable at all, and
//!   `SimCommit::of_this_build` reads that as `Unknown`. A manifest that lies
//!   about its provenance is worse than one that says it does not know.
//! - **It reports a dirty tree as dirty.** A commit hash from a working tree
//!   that does not match it is a claim about source nobody has, and R13 gives it
//!   its own variant for that reason.
//! - **It adds no dependency.** Two `git` invocations through `std::process` is
//!   the whole of it. `docs/ENGINEERING.md`'s bar for a dependency is a reason a
//!   few lines of code would not satisfy, and this is the few lines.

use std::path::Path;
use std::process::Command;

fn main() {
    // Rerun when HEAD moves or the index changes, which is what makes the stamp
    // follow a checkout rather than the first build of the day. Emitted only for
    // paths that exist, because naming a missing file makes Cargo rebuild this
    // crate unconditionally.
    for path in ["../.git/HEAD", "../.git/index"] {
        if Path::new(path).exists() {
            println!("cargo::rerun-if-changed={path}");
        }
    }
    println!("cargo::rerun-if-env-changed=MOBA_SIM_COMMIT");

    // An explicit value wins, so a release pipeline that knows the commit
    // without a working tree can say so.
    if let Ok(given) = std::env::var("MOBA_SIM_COMMIT") {
        println!("cargo::rustc-env=MOBA_SIM_COMMIT={given}");
        return;
    }

    let Some(head) = run(&["rev-parse", "HEAD"]) else {
        return;
    };
    if head.len() != 40 || !head.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return;
    }
    // `--porcelain` is empty exactly when the tree matches the commit. A `git`
    // that fails here is treated as a dirty tree rather than a clean one: the
    // smaller claim is the safe one.
    let dirty = run(&["status", "--porcelain"]).is_none_or(|status| !status.is_empty());
    let suffix = if dirty { "-dirty" } else { "" };
    println!("cargo::rustc-env=MOBA_SIM_COMMIT={head}{suffix}");
}

/// The trimmed standard output of a `git` invocation, or nothing.
fn run(arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").args(arguments).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8(output.stdout).ok()?.trim().to_owned())
}
