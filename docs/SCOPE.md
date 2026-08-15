# SCOPE

A minimal MOBA used as a substrate for anti-cheat engineering. The game is the
test fixture; the anti-cheat is the subject. Both the attack and the defense
live in this repository.

## Design assumption

**The client is compromised and lying.** This is not a threat to mitigate, it is
the starting axiom. Every defense in scope must hold when the attacker controls
the client binary, its memory, its clock, and its input stack. Any defense that
only works against an unmodified client is out of scope by construction.

## Adversary model

In scope:

| The attacker can | Consequence for design |
| --- | --- |
| Read and modify all client memory and code | No secret may exist client-side |
| Replace the client entirely and speak the protocol directly | The protocol is the only trust boundary |
| Control the client clock and input timing | Only server-observed time is evidence |
| Record and replay traffic | Session state must be sequence-bound |
| Observe packet sizes and arrival times | Side channels count as information leaks |
| Automate play with synthetic inputs | Behavioral statistics are the only lever |

Out of the adversary model (and therefore not defended against):

- Hardware input injection producing statistically human timing and trajectories.
  This is the theoretical ceiling of behavioral detection. Naming the ceiling is
  part of the deliverable.
- Attacks on the server host, the CI, or the operator's machine. Standard
  software security, not anti-cheat.
- Collusion between human players (win trading, intentional feeding) *within a
  team*. Social problem, detected socially. Collusion **between two of the three
  teams against the third** is a different thing and is in the table below as
  class 6, because the three-team format makes it a way to obtain information
  rather than only a way to throw a match.

## In scope

### Game (MVP, frozen)

**3v3v3 on a triangular map**: three teams of three, a base at each vertex, a
lane along each edge, and every lane contested by exactly the two teams whose
bases it joins. One shared champion. Fog of war. One skillshot, one targeted
spell, a basic-attack range. Towers, two per team, one on each lane leaving its
own base. No items, levels, minions, jungle, or matchmaking. Tick-based
authoritative server, client-side reconciliation.

The third team is not a game-design flourish and it is not free: it is the
format that introduces an exploit class the two-team game does not have (class
6 below), and it was taken at M3 rather than later because a corpus of human
matches recorded at six seats is unusable at nine, and so is every replay
recorded under it. Changing the roster after M4 destroys data; changing it
before M4 destroys nothing.

The MVP is frozen. Additions to the game are only in scope when a specific
anti-cheat experiment requires them, and the milestone that requires them says
so. In particular the vision model stays **discs without occlusion**: brushes
and line-of-sight are the obvious thing a fuller MOBA map would add and they are
deliberately not here, because there is no maphack in the repository yet to test
them against. They are reconsidered after M7, when there is.

### Structural invariants

These are not features, they are properties the codebase must never lose:

1. `step(&State, &[Input]) -> State` is pure: no clock, no I/O, no async, no
   floating point, explicit seed, fixed-point arithmetic.
2. Per-player visibility is a projection, `view_for(&State, Seat) ->
   PlayerView`, computed server-side and applied before serialization. An entity
   outside vision is **absent from the message**, not flagged invisible. The
   seat is a nine-valued type rather than an integer, so "whose vision is this"
   has no answer outside the match to get wrong; the byte that names it is
   validated at the protocol boundary or refused there. With three teams the
   projection carries a second obligation: a view must distinguish the two
   *enemy* teams only by what it actually shows, so a field naming the nearest
   enemy team, a counter kept per enemy team, or an ordering correlated with
   team membership are all leaks by construction.
3. `State` is not serializable anywhere in the workspace. Only `PlayerView`
   crosses the wire. This makes the maphack defense a compile-time property
   rather than a code-review habit.
4. Every playable build records replays (seed + timestamped inputs) from the
   first playable build onward.

### Anti-cheat (the actual project)

Five exploit classes. Each defense is only considered delivered once the
matching exploit exists in the repository and fails against it in CI.

**As of M7 the last column is no longer an intention.** Every row below carries
the exploit that attacks it and whether the defense holds against that exploit,
because M7 is the milestone at which the word *delivered* stopped being a promise
about future work. The exploits live in `cheat-client/tests/`; each is run twice,
against a weakened version of the defense that does not stop it and against the
one this project ships, and the test is red if either half comes out wrong.

| # | Exploit class | Defense in scope | Exploit | Delivered? |
| --- | --- | --- | --- | --- |
| 1 | Maphack: reading information not visible | Strict server-side culling; message-size padding against traffic analysis; culling of *derived* signals (sound, damage events, cast events) | `tests/maphack.rs`, `tests/traffic.rs` | **Yes.** The maphack places every living enemy against an omniscient projection and exactly the visible ones against this one; the wiretap reads the entity count off an unpadded stream and sees one packet shape on the real one |
| 2 | Result forgery: unplayed match, edited replay | Signed replays; server-issued match records; offline resimulation of the input log | `tests/forgery.rs` | **Yes.** A forged replay of a match nobody played verifies against a registry that trusts the forger's key and is refused by the one that does not; every byte-level edit of a genuine replay is caught, inside the signature or by the manifest's commitment to the log |
| 3 | Synthetic input and botting | Server-arrival-time telemetry; behavioral statistics calibrated on a human corpus; account-progression coherence over time | `tests/botting.rs`, `cheat_client::bot::Reflexes`, `anticheat/tests/detectors.rs` | **No, and correctly — and M8 narrowed it without closing it.** The bot plays a whole match, the server accepts every frame, the replay verifies. Two behavioural detectors now respond to it, a reaction floor and a reaction dispersion, and **neither has a calibrated threshold** because there is no human corpus; a third variant with plausible reflexes defeats both and is the ceiling executed. `docs/detectors/` |
| 4 | Time manipulation: slowdown, clock desync | Divergence between client-claimed and server-observed input timestamps as a first-class signal | `tests/clock.rs`, `anticheat/tests/detectors.rs` | **Yes for the inertness, and uncalibrated for the detection.** Four different claimed clocks produce one identical world digest, because no rule reads the field. M8's `clock-divergence` reads the divergence as a *rate* and separates a half-speed clock from an honest one whose epoch is a trillion milliseconds away — with no threshold, for the same reason as class 3 |
| 5 | Protocol abuse: replay, concurrency, out-of-sequence | Monotonic input sequence numbers, idempotent session commands, per-player rate limits | `tests/abuse.rs` | **Yes.** A replayed intention is applied once, a stale sequence number is refused, a second `Join` is out of order, an unresolvable or friendly handle never becomes an order, and hostile byte strings are refused at the frontier |
| 6 | Cross-team collusion: two teams cooperating against the third, including sharing vision outside the game | **None applicable.** See below | **None, deliberately** — `tests/collusion.rs` is a demonstration and not an exploit | Not applicable, and that is the finished state |

Two things that column does not say, and a reader is entitled to both.

**"Delivered" is a statement about an exploit, not about safety.** It means this
repository contains an attack that works against a weakened version of the defense
and fails against this one, in CI, permanently. It does not mean the class is
closed: it means the specific attack written here does not get through, and
`SCOPE.md`'s own note below on self-adversarial testing is the limit — you cannot
find the exploit you did not think of.

**One exploit in class 1 lands and is not defended.** A projectile is shown with
its position and its velocity and no owner, and the velocity is constant for its
life, so an attacker can run it backwards and recover the ray its caster stood on
— a caster the fog was hiding.
`tests/maphack.rs::a_projectile_betrays_the_ray_its_caster_stood_on` executes it.
What it recovers is a line rather than a position, and what would close it is
removing projectiles from views (not a game) or capping the entity list, which
`ARCHITECTURE.md` refuses because it trades a length channel for a content
channel. It is recorded in the same register as the behavioral ceiling: named,
not defended.

Note on class 5: a QUIC/TLS transport already defeats naive packet replay and
reordering. The in-scope work is the application-level residue — session
commands and input sequencing — and the documentation must say which layer is
doing the work rather than claim credit for the transport.

Note on class 6, and it is the note that matters most about it: **no technical
defence applies, and this project will not pretend one does.** A three-team
format creates a coalition a two-team format cannot have, and the useful thing
two colluding teams exchange is *vision*: each is entitled to what it sees, and
putting the two entitlements together on a voice call produces a map neither
player could obtain from the protocol. Every frame involved is correctly culled.
Every message is one the server intended to send. There is no invalid input, no
malformed frame and no impossible action for the server to reject, because
nothing invalid happens — the leak is two people talking, outside the system.

That places it squarely in the register of `SCOPE.md`'s adversary model line
"automate play with synthetic inputs": behavioural statistics are the only lever
that reaches it at all, and even then only as a weak one — coordinated
positioning between two teams looks like two teams that happened to move well.
It is recorded as a class rather than omitted for the same reason the ceiling of
behavioural detection is recorded: naming what the design cannot defend is part
of the deliverable, and a reader who notices the hole before the document does
is entitled to conclude the rest was written the same way. No milestone
delivers a defence for it, and M8's detectors do not claim one.

Note on class 2: against a fully authoritative server, resimulating the server's
own inputs proves the server did not corrupt itself; it does not catch a
cheating client. Resimulation earns its place at the surfaces where a
**client-supplied artifact asserts an outcome**: replay files, submitted match
records, third-party verification of a published replay, and offline reanalysis
at higher-than-realtime cost. That is where it is scoped, and where the tests
live.

### What M8 added to that table, and the word it is not allowed to use

**M8's detectors exist and none of them is a delivered defence.** `SCOPE.md`
reserves that word for a class with a matching exploit *failing* against it in
CI, and nothing fails against an uncalibrated threshold — a detector with no
threshold cannot refuse anybody, which is why classes 3 and 4 still read "no" in
the column above.

What the milestone did deliver is in `docs/detectors/`: three statistics with
stated null models, an exploit and a control for each, an evidence bundle, and
the arithmetic that says what a corpus would have to be before any of it becomes
a threshold. The exploit suite gained `cheat_client::bot::Reflexes` and
`ClaimedClock`, and `anticheat/tests/detectors.rs` asserts that each detector
responds to its own exploit and is quiet against the same match played without
the behaviour.

**Two of M8's five candidate signals cannot be built at all, and it is not a
question of calibration.** The kilohertz device stream is deliberately outside
the artefact resimulation is a function of and reaches the corpus as four summary
numbers per seat (`docs/SCHEMA.md` §3, §4b), so "input inter-arrival distribution
and quantisation" has no distribution to read; and the aim reaches the wire only
at the instant of a click, so an aim-curvature detector has no trajectory to
compute over. `docs/detectors/README.md` carries both verdicts at length,
including why neither reopens `evdev`: the binding resolution is the record's
millisecond and the tick, not the capture path's 16 µs.

**And a bot with human-plausible reflexes defeats both reaction detectors.** That
is the ceiling this document already names — hardware input injection producing
statistically human timing — arriving as a green test rather than as a paragraph,
and it is approached from below rather than measured, because `docs/RISKS.md` R7
refuses to publish the device-injection layer that would measure it.

### The corpus is nine people, and here is what that fixes

**Decided, and not open for revision: nine distinct participants in total** — the
nine seats of a 3v3v3 match, the same people from one match to the next.
`MILESTONES.md` M6 carries the calendar arithmetic. What belongs here are the
three consequences, written before the first session rather than discovered at
M8, because each of them constrains what M8 is allowed to claim.

**1. The "style" bound is about 33% and more matches do not improve it.**
`RISKS.md` R8's rule of three: zero false positives observed over `N` independent
trials supports an upper bound near `3/N` at 95% confidence, and what counts as
`N` depends on what the detector reads. Anything driven by *a person's style* has
`N` = the number of distinct **people** — nine, forever, at any number of matches
— so the supportable claim is `3/9 ≈ 33%`, and recording a hundred more matches
from the same nine people does not move it by a single point. Only the
*circumstances* bound improves with matches: `3/40 ≈ 7.5%`, or `3/20 ≈ 15%` at the
reduced count M6 proposes.

**The two bounds appear together wherever a claim is made.** In every detector
document at M8, in every published statistic, in `replay census`'s own output. A
reader shown only the friendlier one has been handled, and the friendlier one is
always whichever the author is quoting.

**2. M8 can only produce detectors that flag for review. No automatic sanction.**
This is a decision taken here, not a limitation discovered later. **No threshold
calibrated on nine people supports an automatic penalty of any kind** — not a ban,
not a suspension, not a queue restriction, not a silent match-quality adjustment.
A 33% upper bound on the false-positive rate means one in three flagged players
could be innocent and the corpus cannot rule it out; acting on that automatically
would be punishing people on evidence this project itself says is insufficient.
So M8's detectors emit a score and an evidence bundle, a human reads them, and the
decision is the human's. That is consistent with the "Automatic bans" exclusion
below and is the stronger form of it: the exclusion says a false positive is worse
than a missed cheater, and this says why the arithmetic forbids the alternative at
this corpus size.

**3. Generalising to a hand this project has never seen is out of reach, and this
document says so in plain words.** A detector calibrated on nine people has
learned nine hands. **It says nothing about a tenth player.** Not "less", not
"with wider bounds" — nothing: a null model for human behaviour is a distribution
*over humans*, nine draws do not characterise one, and there is no statistical
treatment that recovers from that. So no claim of the form "this detector achieves
X on players in general" may be made anywhere in this repository, at any corpus
size it can reach. What may be claimed is what was measured: how these detectors
scored on these nine people and on the bots in `cheat-client`, with both bounds
beside it.

**4. Person and device are perfectly confounded, and measuring the device is the
most that can be done about it.** Nine participants play on nine mice — a choice
taken on purpose, because a production anti-cheat does not choose its players'
hardware and a corpus recorded on nine identical mice would demonstrate a
detector that works on one mouse. The price is that every hand appears with
exactly one device, so every behavioural statistic here is about a person *and*
their hardware and nothing identifies the two separately. `docs/RISKS.md` R17
carries it and `docs/SCHEMA.md` §4e is the parade: the lobby is laid out so that
crossing it measures the map from device counts to world units, which lets a
distance-shaped statistic be computed in normalised units instead of raw counts.

**What that fixes is a scale and not a style, and the difference is the claim.**
The conversion makes two participants' distances comparable. It says nothing about
the parts of a hardware response that are not a scale, `device_cpi` remains a
declaration nobody can check, and the `3/9 ≈ 33%` bound is untouched — a
measurement of a mouse moves it by nothing. So no page in this repository may say
that hardware has been controlled for; what may be said is that the scale was
measured and the rest was not.

### What M6 established about synthetic play, and where authenticity comes from

The corpus refuses a seat that recorded **zero device events**. That is the one
mechanical thing a file can say about synthetic play, and it catches a scripted or
headless client — one that drives the protocol and never touches an input device.

**A bot that moves a real mouse is indistinguishable in a file.** It records
exactly as many samples as a person, at the same rate, through the same capture
path. There is no field in `SCHEMA.md` that separates them and there will not be
one, because the difference is not in the data.

**M8's telemetry companion does not change that sentence and is worth reading
against it.** `SCHEMA.md` §11 keeps the whole device stream now rather than four
summary numbers per seat, so a great deal more is in the file than there was —
and a bot moving a real mouse produces exactly that stream too. What the companion
changes is which *behavioural statistics* can be computed at all; what it does not
touch is the ceiling above, which is a claim about hardware input injection
producing statistically human timing and is out of the adversary model by
construction. A larger file does not move a ceiling that was never about file
size.

So **what guarantees a match is human is supervision — a fact about a person, not
a property of the format.** Somebody was in the room. That is a real guarantee and
it is the only one there is, which is why it is written down rather than
remembered: every session record carries the conditions it was recorded under —
in person, remote, or unsupervised — so that M8 can stratify and a reader can tell
what a claim rests on. `SCHEMA.md` §5a is the schema and
`cheat-client/tests/botting.rs` is both halves executed: the bot plays a whole
match nothing catches, and the silent-seat check catches the headless version and
not the mouse-moving one.

### Cheat client

A first-class crate, not a test utility. It speaks the protocol directly, never
links the real client's internals, and each exploit is expressed as a headless
assertion — "the attacker learns X" or "the server accepts Y" — so the whole
suite runs in CI without rendering.

**Delivered at M7.** `cheat-client`'s only workspace dependency is `protocol` —
`sim` appears beneath it, because `protocol`'s own message types are stated in
`sim`'s and anything that speaks this protocol reaches them — and it links no
`client`, `server`, `replay` or `anticheat`. The exploit harness links `sim`,
`server` and `replay` as dev-dependencies, which is the division that makes an
exploit mean anything: the judge needs the truth, and the attacker must not have
it. `ci` asserts both directions with `cargo tree`, including that no production
crate links the attacker.

**Every exploit is run twice and the test is red if either half fails.** Once
against a weakened defence that does not stop it, and once against the one this
project ships. The first half is `RISKS.md` R15 applied to attacks: an exploit
that fails against the real defence *without ever having worked* proves nothing —
it looks exactly like a defence that holds, and there is no red to tell them
apart.

### Deferred sub-projects (in order)

1. Anti-cheat — the subject.
2. Reinforcement-learning bots — requires a fast headless environment around `sim`.
3. Matchmaking.

They are named to constrain today's architecture (a headless, allocation-cheap
`sim` API; a player-identity notion that survives across matches), not to be
built now.

## Out of scope, with reasons

### Client-side anti-tamper — excluded on evidence

Excluded: obfuscation, asset encryption, custom VM, debugger detection, binary
anti-tamper, kernel-mode anti-cheat.

Reason: the reference devlog this project draws from spent over a month on
client hardening; the first build was decompiled in two minutes with a public
tool, antivirus vendors flagged the binary, and the author deliberately removed
defenses at the end because the cost imposed on legitimate players did not
justify them. The defenses that survived are the server-side ones. This project
starts from that conclusion instead of re-deriving it. A kernel driver in
particular is unshippable for a solo open-source portfolio project and its
absence is a feature.

### Other exclusions

| Excluded | Reason |
| --- | --- |
| Automatic bans, and every other automatic sanction | A false positive is worse than a missed cheater, and at nine participants the arithmetic forbids the alternative outright: no threshold calibrated on nine people supports a penalty, because the supportable bound on the false-positive rate is about 33%. Detectors emit scores and evidence bundles; acting on them is a human decision. See "The corpus is nine people" above |
| Machine-learned detection classifiers | Cannot be honestly calibrated on the tens of human matches a solo project can collect. Physically-motivated statistics with defensible null models instead |
| Anti-cheat for other games | Stated in `SECURITY.md`: the cheat client targets this project only, and contributions targeting other games are refused |
| Rollback/GGPO netcode | Real engineering, unrelated subject, and it would compete with the tick-based authority the anti-cheat depends on |
| ECS or game framework inside `sim` | Iteration-order and parallel-scheduling nondeterminism directly attacks invariant 1. Frameworks are allowed in `client` only |
| Scale, ops, live service | One server process, matches counted in dozens. Nothing here demonstrates operating at scale and nothing should pretend to |
| Game design, art, audio, UX | The game is a fixture |
| Cross-play with real clients, anti-cheat SDK, plugin system | Two consumers do not justify a plugin system, one champion does not justify a champion trait |
| Reproducible binary builds | See `ENGINEERING.md`: enormous cost in Rust, near-zero portfolio value; provenance attestation gives the property people actually check |

## What this project demonstrates

- Designing a deterministic simulation as a verification primitive, and holding
  that property across three CPU targets under CI.
- Stating a side-channel property that only a three-team format makes
  expressible — that a view distinguishes two enemies only by what it shows —
  and exercising it by mutation rather than asserting it.
- Making a security property structural (`State` cannot be serialized) rather
  than procedural.
- Adversarial engineering discipline: exploit first, defense second, exploit
  retained forever as a regression test.
- Statistical detection with an honest error budget, including what a small
  corpus can and cannot substantiate.
- Solo-scale industrial hygiene: pinned toolchain, supply-chain policy, signed
  and attested releases, automation chosen for comprehension rather than count.

## What this project does not demonstrate

- That the anti-cheat would survive a real player population. It will never be
  attacked by anyone but its author, and self-adversarial testing has a known
  blind spot: you cannot find the exploit you did not think of.
- Client integrity, binary protection, or anything requiring a trusted client.
- Anti-cheat at scale, incident response, appeals processes, or ban policy.
- Netcode research, game design, or graphics.
- Detection quality with statistical power. With a corpus of tens of matches,
  a claimed false-positive rate below a few percent is not supportable, and the
  documents will report bounds instead of point estimates.
- **Anything about a player this project has never recorded.** The corpus is nine
  people. A detector calibrated on it has learned nine hands and says nothing
  whatever about a tenth — not "less confidently", nothing — because a null model
  for human behaviour is a distribution over humans and nine draws do not
  characterise one. No claim of the form "this detector achieves X on players in
  general" appears anywhere in this repository, at any corpus size it can reach.
