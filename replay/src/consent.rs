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
//!
//! # What a version alone could not say, and why [`Purpose`] exists
//!
//! A version answers *which text* somebody signed. It cannot answer **which of
//! its optional parts they agreed to**, and until this module grew [`Purpose`]
//! the corpus could only hold one such answer — a `publication` boolean —
//! because the document only offered one box.
//!
//! `docs/CONSENT.md` now offers four, and the rule that decides what is a box is
//! stated there and worth repeating where the code is: **a box exists only if
//! refusing it leaves the rest of the participation possible.** Everything the
//! declared purpose structurally needs — the intentions, the device stream, the
//! session record, the lobby crossing — is not a box, and the document says so in
//! the participant's own words rather than offering a choice that is really
//! "take part or do not".
//!
//! # And the rule that keeps an old signature from covering a new purpose
//!
//! [`Permissions::decode`] requires a record to carry a line for **every**
//! purpose this build knows. A record that does not is not a consent record — the
//! same equivalence [`ConsentVersion`] draws between an absent version and a
//! stale one, one level down. So adding a variant to [`Purpose`] invalidates
//! every consent record already written, which is exactly right: a purpose
//! nobody was asked about is a purpose nobody granted, and the corpus must say
//! that by refusing rather than by defaulting.

/// The consent document this build records against.
///
/// A date, because that is what a participant is shown and what they can compare
/// against the copy they signed. It must equal the `consent-version:` line in
/// `docs/CONSENT.md`.
pub const CURRENT: &str = "2026-08-17";

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

/// One thing a participant may agree to separately, and may refuse without
/// refusing to take part.
///
/// **Four, and the number is a decision rather than a count.** The test each one
/// passes is the one `docs/CONSENT.md` states: refusing it leaves everything
/// else about the participation unchanged. A tick box for something that fails
/// that test is a tick box for "take part or do not", and offering one is the
/// handling this project criticises elsewhere.
///
/// What is deliberately **not** here, each for a reason the document gives in
/// full:
///
/// - **the intention log, the device stream, the session record.** The declared
///   purpose is calibrating detectors that read exactly these, so a recording
///   without them is not usable for the thing a participant is being asked to
///   help with. The device stream has a second, mechanical reason: a companion
///   covers all nine seats or none (`docs/SCHEMA.md` §9), so one person's refusal
///   would remove it for the other eight, which makes it not an individual
///   choice at all.
/// - **the lobby measurement.** It is a *derivation* over a crossing that is
///   recorded either way — `Element::Ready` is inert until the lobby has been
///   crossed — so refusing it would change nothing about what is held and would
///   only delete a covariate. `docs/SCHEMA.md` §4e already gives the participant
///   the only control that means anything here: spend the wait doing something
///   else, and the record says `absent` with no consequence to them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Purpose {
    /// The **raw** recordings of this participant's matches may be published as
    /// part of an open data set.
    ///
    /// Distinct from publishing statistics *derived* from them, which is inside
    /// the declared purpose and is not refusable. `docs/SCHEMA.md` §10: a match
    /// is one interleaved log, so a match is publishable only if every
    /// participant in it granted this.
    Publication,
    /// The recordings may be used to **train** a bot — the reinforcement-learning
    /// sub-project `docs/SCOPE.md` defers — as opposed to calibrating and
    /// evaluating the cheat detectors.
    ///
    /// A different purpose from detection in the ordinary sense and in the legal
    /// one: detection reads a recording to describe how people play, and training
    /// reads it to produce something that plays. Refusing it leaves detector
    /// calibration, which is what the project is for, completely unaffected.
    BotTraining,
    /// The recordings may be kept until the retention date **even after the
    /// project's own work concludes**.
    ///
    /// Refusing it does not shorten anything about the session: it schedules the
    /// destruction at the earlier of the two dates instead of the later one, and
    /// `Corpus::conclude` is the command that carries it out.
    RetentionAfterProject,
    /// This participant may be **named** — rather than appearing under their
    /// pseudonym — in work derived from the corpus: an acknowledgement, a report,
    /// a talk.
    ///
    /// The one permission whose enforcement is genuinely partial, and the code
    /// says so where a reader will meet it: [`crate::corpus::Corpus::attribution`]
    /// is the only machine-readable path from a pseudonym to a name and it
    /// refuses without this, but no program reaches a sentence somebody writes in
    /// a paper. `docs/CONSENT.md` states that limit to the participant rather
    /// than implying a guarantee the project cannot hold.
    NamedAttribution,
}

impl Purpose {
    /// Every purpose, in the order a record writes them.
    ///
    /// Public and exhaustive because the encoding, the document check and the
    /// command-line surface all iterate it: a purpose added to the enum and
    /// forgotten in one of the three would be a purpose nobody is asked about.
    pub const ALL: [Self; 4] = [
        Self::Publication,
        Self::BotTraining,
        Self::RetentionAfterProject,
        Self::NamedAttribution,
    ];

    /// The tag this purpose is written and typed as.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Publication => "publication",
            Self::BotTraining => "bot-training",
            Self::RetentionAfterProject => "retention-after-project",
            Self::NamedAttribution => "named-attribution",
        }
    }

    /// The purpose this tag names, or `None`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|purpose| purpose.tag() == text)
    }

    /// What refusing this one costs the participant, in one sentence.
    ///
    /// Carried in the code rather than only in the document so that every
    /// refusal a command prints says what it is about, and so that a purpose
    /// added without a sentence does not compile.
    #[must_use]
    pub const fn refusing_means(self) -> &'static str {
        match self {
            Self::Publication => {
                "the raw recordings of their matches are never published; \
                 statistics derived from them still are"
            }
            Self::BotTraining => {
                "their recordings never train a bot; they are still used to \
                 calibrate and evaluate the detectors"
            }
            Self::RetentionAfterProject => {
                "their recordings are destroyed when the project's work concludes \
                 rather than at the retention date"
            }
            Self::NamedAttribution => {
                "they appear under their pseudonym and never under their name in \
                 anything derived from the corpus"
            }
        }
    }
}

impl core::fmt::Display for Purpose {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.tag())
    }
}

/// The prefix a consent record writes each permission on.
///
/// Public for the reason [`DECLARATION`] is: it is the contract between the
/// stored record and everything that reads one.
pub const PERMIT: &str = "permits.";

/// Which of the separable purposes a participant granted.
///
/// **Every purpose is stated, granted or refused.** There is no "unspecified"
/// and no default, because a silence in a file is exactly what a granular
/// consent must not be readable as — and because that is what makes adding a
/// [`Purpose`] a re-consent rather than a widening. See this module's header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Permissions {
    granted: [bool; Purpose::ALL.len()],
}

impl Permissions {
    /// Nothing granted: the participation itself and none of the four.
    ///
    /// The state a form with no box ticked produces, and the right default for
    /// anything that has to invent one.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            granted: [false; Purpose::ALL.len()],
        }
    }

    /// The permissions this list of purposes grants, and nothing else.
    #[must_use]
    pub fn granting(purposes: &[Purpose]) -> Self {
        let mut permissions = Self::none();
        for purpose in purposes {
            permissions.set(*purpose, true);
        }
        permissions
    }

    /// Whether this purpose was granted.
    #[must_use]
    pub fn granted(&self, purpose: Purpose) -> bool {
        self.granted
            .get(Self::index(purpose))
            .copied()
            .unwrap_or(false)
    }

    /// Grants or refuses one purpose.
    ///
    /// Refusing is what a partial withdrawal does, and it is the same operation
    /// as never having granted it: `docs/CONSENT.md` promises that withdrawing
    /// one permission leaves the participation intact, so there is nothing here
    /// that distinguishes "refused on the form" from "withdrawn afterwards"
    /// except the tombstone `Corpus::withdraw_purpose` writes.
    pub const fn set(&mut self, purpose: Purpose, granted: bool) {
        self.granted[Self::index(purpose)] = granted;
    }

    /// The purposes granted, in [`Purpose::ALL`] order.
    #[must_use]
    pub fn granted_purposes(&self) -> Vec<Purpose> {
        Purpose::ALL
            .into_iter()
            .filter(|purpose| self.granted(*purpose))
            .collect()
    }

    /// The lines a consent record carries, one per purpose.
    #[must_use]
    pub fn encode(&self) -> String {
        let mut out = String::new();
        for purpose in Purpose::ALL {
            out.push_str(&format!(
                "{PERMIT}{}: {}\n",
                purpose.tag(),
                if self.granted(purpose) { "yes" } else { "no" }
            ));
        }
        out
    }

    /// Reads the permissions out of a record's lines, or `None` if **any**
    /// purpose this build knows is missing.
    ///
    /// The refusal is the whole mechanism. A record written before a purpose
    /// existed cannot decode, so it is not a consent record, so `Corpus::store`
    /// refuses every match its participant is in until they are asked again —
    /// which is the only honest reading of a document that grew a question
    /// nobody has answered.
    #[must_use]
    pub fn decode(text: &str) -> Option<Self> {
        let mut permissions = Self::none();
        for purpose in Purpose::ALL {
            let line = format!("{PERMIT}{}: ", purpose.tag());
            let value = text.lines().find_map(|line_text| {
                line_text
                    .trim_end()
                    .strip_prefix(line.as_str())
                    .map(str::trim)
            })?;
            permissions.set(
                purpose,
                match value {
                    "yes" => true,
                    "no" => false,
                    _ => return None,
                },
            );
        }
        Some(permissions)
    }

    const fn index(purpose: Purpose) -> usize {
        match purpose {
            Purpose::Publication => 0,
            Purpose::BotTraining => 1,
            Purpose::RetentionAfterProject => 2,
            Purpose::NamedAttribution => 3,
        }
    }
}

impl core::fmt::Display for Permissions {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let granted = self.granted_purposes();
        if granted.is_empty() {
            return f.write_str("none of the four separable purposes");
        }
        let tags: Vec<&str> = granted.iter().map(|purpose| purpose.tag()).collect();
        f.write_str(&tags.join(", "))
    }
}

/// What one version of `docs/CONSENT.md` changed against the one before it.
///
/// # Why this is in the code and not only in the document
///
/// `docs/CONSENT.md` carries a "what changed" section, and a participant asked to
/// sign again is entitled to read the *difference* rather than the whole page —
/// which is what separates an informed re-signature from an administrative one.
/// A section in a document is a thing a person reads when somebody remembers to
/// point at it. This list is what makes `replay enrol` print the difference,
/// unprompted, whenever the record it is replacing was signed against an older
/// text, and what makes a test fail when a version moves and nobody says why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    /// The version this describes the arrival of.
    pub version: &'static str,
    /// What changed against the version before it, in one sentence a
    /// participant reads.
    pub summary: &'static str,
}

/// Every version of the consent document this project has operated, newest
/// first.
///
/// Newest first because that is the order somebody re-signing reads them in, and
/// because [`since`] returns a prefix of this slice.
pub const CHANGES: [Change; 4] = [
    Change {
        version: "2026-08-17",
        summary: "The page is reorganised — a summary first, then the categories, \
                  then the detail — and nothing it says is new. What is new is the \
                  choice: publication of the raw recordings, use for training a \
                  bot, keeping the data after the project's work ends, and being \
                  named rather than pseudonymous are now four separate boxes \
                  instead of one, each refusable on its own, and each refusal is \
                  carried out by the tooling rather than remembered. You are also \
                  asked to confirm you are 18 or over.",
    },
    Change {
        version: "2026-08-16",
        summary: "Recording now starts when the menu appears rather than when the \
                  match does, and the project works out from that menu how many \
                  counts your mouse reports for a distance on screen — a \
                  measurement of the equipment, not of you. One line was added \
                  saying which mouse a session was played on.",
    },
    Change {
        version: "2026-08-15",
        summary: "Every movement your mouse reports is now kept, 125 to 1000 times \
                  a second, rather than the one instruction per thirtieth of a \
                  second the previous text described.",
    },
    Change {
        version: "2026-08-14",
        summary: "The first version of this text.",
    },
];

/// What changed between the version somebody signed and the one this build
/// operates.
///
/// Empty when they signed the current text — which is the case that must produce
/// no output at all, because a command that prints a paragraph on every ordinary
/// run trains its reader to skip it.
///
/// A version this list has never heard of yields the **whole** list rather than
/// nothing: a record naming a document this project did not write is a record
/// whose holder has to be asked again about everything.
#[must_use]
pub fn since(signed: &ConsentVersion) -> &'static [Change] {
    match CHANGES
        .iter()
        .position(|change| change.version == signed.as_str())
    {
        Some(index) => CHANGES.split_at(index).0,
        None => &CHANGES,
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
    use super::{
        CHANGES, CURRENT, ConsentVersion, PERMIT, Permissions, Purpose, declared_by, since,
    };

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

    /// A purpose's tag is what it parses back from, and nothing else is one.
    #[test]
    fn a_purpose_is_one_of_four_tags() {
        for purpose in Purpose::ALL {
            assert_eq!(Purpose::parse(purpose.tag()), Some(purpose));
            assert!(!purpose.refusing_means().is_empty());
        }
        for wrong in ["", "publish", "training", "PUBLICATION", "retention"] {
            assert!(Purpose::parse(wrong).is_none(), "{wrong:?} parsed");
        }
        // Four distinct tags, so no two purposes share a line in a record.
        let mut tags: Vec<&str> = Purpose::ALL.iter().map(|p| p.tag()).collect();
        tags.sort_unstable();
        tags.dedup();
        assert_eq!(tags.len(), Purpose::ALL.len());
    }

    /// **A record must answer every purpose, and a silence is not an answer.**
    ///
    /// The mechanism that keeps an old signature from covering a new purpose,
    /// exercised the only way it can be: by deleting one line from a record that
    /// decoded a moment earlier. `docs/RISKS.md` R3's rule that absent and stale
    /// fail alike, one level down from the version.
    #[test]
    fn a_permission_line_missing_for_any_purpose_is_not_a_consent_record() {
        let permissions = Permissions::granting(&[Purpose::Publication]);
        let text = permissions.encode();
        assert_eq!(Permissions::decode(&text), Some(permissions));

        for purpose in Purpose::ALL {
            let prefix = format!("{PERMIT}{}: ", purpose.tag());
            let without: String = text
                .lines()
                .filter(|line| !line.starts_with(&prefix))
                .map(|line| format!("{line}\n"))
                .collect();
            assert!(
                Permissions::decode(&without).is_none(),
                "a record silent about {purpose} decoded anyway, so a purpose \
                 nobody was asked about would read as one somebody granted"
            );
        }
        // …and a value that is neither yes nor no is refused rather than
        // mapped onto the nearest one.
        assert!(Permissions::decode(&text.replace(": no", ": probably")).is_none());
    }

    /// Granting is per purpose and refusing one leaves the others alone.
    #[test]
    fn a_permission_is_granted_and_refused_one_purpose_at_a_time() {
        let mut permissions = Permissions::granting(&[Purpose::Publication, Purpose::BotTraining]);
        assert!(permissions.granted(Purpose::Publication));
        assert!(permissions.granted(Purpose::BotTraining));
        assert!(!permissions.granted(Purpose::NamedAttribution));

        permissions.set(Purpose::Publication, false);
        assert!(!permissions.granted(Purpose::Publication));
        assert!(
            permissions.granted(Purpose::BotTraining),
            "refusing one purpose refused another"
        );
        assert_eq!(
            Permissions::none().granted_purposes(),
            Vec::<Purpose>::new()
        );
    }

    /// **The current version has an entry saying what it changed.**
    ///
    /// A version that moves without a sentence is a re-signature nobody can make
    /// informed, which is the whole difference this list exists to hold.
    #[test]
    fn every_version_this_project_operated_says_what_it_changed() {
        assert_eq!(
            CHANGES[0].version, CURRENT,
            "the newest entry in CHANGES is {} and this build records against \
             {CURRENT}: a participant re-signing would be shown the difference \
             against the wrong text",
            CHANGES[0].version
        );
        for change in CHANGES {
            assert!(
                ConsentVersion::parse(change.version).is_some(),
                "{} is not a version",
                change.version
            );
            assert!(!change.summary.is_empty());
        }
        // Strictly decreasing, so `since` returning a prefix is returning the
        // versions that came after the one somebody signed.
        for pair in CHANGES.windows(2) {
            assert!(
                pair[0].version > pair[1].version,
                "{} is not newer than {}",
                pair[0].version,
                pair[1].version
            );
        }
    }

    /// What a re-signing participant is shown: the gap, and nothing when there
    /// is none.
    #[test]
    fn the_difference_shown_is_the_versions_that_arrived_after_the_one_signed() {
        let current = ConsentVersion::current();
        assert!(
            since(&current).is_empty(),
            "somebody who signed the current text would be shown a difference"
        );

        let oldest = ConsentVersion::parse(CHANGES[CHANGES.len() - 1].version).expect("a version");
        assert_eq!(
            since(&oldest).len(),
            CHANGES.len() - 1,
            "somebody who signed the first text is not shown every change since"
        );
        assert_eq!(since(&oldest)[0].version, CURRENT);

        // A version this project never wrote: everything, rather than nothing.
        let unknown = ConsentVersion::parse("2001-01-01").expect("a version");
        assert_eq!(since(&unknown).len(), CHANGES.len());
    }

    /// **Every purpose the code knows has a section in the document.**
    ///
    /// The half [`the_document_declares_the_version_this_build_holds`] cannot
    /// see: the version can agree perfectly while the text offers three boxes and
    /// the tooling records four. A purpose a participant was never asked about is
    /// a purpose nobody granted, and the document is where the asking happens.
    #[test]
    fn the_document_offers_a_box_for_every_purpose_this_build_records() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the replay crate has a parent directory")
            .join("docs")
            .join("CONSENT.md");
        let document = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        for purpose in Purpose::ALL {
            assert!(
                document.contains(purpose.tag()),
                "docs/CONSENT.md never names `{purpose}`, so this build records a \
                 permission the text does not ask for"
            );
        }
        for change in CHANGES {
            assert!(
                document.contains(change.version),
                "docs/CONSENT.md does not mention version {}, so a participant \
                 re-signing cannot read what changed",
                change.version
            );
        }
    }
}
