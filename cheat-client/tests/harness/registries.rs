//! The two key registries the forgery exploit is judged between.
//!
//! Included only by `tests/forgery.rs`, and both functions are used by it. The
//! forgery's claim is the whole of `docs/RISKS.md` R4: a signature over a
//! self-consistent artefact reduces to key custody, so the only thing that stands
//! between a forged replay and acceptance is *which keys the verifier trusts*.
//! These are the two registries that make that concrete — the one that trusts the
//! attacker (the exploit works) and the one that does not (the defence holds).

use replay::keys::{KeyRegistry, KeyStatus, SigningKey};

/// A registry that accepts the honest server and nobody else.
///
/// The victim's registry, and the real defence. A forged replay is refused here,
/// and the only thing that refuses it is provenance.
#[must_use]
pub fn honest_registry(honest: &SigningKey) -> KeyRegistry {
    let mut keys = KeyRegistry::new();
    keys.insert(honest.verifying(), KeyStatus::Active, "honest-server");
    keys
}

/// A registry that also accepts the attacker's key.
///
/// The **weakened defence** for class 2, and not a strawman: `docs/RISKS.md` R4's
/// whole argument is that the format's guarantee is relative to key custody, so a
/// registry that trusts the wrong key is exactly the failure the guarantee is
/// relative to. A forged file verifies here, which establishes the exploit works
/// before it is run against the honest registry that stops it.
#[must_use]
pub fn compromised_registry(honest: &SigningKey, attacker_identity: [u8; 32]) -> KeyRegistry {
    let mut keys = honest_registry(honest);
    keys.insert(
        replay::VerifyingKey::from_bytes(attacker_identity),
        KeyStatus::Active,
        "attacker",
    );
    keys
}
