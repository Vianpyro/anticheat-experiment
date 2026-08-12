# ARCHITECTURE

Seven crates in one Cargo workspace. The boundaries exist to make two security
properties structural rather than procedural: the client cannot receive
information it should not see, and the detection logic never ships to the
attacker.

```
                 sim  (pure, no workspace deps)
                  |
        +---------+---------+---------+
        |         |         |         |
    protocol   replay   anticheat     |
        |         |         |         |
   +----+----+    |         |         |
   |         |    |         |         |
client   server --+---------+         |
             |                        |
        cheat-client ------------------
        (protocol only)
```

## Dependency rules

| Crate | Owns | Depends on | Must never depend on |
| --- | --- | --- | --- |
| `sim` | The rules of the game. `State`, `Input`, `step`, `view_for`, fixed-point math, seeded RNG | Nothing in the workspace. Externally: a fixed-point crate; `serde` only for view types | Anything with a clock, an allocator strategy, I/O, async, threads, or floats |
| `protocol` | The wire. Message types, framing, versioning, sequence numbers | `sim` (for `PlayerView`, `Input`, ids) | `server`, `client`, `anticheat`, any runtime |
| `replay` | The replay container: format, signing, verification, resimulation | `sim`, `protocol` | `server`, `client`, `anticheat`, any runtime |
| `server` | Authority. Tick loop, the clock, sockets, sessions, fog application, telemetry capture, replay recording | `sim`, `protocol`, `replay`, `anticheat`, a runtime | `client`, `cheat-client` |
| `client` | Presentation. Rendering, input capture, prediction, reconciliation | `sim`, `protocol`, a game framework | `server`, **`anticheat`**, `replay`'s signing keys |
| `anticheat` | Detection. Feature extraction from telemetry, detectors, thresholds, evidence bundles | `sim`, `replay` | `server` (it is called by the server, not the reverse), `client`, any network or filesystem I/O |
| `cheat-client` | The attacker, and the exploit suite | `protocol` only, plus `server` as a dev-dependency for the in-process harness | `sim` internals, `client`, `anticheat` |

Three of these deserve their reason stated:

**`client` must not depend on `anticheat`.** Shipping detector logic to the
machine you assume is compromised hands the attacker your thresholds. All
detection runs server-side or offline over recorded telemetry. This is why
`anticheat` does no I/O — it is a pure function from telemetry to scores, which
also makes it replayable and testable without a server.

**`cheat-client` must not depend on `sim` or `client`.** An exploit that reaches
into the real client's internals is not an exploit, it is a test double. The
attacker's only legitimate surface is the protocol, so that is its only
dependency. It reimplements whatever it needs.

**`sim` must not depend on anything in the workspace.** It is the verification
kernel: the same `step` runs in the server, in replay verification, in the
determinism suite, and eventually in the RL environment. Any upward dependency
makes those four consumers diverge.

`server` is a library with a thin binary. The exploit suite boots it in-process;
that is the only reason the split exists.

No `xtask` crate. Cargo aliases in `.cargo/config.toml` cover the handful of
composite commands, and a crate for two commands is a crate to maintain.

## Central types

Signatures only. Field lists are indicative — what matters is the shape of the
boundary.

### `sim`

```rust
#![forbid(unsafe_code)]
#![deny(clippy::float_arithmetic, clippy::arithmetic_side_effects)]

pub struct Tick(pub u32);
pub enum Seat { Blue0, Blue1, Blue2, Red0, Red1, Red2 }   // and there is no seventh
pub struct EntityId(pub u16);
pub struct Fx(i32);                 // Q15.16: i32 read as a multiple of 2^-16
pub struct FxVec2 { pub x: Fx, pub y: Fx }

pub struct State { /* tick, rng, next_projectile_id, [Champion; 6], towers, projectiles, events, outcome */ }

pub struct Input {
    pub tick: Tick,               // tick this input applies to
    pub seq: u32,                 // per-player, monotonic; protocol-level identity
    pub player: Seat,             // written by the server from the session, never by the sender
    pub action: Action,
}

pub enum Action {
    Idle,
    Move(FxVec2),
    Skillshot(FxVec2),            // direction
    Targeted(EntityId),
    Attack(EntityId),
}

/// Pure. No clock, no I/O, no async, no floats. Inputs are pre-sorted by
/// (player, seq) by the caller; `step` does not sort and does not deduplicate.
pub fn step(state: &State, inputs: &[Input]) -> State;

/// The same function with the constants passed in rather than read from
/// `RULES`. Not a configuration hook: see "Rules a fixture brings with it".
pub fn step_with_rules(state: &State, inputs: &[Input], rules: &Rules) -> State;

impl State {
    /// The only way to compare states. `State` is deliberately not serializable;
    /// see docs/RISKS.md R5.
    pub fn digest(&self) -> Digest;   // [u8; 32]
}

pub fn new_state(seed: u64) -> State;
```

### Fixed point: the domain, and what happens outside it

`Fx` is Q15.16 — an `i32` read as a multiple of `2^-16`. Representable range
`[-32768, 32767.99998]`, resolution `0.0000153`. The tick rate is **30 Hz**.
Both are frozen by `RISKS.md` R2 and both are covered by `rules_hash()`, so a
change to either is a loud verification failure rather than a silent
resimulation into a different match.

**The legal domain**, which the property tests assert and which is what "the
operations do not overflow" is a claim *about*:

| Quantity | Domain | Why that bound |
| --- | --- | --- |
| Coordinates, both axes | `[-128, 128]` | The product of two in-domain values is at most `16384`, comfortably inside the type. This is the bound that makes multiplication closed |
| Per-tick displacement, per component | `[-16, 16]` | Two orders of magnitude above any speed in the rules. A displacement is added to a coordinate, so the sum stays inside the type |
| Divisor, relative to dividend | `abs(a) <= abs(b)`, `b != 0` | Division is *not* closed on the coordinate domain: `128 / 2^-16` is far outside the type. The rules only ever divide a vector component by that vector's own length, where the quotient cannot exceed one, and that is the domain stated |
| Direction to be normalised | length `>= 1/16` | Below that the integer square root has too few significant bits: `(2^-16, 2^-16)` normalises 41% too long. Shorter directions are discarded by the rules, not normalised |

Positions and hit points are held inside these bounds by the rules themselves —
clamped, never rejected — so a client sending a coordinate of `i32::MAX` moves
its champion to the edge of the map and produces no error path.

**Outside the domain, arithmetic saturates.** This is a decision, and the two
alternatives were rejected for stated reasons:

- *Not wrapping.* Wrapping is what `release` does to `i32` by default, and it is
  the behaviour this whole type exists to remove. A position that wraps
  teleports across the map; a position that saturates stops at the edge. Only
  one of those is debuggable, and neither is supposed to happen.
- *Not panicking.* `step` runs inside an authoritative server for six players
  and a panic is a match everybody loses. A `checked_*` API returning `Option`
  would push an error path through every rule for a condition the domain already
  excludes. A total function has no failure path to get wrong.

Saturation is still a silent change of value, so it is not treated as a design —
it is a floor under the failure. The property tests assert that no operation
saturates anywhere inside the legal domain, by requiring the `checked_*` form to
return `Some`; saturation is thus provably unreachable in-domain rather than
merely unlikely.

**Overflow checks are on in every Cargo profile**, `dev`, `release`, `test` and
`bench` alike. Cargo's default — panic in debug, wrap silently in release — is a
difference in what arithmetic *means* between the build that gets tested and the
build that gets shipped, which a project claiming bit-identical results across
platforms cannot also tolerate across profiles. The determinism job runs
`--release` on all three targets for the same reason: one profile, one meaning.
The flag is a tripwire rather than the mechanism — `sim` reaches its saturating
semantics through explicit `saturating_*` and `checked_*` calls, which the flag
does not affect — and what it catches is a bare `+` arriving in a crate that was
supposed to have none.

Rounding: multiplication and division truncate **toward zero**, not to nearest
and not toward negative infinity. Toward zero keeps the type symmetric about the
origin, so `(-a) * b == -(a * b)`; on a map whose origin is the middle of the
lane, a rounding rule that drifts one way is a rounding rule that treats the two
teams differently.

### `State::digest()` and the field somebody adds in eight months

`State` is not serializable, so the digest cannot lean on a derive: the encoding
is written by hand, fixed-width big-endian, one tag byte before every enum and
every `Option`. The risk in a hand-written encoding is not that it is wrong at
the start — a wrong encoding fails the first time the fixture runs. It is the
field added later and not hashed. Nothing would break: the determinism suite
would stay green while quietly no longer covering part of the state, and the
first symptom would be two servers disagreeing about a match they had both
digested identically. That is a silent gap in every claim built on top of the
digest, up to "this replay is the match that was played".

So the encoding is written by **exhaustive destructuring**, and this is an
invariant rather than a style: `..` is forbidden in a pattern in that module and
so is a `_` arm in a `match`. Adding a field to `State` stops
`let State { .. } = self` compiling; adding a variant to `Order` stops its
`match` compiling; binding a field and forgetting to hash it trips
`unused_variables`, which the crate denies at its root so the invariant holds
under a plain `cargo build` and not only under CI's `-D warnings`. The same
treatment covers every nested type, and `Rules` as well — a balance constant
that escaped `rules_hash()` would defeat R2's hedge in exactly the same way.

The hash is SHA-256, written out in the crate rather than depended upon, pinned
to FIPS 180-4 by its published test vectors. `sim` has no dependencies and this
is one of the two places that policy costs something; it is worth paying, since
the digest has to produce the same 32 bytes for as long as any replay exists.
Signatures at M5 are a different matter and will take an audited crate — a
security portfolio does not hand-roll signature code.

### `State::events` — what happened during the tick

`State` carries the record of the transition that produced it: casts, damage and
deaths, each with the point it happened at, cleared at the top of every tick.
Two things follow from putting it there rather than returning it beside the
state.

The frozen signatures have nowhere else to put it. `SCOPE.md` fixes
`step(&State, &[Input]) -> State` and `view_for(&State, Seat) -> PlayerView`,
and a tuple-returning `step` would be a second signature for the RL sub-project
to diverge on (`RISKS.md` R10).

More importantly it is then under `State::digest()`. Events reach clients, so
two servers that agree on the world and disagree about what their players were
*told* is a divergence with anti-cheat consequences, and it now fails the
determinism suite rather than arriving as a player report.

Each event carries the position it happened at, and that position — not the
entity's identity — is what the projection culls on. The reason is a case that
would otherwise need an exception: a champion killed this tick is off the map by
the end of it and has no current position to test, so an identity-based rule
would need a branch for deaths, and a branch in the culling function is where a
maphack lives. The arena is a fixed array of 48; beyond that events are dropped,
identically everywhere, in the same spirit as a full projectile arena.

### `sim::view` — the visibility projection

Separate module, separate call, computed from the full world state. `step` never
reads it, and `State` carries no per-player visibility.

```rust
/// Strict culling: an entity outside vision is absent from the result, not
/// flagged. Derived signals (damage events, cast events, sound cues) are culled
/// on the same rule.
pub fn view_for(state: &State, player: Seat) -> PlayerView;
pub fn view_for_with_rules(state: &State, player: Seat, rules: &Rules) -> PlayerView;

pub struct PlayerView {
    pub tick: Tick,
    pub outcome: Outcome,
    pub own: OwnView,                 // full detail, always
    pub visible: Vec<EntityView>,     // only what this player can see
    pub events: Vec<VisibleEvent>,    // culled on the same rule
}

impl PlayerView {
    pub const MAX_ENCODED_BYTES: usize;
    pub fn encode(&self) -> Vec<u8>;  // hand-written, canonical, no serde
}
```

#### The seat is a type, not a number

`view_for` takes a `Seat`, which has exactly six values. Until M3 it took
`PlayerId(pub u8)` with a comment saying `0..6`: `Team::of_player(PlayerId(200))`
answered `Red`, and the projection answered a seat nobody was sitting in with a
view built around an invented champion. Inert while the only caller was a test
that wrote the number itself; from M3 the seat comes from a session and the
session comes from the network, and the function that would have handed a team's
vision to an unvalidated handle is the most sensitive one in the repository.

The fix is the type rather than a check inside the projection, and that
distinction is the whole of it. A check would be a branch, a branch in the
culling function is where a maphack lives, and the branch that existed —
"a seat outside the match gets a plausible view" — is exactly the one an
attacker wants to reach. `Seat` deletes the case instead of handling it. What
remains is `Seat::from_index(u8) -> Option<Seat>`, whose only caller is
`protocol`'s decoder: an untrusted byte becomes a seat at the frontier or the
frame is refused there. `Input::player` is the same type and is written by the
server from the session, never from the message, so "a client drove somebody
else's champion" is not a rule that has to reject it either.

Vision is a union of discs: a living champion of the player's own team covers
`champion_vision_radius`, a standing tower covers `tower_vision_radius`. It is
**team** vision, which is the MOBA model and also the one with no ally-only side
channel to get wrong. A dead champion and a destroyed tower see nothing, so
losing a tower costs map control.

`PlayerView` is the sole state type crossing the wire. Because `State`
implements no serialization anywhere in the workspace, "the server accidentally
sends the whole world" is a compile error rather than a bug class.

#### What is in `PlayerView`, field by field

This type is the serialization frontier of the project: what enters it is what a
client can learn, and therefore what an attacker can learn. So the justification
is per field, and the absences are decisions rather than omissions.

| Field | Why a client may have it |
| --- | --- |
| `tick` | Needed to order and reconcile. Public by construction: the server emits one view per player per tick regardless of content, so the number is implied by the message existing |
| `outcome` | The match ending, and who won, is a global fact the moment it happens. Withholding it hides the end of the game from the loser |
| `own` — id, position, liveness, cooldowns | The player's own champion. Nothing here is secret *from this player*, and its respawn timer is its own |
| `visible` — champion: id, position, hp | What an observer standing there would see. Team follows from the handle, which is the seat, and is public |
| `visible` — tower: id, position, hp | The position is already derivable from the rules; the hit points are not, and are given only while the tower is in vision |
| `visible` — projectile: id, position, velocity | Velocity is recoverable from two consecutive positions of the same projectile, so it leaks nothing and saves the client an interpolation guess |
| `events` — cast, damage, death, each with `at` | The derived signals. They exist so that culling them is a real operation; `at` is the culling key |

And what is deliberately **not** in it:

- **No standing order.** `Order::Attack` names an `EntityId` the player may no
  longer be able to see. The client originated the order and can track it;
  reconciling a server-side order change is M3's problem and must not be solved
  by shipping handles.
- **No enemy cooldowns.** A cooldown tracker is a classic cheat. A protocol that
  ships enemy cooldowns has implemented one in the server.
- **No damage source.** The obvious field, and a leak: an attacker within basic
  attack range of a point you can see need not be at a point you can see, so
  "seat 4 hit your ally" hands over the identity and rough position of a champion
  the fog was hiding. The cost is damage attribution in a UI that does not exist
  yet.
- **No projectile owner.** A skillshot outlives its caster's visibility, and
  naming the owner would identify a hidden champion from the projectile alone.
- **No dead champions, including allies.** A dead champion is not on the map. An
  ally's respawn timer is information about a player rather than about the world,
  and carrying it would mean a second visibility rule — a second place for a leak
  to hide — for an ally panel that does not exist.

#### The order of `visible` is the handle, and that is a culling rule

`visible` is emitted in ascending `EntityId` order. Champions, towers and
projectiles occupy ascending handle ranges, so the first two fall out of
iterating them; the projectiles are sorted.

They were not, until the property suite said so. The arena allocates the lowest
free slot, so which slot a projectile occupies is a function of every cast that
came before it, and emitting them in arena order made the *order* of two
perfectly legitimate sightings depend on casts the recipient was never shown.
Two skillshots from one seat, the first expiring before the second is cast,
produce a view listing the newer projectile first — and a client that knows the
rules reads a freed slot out of that. It is a thin channel and it is exactly the
kind this project counts: `SCOPE.md`'s adversary model puts packet sizes and
arrival times in the same category, and this is smaller than either.

What the sort does **not** close, and what is therefore still open: projectile
handles come from a match-global counter, so a gap between two visible handles
says how many casts happened out of sight. Closing that needs a per-recipient
handle space, which is a protocol decision with reconciliation consequences and
belongs to M3 rather than to a test-coverage change. It is recorded here so that
the sort is not mistaken for a complete answer.

#### Serialization, and why `serde` is not here yet

`PlayerView::encode` is written by hand, by exhaustive destructuring, exactly as
`canonical.rs` is and for the same reason: a field added to a view type and not
encoded must stop the build rather than quietly never reach a client. `sim`'s
`[dependencies]` table is still empty.

This document allows `serde` for the view types and that permission stands; it
is deliberately not taken yet. The transport that would choose a codec arrives at
M3, and the traffic-shape invariant below wants a byte layout decided here rather
than by a crate. The CI grep for a serialization derive in `sim` is unaffected —
it is textual, it excludes `sim/src/view.rs`, and it has been exercised against a
deliberate `#[derive(Serialize)]` on `State`.

The encoded size is **variable**, and that is not the finished state of the
system: the traffic-shape invariant requires every `View` message to have the
same encoded size at a constant cadence, because message length and message
count leak the number of visible entities as surely as the entities would.
Padding is the transport's job, the transport is M3, and
`PlayerView::MAX_ENCODED_BYTES` — 1498, derived from the encoding rather than
measured from a run — is the bound it will round up to.

### The `State` escape hatch, decided in advance

A non-serializable `State` has a predictable cost, and the two places it will be
felt are known now: mid-match reconnection wants to hand a client a world, and
tests want to construct one. Both are answered here so that neither gets
improvised under pressure later, because the rule that has to survive is
absolute: **no production code path can serialize `State`.**

**Reconnection does not transport state.** A reconnecting client is
resynchronised exactly like a joining one: the server sends the current
`PlayerView`, already culled, and the client rebuilds its local prediction from
it and from subsequent ticks. There is no snapshot message and no fast-forward
of the input log on the client. Shipping a state snapshot to a reconnecting
player would be a maphack with a handshake in front of it, and the fact that
`State` cannot be encoded is what makes that unavailable rather than merely
discouraged.

**Fixtures are seeds and input logs, not states.** A fixture is
`(seed, Vec<Input>)` and an expected `Digest`. That is what the determinism
suite compares, what the replay container stores, and what resimulation
consumes. Any crate's tests can therefore build any reachable state, in normal
builds and without a feature flag, through the ordinary public API — `new_state`
then `step` — because a state reached by simulation is the only kind of state
the game has. No serialization is involved at any point.

**Rules a fixture brings with it.** A fixture is `(seed, Vec<Input>)` *and the
constants it was recorded under*. `step_with_rules` and `new_state_with_rules`
take a `Rules` value; `step` and `new_state` are those two applied to `RULES`.
The reason is a failure this project already committed once and reverted: a
fixture that has to reach death and respawn cannot afford a fifteen-second
respawn timer, and the tempting fix is to lower the timer in `RULES` until the
test fits. Six months later that number reads as a decision about how the game
plays, and there is nothing in the file to say otherwise. Balance is not a place
to store test requirements, so the fixture that needs other constants declares
them, and `rules_hash()` — which covers the constants and is deliberately
separate from `State::digest()` — is what keeps a digest recorded under one set
from ever being compared against the other.

This is not a configuration surface. There is one set of rules the game is
played by, `RULES`, and the server, the replay verifier and the resimulator all
call `step`. `step_with_rules` exists for fixtures and for the tamper cases M5
has to reject, and the moment it appears in `server` it is a bug.

**One narrow door beyond that: `#[cfg(test)]` constructors inside `sim`.** A
unit test that needs a configuration which is awkward to reach by simulation — a
projectile mid-flight, a tower at one hit point — builds it directly through a
constructor gated on `#[cfg(test)]`. That gate is not a feature: it exists only
while compiling `sim`'s own test target and is unreachable from any other crate,
including `sim`'s integration tests. It constructs a `State`; it still cannot
encode one.

**What stays closed: a `Serialize` impl behind a Cargo feature.** `RISKS.md` R5
floats a `dev-snapshot` feature as a hedge, and this document closes it until a
concrete need demonstrates itself. The reason is mechanical: Cargo features are
additive and unified, so "a feature the server binary cannot enable" is a claim
Cargo does not enforce — one crate listing `sim` with that feature in
`[dependencies]` turns it on everywhere. If replay seeking ever genuinely needs
snapshots, reopening R5 is a deliberate decision that must arrive with a CI check
on the resolved feature graph of the server binary, not a convenience added in a
pull request that was about something else.

### `protocol`

```rust
pub const VERSION: u16;

pub enum ClientMessage {
    Join { .. },
    Ready,
    Input { seq: u32, claimed_at_ms: u64, action: Action },  // client time: untrusted
    Surrender,
}

pub enum ServerMessage {
    Accepted { player: PlayerId, seed: u64, rules_hash: [u8; 32] },
    View(PlayerView),                 // already culled before encoding
    Outcome(MatchRecord),
}
```

`claimed_at_ms` is attacker-controlled by definition. It is recorded, never
trusted, and the divergence between it and the server's arrival timestamp is
itself the signal for exploit class 4.

**The traffic-shape invariant: one message per player per tick, at a constant
cadence, of a constant encoded size, independent of content.** Both halves are
load bearing, and padding alone is not the property:

- Padding to fixed size buckets closes the *length* channel. Without it, message
  length is close to a linear readout of the number of visible entities.
- Constant cadence closes the *count and timing* channel, and that channel is
  the one padding does nothing about. A server that sends a `View` only when
  something changed, or that emits an extra message when an event fires, leaks
  the number of visible entities through message counts and inter-arrival times
  no matter how well each individual message is padded.

So the server emits a `View` every tick for every connected player, whether or
not anything happened, at the tick rate rather than at the rate the world
produces news. Events do not get their own messages: they ride inside the tick
message or they wait for the next one. "Nothing visible" and "six entities
visible" must be indistinguishable to an observer who counts and times packets
without reading them.

The cost is bandwidth spent on nothing, which at 3v3 is not a cost. The exploit
that must fail against this is scheduled in `MILESTONES.md` M7: recovering the
number of nearby entities from message sizes *and* arrival times.

### `replay`

```rust
pub struct Manifest {                  // this is what gets signed, not the log
    pub match_id: Uuid,
    pub server_identity: PublicKey,
    pub seed: u64,
    pub rules_hash: [u8; 32],
    pub sim_version: [u16; 3],         // the `sim` crate version: major, minor, patch
    pub sim_commit: SimCommit,         // the commit that build came from, or Unknown
    pub started_at: SystemTime,
    pub participants: Vec<PlayerPseudonym>,
    pub input_log_digest: [u8; 32],
    pub final_state_digest: [u8; 32],
}
```

`rules_hash` covers the constants; `sim_version` and `sim_commit` cover the code
that reads them, which is the gap `RISKS.md` R13 is about — two builds can agree
on every constant and still resolve a tick differently. The version is enforced
mechanically: `sim` owns its version rather than inheriting the workspace's, and
CI refuses a pull request that touches `sim/` without raising it. The commit is
what makes a mismatch investigable rather than merely reportable, and it is
allowed to be absent, because a locally built server is a real case and a
manifest that lies about provenance is worse than one that admits it:

```rust
pub enum SimCommit { Sha([u8; 20]), Dirty([u8; 20]), Unknown }

pub struct Replay { pub manifest: Manifest, pub signature: Signature, pub inputs: Vec<TimedInput> }

pub struct TimedInput {
    pub input: Input,
    pub claimed_at_ms: u64,            // untrusted
    pub received_at_ms: u64,           // server-observed: the only real clock
}

/// Resimulates and checks the signature. The only defined way to assert that a
/// match was played.
pub fn verify(replay: &Replay, keys: &KeyRegistry) -> Result<Digest, VerifyError>;
```

`VerifyError` distinguishes its cases (truncated, reordered, digest mismatch,
unknown key, version mismatch) because M5's exit criterion is that each tamper
case is rejected for the right reason. "This replay is from another build" is
one of those cases and not a digest mismatch: the two have different answers,
and a verifier that conflates them teaches its reader to distrust the loud one.

### `anticheat`

```rust
/// Everything a detector may look at. Constructed by the server from live
/// telemetry, or by the offline tooling from a replay — identical either way,
/// so every detector is reproducible from a stored match.
pub struct MatchTelemetry { pub inputs: Vec<TimedInput>, pub views: Vec<ViewDigest>, .. }
pub struct AccountHistory { .. }      // progression coherence, exploit class 3

pub trait Detector {                   // more than one implementation, so a trait is earned
    fn name(&self) -> &'static str;
    fn score(&self, t: &MatchTelemetry, h: &AccountHistory) -> Score;
    fn threshold(&self) -> Score;      // justified in docs/detectors/<name>.md
}

pub struct Finding { pub detector: &'static str, pub score: Score, pub evidence: Evidence }
```

Detectors return findings. Nothing in this crate bans, disconnects, or notifies —
acting on a finding is a human decision, per `SCOPE.md`.

## Enforced invariants

Each is a test or a lint, not a convention:

1. `sim` compiles with `forbid(unsafe_code)` and denies float arithmetic, bare
   arithmetic (`clippy::arithmetic_side_effects`, so every operation names what
   it does on overflow) and `unused_variables`; a `clippy.toml` disallowed-types
   list blocks `f32`, `f64`, `std::time`, and the randomized-hasher collections.
   `rand` is absent from that list on purpose: it is not a dependency of `sim`
   at all, so no path into it resolves, and an empty `[dependencies]` table is a
   stronger statement than a lint — a lint can be allowed, a missing dependency
   cannot be named.
2. Identical seed and input log produce an identical `State::digest()` on
   x86-64 Linux, x86-64 Windows, and aarch64 macOS. Two fixtures, both run under
   `--release` with overflow checks on, compared against digests committed in
   the repository — which catches disagreement between platforms *and* drift
   over time on one, where comparing the three jobs to each other would catch
   only the first. Each fixture also pins the `rules_hash()` of the constants it
   was recorded under, so the two cannot be confused for one another and neither
   can be silently reinterpreted after a balance change.
3. No `Serialize` impl exists for `State` or its components; only the view types
   may have one. Checked in CI by a grep over `sim/src` excluding
   `sim/src/view.rs`, and that grep has been exercised — a `#[derive(Serialize)]`
   placed on `State` and then removed produced
   `a serialization derive reached sim outside the view types (docs/RISKS.md R5)`.
   The enforcement is stronger still today: `sim` has no serialization
   dependency, so no type in it *can* derive one, and invariant 11 is what keeps
   that true.
4. Every field of `State` and of `Rules` reaches the digest, because the
   encoding destructures both exhaustively and a new field stops the build.
5. For every tick and player of both reference fixtures, every `EntityId` in
   `view_for`'s output is accompanied by the position it was seen at, and that
   position is inside the player's vision — in the entity list and in the
   events alike. Asserted in `sim/tests/visibility.rs` against a visibility
   predicate re-derived in `sim/tests/spec/mod.rs`, so that the test is not the
   implementation agreeing with itself, and paired with a completeness
   assertion so that returning nothing does not pass. The same test reruns the
   fixture under constants that differ only in the vision radii and requires
   every state digest to be unchanged, which is how "`step` never reads
   visibility" is a test rather than a habit.

   **The re-derivation is duplication, it is deliberate, and it stays.** It has
   diverged once — truncated distances against exact squares, a shell one raw
   unit thick outside every circle — and it can diverge again, so the tempting
   move is to have `sim` export its predicate and have the test consume it. That
   is refused: a test calling `sim::view`'s own predicate asserts that
   `view_for` agrees with itself, which a projection that leaks everything
   satisfies as long as it leaks consistently, and the independence is the first
   of the two grounds on which `MILESTONES.md` records M2 as reached rather than
   written.

   What guards it is measured rather than asserted, and the measurement is less
   flattering than the argument. Putting the truncating comparison back turns
   `everything_inside_vision_is_named` red — the laxer specification admits a
   champion the view withholds — but **only at the `properties` job's raised
   case budget**; at proptest's development default of 256 the divergence
   passes unnoticed. And `vision_flips_exactly_at_the_radius`, the property that
   looks like it should be the one, does not catch it at any budget: its two
   champions are separated along `x` alone, so the squared distance is a perfect
   square and the integer square root is exact on it. The guard is completeness,
   it lives in CI rather than on a developer's machine, and the obligation
   stated at the top of that module is the real mechanism: a change to the rule
   changes the specification in the same commit.

   The same criterion holds over states nobody scripted:
   `sim/tests/view_properties.rs` reaches them by simulation from a drawn seed
   and a drawn script — half of it hostile — and asserts soundness,
   completeness, that the view is a function of what its player is entitled to,
   and that the projection is pure. Each of those was checked by breaking
   `view_for` on purpose and watching it go red; the one that could not be made
   to go red says so in its own comment. They are not a delivered defence:
   `SCOPE.md` reserves that word for a class with a matching exploit failing
   against it in CI, which is M7. They are coverage of the state space a
   fixture cannot reach, and they found a leak on their first run — see the
   ordering rule below.
6. `cargo tree -p cheat-client` shows no path to `sim`, `client`, or `anticheat`.
7. `cargo tree -p client` shows no path to `anticheat`.
8. Every detector in `anticheat` has an exploit in `cheat-client` that fails
   against it in CI.
9. Every `View` message has the same encoded size, and the server emits exactly
   one per connected player per tick. Checked by the M7 traffic-analysis
   exploit, which must fail to recover the visible-entity count from a recorded
   session's message sizes and arrival times.
10. No `Serialize` impl for `State` exists behind any Cargo feature either — the
   only sanctioned constructors are `#[cfg(test)]`-gated, and no reconnection
   path transports state.
11. `cargo tree -p sim --edges normal` prints exactly one node: `sim` itself.
   Checked in CI. This is the dependency rule that matters most — the same
   `step` runs in the server, in replay verification, in the determinism suite
   and eventually in the RL environment, and any dependency is a place for
   `RISKS.md` R9 to enter. `--edges normal` so that `proptest`, which links into
   the test harness and never into the crate, does not trip it.

## Deliberate non-abstractions

One champion means a concrete `Champion` struct, not a trait. One transport
means concrete types, not a `Transport` trait. Two message directions mean two
enums, not a codec framework. `Detector` is a trait because there will be five
of them and the server iterates over a collection — that is the bar an
abstraction has to clear here.
