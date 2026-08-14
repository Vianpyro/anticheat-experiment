# MILESTONES

Budget: one developer, ~10 h/week, no deadline. Estimates are in calendar weeks
at that rate. Total to M9: **26–34 weeks**, roughly seven to nine months.

A milestone is reached when its exit criterion is verifiable by running a
command, not by inspection. Detector milestones carry the additional rule from
`SCOPE.md`: **a detector without a corresponding exploit failing against it in
CI is not a delivered detector.**

## Current state

**M0, M1, M2, M3, M5 and M7 are reached**, and M3's is for a game that is now
3v3v3 on a triangular map. M5 and M7 are out of order deliberately: their criteria
are code — a table of tamper cases and a verifier, then the attacker that runs
against it — and neither needs a person, whereas M4's remaining clause and M6 are
both waiting on a calendar. **M4 is built and
not reached**: everything its
criterion asks for except the three humans on two operating systems runs in
`client/tests/m4_exit.rs`, and that clause is a fact about a calendar rather
than a thing CI can stand in for. The client it will be played on is a window
rather than a terminal, changed after M4 was merged and before M5 was started
for the reason `RISKS.md` R14 now records: the terminal's sampling rate followed
the pointer's speed, so the timing statistics M8 rests on were contaminated at
the source and not, as R14 claimed, untouched. The workspace exists with
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
exit criterion over both M1 fixtures — every player, every tick, entity list and
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
entities anyway. The size half is carried by types — `ServerFrame` wraps a fixed
array and `ServerFrame::shards` returns a fixed array of them, so neither a
bucketing scheme nor a packet count that follows content compiles — and the
cadence half is the shape of the tick loop: one frame per occupied seat, every
tick, whatever happened. The property with teeth is neither of those: it is that
two states a player cannot tell apart produce byte-identical frames for that
player, which covers the padding, the framing, the handle space and the event
backlog at once. `ARCHITECTURE.md` carries the padding budget with the numbers
in it — two datagrams of 555 bytes a tick a player, 266 kbit/s, ten times the
unpadded mean — instead of the sentence that used to stand in for them. (M4 put
five bytes of input acknowledgement beside the view; the numbers there are 558
and 268 now, and `ARCHITECTURE.md` carries the current arithmetic.)

That budget was re-cut once, and the question that re-cut it is worth recording
because it had not been asked: **is the worst case the bound is derived from
reachable?** `MAX_EVENTS` is what a *tick* can record, and a frame does not have
to carry all of it, because an event held back for one frame arrives a
thirtieth of a second later rather than not at all. Giving the view its own
event budget took the frame from 1501 bytes to 1096, which fits two datagrams
under any path MTU — so state travels on QUIC datagrams, the session keeps its
reliable stream, and head-of-line blocking is gone. `RISKS.md` R6 records the
round trip; the entity list was left alone, because capping *that* trades a
length channel for a content channel.

**The match is 3v3v3 on a triangular map, and the timing is the argument.**
Three bases at the vertices, a lane along each edge, each lane contested by
exactly two of the three teams. It landed at M3 rather than later because a
corpus of human matches recorded at six seats is unusable at nine and so is
every replay of M5: this was the last moment at which the change destroyed
nothing. What three teams bought, besides a game that is more than a corridor,
is a side-channel property that two teams cannot even state — a view must
distinguish its two enemies only by what it shows — and it closed the byte-
equality property's known blind spot for free, because hidden activity and equal
entitlement stopped being opposites. What they cost is an exploit class with no
technical defence, which `SCOPE.md` now carries as class 6.

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

**A property that did not have the teeth it looked like it had, and the third
team gave them to it.** Naming every projectile in the arena instead of only the
ones a recipient was shown — exactly the leak the handle space exists to close —
passed the byte-equality property at 4096 cases when there were two teams. Its
antecedent is full entitlement equality, and on a two-team map a fork nobody
could tell apart was almost always a fork in which nothing had happened: the
antecedent and the leak were anti-correlated. With three teams an entire enemy
team can act at a vertex a lane away while the observer's entitlement is
untouched, and the same mutation now fails on the property's first case. The
scripted scenario that was written to cover the gap stays: a property that
happens to reach a channel is evidence about a generator, and a state built to
expose it is evidence about the channel.

**And a property that the two-team format could not express.** With two sides,
"which enemy is this about" has one answer and no view can encode it. With three
it can, so `sim/tests/view_properties.rs` asserts that two states differing only
in *which* of an observer's two enemies performed which of two hidden plays
produce byte-identical views. It was exercised by mutation — a counter kept per
enemy team, leaked into the observer's own cooldowns — and reports `Blue0 can
tell Red having acted from Green having acted, and is entitled to neither`.

**One thing M3 could not build, which M4 has to answer.** Client-side prediction
needs the client to know which of its inputs the server applied to which tick.
Nothing tells it: the server buckets an intention into whichever tick it is
about to run, and `PlayerView` carries no acknowledgement. M4 needs a field or a
message, and its shape is M4's decision rather than one M3 should have made by
accident.

`cargo-deny` and Renovate arrive with the dependency graph they exist for, and
the third-party actions are SHA-pinned in the same change — `RISKS.md` R12 says
the pins and the thing that maintains them land together or not at all.

**And a class of defect was named rather than fixed four more times.** Four times
in this project a test has been green because the condition it was about never
occurred, and each discovery was accidental. `RISKS.md` R15 is that class, the
four instances, and the hedge: every scripted fixture carries an assertion on
what it actually reaches. The pass that entry describes found four hollow
fixtures — including M3's own exit criterion, whose three clients covered four
units of a hundred-and-seventy-three-unit lane and agreed about a world in which
nothing happened — and one thin one. The floors are counted, printed, and red at
zero.

**M6's machinery is built and M6 is not reached**, for the same shape of reason
M4 is not: its criterion asks for forty matches from nine people and that is a
calendar rather than a test. The schema, the identity scheme, the consent-version
refusal, the frozen split, the recording harness and the destruction procedure all
exist and run; `docs/SCHEMA.md` is the document and `replay/tests/destruction.rs`
executes the procedure end to end on every pull request. What does not exist is a
corpus, and this document proposes the revision — hold nine people, drop to twenty
matches — rather than declaring the criterion met on one that cannot support it.

**M7 is reached**, and it is the milestone at which `docs/SCOPE.md`'s "defense in
scope" column stops being an intention: every exploit class now carries the
attack written against it and the verdict, including the two that are **not**
caught and are correct not to be. The account is under M7 below; the two things
worth knowing from here are that the weakened build is a weakened *projection*
rather than a Cargo feature — a feature on `sim` is a switch any crate can throw
for the server binary, which is the shape `docs/ARCHITECTURE.md` already refuses —
and that the mutation pass found the defect in the exploit suite rather than in
the defences, which is recorded there.

What is left is M4's remaining clause and M6's recordings, both of which are
waiting on people rather than on hours, and then M8. M8's own baseline exists
now: `cheat-client::bot::Bot` plays a whole match that nothing delivered catches.

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
types. Vision sources: champions, towers. `step` never reads visibility. Vision
is discs without occlusion, and stays that way until there is a maphack to test
brushes against (`SCOPE.md`).

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
  nine champions and six towers there is no dramatic difference between a leaked
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
input, and (a) no two of them ever report different digests of their reconciled
local view at a checkpoint tick they both received, each having received at
least nine tenths of the checkpoints, (b) the server's authoritative digest
matches an offline resimulation of the recorded input log, run as a separate
process.

Half (a) said "all three report identical digests at every checkpoint tick"
until state moved onto QUIC datagrams. The weakening and its price are recorded
above and in `RISKS.md` R6; the observed loss on loopback is zero and the test
prints it.

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
  the log: `replay verify` refuses the file (the tool was called `resim` at M3
  and gained subcommands at M4). A criterion that has never been red is a
  criterion nobody has verified.
- **Half (a) is weaker than the sentence above, and the transport is why.**
  State travels in QUIC datagrams now, so a client can legitimately miss a tick;
  requiring all three to hold *every* checkpoint would be requiring the loss
  rate to be zero, which is a property of the loopback rather than of this
  project. What the test asserts instead is the claim the criterion was making —
  **no two clients ever disagree about a checkpoint they both received** — plus
  a floor of nine tenths of the checkpoints reached per client and a count of
  the digests actually compared, so that a run in which the three shared no
  checkpoint cannot pass by comparing nothing. On loopback the observed loss is
  zero and the test prints it. This is a weakening, it is recorded rather than
  absorbed, and it is the price of removing head-of-line blocking (`RISKS.md`
  R6).

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
  for nine players; deleting one participant's inputs leaves a log that no longer
  resimulates, so surgical removal is not on offer. Withdrawal therefore
  destroys **every match that participant played in, in full**, together with
  their pseudonym mapping — which also means it destroys other participants'
  contributions to those matches. Aggregate statistics already published are not
  retracted. Both consequences are stated before recording, because a
  participant who learns them afterwards was not informed.

**Exit:** three humans play a match end to end on two operating systems — one
team of the three, with the other six seats unoccupied, which is a state the
rules already handle and the fixtures already cover; the match writes a replay;
`replay verify` resimulates it to the server's final digest. The consent text exists, states the four points above, and was signed by
all three before the match rather than after.

### M4 is built, and it is not reached

Everything the criterion asks for **except the three humans on two operating
systems** is in the repository and runs. That clause is a fact about a calendar
and three people; no test stands in for it, and the milestone stays open until
it happens. `client/tests/m4_exit.rs` states the split in a table at the top of
the file rather than in a commit message, so that a reader who opens it in six
months finds out immediately which half of the criterion they are looking at.

What that file does run: three clients take one team's seats through the real
QUIC transport, driving the **same input path a person drives** — one intention
per tick, a standing order that persists, one-shot abilities that leave it alone
— for a thousand ticks; the match writes a replay; and `replay verify`
resimulates it to the server's own final digest in a separate process. The
prediction is exact on all 1000 of 1000 views for each of the three, which is
the number the criterion could not have asked for at M3 because there was
nothing to predict with.

Four things the criterion did not say, which the work had to decide:

- **The acknowledgement M3 left open is a field beside the view, not in it.**
  `ServerMessage::View` carries `applied_through: Option<u32>`. A view is what
  `view_for` computes from a `State` and that function has no session to ask, so
  the alternatives were a second argument to the most sensitive function in the
  project or a view type with a field its own constructor cannot fill.
  `ARCHITECTURE.md` has the reasoning and the five bytes it costs.
- **`Action::Idle` is not silence.** It is a rule that stops the champion, so a
  client with nothing new to ask for repeats its standing intention. That is
  also what keeps exactly one intention outstanding, which is the traffic under
  which the prediction is exact — the two facts are the same fact.
- **The renderer was a terminal, and it quantised aim.** `RISKS.md` R14 carried
  the decision and its price: no aim-curvature detector at M8, and everything
  timing-shaped untouched. **The second half of that was wrong and the client
  has been replaced before M5.** A terminal reports the pointer only when it
  crosses into a new cell, so the sampling rate followed the pointer's speed and
  the inter-arrival distribution — M8's *first* candidate signal — was
  contaminated at the source; the cell was also anisotropic, 1.158 world units
  across against 4.111 down, so the loss was directional. The client is a window
  now, its capture path records the raw device delta with a per-event timestamp
  and never passes through the renderer, and `RISKS.md` R14 carries the reversal,
  the measurement and what remains open. It landed before M5 rather than after
  because M5 freezes a record format and the client is what decides which
  telemetry exists to put in one.
- **The consent regime is code as well as text.** `docs/CONSENT.md` is the
  instrument; `replay withdraw` and `replay audit` are the mechanism, and
  `replay/tests/withdrawal.rs` exercises the audit by breaking the withdrawal
  three ways.

And one thing M4 found in M3's own criterion, which is the reason to run a
criterion under conditions it has never seen. `LocalWorld::digest` hashed the
client's **own** liveness — its own hit points and its own respawn timer, which
`ARCHITECTURE.md` is explicit that teammates are not entitled to. Three clients
on one team therefore reported different digests as soon as anybody took damage,
and M3 never noticed because its scripted match produces no damage at all: the
three walk a lane, nothing touches them, and their hit points stay equal by
accident. Walking them into a tower's range instead produced `Blue0 disagrees
with Blue1 about the world at tick 620`. M3's criterion was passing on a
fixture that could not reach the case it was about.

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

**What arrived before M5 rather than in it, and why the order matters.** The
client's input capture was rebuilt first. M5 freezes a record format, and the
client is what determines which telemetry exists to put in one — raw device
deltas, what a timestamp actually is, how often a sample is taken. Deciding the
format before knowing the source is deciding it twice, and the second time is
after a corpus exists.

### M5 is reached

`replay/tests/tamper.rs` runs the criterion. Nine rows rather than six — the six
the criterion names, plus a rules-hash and a version case counted separately, and
two rows for the attacker who cannot re-sign — each refused by a different check,
with a genuine replay accepted before the table runs so that a suite in which
everything fails cannot pass by failing.

Five things the criterion did not say, which the work had to decide.

**The structural decision is that there is exactly one file format.** The
unsigned container M3 shipped is *gone*, not kept beside the signed one.
`Recording` survives as the authority's in-memory product and has no encoding at
all; `replay::seal` is the only path to a disk. Two formats would have been two
things to parse and two things to verify, and — the argument that decided it — a
question with no good answer the first time somebody hands you the weaker one: a
reader that accepts both accepts the weaker, and a corpus holding both holds
files nobody can tell apart at a glance. The cost is that M3's and M4's exit
criteria had to learn to seal before they verify, which is the right cost, since
what they now exercise is the artefact a person would actually keep.

**What is signed is the manifest, and the manifest covers the log by carrying its
digest.** `RISKS.md` R4 lists three failure modes and this answers all of them at
once. The fields, and the *absences*, are in `replay/src/manifest.rs` field by
field — because M5 freezes a format and whatever is missing here is missing from
the whole corpus. The absences worth naming here: no events and no frames (a
replay carries the seed and the log; resimulation derives the events, so there is
no field for delivery order to get into); no telemetry above one intention per
tick, so `client::input::InputTrace`'s kilohertz stream stays a separate artefact
beside a replay rather than inside the one resimulation is a function of; no
player identity beyond the pseudonym; and no client-reported version, because the
client is assumed compromised and the build that matters is the one that resolved
the match.

**The outcome is a field, and that is what makes result forgery checkable.** It
is the claim a replay is *submitted* to make — exploit class 2 is "unplayed
match, edited replay", and what a forger wants to assert is that they won. Being
a field means resimulation can contradict it, which is why "altered outcome
record" has an error of its own rather than being a digest mismatch.

**Six distinct errors are only possible against an attacker who can re-sign.**
The naive tamper — editing bytes — is a signature failure every time, so a table
built that way would have six rows and one answer. Every row except the two
signature rows is therefore **re-signed with a key the registry accepts**, and
what catches each is a different check: the checks run in order and each one
catches the attacker who stopped one step short of the next. `replay/src/container.rs`
carries the escalation as a table.

**And the escalation ends where key custody begins.** An attacker holding an
accepted key who adjusts every field consistently has not tampered with a replay:
they have produced a replay of a different match, honestly simulated, and there
is nothing in the bytes to distinguish it from one that was played. That is the
boundary of what a signature over a self-consistent artefact can mean, and
`the_escalation_ends_where_key_custody_begins` executes it rather than leaving it
as a paragraph.

### What resimulation establishes here, and what it does not

`SCOPE.md`'s note on class 2 is the frame and this milestone does not widen it.
**Resimulating the inputs of a fully authoritative server proves the server did
not corrupt itself. It does not catch a cheating client** — every input in the
log is one the server accepted, and a client that aimed with a script sent inputs
that are in that log and that resimulate perfectly.

What verification establishes about a file somebody hands you is four things:

1. a key in your registry sealed this manifest;
2. the log is the log the manifest names, in order and in full;
3. that log, run through this build, reaches the state **and the result** the
   manifest claims;
4. the build verifying is the build the match was played under, or the failure
   says so in its own error rather than as a digest mismatch.

And what it does not establish: nothing about how anybody played; nothing against
somebody holding a key; and nothing `sim` cannot reproduce, which is the
comparability question below. **This is not a delivered defence.** `SCOPE.md`
reserves that word for a class with a matching exploit failing against it in CI,
and class 2's exploit is M7's — a cheat client that submits a replay of a match
it did not play. The format is what that exploit will be run against.

### The comparability trap, and what the comparison is actually worth

Resimulation compares `sim` against `sim`, which is the same shape as M2's
encoded-size bound — a test that re-executed one function on both sides and could
not be made to fail. M2 escaped it by re-deriving the visibility predicate; there
is no equivalent move here and pretending otherwise would be worse than saying
so, because a second implementation of `step` is exactly what `ARCHITECTURE.md`
refuses.

So the honest statement is narrower than "the match was played correctly": **it
is a check on the recording, not on the rules.** That was verified by mutation
rather than argued. Doubling a champion's displacement inside `step` leaves the
M5 verification green — both sides move together — and turns the tri-platform
fixture red at `divergence first visible at tick 100`. Stamping a log entry with
the wrong tick in `Match::recording` does the opposite: the fixture is untouched
and `replay verify` reports `the log does not reproduce the state it claims`.
What covers the rules is the committed digests; what M5 covers is that the
recording is complete, ordered and unaltered.

### The cross-platform case, which had never been run

Every digest here was recorded on x86-64 Linux and checked on three targets, so
the *simulation's* cross-platform claim has been evidence since M1. A replay is a
**file**, and between `State::digest` and the bytes on a disk M5 adds three
places a platform can differ: the manifest's encoding, the log's encoding, and
the signature over them. A log recorded on one machine and verified on another is
what a replay is *for*, and nothing had ever exercised it — the failure would
have been a verifier on Windows reporting a digest mismatch on an honest replay
from Linux, in the one milestone whose subject is telling those two apart.

`replay/tests/sealed.rs` carries bytes sealed on Linux and committed, and the
`determinism` workflow's `fixture` job requires byte equality with them on all
three targets before verifying and resimulating them there. Two fields in it are
pinned: the commit, because a committed fixture is not a build artefact, and the
`sim` version at `0.0.0`, which no build has — so the blob survives the version
bumps R13 demands of every change to `sim/`, and verifying it as *this* build
must fail with `SimVersion`, which is R13's mechanism demonstrated against a real
file.

## M6 — Human match corpus · 2 weeks of work, calendar-bound

This milestone gates every behavioral detector and it is bound by wall-clock
availability of other people, not by your hours. **Start recruiting during M4**,
and recruit for nine seats rather than six: the match is 3v3v3, so a recorded
match needs nine people at once and the calendar cost of that is the thing this
milestone is actually bound by.

Work: operating the consent regime written at M4 — collecting a consent record
per participant and honouring withdrawal on the stated timeline — a pseudonymous
player identity scheme, a documented telemetry schema (client-claimed timestamp
*and* server arrival timestamp for every input — see `RISKS.md` R3 on why raw
input telemetry is personal information), a recording harness, and a held-out
split fixed before any detector is written.

**Exit:** at least 40 recorded matches from at least 9 distinct people, each
with a consent record naming its retention date; a documented schema; a frozen
train/holdout split; a written destruction procedure that has been executed once
end to end on a discarded test recording; and a published summary statistic set.
Whether the raw corpus can be published at all is decided here, not later, and
only for the participants who opted into that purpose separately.

### What forty matches actually costs, and what the number is for

Written at M4 rather than at M6, because the constraint is a calendar and the
time to look at a calendar is before it is the thing blocking you.

**The arithmetic.** A recorded match needs nine people at once. Forty matches is
not forty evenings: a session that gets nine people into a voice call can
plausibly record six to ten matches once everyone is connected, so forty matches
is **four to seven sessions of nine people**. That is the real unit, and it is
the one to schedule against. Assembling nine adults on the same evening is,
empirically, a fortnightly event at best; four to seven of them is three to six
months of wall clock, running in parallel with M5 and M7 rather than after them.
`MILESTONES.md` already says to start recruiting during M4, and this is the
number that makes that instruction concrete.

**What the corpus can support at that size, stated as a bound and not a rate.**
`RISKS.md` R8 is the rule and the arithmetic is the rule of three: zero false
positives observed over N independent trials supports an upper bound of about
`3/N` at 95% confidence. What counts as N is the question people get wrong, and
it is not forty. A detector that scores a *player-match* has `9 × 40 = 360`
scored units, but they are not independent — nine of them share a match and a
few dozen share a person — so the honest N is closer to the number of distinct
people, **9**, for anything a person's style drives, and closer to the number of
matches, **40**, for anything a match's circumstances drive. The supportable
claims are therefore about `3/9 ≈ 33%` and `3/40 ≈ 7.5%` respectively, and
**both** belong in a detector's document, because a reader who is shown only the
friendlier one has been handled.

No number in this repository may be written as "0% false positives", at any
corpus size this project can reach. `RISKS.md` R8 says why in one sentence: the
audience is engineers who will check.

**Partially filled seats and short sessions are usable, and are not the same
kind of data.** A match with six humans and three empty seats is a legitimate
`State` — the rules handle it, the fixtures cover it, and M4's own criterion is
three humans and six empty seats. Its *telemetry* is as good as any: an input's
inter-arrival time does not know how many seats were occupied. What it is not
usable for is anything that reads the *situation* a player was in, because a
match with three absent champions has different fights in it. So: keep them,
count them separately, and never mix them into a distribution a detector
thresholds on without saying which. The same goes for short matches — a
five-minute match is five minutes of inputs, and a detector reading
per-match aggregates has to weight it as such rather than as a match.

**If the calendar does not produce nine people forty times.** The revision to
propose, rather than declaring the milestone reached on a corpus that does not
support it: hold the *exit criterion* at 9 distinct people and drop the match
count to **20**, which halves the sessions to two to four; report `3/20 ≈ 15%`
alongside `3/9 ≈ 33%`; and move the difference into M8's document as a stated
limit on which detectors may ship. What must not be traded is the *people*
count: a corpus of forty matches from nine people and a corpus of forty matches
from four people cost the same to collect and the second supports nothing at
all, because the null model a behavioural detector needs is a distribution over
humans.

### M6's machinery is built, and M6 is not reached

Everything the criterion asks for **except the recordings** exists and runs.
`docs/SCHEMA.md` is the document; `replay::consent`, `replay::session` and
`replay::split` are what make each of its rules a refusal rather than a paragraph.
The split between what a machine can do and what a calendar has to is the same
split M4 records, and it is stated here rather than in a commit message:

| Clause | Who can satisfy it |
| --- | --- |
| a documented schema | this repository — `docs/SCHEMA.md` |
| a pseudonymous player identity scheme | this repository — `docs/SCHEMA.md` §2, `replay::Pseudonym` |
| a consent record per participant naming its retention date | this repository — `replay enrol`, and `Corpus::store` refuses a match without one |
| a frozen train/holdout split | this repository — `replay::split`, frozen and computed rather than stored |
| a destruction procedure executed once end to end | this repository — `docs/SCHEMA.md` §9, run on every pull request by `replay/tests/destruction.rs` |
| a recording harness | this repository — `moba-client --record`, `replay store` |
| **at least 40 matches from at least 9 distinct people** | **nobody but nine people, four to seven times** |
| a published summary statistic set | nobody, until the line above happens |

**The last two are not work and no test stands in for them.** `replay census`
computes and prints the summary set the criterion asks for, and on an empty corpus
it correctly prints that the corpus supports nothing at all. That is a working
instrument, not a satisfied criterion, and the milestone stays open.

### Nine participants is decided, and these are its consequences

**Nine distinct people in total** — the nine seats of one 3v3v3 match, the same
people from one match to the next. Not a number to revise; what needed writing
down is what follows from it, and it is written before the first session rather
than discovered at M8. `docs/SCOPE.md` carries the same three under "The corpus is
nine people", because they constrain what may be *claimed* as much as what may be
*collected*.

1. **The "style" bound stays at about 33% and no number of matches improves it.**
   `3/9` is a function of the people, and recording a hundred more matches from
   the same nine moves it by nothing. Only the "circumstances" bound —
   `3/40 ≈ 7.5%`, or `3/20 ≈ 15%` at the reduced count proposed below — improves
   with matches. **The two travel together everywhere a claim is made**, and
   `replay census` prints both on every run.
2. **M8 can produce detectors that flag for review and nothing else.** No
   threshold calibrated on nine people supports an automatic sanction of any kind
   — a 33% upper bound on the false-positive rate means one flagged player in
   three could be innocent and this corpus cannot rule it out. **This is a decision
   taken here, not a limitation discovered at M8**, and M8's exit criterion below
   carries it: a detector ships as a score and an evidence bundle, and a human
   decides.
3. **Generalisation to a hand this project has never recorded is out of reach.** A
   detector calibrated on nine people has learned nine hands and says nothing about
   a tenth player — not less, nothing. `docs/SCOPE.md` says so in plain words under
   "What this project does not demonstrate", because the failure this guards
   against is a reader taking a measured number for a general one.

### And what M6 established about synthetic play, which M7 executed

The corpus refuses a seat that recorded **zero device events**, which catches a
scripted or headless client. **A bot that moves a real mouse is indistinguishable
in a file** — it records as many samples as a person, at the same rate, through
the same capture path.

So what guarantees authenticity is **supervision**, which is a fact about a person
rather than a property of the format. Every session record therefore carries its
conditions — in person, remote, or unsupervised — so that M8 can stratify and a
reader can tell what a claim rests on; `docs/SCHEMA.md` §5a is the schema, and a
record without them does not decode, which is R3's equivalence between an absent
consent version and a stale one. M7's `cheat-client/tests/botting.rs` executes
both halves: a bot plays a whole match nothing catches, and the silent-seat check
catches the headless version and is blind to the mouse-moving one.

### The exit criterion needs a revision, and here it is

The arithmetic above says forty matches is four to seven evenings of nine adults,
which is three to six months of wall clock. That was written as a warning; on the
evidence of the calendar it is the outcome. So the proposal, made here rather than
resolved by declaring the criterion met on a corpus that cannot support it:

**Hold the people count at 9 and drop the match count to 20.** Not because twenty
is enough for anything in particular, but because the *people* count is what a
behavioural null model is a distribution over, and dropping it is the one trade
that makes the corpus worth nothing. Twenty matches is two to four sessions, and
`3/20 ≈ 15%` alongside `3/9 ≈ 33%` is what may then be claimed.

**And the two bounds travel together regardless of which number wins.**
`docs/SCHEMA.md` §8 is the table, `replay census` prints both on every run
together with the sentence refusing "0% false positives", and every detector
document at M8 carries both. A reader shown only the friendlier one has been
handled, and the friendlier one is always whichever the author is quoting.

### Four things the criterion did not say, which the work had to decide

- **A replay cannot say what a match was recorded *on*, so there is a second file
  and it is not an index.** A mouse at 400 counts per inch and one at 1600
  describe the same hand differently; without the number, a difference of
  equipment reads as a difference of style. The manifest is frozen and every field
  in it is something the *authority* knows, so the covariates go in a session
  record filed beside the replay — indexed by **seat and never by pseudonym**, so
  the signed manifest stays the one naming of a person, and inside the match
  directory a withdrawal already removes whole. `docs/SCHEMA.md` §4 is the schema,
  including the three fields that are declarations rather than measurements and
  what refusing pointer acceleration costs.
- **The tick budget was set on a fixture and had never been measured on a match.**
  `docs/RISKS.md` R16 is the entry, `client/tests/cadence.rs` is the measurement —
  nine seats in a group fight at the game's own rate — and the answer is that the
  budget holds with a factor of six in hand. What the milestone keeps regardless
  is the instrumentation: every session reports its worst overrun and its count of
  passes over budget, and a session that fell behind is identifiable in the corpus
  rather than pooled into it.
- **The consent regime became something a program can refuse.** The text existed
  and said the right things; that it was signed *before* the match stayed paper.
  It still is. What changed is that the document declares a version, every consent
  record carries the version its participant signed, and `Corpus::store` refuses a
  match where that version is absent or superseded — so the paper's *absence* is
  now a mechanical error. `docs/RISKS.md` R3 carries it.
- **The split is a function and not a file.** A list of held-out matches would be
  the derived index M5 removed, and it has a failure mode of its own: a withdrawal
  that destroyed a match and left it named in a split file would leave behind a
  line about somebody's participation after they asked for it to be destroyed —
  and a rule like "the first four fifths by date" would silently move matches out
  of a holdout a threshold had already been chosen against. A hash of the
  identifier does neither, and
  `a_withdrawal_cannot_move_a_match_from_one_half_to_the_other` is the assertion.

### And no detector, deliberately

`docs/SCOPE.md` puts behavioural statistics at M8 and this milestone builds none.
A detector written before the corpus exists is a detector whose threshold was
chosen on nothing, and the choice would then be defended by whoever inherits it.
What M6 owes M8 is a corpus, a frozen split, and an honest statement of what the
two can support — and the last of those is a sentence M8 is not allowed to
improve on.

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

### M7 is reached

Nineteen assertions across seven files in `cheat-client/tests/`, run on both
platforms by `cargo test --workspace` and printed with `--nocapture` on Linux so
the exploit-by-exploit account is in the run summary rather than only in a
failure. `docs/SCOPE.md`'s class table now carries the exploit and the verdict for
every row, which is the first time that column has been anything but an intention.

Six things the criterion did not say, which the work had to decide.

**The weakened build is a weakened *projection*, and there is no Cargo feature.**
The criterion asks for "the deliberately weakened build" and named
`--features no-culling`. That feature is not here, and refusing it is the decision
rather than an omission: Cargo features are additive and unified, so a
`no-culling` on `sim` is a switch any crate in the graph can throw *for the server
binary*, and `docs/ARCHITECTURE.md` already refuses exactly that shape for a
`Serialize` impl. Putting the switch on the culling instead of on the serialization
would be putting it on the more dangerous of the two.

What replaced it is stronger and cheaper: **each exploit is run twice inside one
test**, against a weakened surrogate and against the real thing, and the test is
red if either half is wrong. "CI runs both configurations" is satisfied by one
binary rather than two, and the *pairing* is what the two-build scheme could not
have given — the same attacker, the same world, the same tick, one projection that
leaks and one that does not.

**The "it would have worked" half is an assertion and not a courtesy.** It is
`docs/RISKS.md` R15 applied to attacks: an exploit that fails against the real
defence without ever having worked proves nothing about the defence, because it
looks exactly like a defence that holds and there is no red to tell them apart.
Every exploit here therefore has to reach its antecedent — the maphack must place
hidden enemies against the leaking projection, the wiretap must read the entity
count off the unpadded stream, the forgery must verify against a registry that
trusts it — before its failure against the shipping build is allowed to count.

**The mutation pass found a defect in the exploit suite itself, and it is the one
worth recording.** `tests/maphack.rs` measured the attacker's surplus against
"what the fog shows", and read that out of `view_for`'s own output. With culling
removed on purpose the exploit did not go red at the exploit — it went red at its
own R15 antecedent, because the broken projection had redefined what *hidden*
meant. The test was asserting that `view_for` agrees with itself, which a
projection that leaks everything satisfies as long as it leaks consistently. That
is `docs/ARCHITECTURE.md` invariant 5's trap, one crate over, and the fix is the
same: `cheat-client/tests/harness/entitlement.rs` re-derives the vision predicate,
and carries the obligation that a change to the rule changes it in the same
commit.

**Class 2's attacker writes the container by hand, and that is what makes the
format checked rather than self-consistent.** `cheat_client::forge` reimplements
the replay file from the published documents and links no `replay`; the suite
requires its bytes to be byte-identical to the victim's writer's over the same
match. A format whose only writer is its own reader is a format nobody has
independently read.

**The keyless attacker is caught by two layers and not one, which M5 could not
show.** An edit inside the manifest dies at the signature; an edit to the log —
truncation, reordering — dies at the manifest's *commitment* to the log, with the
signature still perfectly valid, because the log rides outside it and the manifest
carries its digest and its count. `docs/RISKS.md` R4's three failure modes, from
the outside. The escalation into six distinct errors still needs an attacker with
a key, which is why `replay/tests/tamper.rs` hands its attacker one, and
`tests/forgery.rs` reproduces that table at the byte level and then walks past its
end: a self-consistent forgery under a trusted key verifies, and is right to.

**One exploit lands against the shipping build and stays.** The projectile
back-track recovers the ray a hidden caster stood on, from a position and a
velocity the recipient is entitled to. It is recorded rather than defended — see
`docs/SCOPE.md` — and it is here because a milestone that keeps only the attacks
that fail is a milestone that has been curated. Class 3's bot is the same shape and
the same decision: it plays a whole match, nothing catches it, and the green
documents a limit.

**And class 6 has no exploit, deliberately.** `tests/collusion.rs` is labelled a
demonstration: it shows that the union of two teams' entitled views is strictly
larger than either and that every frame that produced it was correctly culled.
There is nothing for an attacker to send, so building something that looked like
an attack would have been manufacturing an antecedent to fill a row — the exact
R15 failure the rest of the crate exists to avoid.

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
states the bound — **both** bounds, since M6 fixed the people count at nine:
`3/9 ≈ 33%` for anything a person's style drives and `3/40` (or `3/20`) for
anything a match's circumstances drive.

**Three constraints M6 fixed, which this milestone inherits and may not improve
on.** They are stated at M6 and in `docs/SCOPE.md` and repeated here because this
is where they bite:

- **Detectors flag for review. Nothing here sanctions anybody automatically.** Not
  a ban, not a suspension, not a queue restriction, not a silent match-quality
  adjustment. A 33% upper bound means one flagged player in three could be
  innocent and the corpus cannot rule it out. A detector ships as a score and an
  evidence bundle; a human decides.
- **No claim about a player this project has never recorded.** Nine people is nine
  hands, and a null model for human behaviour is a distribution over humans. No
  page in `docs/detectors/` may say "this detector achieves X on players in
  general".
- **A distribution is built over one supervision stratum, or the document says it
  was not.** `docs/SCHEMA.md` §5a: authenticity comes from an operator having been
  present, not from anything in a file, so a corpus that mixes supervised and
  unsupervised sessions carries a provenance covariate. `replay census` prints the
  three counts.

**Exit:** per detector, a page in `docs/detectors/` giving the null model, the
threshold and its justification, the score distribution over the human corpus
and over the bot corpus, the observed FP/FN counts, **both confidence bounds**,
and the supervision strata the distribution was computed over. The threshold is
chosen at zero false positives on the corpus. The matching bot variant is
detected in CI, and the detector ships only if that CI test exists. **No detector
emits an action**, only a finding.

The bot the detectors are measured against exists as of M7:
`cheat-client::bot::Bot` plays a whole match through the protocol and nothing
delivered catches it, which is the baseline this milestone has to improve on and
the reason `tests/botting.rs` is green on purpose.

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
