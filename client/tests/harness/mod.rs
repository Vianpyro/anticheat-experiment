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

/// What a match a client played actually contained.
///
/// `docs/RISKS.md` R15 is the reason this type exists rather than the three
/// counters being written twice. Both exit criteria in this directory are
/// conditional on a match having happened — three clients agreeing, a
/// prediction being exact — and both ran for a milestone or more over a match
/// in which the champions never left their own base, nothing crossed anybody's
/// fog and nothing was ever shot at. An assertion whose antecedent is never
/// reached looks exactly like an assertion that holds, and the only thing that
/// tells them apart is a count that has to be non-zero.
#[derive(Debug, Default)]
pub struct Reach {
    /// The fewest and the most entities this client ever believed were on the
    /// map. A single value is a client that never saw anything change.
    pub fewest: Option<usize>,
    /// The most entities ever believed on the map.
    pub most: usize,
    /// Views on which this client's own champion was below full health.
    pub hurt_views: u32,
    /// Views carrying at least one derived event.
    pub views_with_an_event: u32,
    /// Times a handle shown on one view was absent from the next: an entity
    /// leaving the fog, rather than merely never being in it.
    pub disappearances: u32,
    previously: std::collections::BTreeSet<u16>,
}

impl Reach {
    /// Folds in whatever the client currently holds.
    pub fn observe(&mut self, headless: &client::Headless) {
        let Some(view) = headless.view() else { return };
        let count = headless.world().len();
        self.fewest = Some(self.fewest.map_or(count, |least| least.min(count)));
        self.most = self.most.max(count);
        if !view.events.is_empty() {
            self.views_with_an_event = self.views_with_an_event.saturating_add(1);
        }
        if let sim::Liveness::Alive { hp } = view.own.liveness
            && hp < sim::RULES.champion_max_hp
        {
            self.hurt_views = self.hurt_views.saturating_add(1);
        }

        let now: std::collections::BTreeSet<u16> = view
            .visible
            .iter()
            .map(|entity| match entity {
                sim::view::EntityView::Champion { id, .. }
                | sim::view::EntityView::Tower { id, .. }
                | sim::view::EntityView::Projectile { id, .. } => id.0,
            })
            .collect();
        self.disappearances = self
            .disappearances
            .saturating_add(self.previously.difference(&now).count() as u32);
        self.previously = now;
    }

    /// One line, for the run summary.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "entities {}..{}, {} disappearances, {} views with an event, {} views \
             under fire",
            self.fewest.unwrap_or(0),
            self.most,
            self.disappearances,
            self.views_with_an_event,
            self.hurt_views
        )
    }

    /// The floors every match driven through the transport has to clear, or the
    /// criterion stated over it is a statement about an empty room.
    ///
    /// # Panics
    ///
    /// If the match this reach describes contained nothing.
    pub fn assert_a_match_happened(&self, who: sim::Seat) {
        assert!(
            self.most > self.fewest.unwrap_or(0),
            "{who:?} was told about {} entities on every view of the match, so \
             nothing ever crossed the fog and any agreement between clients is \
             agreement about a world that never changed (docs/RISKS.md R15)",
            self.most
        );
        assert!(
            self.disappearances > 0,
            "{who:?} never once lost sight of a handle it had been shown, so the \
             culling this criterion rests on was never exercised in the direction \
             that matters"
        );
        assert!(
            self.views_with_an_event > 0,
            "{who:?} was never sent a single derived event"
        );
    }
}
