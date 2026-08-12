# MILESTONES

Budget: one developer, ~10 h/week, no deadline. Estimates are in calendar weeks
at that rate. Total to M9: **26–34 weeks**, roughly seven to nine months.

A milestone is reached when its exit criterion is verifiable by running a
command, not by inspection. Detector milestones carry the additional rule from
`SCOPE.md`: **a detector without a corresponding exploit failing against it in
CI is not a delivered detector.**

## Current state

The repository is a generic project template: `README.md`, a devcontainer on a
plain Debian base, a branch-name validation workflow, a super-linter workflow
that auto-commits to PR branches, VS Code settings, Copilot commit-message
conventions. No Rust, no Cargo, no license.

---

## M0 — Toolchain floor and repository hygiene · 1 week

Everything here is cheaper before the first line of game code exists. Nothing
here is deferrable, because each item constrains code written after it.

Work: Cargo workspace skeleton with seven empty crates. `rust-toolchain.toml`
pinning an exact stable version. `Cargo.lock` committed. A single CI workflow —
build, test, `clippy -D warnings`, `rustfmt --check` — on Linux and Windows,
with `Swatinem/rust-cache`. Default workflow permissions set to
`contents: read`, elevated per job only where required. `LICENSE`,
`SECURITY.md`, `CONTRIBUTING.md`.

Replace `super-linter`: it duplicates rustfmt and clippy, is slow, and — the
real problem — holds `contents: write` in order to push commits onto PR
branches. An automation that rewrites your branch while you work is a
maintenance and supply-chain liability for zero benefit on a Rust workspace.
Keep a markdown linter if you want one; drop the rest.

`SECURITY.md` must state that the cheat client targets this project only, that
contributions targeting other games are refused, and where to report a
vulnerability in the server.

**Exit:** on a clean checkout, `cargo clippy --workspace --all-targets -- -D
warnings`, `cargo fmt --check`, and `cargo test --workspace` pass on
`ubuntu-latest` and `windows-latest`. Warm-cache PR CI completes in under
90 seconds. No workflow holds write permissions. `SECURITY.md`, `LICENSE`,
`CONTRIBUTING.md` exist.

## M1 — Deterministic simulation core · 3–4 weeks

Fixed-point type and vector math, RNG with explicit seed, `State`, `Input`,
`step`. One champion, movement, one skillshot, one targeted spell, basic-attack
range, towers. No networking, no rendering.

Determinism is enforced by lints rather than by discipline: `sim` carries
`#![forbid(unsafe_code)]` and denies `clippy::float_arithmetic`, with a
`clippy.toml` disallowed-types list covering `std::time::*`,
`std::collections::HashMap` (its default hasher randomizes iteration order per
process), and `rand`. This is the cheap version of a `no_std` crate and buys the
same property.

Tooling added here: the determinism job, and property tests (`proptest`) over
fixed-point arithmetic and `step` invariants. The determinism job is the one
test that must exist from the first simulation commit — a determinism bug found
late invalidates every recorded replay and the entire anti-cheat premise.

**Exit:** a 1000-tick fixture (fixed seed, fixed input log) produces an
identical `State::digest()` on `ubuntu-latest` x86-64, `windows-latest` x86-64,
and `macos-14` aarch64 in CI. `sim` has no dependency on any other workspace
crate. No type in `sim` other than the view types implements `Serialize`.
Property tests show fixed-point ops neither panic nor overflow across the legal
input domain.

## M2 — Visibility projection and fog · 1–2 weeks

`view_for(&State, PlayerId) -> PlayerView` as a separate module, plus the view
types. Vision sources: champions, towers. `step` never reads visibility.

**Exit:** across the M1 fixture, for every tick and every player, no `EntityId`
outside that player's vision appears anywhere in `view_for`'s output — including
in derived events (damage, casts, sounds). A size assertion bounds the encoded
`PlayerView` so that an accidental full-state leak fails the test rather than
merely inflating the packet.

## M3 — Server, protocol, three clients · 4–5 weeks

Transport, framing, protocol versioning, session lifecycle, the tick loop, input
intake, fog application before send, replay recording. Headless clients only:
input scripts in, digests out. `server` is a library with a thin binary so the
exploit suite can boot it in-process.

Tooling added here: `cargo-deny` (licenses, bans, sources on PR; advisories on a
weekly schedule so an unrelated new CVE never blocks an unrelated PR), and
Renovate — see `ENGINEERING.md` for why it arrives now and not at M0.

**Exit:** three headless clients join one match, run 1000 ticks of scripted
input, and (a) all three report identical digests of their reconciled local
view at every checkpoint tick, (b) the server's authoritative digest matches an
offline resimulation of the recorded input log, run as a separate process.

## M4 — Playable client · 4–6 weeks

Rendering, input capture, prediction and reconciliation, enough UI to play. This
is the largest and least interesting milestone; it exists because behavioral
detection needs human matches and human matches need a playable game.

**Exit:** three humans play a 3v3 match end to end on two operating systems; the
match writes a replay; `replay verify` resimulates it to the server's final
digest.

## M5 — Replay integrity · 2 weeks

Replay container format with a version stamp and a rules hash, signing, and
verification. Decide and document what is signed — the input log alone is not
enough, see `RISKS.md`.

**Exit:** a table-driven test covers six tamper cases — truncated log, reordered
inputs, altered outcome record, altered seed, unknown signing key, version or
rules-hash mismatch — each rejected with a distinct error, and a genuine replay
accepted. This is exploit class 2, and its exploits live in the cheat crate.

## M6 — Human match corpus · 2 weeks of work, calendar-bound

This milestone gates every behavioral detector and it is bound by wall-clock
availability of other people, not by your hours. **Start recruiting during M4.**

Work: a consent record and its text, a pseudonymous player identity scheme, a
documented telemetry schema (client-claimed timestamp *and* server arrival
timestamp for every input — see `RISKS.md` on why raw input telemetry is
personal data), a recording harness, and a held-out split fixed before any
detector is written.

**Exit:** at least 40 recorded matches from at least 6 distinct people, each
with a consent record; a documented schema; a frozen train/holdout split; and a
published summary statistic set. Whether the raw corpus can be published at all
is decided here, not later.

## M7 — Cheat client and exploit classes 1, 4, 5 · 3 weeks

The cheat crate and its harness: boot a server in-process, connect an adversarial
client, assert a property. Then the exploits whose defenses already exist, so
that the defenses stop being claims:

- Maphack against a build with culling disabled (`--features no-culling`), to
  prove the exploit is real and the culling is what stops it.
- Traffic analysis: recover the number of nearby entities from message sizes and
  arrival times against unpadded messages. This one has no defense yet; it
  motivates padding, which lands here.
- Clock manipulation and protocol abuse: sequence replay, concurrent session
  commands, out-of-order inputs, claimed-timestamp skew.

**Exit:** `cargo test -p cheat-client` passes on the default build and fails on
the deliberately weakened build for every exploit, with each test asserting one
named property. CI runs both configurations.

## M8 — Behavioral detection · 4–6 weeks

Exploit class 3, and the first genuinely measurable anti-cheat result. Write the
bot first: a scripted agent that plays through the real protocol, then variants
that add human-plausible noise. Then the detectors.

Candidate signals, each with a null model that can be stated in one sentence:
input inter-arrival distribution and quantization, reaction latency floor,
aim-correction trajectory curvature, claimed-versus-observed timestamp drift,
and account progression coherence across matches.

Calibration honesty is the deliverable here. With a corpus of N ≈ 40 human
matches and zero observed false positives, the supportable claim is an upper
bound of about 3/N ≈ 7% at 95% confidence, not "0% FPR". Every detector document
states the bound.

**Exit:** per detector, a page in `docs/detectors/` giving the null model, the
threshold and its justification, the score distribution over the human corpus
and over the bot corpus, the observed FP/FN counts, and the confidence bound.
The threshold is chosen at zero false positives on the corpus. The matching bot
variant is detected in CI, and the detector ships only if that CI test exists.

## M9 — Release pipeline · 1–2 weeks

Deliberately last. Nothing before M4 is worth distributing, and release
automation built before there is anything to release is automation you maintain
for free.

Work: `release-plz` for version bump, changelog, and GitHub Release (with
`publish = false` on every crate — nothing here belongs on crates.io);
multi-platform binaries for client and server; a distroless non-root amd64
server image on ghcr.io; SBOM; provenance attestation.

**Exit:** pushing a tag produces, with no further manual step, signed checksums
for Linux and Windows binaries of both client and server, a published container
image that runs as non-root, an SBOM attached to the release, a provenance
attestation, and a changelog entry generated from conventional commits.

---

## Tooling placement summary

| Item | When | Why then |
| --- | --- | --- |
| Pinned toolchain, CI build/test/clippy/fmt, `Cargo.lock`, minimal workflow permissions | M0 | Constrains all code written after; retrofitting a clippy gate onto an existing codebase is a week of noise |
| Devcontainer converted to a Rust base | M0 | Already exists, cheap to fix, and it must not become the toolchain source of truth — `rust-toolchain.toml` is |
| Determinism job across three targets | M1 | The single property everything else rests on |
| Property-based testing | M1 | Fixed-point arithmetic is exactly where proptest pays |
| `cargo-deny` | M3 | Meaningless with zero dependencies |
| Renovate | M3 | Same |
| Coverage | M8, unGated | See below |
| `release-plz`, SBOM, provenance, Docker | M9 | Nothing to release before then |
| Reproducible builds | Never | See `ENGINEERING.md` |
| `cargo-audit` | Never | `cargo-deny`'s advisories check reads the same RustSec database; running both is one more automation for zero information |

Coverage: a percentage gate on PRs pushes you to test getters. What you actually
want to know is which detector branch is untested, and you want to know it once
a month. So: `cargo llvm-cov` on a weekly schedule, producing an artifact, with
no PR gate and no third-party service (which would mean a token, an upload step,
and a flaky external dependency in your critical path). Promote it to a gate only
if you catch yourself shipping an untested detector.
