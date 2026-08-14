//! The replay tool: seal a match, verify one, honour a withdrawal, audit the
//! result.
//!
//! # Why verification runs as a separate process
//!
//! `docs/MILESTONES.md` M3's exit criterion asks that the server's
//! authoritative digest be reproduced by "an offline resimulation of the
//! recorded input log, run as a separate process", and M4's asks that
//! `replay verify` resimulate a human match to the server's final digest. The
//! separation is the point rather than a packaging preference: a check that ran
//! inside the server would share the server's memory, its arithmetic and its
//! bugs, and would agree with it for reasons that have nothing to do with the
//! log. Booting a second process from the file alone is the cheapest thing that
//! is not that.
//!
//! # Why `verify` refuses to run without a key registry
//!
//! Because a verification with no registry establishes nothing. A signature is
//! internally consistent by construction — anybody can produce one — so
//! "verified" without "verified as *whose*" is a word doing no work.
//! `docs/RISKS.md` R4 is where that lives, and the shape of it here is that the
//! registry is a required argument rather than an optional one with a permissive
//! default. There is deliberately no `--insecure`.
//!
//! # Why the corpus commands are in the same binary
//!
//! `docs/ENGINEERING.md`'s rule is five automations understood over fifteen
//! endured, and `docs/ARCHITECTURE.md` refuses a crate for a handful of
//! commands. `withdraw` and `audit` operate on directories of replays, which is
//! what this crate defines, so they are subcommands here rather than a second
//! binary or an eighth crate.
//!
//! ```text
//! replay keygen <name>                  # <name>.signing-key and <name>.public-key
//! replay verify <replay> <keys>         # resimulate, check the seal, report
//! replay inspect <replay>               # print the manifest, check nothing
//! replay enrol <corpus> <pseudonym> <identity> <consented-on> <retention-until> <publication>
//! replay store <corpus> <replay> <parts-dir> <recorded-on> <supervision>
//! replay census <corpus>                # what the corpus is, and what it supports
//! replay withdraw <corpus> <pseudonym> <date>
//! replay audit <corpus> <pseudonym>     # non-zero if anything is left
//! ```
//!
//! Exit status: 0 on success, 1 when the thing being checked is wrong, 2 when
//! the arguments or the file are.
//!
//! # The three M6 commands, and what each of them refuses
//!
//! `enrol` writes a consent record and the pseudonym mapping. It is the one
//! command whose input is a conversation — `docs/ENGINEERING.md` lists admitting
//! a participant among the things that stay manual — and what it mechanises is
//! only the filing. It stamps the consent record with the version of
//! `docs/CONSENT.md` this build holds, so the operator cannot record somebody
//! against a text that is not the one in front of them.
//!
//! `store` is the pipeline. It takes the sealed replay and the directory of
//! session parts the nine clients wrote, assembles them into one session record,
//! and refuses the match if the consent regime cannot account for it — see
//! `replay::corpus`'s table of refusals. Nothing is written until every check has
//! passed.
//!
//! `census` prints what the corpus is and what it supports. It writes **nothing**:
//! a stored summary is the derived index M5 removed wearing a friendly name, and
//! the number it would carry is a number that can disagree with the corpus it
//! came from. Its output is a page rather than a line because `docs/RISKS.md` R8
//! requires the two confidence bounds to travel together — a reader shown the
//! friendlier one has been handled.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use replay::consent::ConsentVersion;
use replay::corpus::{ConsentRecord, Corpus};
use replay::session::{SeatRecord, SessionRecord, Supervision};
use replay::split::{HOLDOUT_IN, Split, split_of};
use replay::{Build, KeyRegistry, KeyStatus, Replay, SigningKey, VerifyError};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = arguments.first() else {
        return usage();
    };
    match (command.as_str(), arguments.len()) {
        ("keygen", 2) => keygen(Path::new(&arguments[1])),
        ("verify", 3) => verify(Path::new(&arguments[1]), Path::new(&arguments[2])),
        ("inspect", 2) => inspect(Path::new(&arguments[1])),
        ("enrol", 7) => enrol(
            &arguments[1],
            &arguments[2],
            &arguments[3],
            &arguments[4],
            &arguments[5],
            &arguments[6],
        ),
        ("store", 6) => store(
            &arguments[1],
            Path::new(&arguments[2]),
            Path::new(&arguments[3]),
            &arguments[4],
            &arguments[5],
        ),
        ("census", 2) => census(&arguments[1]),
        ("withdraw", 4) => withdraw(&arguments[1], &arguments[2], &arguments[3]),
        ("audit", 3) => audit(&arguments[1], &arguments[2]),
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: replay keygen <name>");
    eprintln!("       replay verify <replay> <keys>");
    eprintln!("       replay inspect <replay>");
    eprintln!(
        "       replay enrol <corpus> <pseudonym> <identity> <consented-on> \
         <retention-until> <publication:yes|no>"
    );
    eprintln!(
        "       replay store <corpus> <replay> <parts-dir> <recorded-on> \
         <in-person|remote|unsupervised>"
    );
    eprintln!("       replay census <corpus>");
    eprintln!("       replay withdraw <corpus> <pseudonym> <date>");
    eprintln!("       replay audit <corpus> <pseudonym>");
    ExitCode::from(2)
}

/// Records one participant's consent and their pseudonym mapping.
fn enrol(
    root: &str,
    pseudonym: &str,
    identity: &str,
    consented_on: &str,
    retention_until: &str,
    publication: &str,
) -> ExitCode {
    let publication = match publication {
        "yes" => true,
        "no" => false,
        other => {
            eprintln!("replay: publication is yes or no, not {other}");
            return ExitCode::from(2);
        }
    };
    if replay::Pseudonym::parse(pseudonym).is_none() {
        eprintln!(
            "replay: {pseudonym} is not a pseudonym: letters, digits, '_' and '-', \
             at most 32 bytes (docs/SCHEMA.md)"
        );
        return ExitCode::from(2);
    }
    let record = ConsentRecord {
        pseudonym: pseudonym.to_owned(),
        consented_on: consented_on.to_owned(),
        retention_until: retention_until.to_owned(),
        publication,
        // Stamped from this build rather than typed by the operator: the version
        // is a fact about which document was on the table, and a field somebody
        // types is a field somebody types wrong.
        consent_version: ConsentVersion::current(),
    };
    if let Err(error) = Corpus::open(root).enrol(&record, identity) {
        eprintln!("replay: {root}: {error}");
        return ExitCode::from(2);
    }
    println!(
        "replay: enrolled {pseudonym} under consent document {}, retained until \
         {retention_until}, publication {}",
        record.consent_version,
        if publication { "agreed" } else { "refused" }
    );
    eprintln!(
        "replay: the signed consent text is kept with the corpus and outside this \
         repository. This record is only the machine's note that one exists."
    );
    ExitCode::SUCCESS
}

/// Files a sealed match and the session it was recorded in.
fn store(
    root: &str,
    replay_path: &Path,
    parts: &Path,
    recorded_on: &str,
    supervision: &str,
) -> ExitCode {
    let Some(supervision) = Supervision::parse(supervision) else {
        eprintln!(
            "replay: {supervision:?} is not a supervision condition. One of \
             in-person, remote, unsupervised — what makes a match human is a fact \
             about a person rather than a property of the file, so the fact is \
             recorded (docs/SCHEMA.md)."
        );
        return ExitCode::from(2);
    };

    let Some(replay) = read(replay_path) else {
        return ExitCode::from(2);
    };

    let mut collected: Vec<(String, String)> = Vec::new();
    let entries = match std::fs::read_dir(parts) {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!("replay: {}: {error}", parts.display());
            return ExitCode::from(2);
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|kind| kind != "session-part") {
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => collected.push((path.display().to_string(), text)),
            Err(error) => {
                eprintln!("replay: {}: {error}", path.display());
                return ExitCode::from(2);
            }
        }
    }
    if collected.is_empty() {
        eprintln!(
            "replay: {} holds no *.session-part file. A match with no session \
             record is a match whose hardware nobody wrote down (docs/SCHEMA.md).",
            parts.display()
        );
        return ExitCode::from(2);
    }

    let session = match SessionRecord::assemble(
        replay.manifest.match_id,
        ConsentVersion::current(),
        recorded_on,
        supervision,
        &collected,
    ) {
        Ok(session) => session,
        Err(message) => {
            eprintln!("replay: {message}");
            return ExitCode::from(2);
        }
    };

    match Corpus::open(root).store(&replay, &session) {
        Ok(()) => {
            println!(
                "replay: stored {} — {} seat(s) occupied, {}, supervision {}, {}",
                replay.manifest.match_id,
                session.occupied().len(),
                split_of(replay.manifest.match_id).tag(),
                session.supervision.tag(),
                if session.degraded() {
                    "DEGRADED: a client fell behind the tick"
                } else {
                    "every client kept the tick"
                }
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("replay: refused: {error}");
            ExitCode::FAILURE
        }
    }
}

/// What the corpus is, and what a claim made on it may say.
///
/// Prints and stores nothing. Every number here is recomputed from the matches
/// on disk, so it cannot drift from them and a withdrawal changes it the moment
/// it happens.
#[expect(
    clippy::too_many_lines,
    reason = "one report, printed in the order a reader needs it"
)]
fn census(root: &str) -> ExitCode {
    let corpus = Corpus::open(root);
    let matches = match corpus.matches() {
        Ok(matches) => matches,
        Err(error) => {
            eprintln!("replay: {root}: {error}");
            return ExitCode::from(2);
        }
    };

    let mut people: Vec<String> = Vec::new();
    let mut full = 0u32;
    let mut partial = 0u32;
    let mut degraded = 0u32;
    let mut held = 0u32;
    let mut unaccountable: Vec<String> = Vec::new();
    let mut ticks = 0u64;
    let mut worst_overrun_ns = 0u64;
    let mut supervision: BTreeMap<Supervision, u32> = BTreeMap::new();

    for match_id in &matches {
        let (Ok(replay), Ok(session)) = (corpus.replay_of(match_id), corpus.session_of(match_id))
        else {
            unaccountable.push(match_id.clone());
            continue;
        };
        for pseudonym in replay.manifest.participants() {
            let name = pseudonym.to_string();
            if !people.contains(&name) {
                people.push(name);
            }
        }
        ticks = ticks.saturating_add(u64::from(replay.manifest.ticks));
        if session.occupied().len() == sim::PLAYER_COUNT {
            full = full.saturating_add(1);
        } else {
            partial = partial.saturating_add(1);
        }
        if session.degraded() {
            degraded = degraded.saturating_add(1);
        }
        *supervision.entry(session.supervision).or_insert(0) += 1;
        for seat in &session.seats {
            if let SeatRecord::Human { measured, .. } = seat {
                worst_overrun_ns = worst_overrun_ns.max(measured.worst_overrun_ns);
            }
        }
        if split_of(replay.manifest.match_id) == Split::Holdout {
            held = held.saturating_add(1);
        }
    }
    people.sort();

    let recorded = matches.len().saturating_sub(unaccountable.len());
    println!("corpus: {} match(es) at {root}", recorded);
    println!("corpus: {} distinct participant(s): {}", people.len(), {
        if people.is_empty() {
            "-".to_owned()
        } else {
            people.join(", ")
        }
    });
    println!(
        "corpus: {full} at nine seats, {partial} partially filled — counted \
         separately and never pooled into one distribution (docs/SCHEMA.md)"
    );
    println!(
        "corpus: {degraded} session(s) degraded; worst tick-budget overrun {:.3} ms \
         (docs/RISKS.md R16)",
        (worst_overrun_ns as f64) / 1e6
    );
    // The supervision strata, printed beside the counts they qualify. What makes
    // a match human is the operator having been there, not anything in the file
    // (docs/SCHEMA.md, docs/SCOPE.md's ceiling) — so a corpus that mixes the
    // three has a covariate in it, and this is the line that says so before
    // anybody builds a distribution over the whole of it.
    println!(
        "corpus: supervision — {} in person, {} remote, {} unsupervised; a \
         distribution over more than one of these has a provenance covariate in it \
         (docs/SCHEMA.md)",
        supervision
            .get(&Supervision::InPerson)
            .copied()
            .unwrap_or(0),
        supervision.get(&Supervision::Remote).copied().unwrap_or(0),
        supervision
            .get(&Supervision::Unsupervised)
            .copied()
            .unwrap_or(0)
    );
    println!(
        "corpus: {held} held out, {} for training, one in {HOLDOUT_IN} by \
         replay::split (frozen)",
        recorded.saturating_sub(held as usize)
    );
    println!(
        "corpus: {} tick(s) recorded, {:.1} minute(s) of play",
        ticks,
        (ticks as f64) / f64::from(sim::TICKS_PER_SECOND) / 60.0
    );

    if !unaccountable.is_empty() {
        eprintln!(
            "replay: {} match(es) do not read and are in nobody's account:",
            unaccountable.len()
        );
        for match_id in &unaccountable {
            eprintln!("replay:   {match_id}");
        }
    }

    // The two bounds, together, always. `docs/RISKS.md` R8 and
    // `docs/MILESTONES.md` M6: nine seats of one match are not nine independent
    // observations and several matches share people, so a detector has two
    // honest denominators depending on what it reads, and showing one of them is
    // showing the friendlier one.
    let people_bound = bound(people.len());
    let match_bound = bound(recorded);
    println!();
    println!("what this corpus can support, at 95% confidence and zero observed false positives:");
    println!(
        "  what a person's style drives  : N = {} people  -> upper bound about {people_bound}",
        people.len()
    );
    println!(
        "  what a match's circumstances drive: N = {recorded} matches -> upper bound about {match_bound}"
    );
    println!(
        "  Both belong in every detector document. The rule is 3/N; the 9 x {recorded} = {} scored",
        recorded.saturating_mul(9)
    );
    println!("  player-matches are NOT independent and must never be used as N. No claim of");
    println!("  the form \"0% false positives\" is supportable at any size this project reaches.");

    if unaccountable.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The rule of three, rendered.
///
/// Zero observations in `n` independent trials supports roughly a `3/n` upper
/// bound at 95% confidence. `n = 0` has no bound at all and says so rather than
/// dividing.
fn bound(n: usize) -> String {
    if n == 0 {
        return "nothing at all (no observations)".to_owned();
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "a percentage printed in a report"
    )]
    let percent = 300.0 / (n as f64);
    format!("{percent:.1}%")
}

/// Generates a signing key and the registry line that accepts it.
///
/// Two files, and the split is the whole of the operational advice this tool
/// gives: `<name>.signing-key` is the secret and never leaves the machine —
/// `.gitignore` refuses it and `ci` fails on a tracked one — while
/// `<name>.public-key` is meant to be published beside a release and kept
/// published for ever after the key is retired, because a retired key that
/// stops being published orphans every replay it sealed
/// (`docs/RISKS.md` R4).
fn keygen(name: &Path) -> ExitCode {
    let key = match SigningKey::generate() {
        Ok(key) => key,
        Err(error) => {
            eprintln!("replay: {error}");
            return ExitCode::from(2);
        }
    };
    let secret = name.with_extension("signing-key");
    let public = name.with_extension("public-key");
    let label = name
        .file_name()
        .map_or_else(|| "server".to_owned(), |name| name.to_string_lossy().into());

    let mut registry = KeyRegistry::new();
    registry.insert(key.verifying(), KeyStatus::Active, label);

    if let Err(error) = std::fs::write(&secret, hex(&key.to_bytes())) {
        eprintln!("replay: {}: {error}", secret.display());
        return ExitCode::from(2);
    }
    if let Err(error) = std::fs::write(&public, registry.encode()) {
        eprintln!("replay: {}: {error}", public.display());
        return ExitCode::from(2);
    }
    println!("replay: signing key  {}", secret.display());
    println!("replay: registry line {}", public.display());
    println!("replay: identity {}", key.verifying());
    eprintln!(
        "replay: the signing key is a secret. It belongs outside this repository, \
         and `.gitignore` and `ci` both refuse a tracked one."
    );
    ExitCode::SUCCESS
}

/// Resimulates a replay, checks its seal, and reports what that establishes.
fn verify(path: &Path, keys: &Path) -> ExitCode {
    let Some(replay) = read(path) else {
        return ExitCode::from(2);
    };
    let registry = match KeyRegistry::load(keys) {
        Ok(registry) => registry,
        Err(error) => {
            eprintln!("replay: {}: {error}", keys.display());
            return ExitCode::from(2);
        }
    };

    describe(&replay);
    println!("replay: accepting {} key(s)", registry.entries().len());

    match replay::verify(&replay, &registry, &Build::current()) {
        Ok(verified) => {
            println!(
                "replay: digest {}",
                hex(verified.final_state_digest.as_bytes())
            );
            println!("replay: outcome {:?}", verified.outcome);
            if verified.retired {
                println!(
                    "replay: sealed by a retired key, which still verifies \
                     (docs/RISKS.md R4)"
                );
            }
            // The one line a reader should take away, and it is deliberately
            // narrower than "this match was played fairly": `docs/SCOPE.md` is
            // explicit that resimulating an authoritative server's own inputs
            // catches a broken server and not a cheating client.
            println!(
                "replay: ok — {} sealed this manifest, the log is the one it names, \
                 and it reaches the state and result it claims. This says nothing \
                 about how anybody played.",
                verified.signer
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("replay: {error}");
            match error {
                VerifyError::UnknownKey(key) => {
                    eprintln!("replay: {key} is not in {}", keys.display());
                }
                VerifyError::Signature => {
                    eprintln!("replay: the manifest is not what that key signed");
                }
                VerifyError::RulesHash { recorded, local } => {
                    eprintln!("replay: recorded under {}", hex(recorded.as_bytes()));
                    eprintln!("replay: this build plays {}", hex(local.as_bytes()));
                }
                VerifyError::SimVersion { .. } => {
                    eprintln!(
                        "replay: this is another build of the same rules, not a \
                         tampered file (docs/RISKS.md R13)"
                    );
                }
                VerifyError::Truncated { claimed, found } => {
                    eprintln!("replay: the log is {} input(s) short", claimed - found);
                }
                VerifyError::InputLog { claimed, computed } => {
                    eprintln!("replay: claimed  {}", hex(claimed.as_bytes()));
                    eprintln!("replay: computed {}", hex(computed.as_bytes()));
                }
                VerifyError::FinalDigest { claimed, computed } => {
                    eprintln!("replay: claimed  {}", hex(claimed.as_bytes()));
                    eprintln!("replay: computed {}", hex(computed.as_bytes()));
                }
                VerifyError::Outcome { .. } => {
                    eprintln!(
                        "replay: the result this file asserts is not the one its own log produces"
                    );
                }
            }
            ExitCode::FAILURE
        }
    }
}

/// Prints a replay's manifest without checking anything about it.
///
/// Separate from `verify` on purpose. A tool that printed a manifest and a
/// verdict in one breath would invite a reader to believe the manifest because
/// the verdict was there; this one prints a manifest and says, in the last line,
/// that it has checked nothing.
fn inspect(path: &Path) -> ExitCode {
    let Some(replay) = read(path) else {
        return ExitCode::from(2);
    };
    describe(&replay);
    println!(
        "replay: inspect checks nothing. The signature, the log and the outcome \
         are unverified; run `replay verify <replay> <keys>`."
    );
    ExitCode::SUCCESS
}

fn read(path: &Path) -> Option<Replay> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("replay: {}: {error}", path.display());
            return None;
        }
    };
    match Replay::decode(&bytes) {
        Ok(replay) => Some(replay),
        Err(error) => {
            eprintln!("replay: {}: {error}", path.display());
            None
        }
    }
}

fn describe(replay: &Replay) {
    let manifest = &replay.manifest;
    println!("replay: match {}", manifest.match_id);
    println!("replay: sealed by {}", manifest.server_identity);
    println!("replay: seed {:#018x}", manifest.seed);
    println!("replay: ticks {}", manifest.ticks);
    println!("replay: inputs {}", replay.inputs.len());
    println!("replay: rules {}", hex(manifest.rules_hash.as_bytes()));
    println!(
        "replay: sim {}.{}.{} at {}",
        manifest.sim_version[0],
        manifest.sim_version[1],
        manifest.sim_version[2],
        manifest.sim_commit
    );
    println!("replay: started at {} ms", manifest.started_at_unix_ms);
    let participants = manifest.participants();
    println!(
        "replay: participants {}",
        if participants.is_empty() {
            "-".to_owned()
        } else {
            participants
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    println!("replay: claims outcome {:?}", manifest.outcome);
}

/// Destroys everything a participant's withdrawal reaches, and then checks.
///
/// The audit runs here rather than being left to the operator, because a
/// destruction command that reports success without looking is the failure mode
/// the whole mechanism exists to avoid. If anything is left, this exits non-zero
/// having *said what*, and the corpus is in the state the next run repairs.
fn withdraw(root: &str, pseudonym: &str, on: &str) -> ExitCode {
    let corpus = Corpus::open(root);
    let destroyed = match corpus.withdraw(pseudonym, on) {
        Ok(destroyed) => destroyed,
        Err(error) => {
            eprintln!("replay: {root}: {error}");
            return ExitCode::from(2);
        }
    };
    println!(
        "replay: destroyed {} match(es): {}",
        destroyed.matches.len(),
        if destroyed.matches.is_empty() {
            "-".to_owned()
        } else {
            destroyed.matches.join(", ")
        }
    );
    println!(
        "replay: pseudonym mapping {}, consent record {}",
        if destroyed.identity {
            "destroyed"
        } else {
            "not present"
        },
        if destroyed.consent {
            "destroyed"
        } else {
            "not present"
        }
    );
    audit(root, pseudonym)
}

/// Fails, loudly and by name, if anything about a pseudonym is left.
fn audit(root: &str, pseudonym: &str) -> ExitCode {
    let corpus = Corpus::open(root);
    match corpus.audit(pseudonym) {
        Ok(traces) if traces.is_empty() => {
            println!("replay: no trace of {pseudonym} outside its withdrawal record");
            ExitCode::SUCCESS
        }
        Ok(traces) => {
            eprintln!(
                "replay: {} file(s) still mention {pseudonym}:",
                traces.len()
            );
            for path in traces {
                eprintln!("replay:   {}", path.display());
            }
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("replay: {root}: {error}");
            ExitCode::from(2)
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}
