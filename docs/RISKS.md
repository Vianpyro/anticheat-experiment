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

### The negative control, run 2026-08-12

The hedge above is only worth what the job can actually detect, and until this
date nothing had established that. The three targets agreed on the first run of
M1 and on every run since; a job that would stay green on a real divergence and
a job with nothing to report look identical from the outside. So the divergence
was manufactured once, deliberately, on the throwaway branch
`experiment/negative-control-aarch64` (pull request #3, closed unmerged, branch
deleted).

**The operation.** `f64` libm transcendentals — `sin`, `cos`, `tan`, `powf` —
evaluated on champion positions in `step`, with their raw bits folded into the
generator's state. The generator is read by no rule, so all three targets
simulated exactly the same match and only the digest moved: any disagreement
observed is the floating point and nothing else. `clippy::float_arithmetic` and
`clippy::disallowed_types` were lifted on that one statement with a comment
naming the branch.

**What diverged.** All three targets produced three *different* digests, at the
first checkpoint (tick 100) and in the duel fixture, and the `fixture` job went
red on all three. A companion test printing the raw bits of individual functions
attributes it: `f64::tan` differs by one unit in the last place, and it differs
three ways.

| `x` | `tan(x)` glibc / x86-64 Linux | MSVC / x86-64 Windows | Apple libm / aarch64 |
| --- | --- | --- | --- |
| `0.1` | `3fb9af8877430b80` | `3fb9af8877430b80` | `3fb9af8877430b7f` |
| `123.456` | `3ff5a0fe5da94891` | `3ff5a0fe5da94890` | `3ff5a0fe5da9488f` |
| `98765.4321` | `3fa5a5ef83cff794` | `3fa5a5ef83cff794` | `3fa5a5ef83cff793` |

`sin`, `cos`, `exp`, `ln` and `powf` agreed bit-for-bit on every sampled point.

**What this establishes, and what it does not.** The job reports a divergence
rather than absorbing it, and it distinguishes all three targets rather than
only detecting that something moved. That is the property that was missing.

It does *not* isolate "aarch64 disagrees with x86-64" as a separate phenomenon,
and the expectation going in — the two x86-64 targets agreeing while `macos-14`
stood apart — was wrong. The two x86-64 targets link different libms, so
Windows diverged from Linux on the same operation. The honest statement is
narrower than "the second architecture caught it": what the matrix catches is a
*per-platform* disagreement, of which a per-architecture one is a special case.
Basic IEEE-754 arithmetic — `+`, `-`, `*`, `/`, `sqrt` — is exactly specified
and does not differ between these targets at all; the divergence lives in libm
and in anything a compiler is free to contract. That is a narrower attack
surface than R1's framing suggests, and it is the reason the mechanical defence
is the *type* (`Fx`, integers only) rather than the matrix. The matrix is the
detector of last resort, and it now demonstrably works.

## R2 — Fixed-point representation and tick rate

**Irreversible because:** they are baked into every replay file and every corpus
match. Changing the fractional-bit count or the tick rate silently changes
trajectories, so old replays no longer resimulate and old human matches no
longer describe the game you now have. The corpus is the expensive artifact, and
it is the one that dies.

**Decide:** M1, before the first fixture is committed.

**The roster and the map belong to this risk too, and they were changed at M3
for that reason.** A match is nine seats on a triangle now, not six on a lane.
Nothing in the corpus died, because there is no corpus yet: M4 is the first
recording session and M5 the first replay anyone keeps. Made after either of
those, the same change would have invalidated every recorded human match and
every replay, for a reason no verifier could report as anything but a digest
mismatch. It was taken at the last moment at which it destroyed nothing, and
that timing is the whole of the argument — `MILESTONES.md` says so where the
milestone records it.

**Hedge:** a `rules_hash` in the replay header covering the fixed-point
parameters, tick rate, and balance constants. Verification refuses a mismatch
loudly instead of resimulating into garbage. That converts a silent corruption
into a clean "this replay belongs to another version", which is survivable.
What `rules_hash` does *not* cover is the code that reads those constants; that
is R13.

## R3 — Personal data in the input corpus

**Irreversible because:** high-resolution input telemetry tied to an account is
behavioral biometrics, and it is personal information. The project is operated
from Quebec, so **Quebec's Law 25** is the governing regime (the GDPR would
reach the same conclusion for a participant in the EU). Once the data is
collected without a lawful consent record you cannot afterwards publish or
redistribute it, and a public repository makes publication irreversible in the
literal sense: git history and forks. Retroactive consent from people who played
six months ago is, in practice, not obtainable.

**Pseudonymisation is not the answer.** Law 25 draws the line at anonymisation,
which must be irreversible and performed according to generally accepted best
practices; pseudonymised data — data that can be re-associated with a person
using information held separately, which is exactly what a pseudonym mapping is
— remains personal information and carries every obligation attached to it.
Replacing a name with an opaque identifier is a security measure, not a change
of legal category, and input timing distributions are themselves distinctive
enough that "the mapping is in another file" is a thin claim.

Whether the private-sector Act formally binds a non-commercial hobby project is
genuinely unsettled, and the honest position is that it does not matter: the
project holds itself to the regime regardless, because the cost is a page of
text and the alternative is a security portfolio that collects behavioral
biometrics from friends with no stated rules. This document is engineering, not
legal advice.

**Decide:** before the first recording session, i.e. during M4, not at M6.
M4's own exit criterion — three humans playing a match — is already a recording
session, so the regime must exist before that criterion is run.

**Hedge:** a written consent text stating four things, which is where the
substance lives (`MILESTONES.md` M4): the declared purpose, the retention
period, how consent is withdrawn, and what withdrawal actually destroys.
Consent is requested per purpose, so publication of the raw corpus is a separate
opt-in that can be refused without refusing detector calibration. Pseudonymous
identifiers with the mapping held outside the repository. The corpus distributed
as a release asset or a separate repository, never committed to git history,
because deleting a committed file does not delete it. Default to publishing
derived statistics only.

One distinction worth recording, because it changes what is owed: this corpus is
collected to calibrate detectors, never to verify or confirm anyone's identity.
That is what keeps it outside the biometric-database regime of the *Act to
establish a legal framework for information technology* (arts. 44–45), which
would otherwise require disclosure to the Commission d'accès à l'information
before the database is brought into service. If the project ever uses input
biometrics to authenticate a player — for account-sharing detection, say — that
obligation attaches and this risk is reopened.

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

### Taken at M5, with two things the hedge did not name

The manifest is what is signed and it carries every field above, plus three the
hedge did not ask for and the work found it needed: the **outcome**, because that
is the claim a forged replay exists to make and a field is what lets resimulation
contradict it; the **input count**, because it is what makes a shortened log a
different answer from an altered one; and the **tick count**, because a verifier
that took the match's length from the log would be taking it from the part an
attacker shortens. The signed bytes are the magic, the format and the manifest,
so a file cannot be re-labelled as another format's and re-parsed under different
rules while keeping a signature that verifies.

**Key rotation is implemented as this entry demands and the demand is the
unobvious half.** A retired key still verifies what it sealed; the registry
records its status and `verify` reports it. The tempting reading of "retired" is
"refused", and that reading destroys evidence by housekeeping — every replay
signed with the old key becomes unverifiable the day it is rotated.
`a_retired_key_still_verifies_what_it_sealed` is the assertion.

**The signing key gets the same treatment as the corpus, for a different reason.**
`.gitignore` refuses `*.signing-key` and `ci` fails on a tracked one: whoever
holds it can seal a manifest this project's own verifier accepts, so a committed
key is a committed authority to mint evidence, with R3's irreversibility — history
and forks. The **public** half is deliberately not refused, because this entry
requires it to stay published.

**And the limit this hedge cannot reach, stated because a table of eight
refusals invites a reader to conclude more.** An attacker who holds a key the
registry accepts, and who adjusts every field consistently, has produced a replay
of a different match, honestly simulated. Nothing in the bytes distinguishes it
from one that was played, because nothing in it is false. What lies past that
point is key custody, not verification, and
`the_escalation_ends_where_key_custody_begins` executes it rather than leaving it
to a reader's charity.

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

That grep was true by vacuity until M2 — `sim` had no serialization dependency,
so nothing in it *could* derive one, and the check had never rejected anything.
It has now been exercised: a `#[derive(Serialize)]` placed on `State` and then
removed produced `a serialization derive reached sim outside the view types
(docs/RISKS.md R5)`, naming the line. The view types arrived with M2 and are
excluded from the grep by path, and they carry a hand-written encoding rather
than a derive, so `sim`'s `[dependencies]` table is still empty — which is the
stronger statement, and is itself now asserted by `cargo tree` in CI.

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

**Hedge:** QUIC (`quinn`). Transport-level encryption and anti-replay for free,
and no hand-rolled cryptography — which is the failure mode to avoid in a
security portfolio. The obligation this creates is honesty: `SCOPE.md` and the
class 5 documentation must say which protections come from the transport rather
than claiming them.

### Taken at M3. The hedge was overruled by arithmetic, and then restored by it

The hedge said "datagrams for state, reliable streams for session commands", and
that is what the transport does — but it did not, for most of M3, and the round
trip is the useful part of this entry.

**What overruled it.** A padded `View` frame was 1501 bytes: a three-byte header
plus a bound derived from the widest view the type could produce, whose two
largest terms were the projectile arena (32 × 19 = 608) and the *tick's* event
buffer (48 × 15 = 720). A QUIC datagram is bounded by the path MTU, near 1200,
so the frame did not fit in one and everything moved to a single bidirectional
stream per session. The traffic-shape invariant survived — a constant number of
bytes at a constant period is packetised into a constant number of packets — and
what it cost was head-of-line blocking: one lost packet stalls every frame
behind it, at 30 Hz, while the client predicts. That is real netcode and the
wrong trade for anything a client predicts from, and it was recorded here as the
honest price of a bucket sized for a worst case.

**What restored it was asking whether that worst case was reachable.** It was
not, on the half that mattered. `MAX_EVENTS` is what a *tick* can record; a
*frame* does not have to carry all of it, because an event held back for one
frame arrives a thirtieth of a second later rather than not at all. So the view
gained an event budget of its own — `MAX_EVENTS_PER_VIEW`, sixteen — with a
per-recipient queue that defers the overflow in rule order, and the frame fell
to 1096 bytes — 1102 since M4 put the input acknowledgement prediction needs
beside the view. It travels as **two datagrams of 558 bytes**, each far inside
any path MTU, and the session's own messages keep the reliable stream, which is
the hedge as written.

The invariant is stated more directly than it was rather than more weakly: it
used to be an argument about QUIC's packetiser, and it is now a constant number
of datagrams of a constant size at a constant period, carried by the type that
produces them. Per player the traffic *fell*, from 360 kbit/s to 268.
`ARCHITECTURE.md` carries the arithmetic under "The padding budget", including
what is still not taken and why: the projectile arena stays at 32 because
shrinking it to the occupancy the game's cooldowns actually permit would be
sizing the bucket to the observed maximum, and capping the entity list would
trade a length channel for a content channel.

**What it costs now, stated because it is a real cost and a different one.**
State delivery is unreliable. A client can miss a tick: a frame with a shard
missing is abandoned when a newer frame starts arriving, and a view older than
the one already applied is discarded. Both are counted rather than silent. M3's
exit criterion said "identical digests at every checkpoint tick", which was a
claim about a transport that retransmitted; it now asserts that no two clients
ever disagree on a checkpoint they both received and that essentially everything
was delivered. `MILESTONES.md` records the weakening. Trading "a lost packet
stalls everything" for "a lost packet costs one tick" is the trade a 30 Hz game
wants, and the criterion had to follow the transport rather than the other way
round.

The honesty obligation, discharged: packet-level replay and reordering are
rejected by QUIC and not by this project — and with state on datagrams, QUIC
does not *reorder* them either, it simply delivers what arrives, which is why
the client counts what it discards. What is left for exploit class 5 is the
application-level residue — idempotent session commands, and input sequence
numbers that must be strictly increasing — and that lives in `Match::deliver`.
The certificate is self-signed and handed to clients out of band, which they
trust exactly; there is deliberately no verifier that accepts anything.

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

In practice `sim` has taken neither of the two allowances: the fixed-point type
is written out, and the M2 view types encode themselves by hand rather than
deriving. So the hedge is enforced at its strongest — `cargo tree -p sim --edges
normal` must print one node, checked in `ci` and exercised against a path
dependency on `protocol`. The permission to add `serde` for the view types
stands and is deliberately unused until a transport picks a codec (M3).

## R10 — Signature of `step`

**Cheap to hedge now, annoying later.** `step(&State, &[Input]) -> State`
allocates a fresh state per tick. At 3v3v3 and a handful of projectiles this is
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

## R12 — Third-party actions pinned by mutable tag

**Cheap now, and it is the one supply-chain surface the project has today.**
Every third-party action in `.github/workflows/` is referenced by tag —
`actions/checkout@v5`, `Swatinem/rust-cache@v2`. A tag is a mutable pointer:
whoever controls the action's repository can move `v5` to any commit, and every
workflow run afterwards executes that commit with whatever token the job holds.
This is not hypothetical for the ecosystem — it is the shape of every action
compromise published so far, `tj-actions/changed-files` in March 2025 being the
widely reported one, where a moved tag exfiltrated CI secrets from thousands of
repositories.

What limits the blast radius here is already in place and worth stating, because
it is why this is R12 and not R1: every workflow declares `permissions:
contents: read`, no job holds write permissions, and the repository has no
secrets. A compromised action today can read a public checkout and poison a
build cache. It cannot push, tag, publish, or steal a credential that does not
exist. That changes at M9, when `release` becomes the first job to hold
`contents: write`, `packages: write` and `id-token: write` — at which point a
moved tag mints signed releases and provenance attestations in this project's
name, and the risk stops being survivable.

**Decide:** M3. Pinning by commit SHA is the fix, and it was deliberately not
taken earlier for the reason stated in `ci.yml`: a SHA pin with nothing to bump
it is a pin that rots into an unpatched action, and the project would trade one
silent failure for another. Renovate arrives at M3 and its
`helpers:pinGitHubActionDigests` preset keeps the SHAs moving, with the weekly
grouped pull request as the review point. Pinning and the thing that updates the
pins land together or not at all.

**Taken at M3.** Every third-party action is now referenced by commit SHA with
its tag in a trailing comment, in the same change that adds `renovate.json`. The
count of third-party actions is still two: the `supply-chain` workflow installs
`cargo-deny` with `cargo install --locked` rather than through an action,
because a tool that reads a lockfile is not worth a third supply-chain surface.

One thing this does **not** yet establish, and it is the same shape of gap the
determinism matrix had before its negative control: Renovate has not run. The
configuration is committed and the preset is named, but until the app is
installed on the repository and its first weekly pull request appears, "the pins
are maintained" is a claim about a file rather than an observation. If that
first pull request never arrives, the pins are on the clock `ENGINEERING.md`
already describes — delete Renovate and the SHA pins must be replaced by tags in
the same commit, so the pins never outlive the automation that maintains them.

**Hedge until then:** the count of third-party actions stays at two, both from
widely used publishers; no job gains a write permission before M9; and the M9
release workflow does not merge unless its actions are SHA-pinned, which is a
precondition on that milestone rather than a hope. If Renovate is ever deleted
as noise (`ENGINEERING.md` offers that exit), the SHA pins must be replaced by
tags in the same commit, so the pins never outlive the automation that maintains
them.

## R13 — Two builds that agree on `rules_hash` and disagree on the match

**Irreversible in the same way R2 is, and less visible.** `rules_hash()` covers
the constants. It does not cover the code that reads them. Swap steps 5 and 6 of
`step` — projectiles before towers becomes towers before projectiles — and every
constant, every hash and every type is unchanged while a champion who survived a
tick now dies in it. A replay recorded before the change resimulates to a
different final digest afterwards, and the verifier reports a digest mismatch
with no way to say which of the two builds was right. The corpus outlives the
code that produced it, so this is discovered long after the change that caused
it.

**Decide:** M1 for the mechanism, M5 for what the manifest carries.

**Hedge, in two parts, and the second is the honest one:**

*A version the compiler cannot forget.* `sim` owns its version rather than
inheriting the workspace's, and the `sim-version` job refuses a pull request that
changes anything under `sim/` without raising it. A number bumped by convention
is a number forgotten on the Tuesday it matters — the same failure as a field
missing from the digest, minus the compiler. The manifest at M5 carries this
version alongside `rules_hash`, and `verify` rejects a mismatch as its own error
case rather than as a digest mismatch, because "this replay is from another
build" and "this replay was tampered with" must not look alike.

*A commit hash, because the version is a claim and the commit is a fact.* The M5
manifest also records the git commit the server was built from. The version says
"something changed"; only the commit says *what*, and it is the difference
between a verification failure you can bisect and one you can only file. It is
stamped by the release build, so a locally-built server records the commit it was
built from and an unknown or dirty tree records that it was one.

**The imperfection, stated rather than hidden.** This mechanism is weaker than
the digest's exhaustive destructuring and it is worth being precise about where:

- It catches a *changed file*, not a changed behaviour. A comment-only edit
  demands a bump it does not need, and the response to friction like that is
  usually to weaken the check. Keep it: a spurious patch bump costs a line.
- A change that perturbs the simulation *without touching `sim/`* — a compiler
  upgrade, a lockfile change, a profile flag — moves no version at all. That is
  what the tri-platform fixture and its committed digests are for, and the
  determinism job's path filter covers exactly those inputs.
- Nothing forces the *size* of the bump to match the size of the change, so a
  minor-vs-patch judgement remains a judgement.
- Recorded in a manifest signed by the server, the version and the commit are
  only as trustworthy as the server (R4). They order *this project's own*
  builds. They are not evidence against an attacker who controls the build, and
  no claim in this repository should read as though they were.

The alternative — hashing the compiled `sim` artifact, or the source tree — was
considered and rejected: it is defeated by any build-path or debug-info
difference, which puts it in the reproducible-builds swamp `ENGINEERING.md`
declines to enter, and it would report a difference between two builds of
identical source. A version plus a commit is a weaker guarantee that is actually
true.

### Taken at M5, and the version stopped being a number somebody types

`sim::VERSION` is read from Cargo's own environment at compile time rather than
written out in `replay`, so the crate manifest is the source of truth and there
is no second copy of the digits to forget. `replay`'s build script stamps the
commit from `git rev-parse HEAD` and `git status --porcelain`, so a binary
carries the commit it was *built* from rather than whatever the machine it runs
on has checked out — and a tarball with no `.git` produces `Unknown`, which is
this entry's "a manifest that lies about provenance is worse than one that admits
it" in the one place it can be enforced.

Both are their own `VerifyError`, checked in order after the signature and before
anything is resimulated, so a replay from another build costs nothing to reject
and reports why. `replay/tests/sealed.rs` demonstrates it against a real file
rather than a constructed one: the committed cross-platform fixture pins its
version field at `0.0.0`, which no build has, so verifying it as *this* build
must fail with `SimVersion` — and that assertion is stable across every bump this
entry's mechanism will ever demand.

## R14 — Input fidelity is a property of the client, and the corpus inherits it

**Reopened and re-decided before M5, and the entry below the fold is kept as it
was written.** The decision it records was taken at M4 and reversed here; the
reversal is worth more than a rewrite, because what R14 got wrong is the
instructive part and a risk register that quietly agrees with itself is not
evidence about anything.

### What R14 said, and which half of it was true

R14 said aim was quantised to a character cell, that this killed the
aim-curvature detector, and that **everything timing-shaped was untouched**. The
first two are right. The third is wrong, and reading a real SGR trace is what
said so:

- **A terminal reports the pointer only when it crosses into a new cell.** The
  sampling *rate* is therefore a function of the pointer's speed — an event per
  cell crossed on a fast sweep, almost none on a slow creep. Inter-arrival times
  recorded that way measure how fast the pointer was moving, not the rhythm of
  the hand. That is `MILESTONES.md` M8's *first* candidate signal — "input
  inter-arrival distribution and quantisation" — contaminated at the source, in
  the same corpus R14 declared safe for it.
- **The quantisation is anisotropic, so the loss is directional.** About 190
  columns by 45 rows, and a cell about twice as tall as it is wide: over the
  window the map was drawn to, **1.158 world units across and 4.111 down**, a
  ratio of 3.55. R14 described a coarse grid; it was a coarse grid with a
  preferred direction, which is a bias rather than a loss of precision.
- **The trace carries no device timestamp at all.** An SGR sequence carries
  coordinates and buttons. Time could only ever be the moment the client read
  the byte, with tty and scheduler latency in it.

The error in R14 is not the terminal. It is that R14 reasoned about the
*renderer* — "aim is quantised to a cell" — and then made a claim about the
*capture*, which is a different thing that happened to be implemented by the
same component. The hedge it wrote was correct and narrow: the quantisation
lived in one function, `Camera::world`, and everything downstream carried full
precision. What it missed is that the same component also decided *when* a
sample existed, and no amount of precision downstream recovers a sample that was
never taken.

### What replaced it

A graphical client whose capture path never passes through the renderer;
`ARCHITECTURE.md`, "The client's input path, and the renderer it chose", carries
the library decision, the per-platform timestamp findings and the reopening
criteria. In one line: the telemetry records the raw device delta with a
per-event timestamp, one sample per device event unconditionally, and the
simulation gets a fixed-point world point integrated from the same deltas.

**Measured rather than asserted**, on a stream of 1200 synthesised device
motions emitted at a constant 125 Hz with the pointer's speed changed twentyfold
part way through:

| | slow, 1 count per event | fast, 20 counts per event |
| --- | --- | --- |
| Recorded inter-arrival, median | 7.992 ms | 8.001 ms |
| …mean, standard deviation | 8.000, 0.247 | 8.000, 0.226 |
| The same path through the terminal's arithmetic | 34 reports from 800 events — **4.2 %** | 345 reports from 400 events — **86.2 %** |

The last row is R14's second failure in one number: at the same hand rate, the
terminal's event rate tracked the pointer's speed by a factor of twenty, and the
new path's did not move at all. Spatial resolution over the same run is one
device count — **0.05 world units**, 23 times finer than a cell across and 82
times finer down, with the 3.55 anisotropy gone because the capture no longer
touches a projection.

### What R14 becomes: reduced, and its last live half now measured rather than feared

Three things. The middle one was why this entry stayed open; the measurement
below it is what closed it.

- **The aim-resolution half is closed.** A curvature detector at M8 is now a
  detector that may be written against this corpus. That is a permission and not
  a promise: `SCOPE.md` still reserves "delivered" for a class with an exploit
  failing against it in CI, and nothing here says the detector will work.
- **The timestamp half is quantified and closed as a live risk.** See the
  section below, which is the measurement it was closed on. The substitution
  itself stands and is unchanged: no platform in `ENGINEERING.md`'s matrix hands
  this client a device timestamp through `winit` — the data exists on Wayland and
  macOS and the library discards it, and on Windows it does not exist for raw
  mouse input at all — so the client records the *dequeue* time and
  `client::input::CLOCK` says so in a type rather than substituting silently.
  What has changed is that the residual has a number on it instead of a worry.
- **The general form is what the number is now attached to.** R14 was an entry
  about a renderer. It is an entry about the client: what a corpus can support is
  fixed by what the capture path records, and every claim of the form "this
  signal is untouched" has to name the mechanism that takes the sample, not the
  component that draws the screen.

### The measurement that closed the timestamp half, run 2026-08-13

The number above — a standard deviation of 0.247 ms on a stream emitted at 8 ms
— was taken **on an idle container, with no renderer and no socket**, which is
the condition under which the answer was always going to be flattering. A
corpus is not recorded under that condition. So the measurement was repeated
with the client doing its job, and it reports the tail rather than only the
spread, because a Gaussian jitter of a quarter of a millisecond and an
occasional stall of fifteen are two different things and only one of them
matters: 0.25 ms of Gaussian noise against a human signal whose spread is of the
order of ten milliseconds is a few per cent of the signal and destroys nothing,
whereas one sample in a hundred arriving fifteen milliseconds late looks exactly
like a hand that hesitated, and a detector calibrated on that has been
calibrated on the client's scheduler.

`client/tests/jitter.rs` is the instrument. What is real in it: the rasteriser,
over the mark list of an actual view into an actual 1280×800 framebuffer; a
`quinn` endpoint with a real server ticking at 30 Hz; real datagrams, real
reassembly, real reconciliation; and the thread arrangement `client::gfx::play`
uses, where one thread drains device events and draws while a `tokio` runtime
beside it carries the socket. What is synthesised: the device events, because CI
has no display server and `winit` cannot open a window without one — a thread
emits them on an absolute schedule at 125 Hz and the capture loop stamps each
one as it drains it, which is `Session::device_event`'s first line.

**And the first thing it measured was itself, which is why the number below is a
different number.** The obvious statistic is the recorded *inter-arrival*
distribution, and the first Windows run of this test reported a standard
deviation of 2.356 ms against Linux's 0.039 — with eighteen samples flagged as
platform duplicates. Neither number was about the client. An inter-arrival is the
sum of two things, how regularly the events were produced and how promptly they
were stamped, and Windows' default timer resolution is about 15.6 ms: a producer
asking for 8 ms overshoots, then sends the overdue events back to back, and the
distribution it generates is its own. **A measurement whose instrument is in the
answer is R15 wearing a stopwatch.**

Differencing against a timestamp the producer reads from the same clock removes
that term exactly, and what is left is the delay the capture loop adds — which is
what this residual *is*. That is what the table reports.

| | R14's first run (idle, no renderer, no socket) | Rendering and talking, `release` | Rendering and talking, `dev` |
| --- | --- | --- | --- |
| Samples recorded / emitted | 1200 / 1200 | 1200 / 1200 | 1200 / 1200 |
| Views reconciled, frames rasterised | none, none | 290, 532 | 290, 435 |
| **Added latency, mean** | not isolated | **0.036 ms** | **0.240 ms** |
| **…standard deviation** | not isolated | **0.028 ms** | **0.613 ms** |
| …95th percentile | — | 0.045 ms | 2.034 ms |
| **…99th percentile** | — | **0.058 ms** | **2.468 ms** |
| …maximum | — | 0.944 ms | 3.428 ms |
| Slowest single pass of the capture loop | — | 4.795 ms | 8.067 ms |
| Recorded inter-arrival, standard deviation | 0.247 ms | 0.049 ms | 0.912 ms |

**The conclusion, and it is the one the arithmetic supports rather than the one
the numbers flatter.** In the profile a player runs, the delay this client adds
between an event existing and being stamped has a standard deviation of **28
microseconds**, a 99th percentile of **58 microseconds**, and a worst case over
1200 samples of **0.94 ms**. Against the grandeurs M8 will look for —
inter-arrival distributions and reaction latencies whose human spreads are tens
of milliseconds — that is a fraction of a per cent of the signal, and the tail is
bounded rather than heavy: there is no fifteen-millisecond mode in either
profile. The unoptimised build is an order of magnitude worse and still under a
millisecond of spread, and its tail is bounded by its own frame, which is the
mechanism visible in the two rows at the bottom of the table.

**So the residual is quantified and without consequence for the detectors in
scope, and R14's timestamp half stops being a live risk.** What replaces it is a
test that runs on every pull request and prints the distribution, so a regression
is a number in a log rather than a discovery at M8.

**Three things this does not establish**, because a table is exactly where a
reader stops asking:

- **The synthesised half is not measured.** The kernel input stack and the
  compositor are not in this loop, so the number is a **lower bound** on the real
  residual. What it does cover is the mechanism the worry was actually about: a
  long frame does not delay an event in the kernel, it delays the *drain*, and
  that is in the covered half. The uncovered contribution is a roughly constant
  offset plus its own jitter, and a constant offset is invisible to every signal
  on M8's list, all of which are differences.
- **It is one host, and the assertion is deliberately not.** These are numbers
  from one container. What the test asserts is machine-independent: every event
  produces exactly one sample, no two samples share a timestamp, none is a
  coincident duplicate, and **an event waits at most one pass of the capture loop
  before it is stamped**. That last one is the residual stated as a property
  rather than as a threshold — a latency above one pass is the loop falling
  behind the device, which is the only way this design can write a delay into a
  corpus. Reading the clock once per frame instead of once per event fails it and
  the sample count together, with the frame written into the record as an added
  latency of mean 8.195 ms and maximum 30.3 ms against a slowest pass of 4.8.
- **`evdev` stays refused, and this measurement is why.** A per-platform input
  stack would buy a device timestamp on Linux and nothing on Windows, at the cost
  of a `/dev/input` permission each participant has to be granted and a corpus
  whose timestamps mean different things on the two platforms it is recorded on.
  That was a defensible trade against an unmeasured residual. Against 28
  microseconds it is not a trade at all. **This reopens only if a detector at M8
  turns out to depend on a quantity at the scale of a millisecond**, which none
  of the candidates in `MILESTONES.md` M8 does — and the reopening would then be
  a decision about that detector, taken before M6 or not at all, for the covariate
  reason the paragraph above this one used to give.

**One more thing this found, and it is the reason to measure rather than
argue.** The first run of the new client reported a median inter-arrival of
0.38 ms for a stream emitted at 8 ms. An X11 pointer grab — `Confined`, the
obvious way to keep an invisible OS pointer inside the window — makes the server
deliver every raw motion event twice, five microseconds apart: 50 synthesised
motions produce 50 `DeviceEvent::MouseMotion` without the grab and 100 with it,
measured against `winit` alone. Invisible on screen, and a second mode near zero
in every distribution M8 would read. The grab is gone and
`InputTrace::stats().coincident` now reports the class of fault rather than
filtering it, because filtering would be a predicate on the contents of the
record — which is what the cell crossing was.

---

### The entry as it stood at M4, kept

## R14 (as written at M4) — Aim resolution is a property of the renderer, and the corpus inherits it

**Expensive because the corpus is the artifact.** M4 ships a terminal client, so
a player points with a character cell: at eighty to a hundred and twenty columns
across two hundred and twenty world units, one cell is two to three units, which
is wider than a champion. Every aimed input in the corpus is therefore quantised
to a grid the renderer chose, and no later analysis recovers what the hand was
doing between two cells.

**What that costs, exactly.** `MILESTONES.md` M8 lists *aim-correction
trajectory curvature* among the candidate behavioural signals. A corpus recorded
through this renderer **cannot support a curvature detector at any threshold**,
because the trajectories in it are the grid rather than the player. That is not
a detector that would be weak; it is one whose null model is about a continuous
pointing device that was not present. It must not be written against this
corpus, and M8's document has to say so rather than report a bound.

What is untouched is everything timing-shaped, which is most of M8's list:
input inter-arrival distribution and quantisation *in time*, reaction latency
floor, claimed-against-observed timestamp drift, and account-progression
coherence. None of those reads a coordinate.

**Decide:** M4, because the client is what records the corpus and M6 is when the
recording happens. Reversing it after M6 means re-recording every match, which
is the calendar cost `MILESTONES.md` M6 is already bound by.

**Why the terminal anyway.** The alternative is a graphics stack, a font stack,
a game framework and a display server in CI, for a game `SCOPE.md` calls a
fixture — nine discs, six towers and some projectiles. `ENGINEERING.md`'s bar
for a dependency is a reason a few lines of code would not satisfy, and a
windowed renderer clears it only for the one signal named above. It is one
dependency against four, on the milestone the documents already describe as the
largest and least interesting.

**Hedge, and it is a real one rather than a shrug.** The quantisation lives in
`client::ui::Camera` and nowhere else: `Camera::world` is the only function that
turns a pointer position into a world coordinate, and everything downstream —
the intention, the protocol frame, the recorded log — carries a full-precision
`FxVec2`. A pointing device with sub-cell resolution is therefore a change to
one function and its tests, not to the protocol, the recording format or the
rules. If a curvature detector is ever wanted, the order is: replace `Camera`,
then record, and never the other way round.

*(End of the M4 entry. The hedge above was the one thing R14 got entirely right
and it is what made the replacement cheap: `Input`, `Action`, the protocol
frames, the digest and the recording format did not change by a byte. What the
entry above did not have was any statement about* when *a sample is taken, which
is the half that was wrong — see the top of this section.)*

---

## R15 — A scripted fixture whose antecedent is never reached

**Not irreversible, and it is here because it is the defect this project keeps
producing.** Four times now a test has been green because the condition it was
about never occurred, and each time the discovery was accidental — somebody
changed something nearby and the case finally arrived. A test that stops being
about anything looks, from the outside, exactly like a test that holds; there is
no red, no warning, and no diff. The cost is not the failing assertion, it is
every claim built on top of it in the interval, and the interval is measured in
milestones.

**The four, in the order they were found:**

| # | The fixture | What its antecedent needed | What it did |
| --- | --- | --- | --- |
| 1 | `client/tests/m3_exit.rs`, the `replay` binary | The tool to exist | `cargo test --workspace` does not build binaries no test target needs; it existed locally only because a `--all-targets` build had happened first |
| 2 | `sim::MAX_EVENTS` | To be a bound on a tick | The roster went from six seats to nine, the derivation moved from 48 to 60, and the constant stayed at 48 with the derivation living in prose |
| 3 | `LocalWorld::digest` and M3's criterion | Two teammates on different hit points | M3's scripted match produced no damage; the three walked a lane and nothing touched them |
| 4 | `client/tests/capture.rs` | Two aims that are not both saturated | The fixture walked far enough to hit the clamp, so "two windows give the same aim" was true of any two windows |

**Why the existing defences do not catch it.** Property tests have a defence and
it works: `sim/tests/view_properties.rs` and `server/tests/traffic.rs` both end
in a test whose only job is to sample the generators and assert floors on what
they reach — dead champions, respawns, forks a player cannot tell apart. Nothing
equivalent existed for **scripted** fixtures, and all four instances above are
scripted fixtures. A generator can be measured for what it produces; a script
was assumed to produce what its comment said it produced.

**Decide:** continuously, at the moment a fixture is written.

**Hedge, and it is the same shape as the property-test one.** Every scripted
fixture carries an assertion on what it *reaches* — damage produced, events
emitted, saturations hit, limit cases crossed — printed with the run and set to
fail when the count falls to zero. Not on what the fixture is *for*: on the
antecedent of the assertions stated over it. And the number is printed even when
it passes, because the value of the counter is that a reader sees `26
skillshots, 0 targeted spells` and asks the question.

### The pass that entry made, and what it found

Run over every scripted fixture in the repository, 2026-08-13. Four were hollow,
and three of the four had a doc comment asserting the opposite:

- **M3's exit criterion ran on three champions standing still.** `m3_exit.rs`
  filled the ticks between orders with `Action::Idle` on the reading that a
  client with nothing new to say says nothing. `Idle` is a rule that **stops the
  champion**: the three of them moved on one tick in a hundred and twenty,
  covered **four units of the hundred and seventy-three** their own comment
  describes, never left their base, and never put an entity into or out of
  anybody's fog. The criterion — three clients agreeing — is satisfied by three
  clients who can see nothing but each other.
- **The lost-event tripwire had nothing to trip on.**
  `client/tests/loss.rs::nothing_a_client_was_entitled_to_is_ever_dropped_rather_than_deferred`
  seated three clients and ticked a thousand times **without sending an input**.
  The match produced **zero events**, so `dropped == 0` was a statement about an
  empty match, and an `EventBacklog` that dropped on the first overflow passed
  it.
- **`protocol`'s "busy" state carried no events.** `busy_state()` cast once and
  then stepped twice, and `step` clears the event record at the top of every
  tick — so the state it returned had none, for three milestones, under a doc
  comment reading "and an event from the tick that produced it". Two tests paid
  for it: no `VisibleEvent` ever survived a round trip, and "a view inside the
  budget passes through unchanged" compared two empty lists.
- **No fixture in the repository ever landed a targeted spell.** The scripted
  match issues the order about twenty-seven times at a champion a lane away, and
  the spell has a range, so it lands **none**; the fixture's documentation said
  it exercised "both abilities". One of the game's five actions was executed by
  nothing on any of the three platforms. The duel does land it — six times — and
  that is now a floor rather than an accident.

And one that was thin rather than hollow: **M4's exit match turned for home at
tick 500**, a hundred units into a hundred-and-seventy-three-unit lane, which
stops it thirty units short of Red's tower. It reproduced instance 3 above
exactly — the criterion it shares with M3 was again being checked on three
teammates whose hit points were equal by accident. The turn is at tick 800 now,
the first shot lands at about tick 607, and the run asserts that one of the three
was under fire and the others were not.

**What the hedge does not do**, stated because the temptation is to read a
counter as a proof: a reach assertion says the fixture arrived at the case, not
that the case is the interesting one. It converts "this test is about nothing"
into a red, which is the only failure mode listed above, and leaves "this test is
about the wrong thing" exactly where it was.
