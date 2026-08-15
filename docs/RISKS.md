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

### What M8 added, and it is the largest collection this project makes

The hedge and the M6 machinery below are unchanged and all of it still applies.
What changed is the **volume and the kind** of what is held: `docs/SCHEMA.md` §11
adds a sealed companion carrying every device event at the mouse's own 125 Hz to
1 kHz, per seat, per match. That is one to two orders of magnitude more data than
a replay and it is a different thing — it describes the movement of a hand rather
than the decisions of a player, and it is distinctive in the way this entry's own
first paragraph says input telemetry is: *behavioural biometrics*.

Three consequences, and none of them is new machinery:

- **The consent text names it, before it is collected.** `docs/CONSENT.md` §2b,
  in a participant's words: what is recorded, at what rate, and what can be
  worked out from it. The document's version moved, so the tooling refuses any
  match whose participants signed the older text — which is this risk's existing
  mechanism doing exactly the job it was built for, on the first occasion that
  actually needed it.
- **It is destroyed by the same withdrawal**, because it lives inside the match
  directory a withdrawal removes whole. It names **no pseudonym**, so a search
  for a name cannot find one left behind; `Corpus::accountable` is what reaches
  it instead, and `replay/tests/withdrawal.rs` breaks the destruction on purpose
  to prove that it does.
- **The distinction that keeps this outside the biometric-database regime is
  unchanged and is worth re-reading against the new data.** This corpus is
  collected to calibrate detectors and never to verify or confirm anyone's
  identity. A stream of hand movements would make an identity check *easier* than
  the old summary did, which is precisely why the sentence below matters more
  now than it did: if this project ever uses input biometrics to authenticate a
  player, arts. 44–45 attach and this risk is reopened.

### What the granular regime added, and the failure it was closing

The hedge above says "consent is requested per purpose, so publication of the raw
corpus is a separate opt-in". Until `docs/CONSENT.md` `2026-08-17` that sentence
was **half true and getting less true**: publication was per purpose, and three
other decisions about a participant's data were being taken on their behalf by
the project's own defaults. Whether the recordings may train a bot, whether they
are kept after the work ends, and whether somebody is named rather than
pseudonymous are all things a reasonable person could want to decide separately,
and none of them was a question anybody was asked.

Three things changed and only the first is about the document:

- **Four boxes instead of one**, chosen by a test rather than by taste: a box
  exists only if refusing it leaves the rest of the participation possible.
  Everything the declared purpose is structurally made of — the intentions, the
  device stream, the session record, the lobby crossing — is stated as necessity
  in the participant's own words instead of being offered as a choice whose two
  outcomes are "take part" and "do not".
- **Every one of them is a gate rather than a field.** The check is the only
  constructor of the value the use needs (`docs/SCHEMA.md` §10a), which is this
  register's usual answer: R8 did it for thresholds, M5 did it for the participant
  list, and the failure mode removed is the same one — a rule somebody has to
  remember.
- **A consent record silent about any purpose does not decode**, so adding a
  fifth box invalidates every signature already given. That is this entry's
  "absent and stale fail alike" applied one level down from the version, and it is
  what makes "an old consent cannot silently authorise a new collection" true of
  *purposes* and not only of *fields*.

**And one new artefact class, named before it exists.** A model trained on this
corpus is a derived artefact that outlives what it derived from — the shape M5
removed and this entry has been guarding ever since. `docs/SCHEMA.md` §10b is the
rule: a model carries the pseudonyms it learned from, stored under the corpus
root, so the audit's byte search reports it the first time one of them withdraws.
Nothing forces a future model to be stored where the audit looks, and
`docs/CONSENT.md` sends that to a human review rather than claiming otherwise.

**Age is the one new refusal that is not about a purpose.** A participant under 18
cannot give sufficient consent alone, this project has no parental-consent
procedure and no second text, and inventing one at the door of a corpus is not a
thing a program should do quietly — so the record carries one bit, `Corpus::store`
refuses a match anybody under 18 is in, and the message names the human decision
it is standing in for. No date of birth is collected: what is needed is the bit.

### What M6 added, and the one thing it made mechanical

The hedge above is a page of text, a mapping held outside the repository, and a
`.gitignore`. M6 keeps all of it and adds the piece that was missing: **which
version of the text somebody signed**. A consent document that later gains a
field — and this milestone added several, see `docs/SCHEMA.md` §4 — has stopped
being the document a participant read six months ago, and nothing in a corpus of
replays would have said so.

So `docs/CONSENT.md` declares a `consent-version`, `replay::consent::CURRENT`
repeats it, a test fails if they disagree, `ci` refuses an edit to the document
that leaves the version alone, and `Corpus::store` refuses a match whose
participant consented under another version — or under none, because a record
written before the field existed does not decode and is therefore not a consent
record. That last equivalence is the deliberate half: absent and stale have to
fail alike, or a corpus assembled under an older regime is readmitted by the
silence of its own files.

The schema M6 added is personal information like everything else here, so it is
named in `docs/CONSENT.md` field by field and destroyed by withdrawal. It lives
inside the match directory that `withdraw` already removes whole; and because it
names **no pseudonym** — deliberately, so the signed manifest stays the one naming
of a person — a search for a name cannot find one left behind, which is why
`Corpus::audit` reports a match directory whose replay *or* session record fails
to read, unconditionally, for every pseudonym.

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

**And the companion the manifest commits to, which is this entry's shape one
level down.** `docs/SCHEMA.md` §11 puts the device stream in a second sealed
file, and the manifest carries its **digest**. That answers the same three
failure modes for the same reasons: a companion cannot be resubmitted under
another match, no unregistered party can mint one that a replay will accept, and
a companion is distinguishable from a second companion of the same match — which
is the one that matters, because an attacker holding an accepted key can seal a
smoother stream and everything in that file is internally true. The absence of a
companion is a **signed** state rather than a missing file, so it cannot be
upgraded afterwards either.

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

### Taken at M7, and the hedge is now enforced rather than described

Every exploit is a `#[test]` assertion — *the attacker learns X*, *the server
accepts Y* — and there is no binary in the crate, so there is nothing to point at
anything. The dependency list is the mechanism: `protocol`, plus `ed25519-dalek`
because `forge` has to sign, and `ci` asserts it with `cargo tree` in both
directions — the attacker links no `client`, `server`, `replay` or `anticheat`,
and no production crate links the attacker.

**What is in it that is worth being precise about, since "no generic technique" is
a claim a reader should check.** A protocol session that constructs frames; a
reader of `PlayerView`s; an observer of datagram sizes and counts; a writer of
this project's replay container; a bot that sends one intention per tick. Every
one of those is a function of *this* wire format and *this* file format, and
none of them is a technique. The one thing that looks generic is the Ed25519
dependency, and it signs a container whose layout is this repository's.

**The judgement that stays a judgement.** Two exploits here do not fail — the
projectile back-track and the bot — and publishing an attack nothing stops is the
part of R7 that cannot be delegated to a dependency list. They are kept because a
milestone that publishes only the attacks that fail has been curated, and because
both are already stated in `docs/SCOPE.md` as limits: neither tells a reader
anything the documents did not already say the project could not defend. Neither
is a tool. The bot plays this game and nothing else, and the back-track is
arithmetic on two numbers in a message this project defines.

### The coupling test, applied to both of them at M8

The sentence above — "neither is a tool" — is a conclusion, and M8 is the
milestone at which the bot grows variants, so it was worth asking the question in
the form that can be answered rather than asserted. **Can this exploit be pointed
at something other than this server without being rewritten?** An exploit that
can is a technique wearing a demonstration; an exploit that cannot is a defect of
this project, which is what belongs in this repository.

**The projectile back-track fails that test in the right direction, and it is the
easy case.** It reads a position and a velocity out of a `PlayerView` this project
defines, divides by a per-tick displacement this project's rules fix, and recovers
a ray on a map whose geometry is in `sim::rules`. Every constant in it is a
constant of this game. There is nothing to point.

**The bot needed the examination, and it passes for a reason worth stating rather
than for the same one.** What would make a bot a tool is not that it plays well —
it is a **layer that synthesises device input**: something that moves a real mouse
or presses real keys through the operating system, because that layer is
game-independent by construction. It drives the OS, not the protocol, so pointing
it at another title is a matter of changing what it aims at rather than of
rewriting it. That layer is exactly what `docs/SCOPE.md` names as the ceiling of
behavioural detection and what `docs/SCHEMA.md` §6 says no file can see: *a bot
that moves a real mouse records exactly as many samples as a person.*

**There is no such layer here, and the dependency list is not the evidence — the
call sites are.** Everything under `cheat-client/src/` imports `protocol`,
`std::collections` and `ed25519_dalek`, and nothing else. There is no window, no
event loop, no `uinput`, no `SendInput`, no `XTest`, no pointer of any kind. What
`bot::Bot` holds is a sequence number and a standing `Action`, and its one piece
of hidden knowledge is `follow` — the rule that `Idle` stops a champion, a `Move`
replaces the standing order and a cast leaves it alone, which is a rule of
`sim::step` and of nothing else. `Bot::walk_to` takes an `FxVec2` in this
project's Q15.16. Pointed at another game, there is no wire to speak, no `Action`
to compose and no rule to mirror: it would have to be written again from nothing.

**So the verdict is the same as the back-track's and the bot stays published as
it is.** No recoupling is needed, because there is no free reusability to remove:
the reusable half was never built.

**What the examination actually bought is a constraint on M8**, and it is the
half worth having written down before the work rather than after. The honest way
to test a behavioural detector against `docs/SCOPE.md`'s ceiling would be to
build the mouse-moving bot and see whether the statistics separate it — and that
is precisely the thing this entry refuses, because it is the one component of a
cheat that generalises. **So M8's bot variants stay on the protocol side of the
line: they compose intentions, and the "human-plausible noise" M8 asks for is
noise added to a decision, never to a device.** The consequence is stated in
`docs/detectors/README.md` rather than discovered: this project can measure its
detectors against a bot that plays through the wire, and it cannot measure them
against the ceiling at all. Naming what cannot be tested is the same obligation
as naming what cannot be defended.

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

### Taken at M8, and the hedge stopped being a habit

Every clause above is now a thing a program does rather than a thing an author
remembers. `anticheat::Bounds` computes both bounds together and its `Display`
prints them together with the sentence refusing a rate of zero, so quoting one
without the other means deleting a line of output. `Detector::null_model` is a
method, so a detector without a stated null model does not compile. The holdout
split was frozen at M6 and is a grouping key here, so a distribution is per half
as well as per stratum. And "never auto-ban" is `Finding::for_review` answering
`None`: a detector with no calibrated threshold cannot report that it decided in
anybody's favour, in either direction.

The clause that needed the most machinery is the one this entry does not
literally contain, and it is the one that mattered at M8: **a threshold may not
be fixed on data that is not a corpus of people.** `Evaluation::basis` is the
only constructor of the value a threshold needs, and it refuses synthetic play by
name, refuses an empty corpus, and refuses fewer than the nine distinct
participants `MILESTONES.md` M6 holds fixed. The refusal exists because the
temptation is real and immediate: the exploit suite's bots are in the repository,
they run in CI, and their scores separate from their controls cleanly enough to
look like a calibration.

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

**The write permissions have arrived, and the precondition held.** `release-plz`
and `release` are in the repository. Between them they hold `contents: write`,
`pull-requests: write` and `packages: write`, every one declared on the job that
needs it and none at the top of a workflow, so the moment this paragraph
describes — a moved tag minting releases in this project's name — is now the
moment being defended against rather than one being predicted.

The count of third-party actions is **three**, and the third is
`release-plz/action`, pinned at commit `2eb1d8bc` with `# v0.5.131` beside it.
It is the one place this register's own preference was not taken: `supply-chain`
installs `cargo-deny` with `cargo install --locked` rather than reaching for an
action, and the same move here would have kept the count at two. It was not
taken because `release-plz` links `cargo` itself and would compile for minutes
on every push to `main` in order to open a pull request, where `cargo-deny`
builds in seconds. The trade is stated rather than hidden: one more publisher to
trust, in the workflow holding `contents: write`, bought with the minutes.

Three things reduce what a moved pin could reach, and they were chosen for that
rather than for tidiness:

- **No `id-token: write`, anywhere.** The provenance attestation is the only
  thing that needs it and `MILESTONES.md` M9 records it as not delivered, so
  nothing in this repository can currently mint a signed statement about an
  artefact. The permission arrives with the attestation and not before.
- **No build cache in `release`.** `Swatinem/rust-cache` stays in `ci` and
  `determinism`, where a poisoned entry costs a wrong test result. In `release`
  it would cost a poisoned binary with a checksum published beside it, which is
  the same paragraph above with a worse ending. A release compiles from scratch,
  once per tag.
- **No secret.** The release pull request is opened with the repository's own
  `GITHUB_TOKEN` rather than a stored personal access token, which is why it
  arrives with no CI on it (`ENGINEERING.md` records the manual step that costs).
  This repository still has no secrets for a compromised action to read.

And a fourth surface arrived with the container that is not an action at all:
`server/Dockerfile`'s distroless base is pinned by digest with its tag beside it,
for the same reason. `nonroot` is a mutable pointer exactly as `v5` is, and an
image that moves under a running release pipeline is the same failure wearing
different clothes. Renovate's `docker` manager keeps that digest current
alongside the action SHAs, which is the same "the pins and the thing that
updates them land together" this entry has insisted on from the start.

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

### Re-decided at M9: the number stays the human's, and `release-plz` is told so

The release pipeline brought a tool that computes version bumps from
conventional commits, which is a second mechanism pointed at the one number this
entry is about — and a number with two owners has none. `release-plz.toml`
therefore carries `[[package]] name = "sim"` with `release = false`, which takes
`sim` out of every release-plz command; the `sim-version` job is unchanged and
remains the only thing that demands the bump.

The choice was between granularity and convenience, and the two reasons it went
this way are both about meaning:

- The manual bump lands in the pull request that changes the rules. A
  release-time bump would give every replay recorded between two releases the
  same `sim` version across a step reorder, which is the case this entry exists
  to catch, moved to where nobody is looking.
- `sim/Cargo.toml` bumps by a determinism judgement — patch for a change that
  cannot move a digest, minor for anything that can. Conventional commits encode
  a different one: a `refactor:` or `perf:` that reorders `step` is a patch under
  semver and a minor under this rule, so the tool would have quietly overruled
  the rule while appearing to enforce it.

**The cost, which belongs beside the imperfections listed above.** release-plz
attributes a commit to the crate whose files it touches, so a change confined to
`sim/` is attributed to a package it does not process: a release cycle containing
only rules changes proposes no version at all, and `[workspace.package] version`
has to be raised by hand. That is the smallest of the failure modes on this page
— it is loud, it happens before anything is built, and `release.yml` refuses a
tag that disagrees with the manifest — but it is a real consequence of keeping
this number out of the tool's hands, and it is the price of the two reasons
above.

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
  failing against it in CI, and nothing here says the detector will work. **And
  a permission was not sufficient**, which took until M8 to notice: the
  resolution was there and the *trajectory* was not, because the aim reaches the
  wire only at the instant of a click and the stream it is integrated from was
  not kept. `docs/SCHEMA.md` §11 is what keeps it, and the section above is what
  that did to this entry's own reopening condition.
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
| **Added latency, mean** | not isolated | **0.041 ms** | **0.288 ms** |
| **…standard deviation** | not isolated | **0.016 ms** | **0.690 ms** |
| …95th percentile | — | 0.055 ms | 2.312 ms |
| **…99th percentile** | — | **0.107 ms** | **2.709 ms** |
| …maximum | — | 0.257 ms | 5.114 ms |
| Slowest single pass of the capture loop | — | 6.013 ms | 9.163 ms |
| Recorded inter-arrival, standard deviation | 0.247 ms | 0.051 ms | 1.033 ms |

**The conclusion, and it is the one the arithmetic supports rather than the one
the numbers flatter.** In the profile a player runs, the delay this client adds
between an event existing and being stamped has a standard deviation of **16
microseconds**, a 99th percentile of **107 microseconds**, and a worst case over
1200 samples of **0.26 ms**. Against the grandeurs M8 will look for —
inter-arrival distributions and reaction latencies whose human spreads are tens
of milliseconds — that is a fraction of a per cent of the signal, and the tail is
bounded rather than heavy: there is no fifteen-millisecond mode in either
profile. The unoptimised build is more than an order of magnitude worse and
still under a millisecond of spread, and its tail is bounded by its own frame,
which is the mechanism visible in the two rows at the bottom of the table.

**So the residual is quantified and without consequence for the detectors in
scope, and R14's timestamp half stops being a live risk.** What replaces it is a
test that runs on every pull request. What that test enforces is the property
below — an event waits at most one pass of the capture loop — and the
distribution is printed *when it fails*, or under `--nocapture` for whoever wants
the number. So a regression is caught in CI and read on a developer's terminal,
which is the right division: a threshold on a runner's timing would be a check
that goes red for reasons that have nothing to do with this repository.

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
  latency of mean 8.195 ms and maximum 30.3 ms against a slowest pass of 4.8 ms.
- **`evdev` stays refused, and this measurement is why.** A per-platform input
  stack would buy a device timestamp on Linux and nothing on Windows, at the cost
  of a `/dev/input` permission each participant has to be granted and a corpus
  whose timestamps mean different things on the two platforms it is recorded on.
  That was a defensible trade against an unmeasured residual. Against 16
  microseconds it is not a trade at all. **This reopens only if a detector turns
  out to depend on a quantity at the scale of a millisecond** — see the section
  below, which is that clause restated after it turned out to be a decision
  rather than a fact.

### The reopening clause, answered at M8 with a number rather than a shrug

The clause above asks a question and M8 is where it had to be answered rather
than assumed. **No detector M8 built depends on a quantity at the scale of a
millisecond, and the reason is that the record cannot express one.**

| Quantity | Its resolution in the corpus |
| --- | --- |
| a reaction latency | **one tick — 33.3 ms**, because both ends of it are tick numbers the log carries |
| a clock rate error | one millisecond over the match span, so ≈ 37 ppm on a 53-second match |
| this client's own capture residual | **16 µs** (the table above) |

The residual sits sixty times below the *field* it is written into and three
orders below the *tick* every reaction detector counts in. **The binding
resolution is the record's, not the capture path's**, so an input stack that
improved the third row would buy nothing any detector in scope could spend.

And the honest half, which this entry's own third clause demands be named rather
than presumed absent: **the thing that would depend on a sub-millisecond scale is
a detector over the device stream itself** — the inter-arrival distribution M8's
candidate list opens with, or an aim trajectory. Neither can be written, because
that stream is deliberately outside the artefact the corpus holds
(`replay/src/manifest.rs`, `docs/SCHEMA.md` §3) and reaches it as four summary
numbers per seat. So the reopening condition is not *unmet*; it is
**unreachable** from what a corpus contains, and the decision that would change
that is a new collection with its own consent version rather than a change of
input stack. `docs/detectors/README.md` carries the full account.

### The reopening condition was a decision, not a property — restated at M8

The clause above said `evdev` reopens only if a detector depends on a quantity at
the scale of a millisecond, and added that none of `MILESTONES.md` M8's
candidates does. **The second half of that was true only given a recording policy
that is no longer the policy**, and this entry has to say so in its own words
rather than leave a reader to find the contradiction.

**What was actually being claimed.** `docs/detectors/README.md` reasoned it out
at M8 and got the arithmetic right: the corpus's own timestamps were whole
milliseconds, the finest quantity any detector could read was a tick at 33.3 ms,
and a 16 µs residual sits three orders below that. It then named the thing that
would change it — "a detector over the device stream itself" — and concluded such
a detector could not exist **because the stream was not in the corpus**.

That was a fact about the *format*, not about the system. Nothing in the platform,
the client or the rules prevented recording the stream; a decision taken at M5 and
kept at M6 excluded it, and `docs/SCHEMA.md` §11 reverses that decision now, while
the corpus is still empty. So the honest form of R14's condition is:

> The condition is closed by a **choice of recording format**, revisable for
> exactly as long as the corpus is empty, and it reopens the moment a detector
> depends on a quantity at the scale of the residual.

**Does the companion reopen it? At 125 Hz to 500 Hz, no. At 1 kHz, the device
does.** The arithmetic, which is the part worth having rather than the verdict:

| Polling rate | Gap between two device events | The residual against it |
| --- | --- | --- |
| 125 Hz | 8 ms | 16 µs standard deviation is 0.2%; the 0.26 ms worst case is 3% of one gap |
| 500 Hz | 2 ms | 0.8%; the worst case is 13% of one gap |
| 1000 Hz | 1 ms | 1.6%; **the worst case is 26% of one gap, and a 5 ms pass of the capture loop is five reports stamped microseconds apart** |

The last row is the live one and it is not about the timestamp's *source*. Five
device reports that arrive while one pass of the capture loop is running are
dequeued back to back and stamped back to back, so the recorded inter-arrival
distribution acquires a burst-and-stall structure that belongs to the client's
scheduler. A device timestamp would fix exactly that, on Linux, and buy nothing
on Windows — which is the trade this entry has refused twice, and it is a *closer*
trade at 1 kHz than it was at 8 ms.

**What is taken instead, and why it is enough for now.** The rate is a covariate
the corpus already carries: `device_polling_hz` is declared per seat and
`median_gap_ns` is measured beside it, `replay census` prints the declared rates
with the sentence saying what pooling them costs, and `docs/SCHEMA.md` §11f
states the rule — a detector reading an inter-arrival distribution stratifies by
polling rate or says it did not. That is a covariate honestly recorded rather than
a residual removed, and it is the weaker answer; it is taken because the stronger
one is a second input stack, a permission every participant must be granted, and
a corpus whose timestamps mean two things on the two platforms it is recorded on.

**Reopened by**, and this is now the whole list: a detector at M8 whose null model
reads the inter-arrival distribution of a 1 kHz seat and cannot be stated over a
stratum instead; or `winit` gaining a device timestamp, which would make the
question moot on three of four backends at no cost at all. Either is a decision
about a specific detector, taken **before the first recording session** or not at
all, because the covariate this is about cannot be removed from a corpus
afterwards.

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

---

## R16 — The client's tick budget was set on a fixture, and a corpus is not one

**Not irreversible, and it is here because the cost lands on the corpus rather
than on the code.** A client that cannot complete a pass of its capture loop
within one tick does not lose data. It writes a **delay** into the record: the
intention it decides one pass late is recorded as though the hand were late, and
that is indistinguishable, afterwards, from a hesitation. It is R14's failure
arriving through a different door — a property of the client contaminating a
signal M8 rests on — and it is worth its own entry because the door is different
and R14's hedge does not cover it.

**What the evidence for the budget actually was.** Two numbers, both from
fixtures, and the second one is the interesting one:

- `client/tests/m4_exit.rs` compresses the server's period so that a thousand
  ticks run in seconds. It was **four** milliseconds, and four was enough.
- Then `docs/RISKS.md` R15's pass gave that match something to happen in —
  walking the three clients under Red's tower instead of turning them round at
  the halfway point — and four was **no longer enough**. Both `check` jobs
  reported `worst correction 13106 raw units`, which is one tick of
  `champion_speed` and is the documented shape of a client that fell behind. The
  period moved to ten.

So the only tick budget this project has ever had evidence for was set by a
fixture with three seats in it, and it moved the first time the fixture reached
more of the game. **Nine occupied seats in one place, with views full and events
at the frame's cap, is strictly more of the game and had never been run.**

**Decide:** M6, before the first recording session, because the reversal costs a
re-recording and a re-recording costs nine people an evening.

**Hedge, in two parts.**

*Measure the case, rather than reasoning about it.*
`client/tests/cadence.rs` is the dense match: nine sessions on one server at the
game's own 30 Hz, the nine of them walking to the centroid and then standing
inside one another's `attack_range`, with one of them rasterising real frames
from real views into a real 1280×800 framebuffer while it captures. It reports
what one pass of the capture loop costs, at the real tick and at M4's compressed
ten milliseconds.

*And record it per session, so that a session that fell behind is visible in the
corpus rather than pooled into it.* `client::health::Cadence` counts the passes
that exceeded the budget and the worst overrun; the client prints both on the way
out and writes them into its session part; `Corpus::store` files them beside the
replay; and `replay census` reports how many sessions in the corpus are degraded
and by how much. `docs/SCHEMA.md` §5 carries the rule: **a degraded session is
never pooled into a distribution with sessions that are not.**

### What the measurement found, run 2026-08-13

One container, nine clients and a server sharing it, 700 ticks at 30 Hz, of which
the last 180 are the fight. The pass count is the loop's own, and the budget is
one tick derived from `sim::TICKS_PER_SECOND` rather than a number anybody chose.

| | `release` | `dev` |
| --- | --- | --- |
| Passes of the capture loop | 11 650 | 10 808 |
| Views reconciled / frames rasterised | 700 / 1 293 | 700 / 1 067 |
| Pass duration, median | 2.039 ms | 2.040 ms |
| …95th percentile | 2.291 ms | 5.492 ms |
| …99th percentile | 2.332 ms | 5.691 ms |
| **…maximum** | **5.144 ms** | **7.910 ms** |
| Passes over the game's tick (33.333 ms) | **0** | **0** |
| Passes over M4's compressed budget (10 ms) | **0** | **0** |
| Entities visible at once, at most | 11 | 11 |
| Events on the busiest view, against `MAX_EVENTS_PER_VIEW` = 16 | 11 | 11 |
| Views on which the measured client was under fire | 180 | 180 |

**The conclusion, and it is narrower than the numbers invite.** At nine seats in a
group fight, on this host, the worst single pass is **5.1 ms** optimised and
**7.9 ms** unoptimised, against a tick of 33.3 ms — so the budget holds with a
factor of six in hand, and it holds against M4's ten-millisecond harness too,
though only by a factor of one and a quarter in `dev`. The worry that raised this
entry was reasonable and the answer is that ten milliseconds is not short at nine
players; what is short is the margin in an unoptimised build, which is why the
harness compresses to ten rather than to four and why the number is recorded here
with the profile beside it.

**Four things this does not establish**, because a table is where a reader stops
asking:

- **It is one host, and the assertion is deliberately not.** These numbers are
  from one container. What the test *asserts* is machine-independent: that the
  fixture reached a dense match at all, and that `Cadence` counted every pass the
  loop made and agrees with an independently kept maximum and an independently
  re-derived overrun count. The distribution is printed, never thresholded — a
  threshold on a shared runner is a check that goes red for reasons that have
  nothing to do with this repository.
- **The dominant cost is the rasteriser, not the roster.** A pass median of 2 ms
  is `rasterize` over 1.02 million pixels, which is the same work at one visible
  entity as at eleven. So the *fight* is not what this measures well; what it
  measures well is that the fight does not add a pass long enough to matter. A
  future renderer, a larger window, or a machine slower than this one moves the
  number, and none of those is a change to `sim`.
- **Eight of the nine do not render.** They fill seats and fight, which is what
  makes the measured client's views full; they are not nine renderers on nine
  machines. The real session has more rendering and less contention, and the two
  push in opposite directions by amounts nobody here has measured.
- **A session that overruns is now visible, not prevented.** The corpus records
  the fact; deciding what to do with a degraded session is a judgement for whoever
  builds a distribution, which is the same division `docs/SCOPE.md` makes about
  detector findings and for the same reason.
- **One link in the chain is covered by no test and says so.** `Cadence` is
  tested directly, the loop is measured against a real match, the session part is
  round-tripped against the corpus's reader — but the two callbacks in
  `client::gfx` that *attach* the measurement to the playable client
  (`new_events` and `about_to_wait`) are checked by nobody, because `winit` needs
  a display server and CI has none. `client/src/gfx.rs` carries the admission
  where a reader will meet it. What limits the damage is that the failure mode is
  loud: an unpaired bracket records no pass at all, so a broken wiring produces a
  session part reporting `passes: 0`, which is a thing an operator reads as broken
  rather than as healthy.

### And the entry found one in the field, which is the reason to write it

While M6 was open, `check (windows-latest)` was failing on `main` — on the merges
of pull requests #6 and #7, intermittently, with `worst correction 13106 raw
units` in `client/tests/m4_exit.rs`. Two things about that are worth recording.

**It is this entry's subject and it had been read as a flake.** The assertion's
own message says "or the client fell behind the tick period", and that is exactly
what it was; the criterion had been re-run and had gone green, which is the
treatment that turns a real failure into a habit. A criterion that is red every
other run trains its reader to press the button again.

**The cause is the harness's compression, and R14's instrument had already
measured the thing that explains it.** Windows' default timer resolution is about
15.6 ms, which `client/tests/jitter.rs` records against the Windows run that made
it isolate the added latency. The M4 harness compressed the server's period to
**ten** milliseconds, so both ends of it were asking for wake-ups finer than the
host clock can give. **Compressing below the host's granularity is not a
compression; it is a different experiment**, and the number the criterion was
being asserted against was a property of the runner.

The period is the game's own 33 ms now and the compression is gone. What makes
that defensible rather than merely safe is the measurement above: the capture loop
at nine seats in a group fight has a factor of six in hand at 33 ms, so a
criterion that cannot hold there is a criterion about a game nobody can play
rather than about a scheduler. It costs that test twenty-three seconds.

One legibility defect went with it. A one-tick correction arrives as 13106 raw
units against a step of 13107 — a raw unit short, because the displacement
truncates toward zero — so integer division was reporting `0 tick(s) of movement`
about a one-tick correction, which sends a reader looking for an arithmetic bug in
`step`. It rounds.

### And it came back, because the period was not the whole of it — closed at M7

`check (windows-latest)` went red again after M6, on the same criterion, with the
same `worst correction 13106 raw units`, at the game's own 33 ms period. So the
compression was **not** the whole cause, and the entry above stopped one step
short of the diagnosis.

**What it actually is.** The prediction advances one tick of movement per
outstanding intention, which is exact exactly while the client and the server stay
in lockstep — one intention per tick, one tick per intention, which is the shape
`docs/ARCHITECTURE.md`'s one-message-per-player-per-tick produces. When **two of a
client's frames reach the server between two of its ticks**, the server drains its
whole queue into one tick and moves the champion once, while the client had drawn
it twice. The correction is exactly one tick of movement, from a prediction that
applied the rule perfectly. `client/src/predict.rs` had said so in its own header
since M4 — "a client that sent four intentions in one tick would … over-predict
and be corrected … a degradation of prediction quality, not of correctness" — and
the criterion asserting `worst_correction == 0` over every view did not know it.
Windows' 15.6 ms scheduling granularity against a 33 ms period is what made the
bunching frequent enough to see; it is not a Windows bug and no period removes it,
because it is a property of a real transport.

**So the criterion was the defect, and it is restated rather than relaxed.** What
it asserts now is the claim it was reaching for, under the condition that makes
the claim true: **on every view where the client had exactly one intention
outstanding when it drew and the server applied exactly that one, the prediction
is bit-exact.** Under that condition the server ran one tick applying one
intention and the client folded one intention forward by one tick of movement, so
a difference is the two disagreeing about how a champion moves and nothing else
can produce one. `client::predict::Reconciled` is the type that carries the two
counts, because a correction on its own cannot distinguish a wrong rule from a
bunched transport and those need different answers.

Three things travel with it, so the restatement is not a weakening in disguise:

- **A floor on the antecedent** (`docs/RISKS.md` R15): more than half the views
  must have been in lockstep, or the exactness above is a claim about a handful of
  ticks. On a healthy run it is 999 of 1000, so the floor has a factor of two in
  hand and is not a timing threshold.
- **Out-of-step corrections are reported and not thresholded**, with the one
  machine-independent thing there is to say about them asserted: a correction is a
  whole number of ticks of movement. A fractional one would mean the two ends
  applied different *rules* rather than a different number of ticks of the same
  one, which no transport jitter produces.

  **The slack that assertion spends was itself an observation, and Windows found
  the direction it had no room for.** It was one raw unit per tick, generalised
  from a single correction of 13106 against a step of 13107 — which is an
  axis-aligned step whose magnitude lost one unit to the *correction's* own
  `isqrt`, not a step that lost anything. `FxVec2::step_toward` normalises and
  then scales, both truncating toward zero per component, so a direction off the
  axes loses up to one unit on each and its magnitude loses up to two: a correction
  of **13105**, which `check (windows-latest)` produced and the bound refused. The
  slack is `2 × ticks + 1` now, and the 2 is a constant with a sweep behind it
  (`the_tick_shortfall_is_what_the_arithmetic_produces`) rather than a number in a
  comment, so a change to the speed, the fixed-point resolution or the rounding
  rule moves it there instead of in a Windows job six weeks later. This is the
  same defect this entry is otherwise about, one level down: a number taken from
  the case that happened to run.
- **Both halves were exercised.** A client whose `champion_speed` differs from the
  server's by **one raw unit** turns the lockstep clause red at `Blue0's prediction
  was corrected by 1 raw units … the client and the server disagree about how a
  champion moves`. And the Windows symptom was reproduced deterministically — the
  harness made to send two intentions between two server ticks on twenty of a
  thousand views — which produces exactly `worst correction 13106 raw units (1
  tick(s) of movement)`, is classified as twenty out-of-step views, and passes. The
  old assertion failed on that run with the CI message verbatim.

**What this does not fix**, because it is the same shape of thing R16 is about: an
out-of-step view still writes a one-tick prediction error onto a player's screen.
That is a quality-of-service fact about a real network, it is now counted rather
than fatal, and if it ever needs reducing the answer is in the client's send
policy rather than in this criterion.

### And the bunching was finally measured on both platforms, at M8

The two sections above diagnosed the same symptom twice and neither of them
compared the two platforms, because **the number was only readable on one**. The
M4 criterion asserts correctly on Windows and on Linux; its per-client counters
went through `cargo test` without `--nocapture`, and the report step that adds
`--nocapture` was Linux-only. So "does `windows-latest` bunch more than
`ubuntu-latest`" was a question about a run log that did not contain the answer —
`docs/RISKS.md` R15's failure committed on a report rather than on a fixture,
with the assertion still green.

That is fixed: the report step runs on both platforms, `client/tests/m4_exit.rs`
is in it, and the criterion prints one aggregate line naming the platform that
produced it. The first paired reading, on the pull request that made the change:

| | `ubuntu-latest` | `windows-latest` |
| --- | --- | --- |
| Views applied, three clients | 3000 | 3000 |
| In lockstep | 2997 | 2997 |
| **Out of step** | **0** | **0** |
| Worst out-of-step correction | 0 raw units | 0 raw units |

**No gap, and one run is not evidence of absence.** The failure this entry exists
for was described as *intermittent*, and an intermittent event that did not occur
in one run of one thousand ticks on one runner has not been shown not to occur.
What has changed is that the instrument exists on both platforms: every pull
request from here on produces a paired reading for free, and the entry can be
closed or reopened on a sample rather than on a diagnosis. The outcome to be
least smug about is a green pair on the first attempt, which is the same thing
`MILESTONES.md` says about the determinism matrix agreeing on its first run.

**Reopened by**, any one of: a renderer that is not a CPU rasteriser over this
scene; a window materially larger than 1280×800; a recording session whose census
reports degraded matches; any change that puts work on the thread between
`new_events` and `about_to_wait`; any harness that compresses a period below
the granularity of the clock the host can schedule; or a paired reading in which
the two platforms disagree.


---

## R17 — Person and device are perfectly confounded, and no amount of data separates them

**Not irreversible in the code, and unrecoverable in the corpus, which is the
worse of the two.** `docs/SCOPE.md` fixes nine participants and the sessions are
recorded on whatever hardware those nine people own — a decision taken on
purpose, because a production anti-cheat does not choose its players' mice, and a
corpus recorded on nine identical mice would demonstrate a detector that works on
one mouse.

The cost of that choice is precise and it is worth stating in one sentence:
**each hand appears with exactly one device, so every behavioural statistic this
project computes is a statistic about a person *and* their hardware, and nothing
in the design tells the two apart.** A mouse at 400 counts per inch and one at
1600 describe the same hand differently; a 125 Hz device and a 1 kHz one report
the same sweep differently; a driver's own scaling shifts every distance by a
constant. That is not variance that shrinks as the corpus grows. Nine people is
nine draws of the pair (hand, device) and there is no design under which nine
draws identify two factors.

**Decide:** before the first recording session, because the covariate cannot be
added to a corpus afterwards and re-recording costs nine people an evening.

**Hedge, and the shape of it is the whole entry: measure the device rather than
standardise it.** `client::lobby` turns the wait for the other players into an
instrument — elements at positions the build fixes, a `Ready` button inert until
the lobby has been crossed, a training dummy at a known distance — so that every
click is a movement with known endpoints and a measured cost in device counts.
`docs/SCHEMA.md` §4e is the schema and `docs/ARCHITECTURE.md` invariant 18 is the
test. What it buys is that a distance-shaped statistic can be computed in
normalised units instead of raw counts.

### What this does not fix, and it is most of the confound

The measurement is a **conversion**, not an identification, and the honest
statement is narrower than "the hardware has been controlled for":

- **`device_cpi` is still a declaration.** The lobby recovers device counts per
  *world unit*, never per inch, because a mouse reports counts and nothing in any
  stream this project records says what physical distance produced them.
  `docs/SCHEMA.md` §4c keeps the true CPI in the unknown column.
- **A scale removed is not a style separated.** Dividing out counts per unit
  makes two participants' distances comparable; it says nothing about the parts
  of a hardware response that are not a scale — a sensor's acceleration curve at
  speed, a switch's travel, the difference between a light mouse and a heavy one.
  Those arrive inside the same nine draws and stay there.
- **The report rate is a covariate and not a correction.** `docs/SCHEMA.md` §11f
  already says a detector reading an inter-arrival distribution stratifies by
  polling rate or says it did not, and a measured rate makes that stratum
  *readable* rather than removable.
- **Nine is still nine.** R8's arithmetic is untouched: the bound on anything a
  person's style drives is `3/9 ≈ 33%` and no measurement of a mouse moves it by
  a point.

So what this entry claims, exactly: the corpus can now say **which part of a
distance is the device's scale**, and it cannot say which part of a *style* is
the device's. The second is the confound and it stays open, named rather than
absorbed — which is the same register `docs/SCOPE.md` uses for the ceiling of
behavioural detection and for exploit class 6.

### And the failure mode the hedge introduces, which is why it is not free

A measurement taken through the interface is a measurement that inherits the
interface. Two things guard it and both are tests rather than arguments:

- **the lobby must not read the display.** A menu driven by the operating
  system's pointer would measure the accelerated pointer — R14's failure in a new
  place — and the scale would not be the scale the match is played at.
  `client/tests/lobby.rs` drives the same device events through two clients six
  times apart in pixels per world unit and requires identical everything.
- **the geometry must reach the cases the criterion is about.** A station table
  re-tuned for how it looks on screen is a table that quietly stops covering
  eight octants, and a measurement aligned on too few directions is exactly the
  anisotropy R14 records. The table carries its own reach assertion
  (`docs/RISKS.md` R15).

**Reopened by**, and this is the whole list: a participant recording on two
devices under one label, which the corpus marks as `mismatched` rather than
detects; a change to `client::input::WORLD_UNITS_PER_COUNT`, which changes what
every recorded scale means and must not happen after the first session; or a
detector whose null model needs a hardware property that is not a scale, at which
point the entry is open again and the answer is not more geometry.
