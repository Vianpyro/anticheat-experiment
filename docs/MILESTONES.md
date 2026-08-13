# MILESTONES

Budget: one developer, ~10 h/week, no deadline. Estimates are in calendar weeks
at that rate. Total to M9: **26–34 weeks**, roughly seven to nine months.

A milestone is reached when its exit criterion is verifiable by running a
command, not by inspection. Detector milestones carry the additional rule from
`SCOPE.md`: **a detector without a corresponding exploit failing against it in
CI is not a delivered detector.**

## Current state

**M0, M1, M2 and M3 are reached.** The workspace exists with
its seven crates; the toolchain is pinned; `ci`, `pr-hygiene` and `determinism`
are the only workflows and none holds write permissions; `LICENSE`,
`SECURITY.md` and `CONTRIBUTING.md` exist. The template's super-linter, its
`.dockerignore` and its branch-deleting VS Code task are gone.

`sim` holds the fixed-point type, the seeded generator, `State`, `Input`,
`step`, the rules constants and their hash, and a hand-written SHA-256 behind
`State::digest()`. It has no dependencies at all. Two fixtures — a scripted
1000-tick match, and a shorter one whose job is to kill somebody and which
carries its own `Rules` value to do it — check their digests against constants
committed in the repository.

**What made M1 reached rather than written.** Every digest in the repository was
recorded on x86-64 Linux, and until the `determinism` workflow had run, a green
local test was evidence that the simulation is deterministic *on this machine* —
the claim R1 says is worth the least. It has now run: `ubuntu-latest` x86-64,
`windows-latest` x86-64 and `macos-14` aarch64 report byte-identical digests for
both fixtures against the constants committed here. The second architecture
agreed on the first attempt, which is the outcome to be least smug about; it
means the aarch64 job has caught nothing yet, not that there is nothing for it to
catch.

The `properties` job did fail, on all three targets at once, the first time it
ran at CI's case budget — and not with a counter-example: two properties
discarded about half their samples through `prop_assume!`, and proptest's global
reject cap does not scale with the case count, so raising the budget aborted the
tests instead of running them. A test that stops running looks nothing like a
test that fails. Both are constructions rather than assumptions now, which is
recorded here because it is the argument for raising the budget in the first
place.

**The determinism matrix has since been shown to work.** Agreement on the first
attempt was the outcome to be least smug about, so a divergence was manufactured
on a throwaway branch: `f64` libm transcendentals folded into a field no rule
reads, so that every target simulated the same match and only the digest moved.
The `fixture` job went red on all three with three distinct digests. The full
record, including which function diverged and the expectation that turned out to
be wrong, is in `RISKS.md` beside R1.

**M2 is reached.** `sim::view::view_for` is the projection, `State` carries the
events of the tick that produced it, and `sim/tests/visibility.rs` asserts the
exit criterion over both M1 fixtures — six players, every tick, entity list and
events alike. Two things about that test are the reason it counts as reached
rather than written. It re-derives the visibility predicate instead of calling
`sim`'s, so it is not a function agreeing with itself. And every assertion has a
completeness half, because a culling test that only checks absences passes
against a projection that returns nothing: the run reports 24 822 sightings
withheld, 30 events withheld and 228 delivered, and fails if those numbers
collapse.

**And the fixtures have since been backed by properties.** Two scripted matches
prove the projection right about the world they walk through and say nothing
about the world they do not, so `sim/tests/view_properties.rs` states the same
criterion over states reached by simulation from a drawn seed and a drawn
script: soundness, completeness, that a view is a function of what its player is
entitled to, that the order of the entity list is a function of its content,
that the projection is pure, that vision flips exactly at the radius and nowhere
else, and that widening a radius can only add. Every one of them was checked by
breaking `view_for` on purpose; the one that could not be made to fail — the
encoded-size bound — says so in its own comment rather than passing for
evidence.

They found something on the first run, which is the argument for having written
them: the entity list was ordered by the projectile arena, and the arena
remembers casts the recipient never saw. The fix and the channel it does not
close are in `ARCHITECTURE.md` under the ordering rule.

Two guards that had never rejected anything were exercised rather than trusted.
The `Serialize` grep, given a deliberate `#[derive(Serialize)]` on `State`,
reported `a serialization derive reached sim outside the view types`. `sim`'s
dependency invariant — read from an empty `[dependencies]` table until now — is
a `cargo tree -p sim --edges normal` assertion in `ci`, and adding a path
dependency on `protocol` turns it red.

**M3 is reached.** `protocol` is the wire, `server` is the authority, `client`
is headless, `replay` records and resimulates, and `client/tests/m3_exit.rs`
runs the exit criterion end to end over QUIC in about a second.

Four things about it are worth recording here, because none of them is legible
from the criterion.

**The traffic-shape invariant is where the milestone actually was.** Culling is
worth nothing if message sizes and arrival times report the number of visible
entities anyway. The size half is carried by a type — `ServerFrame` wraps a
fixed array, so a bucketing scheme does not compile — and the cadence half is
the shape of the tick loop: one frame per occupied seat, every tick, whatever
happened. The property with teeth is neither of those: it is that two states a
player cannot tell apart produce byte-identical frames for that player, which
covers the padding, the framing and the handle space at once. `ARCHITECTURE.md`
now carries the padding budget with the numbers in it — 1501 bytes a tick a
player, 360 kbit/s, thirteen times the unpadded mean — instead of the sentence
that used to stand in for them.

**Two side channels were closed, and one of them was not on the list.** The
projectile-handle counter, which M2 recorded as still open, is closed by a
per-recipient handle space with a monotone counter that never reuses. The
tempting alternative — a free list — closes the counting channel and opens a
recycling one, which is written down beside the type so that nobody re-derives
it. The one that was not on the list: `view_for` took a `PlayerId(pub u8)` whose
documented domain was `0..6` and whose real domain was the byte, so the most
sensitive function in the project would have answered an unvalidated seat with a
plausible view. It takes a six-valued `Seat` now, and the validation lives at
the protocol decoder where the byte arrives.

**A property that does not have the teeth it looks like it has.** Naming every
projectile in the arena instead of only the ones a recipient was shown — exactly
the leak the handle space exists to close — passes the byte-equality property at
4096 cases. Its antecedent is full entitlement equality, and a hidden cast
advances a counter without changing anything the recipient is entitled to. A
scripted scenario covers it, and the limitation is documented beside the
property rather than left for a reader to assume away.

**One thing M3 could not build, which M4 has to answer.** Client-side prediction
needs the client to know which of its inputs the server applied to which tick.
Nothing tells it: the server buckets an intention into whichever tick it is
about to run, and `PlayerView` carries no acknowledgement. M4 needs a field or a
message, and its shape is M4's decision rather than one M3 should have made by
accident.

`cargo-deny` and Renovate arrive with the dependency graph they exist for, and
the third-party actions are SHA-pinned in the same change — `RISKS.md` R12 says
the pins and the thing that maintains them land together or not at all.

Next is M4, the playable client, which is the largest and least interesting
milestone and the one that brings the consent regime with it.

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

`view_for(&State, Seat) -> PlayerView` as a separate module, plus the view
types. Vision sources: champions, towers. `step` never reads visibility.

**Exit:** across the M1 fixture, for every tick and every player, no `EntityId`
outside that player's vision appears anywhere in `view_for`'s output — including
in derived events (damage, casts, sounds). A size assertion bounds the encoded
`PlayerView` so that an accidental full-state leak fails the test rather than
merely inflating the packet.

Reached. Two things the criterion did not say, which the work had to decide:

- **Derived events had to be built before they could be culled.** Nothing
  produced them at M1, and an empty event list satisfies the criterion without
  meaning anything. They live in `State`, and therefore under the digest, for
  the reasons in `ARCHITECTURE.md`.
- **The size assertion is a bound plus a comparison, not a bound alone.** With
  six champions and four towers there is no dramatic difference between a leaked
  full state and a legitimate view, so a constant on its own proves little. Each
  view is also encoded a second time under a vision radius wide enough to cover
  the map and required to be no larger than that — a projection that stopped
  culling stops being smaller than an omniscient one, on 5 000+ of the fixture's
  6 000 views.

`view_for` is where the projection is; nothing sends it anywhere yet. The
transport, the constant message size and the constant cadence are M3, and
`ARCHITECTURE.md` is explicit that the culling here is worth nothing without
them.

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

Reached, in `client/tests/m3_exit.rs`, over the real transport. Two things the
criterion did not say, which the work had to decide:

- **What each half proves.** (a) is a statement about the projection and the
  handle space rather than about the network: the three clients are one team,
  vision is a team property, so their local worlds agreeing is evidence that
  nothing per-player leaked and that the handle spaces stayed in step. It would
  not catch a leak the whole team receives. (b) proves the server did not
  corrupt itself — `SCOPE.md` is explicit that resimulating a fully
  authoritative server's own inputs catches a broken server and not a cheating
  client — and what it does establish is that the recording is complete and
  correctly ordered, which is the precondition everything at M5 rests on.
- **It was checked by breaking it.** Team vision replaced by per-player vision:
  `Blue1 disagrees with Blue0 about the world at tick 800`. An input left out of
  the log: `resim` refuses the file. A criterion that has never been red is a
  criterion nobody has verified.

## M4 — Playable client · 4–6 weeks

Rendering, input capture, prediction and reconciliation, enough UI to play. This
is the largest and least interesting milestone; it exists because behavioral
detection needs human matches and human matches need a playable game.

### The consent regime lands here, not at M6

This milestone's own exit criterion is three real people generating input
telemetry, which makes it the first collection of personal information. Under
Law 25 that telemetry is personal information whether or not the account behind
it is pseudonymous, and pseudonymisation does not lower the obligations
(`RISKS.md` R3). Four things are therefore written down, agreed to in writing by
each participant before the first recording, and stated in the consent text
itself:

- **Declared purpose.** Calibrating and evaluating this project's behavioral
  cheat detectors, and publishing statistics derived from that work. Nothing
  else: no identity verification, no transfer to a third party, no reuse by
  another project, no training of anything that outlives this repository.
  Publication of the *raw* corpus is a separate purpose with its own separate
  opt-in, refusable without refusing the rest.
- **Retention.** Raw telemetry, replays containing it, and the pseudonym mapping
  are destroyed 24 months after recording, or on withdrawal, whichever comes
  first. Aggregate statistics that identify no one are kept without a time
  limit, and the consent text says so plainly rather than letting a participant
  infer that everything disappears.
- **Withdrawal of consent.** At any time, without justification and without
  consequence, by a single message to a contact address printed in the consent
  text. Acknowledged within 7 days, carried out within 30. No re-consent is
  needed to withdraw and no reason is asked for.
- **What withdrawal actually destroys.** A match is one interleaved input log
  for six players; deleting one participant's inputs leaves a log that no longer
  resimulates, so surgical removal is not on offer. Withdrawal therefore
  destroys **every match that participant played in, in full**, together with
  their pseudonym mapping — which also means it destroys other participants'
  contributions to those matches. Aggregate statistics already published are not
  retracted. Both consequences are stated before recording, because a
  participant who learns them afterwards was not informed.

**Exit:** three humans play a 3v3 match end to end on two operating systems; the
match writes a replay; `replay verify` resimulates it to the server's final
digest. The consent text exists, states the four points above, and was signed by
all three before the match rather than after.

## M5 — Replay integrity · 2 weeks

Replay container format with a version stamp and a rules hash, signing, and
verification. Decide and document what is signed — the input log alone is not
enough, see `RISKS.md`.

The version stamp is two fields, not one, and `RISKS.md` R13 says why: the
`sim` crate version — already enforced at M1 by a CI check that refuses a change
to `sim/` without a bump — and the commit the server was built from. `rules_hash`
covers the constants, the version and the commit cover the code that reads them,
and a mismatch on either is its own `VerifyError` rather than a digest mismatch.

**Exit:** a table-driven test covers six tamper cases — truncated log, reordered
inputs, altered outcome record, altered seed, unknown signing key, version or
rules-hash mismatch — each rejected with a distinct error, and a genuine replay
accepted. This is exploit class 2, and its exploits live in the cheat crate.

## M6 — Human match corpus · 2 weeks of work, calendar-bound

This milestone gates every behavioral detector and it is bound by wall-clock
availability of other people, not by your hours. **Start recruiting during M4.**

Work: operating the consent regime written at M4 — collecting a consent record
per participant and honouring withdrawal on the stated timeline — a pseudonymous
player identity scheme, a documented telemetry schema (client-claimed timestamp
*and* server arrival timestamp for every input — see `RISKS.md` R3 on why raw
input telemetry is personal information), a recording harness, and a held-out
split fixed before any detector is written.

**Exit:** at least 40 recorded matches from at least 6 distinct people, each
with a consent record naming its retention date; a documented schema; a frozen
train/holdout split; a written destruction procedure that has been executed once
end to end on a discarded test recording; and a published summary statistic set.
Whether the raw corpus can be published at all is decided here, not later, and
only for the participants who opted into that purpose separately.

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
