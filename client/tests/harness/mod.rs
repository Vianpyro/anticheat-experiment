//! The bits two exit-criterion tests both need.
//!
//! Shared by `#[path]` rather than by a crate, exactly as `sim/tests/spec` is:
//! a helper crate for one function is a crate to maintain, and a copy in each
//! test is a copy to keep in step. What is here is the awkward part of running
//! the verifier as a separate process, and it is awkward for reasons that are
//! worth writing down once.

use std::path::PathBuf;

/// The `replay` binary, built if it is not current.
///
/// Found beside the test binary rather than through `CARGO_BIN_EXE_`, which
/// Cargo only sets for a package's *own* binaries and `replay` belongs to the
/// `replay` crate.
///
/// It is built here because the obvious assumption is wrong, and CI is where
/// that showed: `cargo test --workspace` does **not** build every binary in the
/// workspace. It builds what each package's test targets need, and no test
/// target needs this one — locally it existed only because a
/// `cargo build --all-targets` had happened to run first, which is the shape of
/// green that turns red on a clean checkout. Building it here makes the test
/// self-contained on any machine, and the nested invocation is safe because the
/// outer cargo is not holding the build lock while its tests run.
///
/// **Built every time, not only when missing.** The "if it exists, use it"
/// version had a worse failure than the one it fixed: a binary left over from a
/// previous build resimulates under the rules *it* was compiled with, so a
/// change to `sim` makes the criterion compare a new server against an old
/// verifier — which fails, correctly, for a reason that has nothing to do with
/// the change. Cargo is a no-op when the binary is current, and the seconds it
/// costs otherwise are seconds the alternative spends misleading somebody.
#[must_use]
pub fn replay_binary() -> PathBuf {
    let mut path = std::env::current_exe().expect("the test binary has a path");
    path.pop(); // deps/
    path.pop(); // debug/ or release/
    path.push(if cfg!(windows) {
        "replay.exe"
    } else {
        "replay"
    });

    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the client crate has a parent directory")
        .join("Cargo.toml");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let built = std::process::Command::new(cargo)
        .args(["build", "--locked", "-p", "replay", "--bin", "replay"])
        .arg("--manifest-path")
        .arg(&workspace)
        .status()
        .expect("run cargo to build the replay tool");
    assert!(built.success(), "cargo could not build the replay tool");
    assert!(
        path.exists(),
        "the replay tool was built but is not at {}",
        path.display()
    );
    path
}

/// A digest as the tools print it.
#[must_use]
pub fn hex(digest: &sim::Digest) -> String {
    digest
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
