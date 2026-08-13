//! Signing keys, verifying keys, and the registry that says which ones a
//! verifier accepts.
//!
//! # Why an audited crate and not thirty lines
//!
//! `docs/ARCHITECTURE.md` says it in one sentence and it is the rule this module
//! exists to obey: a security portfolio does not hand-roll signature code.
//! `sim` writes out SHA-256 because a digest has to produce the same 32 bytes
//! for as long as any replay exists and a dependency there is a dependency that
//! can change under `docs/RISKS.md` R1; a signature is the opposite situation —
//! the failure modes are subtle, they are not detectable by a test that passes,
//! and the ecosystem has an implementation whose whole purpose is to have been
//! looked at. So this is `ed25519-dalek`, and `replay` is the only crate in the
//! workspace that has it.
//!
//! Ed25519 rather than anything else for two reasons that are about this project
//! rather than about cryptography: signing is **deterministic** — the same
//! manifest signed twice by the same key produces the same 64 bytes, so a
//! replay is a function of the match and not of the moment it was sealed, which
//! is what lets `server/tests/produced_not_delivered.rs` compare two sealed
//! replays byte for byte — and a public key is 32 bytes, so a key registry is a
//! text file a person can read.
//!
//! # What a key is and is not evidence of
//!
//! `docs/RISKS.md` R13's last paragraph, restated here because this is where
//! somebody will look for it: a signature says that whoever held this key sealed
//! these bytes. It orders **this project's own builds**. It is not evidence
//! against an attacker who controls the server, and no claim in this repository
//! reads as though it were.

use std::fs;
use std::io;
use std::path::Path;

/// A key that signs. Never leaves the machine that generated it.
///
/// Deliberately not `Clone` and deliberately without a `Display`: the two ways
/// a private key escapes a program are being copied somewhere it is not
/// expected and being printed into a log. [`SigningKey::to_bytes`] is the one
/// way out and its name says what it is doing.
#[derive(Debug)]
pub struct SigningKey(ed25519_dalek::SigningKey);

impl SigningKey {
    /// A key from 32 bytes of seed material.
    ///
    /// Every 32-byte string is a valid Ed25519 seed, so this is total. What it
    /// does not do is judge whether the bytes were unpredictable: a key built
    /// from a written-down constant is a perfectly valid key that signs nothing
    /// anybody should believe, and the fixtures in this repository use exactly
    /// that on purpose.
    #[must_use]
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self(ed25519_dalek::SigningKey::from_bytes(&seed))
    }

    /// A key from the operating system's entropy.
    ///
    /// # Errors
    ///
    /// Whatever the platform's random source refuses. A failure here is not
    /// something to retry or to work around: a key generated from a source that
    /// erred is a key nobody can reason about.
    pub fn generate() -> io::Result<Self> {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed)
            .map_err(|error| io::Error::other(format!("no entropy: {error}")))?;
        Ok(Self::from_seed(seed))
    }

    /// The seed, for writing to a file the operator protects.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    /// Reads a signing key from a file holding its 64 hex characters.
    ///
    /// Hex rather than raw bytes so that a person can look at the file and see
    /// that they are holding a key, and so that a stray newline from an editor
    /// is trimmed rather than silently producing a different key.
    ///
    /// # Errors
    ///
    /// Anything the filesystem refuses, and [`io::ErrorKind::InvalidData`] for a
    /// file that is not 64 hex characters.
    pub fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        let text = fs::read_to_string(path)?;
        let seed = unhex32(text.trim()).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "a signing key is 64 hexadecimal characters",
            )
        })?;
        Ok(Self::from_seed(seed))
    }

    /// The public half, which is what a manifest carries and a registry holds.
    #[must_use]
    pub fn verifying(&self) -> VerifyingKey {
        VerifyingKey(self.0.verifying_key().to_bytes())
    }

    /// Signs a byte string.
    #[must_use]
    pub fn sign(&self, bytes: &[u8]) -> Signature {
        use ed25519_dalek::Signer as _;
        Signature(self.0.sign(bytes).to_bytes())
    }
}

/// A key that verifies. Public by construction and published with releases.
///
/// Held as bytes rather than as a parsed point, because it arrives from a file
/// somebody else wrote: a manifest carrying 32 bytes that are not a point is a
/// manifest this type has to be able to *hold* in order to report it, and
/// refusing to construct it would turn a verification failure into a decode
/// failure. The parse happens in [`VerifyingKey::verifies`], once, at the moment
/// the answer is wanted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VerifyingKey([u8; 32]);

impl VerifyingKey {
    /// The key these bytes name.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Whether this key signed these bytes.
    ///
    /// `verify_strict` rather than `verify`, which is the whole reason this is a
    /// method and not a one-liner at the call site: the strict form rejects
    /// small-order and non-canonical public keys, which are the inputs that
    /// make one signature verify under two keys. A verifier that accepted them
    /// would let a replay be attributed to a server that never sealed it.
    #[must_use]
    pub fn verifies(&self, bytes: &[u8], signature: &Signature) -> bool {
        let Ok(key) = ed25519_dalek::VerifyingKey::from_bytes(&self.0) else {
            return false;
        };
        key.verify_strict(bytes, &ed25519_dalek::Signature::from_bytes(&signature.0))
            .is_ok()
    }
}

impl core::fmt::Display for VerifyingKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Sixty-four bytes over a manifest.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Signature([u8; 64]);

impl Signature {
    /// The signature these bytes name.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    /// The bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}

impl core::fmt::Debug for Signature {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Signature(")?;
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        write!(f, ")")
    }
}

/// Whether a key may still be used to seal.
///
/// Both statuses **verify**, and that is the decision rather than an oversight.
/// `docs/RISKS.md` R4: rotating a key without keeping the retired one published
/// orphans every replay signed with it, which is a way of destroying evidence by
/// housekeeping. So retirement is a statement about what may be *signed* from
/// now on, and a verifier reports it rather than acting on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyStatus {
    /// In use. New replays may be sealed with it.
    Active,
    /// Withdrawn from service. Replays already sealed with it still verify.
    Retired,
}

/// One line of a registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyEntry {
    /// The public key.
    pub key: VerifyingKey,
    /// Whether it may still seal.
    pub status: KeyStatus,
    /// What a person calls it. Never read by verification.
    pub label: String,
}

/// The keys a verifier accepts, and nothing else.
///
/// A verifier with no registry establishes nothing at all: a replay is signed
/// by whoever sealed it, and "the signature is internally consistent" is a
/// statement about arithmetic rather than about provenance. So there is no
/// default registry, no implicit trust, and `replay verify` refuses to run
/// without one.
#[derive(Clone, Debug, Default)]
pub struct KeyRegistry {
    entries: Vec<KeyEntry>,
}

/// Why a byte string is not a registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryError {
    /// A line that is not a comment, not blank, and not a key.
    Malformed(usize),
    /// A key given twice, which makes "which entry is this" ambiguous.
    Duplicate(VerifyingKey),
}

impl core::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Malformed(line) => write!(f, "line {line} is not a key entry"),
            Self::Duplicate(key) => write!(f, "{key} appears twice"),
        }
    }
}

impl core::error::Error for RegistryError {}

impl KeyRegistry {
    /// A registry holding nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Adds a key.
    pub fn insert(&mut self, key: VerifyingKey, status: KeyStatus, label: impl Into<String>) {
        self.entries.push(KeyEntry {
            key,
            status,
            label: label.into(),
        });
    }

    /// The entry for a key, if this registry holds it.
    #[must_use]
    pub fn find(&self, key: VerifyingKey) -> Option<&KeyEntry> {
        self.entries.iter().find(|entry| entry.key == key)
    }

    /// Every entry, in the order the file gave them.
    #[must_use]
    pub fn entries(&self) -> &[KeyEntry] {
        &self.entries
    }

    /// The registry as it is stored.
    ///
    /// One `key <64 hex> <active|retired> <label>` per line, `#` for a comment.
    /// Written by hand for the reason `ConsentRecord` is: the file is small, a
    /// person has to be able to read it and to check a key against a published
    /// one by eye, and a derive would put the field list somewhere the reader
    /// cannot see.
    #[must_use]
    pub fn encode(&self) -> String {
        let mut out = String::from(
            "# moba replay key registry, format 1\n\
             # key <64 hex> <active|retired> <label>\n",
        );
        for KeyEntry { key, status, label } in &self.entries {
            let status = match status {
                KeyStatus::Active => "active",
                KeyStatus::Retired => "retired",
            };
            out.push_str(&format!("key {key} {status} {label}\n"));
        }
        out
    }

    /// Reads a registry.
    ///
    /// # Errors
    ///
    /// [`RegistryError`] for a line that is not an entry, and for a key given
    /// twice — which is refused rather than resolved, because a registry with
    /// one key listed active and retired has no answer to give.
    pub fn decode(text: &str) -> Result<Self, RegistryError> {
        let mut registry = Self::new();
        for (number, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut fields = line.split_whitespace();
            let malformed = || RegistryError::Malformed(number.saturating_add(1));
            if fields.next() != Some("key") {
                return Err(malformed());
            }
            let key = fields
                .next()
                .and_then(unhex32)
                .map(VerifyingKey::from_bytes)
                .ok_or_else(malformed)?;
            let status = match fields.next() {
                Some("active") => KeyStatus::Active,
                Some("retired") => KeyStatus::Retired,
                _ => return Err(malformed()),
            };
            let label = fields.collect::<Vec<_>>().join(" ");
            if label.is_empty() {
                return Err(malformed());
            }
            if registry.find(key).is_some() {
                return Err(RegistryError::Duplicate(key));
            }
            registry.insert(key, status, label);
        }
        Ok(registry)
    }

    /// Reads a registry from a file.
    ///
    /// # Errors
    ///
    /// Anything the filesystem refuses, and [`io::ErrorKind::InvalidData`] for a
    /// file that is not a registry.
    pub fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        let text = fs::read_to_string(path)?;
        Self::decode(&text)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
    }
}

/// Thirty-two bytes from sixty-four hex characters, or nothing.
fn unhex32(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (index, slot) in out.iter_mut().enumerate() {
        let at = index.checked_mul(2)?;
        *slot = u8::from_str_radix(text.get(at..at.checked_add(2)?)?, 16).ok()?;
    }
    Some(out)
}
