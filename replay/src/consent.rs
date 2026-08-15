//! Which consent document a session was recorded under, as a thing a program
//! can refuse.
//!
//! # The gap this closes
//!
//! `docs/CONSENT.md` exists, says the four things `docs/MILESTONES.md` M4
//! requires, and is checked by a test that reads it. What none of that reaches is
//! the question that actually governs whether a recording may be held: **was the
//! document this participant signed the document this session was recorded
//! under?** A consent text that gains a field — a new covariate, a new retention
//! rule, a new purpose — has stopped being the text somebody signed six months
//! ago, and nothing in a corpus of replays would say so. The paper stays paper;
//! what this module does is make its *absence* a mechanical error.
//!
//! So a consent record carries the version of the document its participant
//! signed, a session record carries the version it was operated under, and
//! `Corpus::store` refuses a match where either is missing or is not the current
//! one. "Missing" is the important half: a record written under the old format
//! has no version field, does not decode, and is therefore not a consent record —
//! which is the answer that keeps a corpus assembled before this existed from
//! being quietly readmitted.
//!
//! # Why a date and not a digest of the file
//!
//! A digest of `docs/CONSENT.md` would be the tempting mechanism and it is the
//! wrong one: it changes when somebody fixes a typo, so it would demand a
//! re-consent for an edit that changed nothing a participant agreed to, and the
//! response to friction like that is to stop asking. A declared version is a
//! judgement — the same judgement `docs/RISKS.md` R13 leaves in a `sim` version
//! bump — and it gets the same mechanical support: the document declares it, this
//! constant repeats it, [`the_document_declares_the_version_this_build_holds`]
//! fails if they disagree, and `ci` refuses a pull request that edits the
//! document without raising it.
//!
//! What that does not catch is an edit that raises the version and does not
//! deserve to, or one that deserves it and gets a patch. Both are judgements, and
//! the honest statement is the same one R13 makes: a spurious bump costs a line
//! and a missed one costs a re-consent, so the check is set to demand the cheap
//! mistake.

/// The consent document this build records against.
///
/// A date, because that is what a participant is shown and what they can compare
/// against the copy they signed. It must equal the `consent-version:` line in
/// `docs/CONSENT.md`.
pub const CURRENT: &str = "2026-08-16";

/// The identifier of a version of the consent document.
///
/// Constrained to exactly `YYYY-MM-DD` for the reason [`crate::Pseudonym`] is
/// constrained: these strings are written into a text record that
/// `Corpus::audit` reads byte by byte, and a free-form field is a place a
/// sentence — or a name — ends up.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConsentVersion(String);

impl ConsentVersion {
    /// The version this text names, or `None` if it is not one.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        if bytes.len() != 10 {
            return None;
        }
        for (index, byte) in bytes.iter().enumerate() {
            let dash = index == 4 || index == 7;
            if dash != (*byte == b'-') || (!dash && !byte.is_ascii_digit()) {
                return None;
            }
        }
        Some(Self(text.to_owned()))
    }

    /// The version this build records against.
    ///
    /// # Panics
    ///
    /// Never in a build that compiles: [`CURRENT`] is a constant of this crate
    /// and the assertion below is what keeps it well formed.
    #[must_use]
    pub fn current() -> Self {
        Self::parse(CURRENT).expect("CURRENT is a well-formed consent version")
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this is the version this build records against.
    #[must_use]
    pub fn is_current(&self) -> bool {
        self.0 == CURRENT
    }
}

impl core::fmt::Display for ConsentVersion {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The line `docs/CONSENT.md` declares its version on.
///
/// Public because it is the contract between the document and this crate, and a
/// contract in a private constant is a contract one side can change alone.
pub const DECLARATION: &str = "consent-version: ";

/// The version a copy of the consent document declares, or `None`.
///
/// Reads the *first* declaration, so that a document quoting an older version in
/// prose below cannot change the answer.
#[must_use]
pub fn declared_by(document: &str) -> Option<ConsentVersion> {
    document
        .lines()
        .find_map(|line| line.trim().strip_prefix(DECLARATION))
        .and_then(|text| ConsentVersion::parse(text.trim()))
}

#[cfg(test)]
mod tests {
    use super::{CURRENT, ConsentVersion, declared_by};

    #[test]
    fn a_version_is_a_date_and_nothing_else() {
        assert!(ConsentVersion::parse("2026-08-13").is_some());
        for wrong in [
            "",
            "2026-8-13",
            "2026/08/13",
            "2026-08-13 ",
            "yyyy-mm-dd",
            "2026-08-13-extra",
        ] {
            assert!(
                ConsentVersion::parse(wrong).is_none(),
                "{wrong:?} parsed as a consent version"
            );
        }
    }

    #[test]
    fn the_current_version_is_well_formed_and_is_current() {
        assert!(ConsentVersion::current().is_current());
        assert_eq!(ConsentVersion::current().as_str(), CURRENT);
    }

    #[test]
    fn a_declaration_is_read_from_the_first_line_that_carries_one() {
        let document = format!(
            "# CONSENT\n\nconsent-version: {CURRENT}\n\n> an older text said \
             consent-version: 2020-01-01\n"
        );
        assert_eq!(
            declared_by(&document).as_ref().map(ConsentVersion::as_str),
            Some(CURRENT)
        );
        assert!(declared_by("# CONSENT\n\nnothing here\n").is_none());
    }

    /// **The document and this build agree about which text is current.**
    ///
    /// The mechanism `docs/RISKS.md` R13 uses for `sim`'s version, one level up:
    /// the source of truth is the document a participant reads, this constant is
    /// what the program refuses against, and a change to one without the other
    /// fails here. `ci` is what refuses an edit to the document that leaves the
    /// version alone — this test cannot see that, because a document edited
    /// without a bump still agrees with a constant that was not bumped either.
    #[test]
    fn the_document_declares_the_version_this_build_holds() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the replay crate has a parent directory")
            .join("docs")
            .join("CONSENT.md");
        let document = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let declared = declared_by(&document).unwrap_or_else(|| {
            panic!(
                "{} declares no `{}<YYYY-MM-DD>` line, so nothing says which text \
                 a participant signed",
                path.display(),
                super::DECLARATION
            )
        });
        assert_eq!(
            declared.as_str(),
            CURRENT,
            "docs/CONSENT.md declares version {declared} and this build records \
             against {CURRENT}: one of the two was changed without the other, and \
             every session recorded by this build would name a document that does \
             not exist"
        );
    }
}
