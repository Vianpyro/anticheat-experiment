# RISKS

Decisions that are irreversible, or whose cost to reverse grows superlinearly
with time. Each entry states what makes it hard to undo, when it must be taken,
and the cheapest hedge available now.

Ordered by cost of getting it wrong.

---

## R1 — Floating point anywhere in `sim`

**Irreversible because:** cross-platform bit-identical results are the load
bearing property of this project. Replays, the determinism suite, resimulation,
and the RL environment all rest on it. Discovering an `f32` in the physics after
the corpus is recorded invalidates every recorded match, every replay, and the
detector calibration derived from them — not just the code.

**Decide:** first commit of `sim`, M1.

**Hedge:** enforce mechanically rather than by review. `#![forbid(unsafe_code)]`
plus `deny(clippy::float_arithmetic)` in `sim`, and a `clippy.toml`
disallowed-types list covering `f32`, `f64`, `std::time::*`,
`std::collections::HashMap`, and `rand`. Run the tri-platform determinism job
(x86-64 Linux, x86-64 Windows, aarch64 macOS) from the first simulation commit,
not once the game is playable. The aarch64 target is not optional: it is what
catches the leaks x86-only CI hides.

## R2 — Fixed-point representation and tick rate

**Irreversible because:** they are baked into every replay file and every corpus
match. Changing the fractional-bit count or the tick rate silently changes
trajectories, so old replays no longer resimulate and old human matches no
longer describe the game you now have. The corpus is the expensive artifact, and
it is the one that dies.

**Decide:** M1, before the first fixture is committed.

**Hedge:** a `rules_hash` in the replay header covering the fixed-point
parameters, tick rate, and balance constants. Verification refuses a mismatch
loudly instead of resimulating into garbage. That converts a silent corruption
into a clean "this replay belongs to another version", which is survivable.

## R3 — Personal data in the input corpus

**Irreversible because:** high-resolution input telemetry is behavioral
biometrics. Under GDPR it is personal data, and once it is collected without a
consent record you cannot lawfully publish or redistribute it. Worse, a public
repository makes publication irreversible in the literal sense: git history and
forks. Retroactive consent from people who played six months ago is, in
practice, not obtainable.

**Decide:** before the first recording session, i.e. during M4, not at M6.

**Hedge:** pseudonymous player identifiers with the mapping held outside the
repository; written consent text covering the specific uses (detector
calibration, publication of derived statistics, and — separately opted into —
publication of the raw corpus); the corpus distributed as a release asset or a
separate repository, never committed to git history, because deleting a
committed file does not delete it. Default to publishing derived statistics
only, and treat raw-corpus publication as a separate opt-in decision.

## R4 — What the replay signature actually covers

**Irreversible because:** the signature's coverage defines what a replay proves.
Sign only the input log and a legitimate log can be resubmitted under another
match identity; omit the server identity and any party can mint replays; omit a
nonce and replays are indistinguishable from their own copies. Once replays are
published or used as evidence, the format is a compatibility surface and
widening the signature retroactively means every old replay is unverifiable.
Key rotation has the same shape: rotating without a key registry orphans every
replay signed by the old key.

**Decide:** M5, before any replay leaves the machine that produced it.

**Hedge:** sign a manifest — match id, server identity, seed, `rules_hash`,
start time, participant pseudonyms, the input log digest, and the final state
digest — rather than the log alone. Version the manifest from day one. Publish
the public key alongside releases and keep every retired key published.

## R5 — Serializability of `State`

**Irreversible in practice because:** the moment one debug endpoint, one
`#[derive(Serialize)]` for a test fixture, or one `Debug`-dump-to-disk helper
exists, the maphack guarantee degrades from a compile-time property to a habit —
and habits are what a project revisited every few months loses first. Removing
it later means auditing every path that touches `State`.

**Decide:** M1, as a stated invariant with a test.

**Hedge:** `State` never derives `Serialize`; determinism tests compare
`State::digest()` rather than encoded bytes; replay storage holds seed and inputs,
never snapshots. A CI check greps for a serialization derive on the state types.

The two pressures that will push back — mid-match reconnection and test fixtures
— are answered in advance in `ARCHITECTURE.md`, "The `State` escape hatch",
rather than left to be improvised the week they bite: reconnection resends a
`PlayerView` and transports no state, fixtures are `(seed, inputs)` and are built
by simulation through the public API, and the only direct constructors are
`#[cfg(test)]`-gated inside `sim`.

A `dev-snapshot` feature is deliberately **not** that escape hatch. Cargo
features are additive and unified, so "a feature the server binary cannot enable"
is not something Cargo enforces. If replay seeking ever genuinely needs
snapshots, this risk is reopened as its own decision, and the reopening has to
bring a CI check on the server binary's resolved feature graph with it.

## R6 — Transport choice

**Irreversible because:** it shapes reconciliation, the fidelity of arrival
timestamps (your only trustworthy clock), and how much of exploit class 5 is
solved beneath your application. Changing it after M4 means rewriting the
session layer and re-recording the corpus, since timing telemetry changes
distribution.

**Decide:** M3, at the first line of `protocol`.

**Hedge:** QUIC (`quinn`). Datagrams for state, reliable streams for session
commands, transport-level encryption and anti-replay for free, and no
hand-rolled cryptography — which is the failure mode to avoid in a security
portfolio. The obligation this creates is honesty: `SCOPE.md` and the class 5
documentation must say which protections come from the transport rather than
claiming them.

## R7 — Publishing the cheat client

**Irreversible because:** git history and forks. A public repository containing
working exploit code cannot be un-published, and the reputational framing —
security research versus cheat distribution — is set by what is in the
repository on day one, not by a later README edit.

**Decide:** before the first push of cheat code, M7.

**Hedge:** the boundary is already right: the cheat client speaks only this
project's protocol, contains no generic technique (no memory scanner, no
injector, no hooking library), and is useless against anything else.
`SECURITY.md` states this and states that contributions targeting other games
are refused. Keep the exploits expressed as test assertions rather than as a
usable tool.

## R8 — Corpus size versus detector claims

**Not irreversible, but unrecoverable within the project's budget.** A corpus of
tens of matches cannot substantiate a low false-positive rate, and no amount of
later modelling fixes it. Publishing a "0% false positives" claim on such a
corpus is the single most credibility-damaging thing this project could do —
precisely because the audience is engineers who will check.

**Decide:** M6, when the corpus size is fixed and before any threshold is chosen.

**Hedge:** report bounds, not point estimates (zero observations in N trials
supports roughly a 3/N upper bound at 95% confidence). Prefer detectors with a
stated physical null model over fitted classifiers. Freeze the holdout split
before writing the first detector. Never auto-ban: detectors emit scores and
evidence, humans decide, and this is a design position rather than an unfinished
feature.

## R9 — Nondeterminism smuggled in by a dependency

**Irreversible because:** it is invisible until it is not. An ECS, a math crate
with an SIMD fast path, a hashmap with a randomized hasher, or a parallel
iterator inside `sim` breaks R1 without any float appearing in your code.

**Decide:** M1 for `sim`'s dependency policy; enforced continuously.

**Hedge:** `sim` depends on nothing but a fixed-point crate and, optionally,
`serde` for the view types. Game frameworks are allowed in `client` only. The
determinism job is the detector, and it must run on every change to `sim`, not
on a schedule.

## R10 — Signature of `step`

**Cheap to hedge now, annoying later.** `step(&State, &[Input]) -> State`
allocates a fresh state per tick. At 3v3 and a handful of projectiles this is
irrelevant for the game and for CI; it becomes the bottleneck for the RL
sub-project, which wants millions of steps per hour.

**Decide:** deferrable to the RL sub-project. Do not pre-optimize.

**Hedge:** keep `State` free of owned indirection where it is easy (fixed-size
arrays over `Vec` for players and towers, a bounded arena for projectiles) so
that the future `step_into(&State, &[Input], &mut State)` is an addition beside
the pure signature rather than a redesign of it. Do not add the second signature
until a benchmark demands it.

## R11 — Automation you inherit rather than choose

**Expensive because:** the failure mode of a solo project revisited
intermittently is not too little automation, it is automation you no longer
understand blocking a merge at 11pm. The current `super-linter` workflow is the
live example: it holds `contents: write` and pushes commits onto your branches,
which is both a supply-chain surface and a source of confusing history.

**Decide:** M0, and re-decide each time a tool is added.

**Hedge:** every workflow declares minimum permissions explicitly; every added
automation must be explainable in one sentence and removable in one commit; and
the count stays small enough to hold in your head. Reaching for five
automations you understand over fifteen you endure is a maintenance decision,
not an aesthetic one.
