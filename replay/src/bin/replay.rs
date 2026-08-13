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
//! replay withdraw <corpus> <pseudonym> <date>
//! replay audit <corpus> <pseudonym>     # non-zero if anything is left
//! ```
//!
//! Exit status: 0 on success, 1 when the thing being checked is wrong, 2 when
//! the arguments or the file are.

use std::path::Path;
use std::process::ExitCode;

use replay::corpus::Corpus;
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
        ("withdraw", 4) => withdraw(&arguments[1], &arguments[2], &arguments[3]),
        ("audit", 3) => audit(&arguments[1], &arguments[2]),
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: replay keygen <name>");
    eprintln!("       replay verify <replay> <keys>");
    eprintln!("       replay inspect <replay>");
    eprintln!("       replay withdraw <corpus> <pseudonym> <date>");
    eprintln!("       replay audit <corpus> <pseudonym>");
    ExitCode::from(2)
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
