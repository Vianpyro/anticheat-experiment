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
| `client` | Presentation. Rendering, input capture, prediction, reconciliation | `sim`, `protocol`, a runtime, a game framework; plus `server` as a dev-dependency for the M3 exit harness | `server`, **`anticheat`**, `replay`'s signing keys |
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

**`client` may have `server` as a dev-dependency, and only that.** M3's exit
criterion is three clients and one server in one process, which needs both ends
linked into one test binary; the alternative is an eighth crate whose only
content is that test. It is the same allowance `cheat-client` has and for the
same reason — a dev-dependency does not ship — and the enforced claim is about
the *normal* graph: `cargo tree -p client --edges normal` shows no path to
`server` or `anticheat`, checked in `ci`.

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
pub enum Seat { Blue0, Blue1, Blue2, Red0, Red1, Red2, Green0, Green1, Green2 }
pub enum Team { Blue, Red, Green }        // no `opponent()`: a team has two
pub struct EntityId(pub u16);
pub struct Fx(i32);                 // Q15.16: i32 read as a multiple of 2^-16
pub struct FxVec2 { pub x: Fx, pub y: Fx }

pub struct State { /* tick, rng, next_projectile_id, [Champion; 9], [Tower; 6], projectiles, events, outcome */ }

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

### The map, and what three teams changed in the rules

Three bases at the vertices of a triangle of circumradius 100, a lane along each
edge, and each lane contested by exactly the two teams whose bases it joins. Two
towers per team, a quarter of the way down each of the two lanes leaving its own
base — so a lane carries one tower per contestant, at that contestant's own end,
and the two teams meet between them rather than under one.

The bases are constants rather than rotations of one another, and that is a
decision with a cost: a rotation by 120 degrees needs `sqrt(3)/2`, which is not
a fixed-point number, so a computed layout would be three approximations with a
transcendental in the middle of the rules — precisely what `RISKS.md` R1 exists
to keep out. Written down, they are exact values `rules_hash()` covers. What it
costs is that the map is symmetric about `x = 0` and *not* under rotation: Blue
sits at the apex and the other two are exact mirrors of each other. The
asymmetry is worth about one raw unit of position on a map two hundred units
across, and it is stated here rather than left for somebody to find.

Two rules changed shape rather than value, and neither is cosmetic:

- **`Team::opponent()` is gone.** A team has two of them. Every rule that used
  it now compares team membership directly — a tower shoots the lowest-numbered
  seat *not on its own team*, a projectile hits the first entity *not on its
  owner's* — which is the form that stayed correct when the third team arrived
  and needs no tie-break between the two enemies beyond the seat order that
  already existed.
- **The match is decided when one team is left standing**, not when a team has
  lost both its towers. With two teams those are the same sentence; with three
  they are not, and a team can be knocked out while two are still playing. An
  eliminated team's champions stay on the map, exactly as an unoccupied seat's
  do, because removing them would be a rule about how a knocked-out team behaves
  and that is game design.

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
| Coordinates, both axes | `[-128, 128]` | The product of two in-domain values is at most `16384`, comfortably inside the type. This is the bound that makes multiplication closed. The map is square and the *game* is a triangle inscribed in it: the domain is a property of the type and does not change shape when the layout does |
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
- *Not panicking.* `step` runs inside an authoritative server for nine players
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
triangle, a rounding rule that drifts one way is a rounding rule that treats one
team differently from the others.

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
| `visible` — champion: id, position, hp | What an observer standing there would see. Team follows from the handle, which is the seat, and is public — which is also the *only* thing in a view that may distinguish one enemy team from the other |
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

What the sort did **not** close was the counter behind the handles: they come
from a match-global allocator, so a gap between two visible handles says how many
casts happened out of sight. That was left to M3 because closing it is a protocol
decision with consequences for reconciliation, and it is closed now —
`protocol::HandleSpace` gives every recipient a naming of its own. See "Handles a
recipient is given rather than told" below.

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
Padding is the transport's job and `PlayerView::MAX_ENCODED_BYTES` — 1093,
derived from the encoding rather than measured from a run — is the bound it
rounds up to. One of its terms is a transport constant expressed here:
`MAX_EVENTS_PER_VIEW` is the events a *frame* has room for, which is fewer than
a *tick* can record, and the difference is deferred by the transport rather than
dropped. See "The padding budget".

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
pub const VERSION: u16;                              // in every frame's header

pub enum ClientMessage {
    Join,                             // the server picks the seat, not the client
    Ready,
    Input { seq: u32, claimed_at_ms: u64, action: Action },  // client time: untrusted
    Surrender,
}

pub enum ServerMessage {
    Accepted { seat: Seat, seed: u64, rules_hash: [u8; 32] },
    Rejected(RejectReason),
    View(PlayerView),                 // already culled, in this recipient's handles
}

pub struct ClientFrame([u8; CLIENT_FRAME_BYTES]);      // 24
pub struct ServerFrame([u8; SERVER_FRAME_BYTES]);      // 1096
pub struct ServerShard([u8; SERVER_DATAGRAM_BYTES]);   // 555, and there are two
pub struct ShardAssembler { .. }     // puts a frame back together, or gives up
pub struct HandleSpace { .. }        // one recipient's naming of the projectiles
pub struct EventBacklog { .. }       // one recipient's undelivered events
```

`claimed_at_ms` is attacker-controlled by definition. It is recorded, never
trusted, and the divergence between it and the server's arrival timestamp is
itself the signal for exploit class 4.

**`Join` carries no seat and `ClientMessage` carries no state at all.** No
position, no hit points, no tick the client believes it is on. Every one of
those is something the server knows better, and a field a client can write is a
field an attacker writes.

**There is no `Outcome(MatchRecord)` message, and its absence is the invariant
below rather than an omission.** This document sketched one; a message whose
*existence* depends on what happened is a message an observer counts, which is
exactly the channel the cadence half closes. The outcome is already a field of
every `PlayerView`, so a client learns the match ended from the frame it was
going to receive anyway, and the signed match record — evidence, not a
notification — is M5's object.

**Every frame is a fixed-size array, and so is every datagram that carries part
of one.** That is how the size half of the invariant stops being a test: there
is no encoder that can return a shorter frame, no bucketing scheme that
compiles, and `ServerFrame::shards` returns `[ServerShard; SERVER_SHARDS]` so
there is no way to emit a number of packets that follows the content either.
Decoding is total on every byte string and refuses a non-zero byte in the
padding: padding a receiver skips is a channel the sender can write into, and it
would give one message two encodings, which is what lets two verifiers disagree
about a recorded log.

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

The exploit that must fail against this is scheduled in `MILESTONES.md` M7:
recovering the number of nearby entities from message sizes *and* arrival times.

#### The padding budget

"The cost is bandwidth spent on nothing, which at 3v3 is not a cost" was the
whole justification until M3. That is a claim with no number in it, so here are
the numbers and the reasoning that fixes them.

**The bucket is one bucket and it is the worst case the type allows.**
`SERVER_FRAME_BYTES` is a three-byte header plus
`PlayerView::MAX_ENCODED_BYTES`, rounded up to a whole number of shards. There
is exactly one bucket because a scheme with several is only content-independent
if the bucket is chosen without looking at the content — and a bucket chosen
without looking at the content is one bucket with extra steps.

**The frame is cut into a constant number of constant-size datagrams.** Two
shards of 555 bytes, every tick, per player, whatever the match is doing. That
is the padding scheme in one line — *constant cadence, constant count, constant
size* — and it is a stronger statement than the one it replaces. The old frame
travelled on a reliable stream and the invariant was carried by an argument
about QUIC's packetiser ("a constant number of bytes at a constant period is
packetised into a constant number of packets"); it is now a fact about
`protocol::ServerFrame::shards`, which returns a fixed-size array of a newtype
over a fixed-size array. An observer counts two packets of 555 bytes per player
per tick and learns the tick rate and the number of connected players, both of
which they already knew.

**The arithmetic.**

| Part | Bytes |
| --- | --- |
| `tick`, `outcome`, `own`, two list lengths | 35 |
| 8 champions + 6 towers, 15 bytes each | 210 |
| `MAX_PROJECTILES` = 32 projectiles, 19 bytes each | 608 |
| `MAX_EVENTS_PER_VIEW` = 16 events, 15 bytes each | 240 |
| **`PlayerView::MAX_ENCODED_BYTES`** | **1093** |
| plus a 3-byte frame header, rounded to 2 shards | **1096** |
| on the wire: 2 × (7-byte shard header + 548) | **1110** |

**What it costs, measured.** Over the thousand-tick nine-player fixture in which
the teams walk their lanes and fight, the encoded views come out at a median of
95 bytes, a mean of 107, a 95th percentile of 140 and a maximum of 190 — against
a bound of 1093.

| | Per player | Per match (9) |
| --- | --- | --- |
| Padded, at 30 Hz | 33.3 kB/s — **266 kbit/s** | 300 kB/s — 2.40 Mbit/s |
| Unpadded, at the measured mean | 3.2 kB/s — 26 kbit/s | 29 kB/s — 231 kbit/s |
| Inflation | **10×** | 10× |

Upstream is negligible either way: a client frame is 24 bytes, one per tick, 5.8
kbit/s per player.

For comparison, the frame this replaced was 1501 bytes at six players: 360
kbit/s each and 2.16 Mbit/s for the match. Per player the new scheme is
**cheaper** — 266 against 360 — and the match total rises only because there are
half again as many players in it.

**Is 266 kbit/s acceptable?** For this project, yes, and the comparison worth
making is not to the unpadded stream but to what a game of this shape normally
costs. A competitive shooter budgets a few hundred kbit/s per client; a MOBA much
less. It is inside that envelope, it is a constant rather than a peak, and the
whole server is one process hosting matches counted in dozens (`SCOPE.md`: scale
is out of scope). The bandwidth is the cheapest thing this project spends.

**Where the bound went, and the question that had not been asked.** The old
bound was 1498 bytes, dominated by two `sim` constants: the projectile arena at
32 × 19 = 608 and the event buffer at 48 × 15 = 720. The event half was never
reachable in one *message*, and nobody had asked: `MAX_EVENTS` is what a **tick**
can record, and a frame does not have to carry all of it, because an event held
back for one frame is delivered a thirtieth of a second later rather than lost.
So the view's event budget is its own constant, `MAX_EVENTS_PER_VIEW` = 16, and
that is what took the frame under the MTU and made datagrams possible.

**What happens to the seventeenth event.** It is **deferred, not dropped**:
`protocol::EventBacklog` is a per-recipient queue that delivers the overflow on
the next frame, in the order the rules produced it. The queue is bounded at one
tick's capacity and drops past that, with a counter so that "waiting" and "lost"
are tellable apart from outside; reaching the bound needs a sustained rate above
sixteen visible events per tick, which no reachable state under the game's
constants produces. The encoder truncates at the same bound as a backstop — that
is what keeps `MAX_ENCODED_BYTES` a property of the *encoding* rather than an
obligation on every caller, so no framing code has a payload it cannot pad.

**Deferral is not a side channel, and the argument is the one the handle space
already makes.** The queue is fed from an already-culled `PlayerView`; it never
sees the state, so there is nothing hidden for it to be a function of. Two
states a player cannot tell apart produce the same entitled events, hence the
same queue, hence the same bytes. What a deferral could in principle reveal is
*timing* — an event a tick late says the previous tick was busy — and it says
that to the recipient who was already shown sixteen events on that tick, so it is
not information they did not have. To a third party it says nothing at all,
because the frame is padded and the cadence is constant.

**What is still not taken, and why.** Two of the three savings the old document
rejected are rejected for the same reasons:

- *Lower `MAX_PROJECTILES`.* It is an array length inside `State`, so it is
  under `State::digest()`; and the arena is 55% of what is left of the bound
  while the game's own cooldowns cap real occupancy at one projectile per seat.
  Sizing it to that would be sizing the bucket to the observed maximum, which
  reopens the channel the first time a fixture's constants or a balance change
  put more in flight. The bucket is for the worst case the *type* allows.
- *Cap the view instead of the state:* emit at most N entities and drop the
  rest. The size stays constant and the *contents* stop being: which entities
  survive the cap is a function of what is visible, so a client that stops
  seeing a distant projectile when a fight starts nearby has been told about the
  fight. Trading a length channel for a content channel is not a saving. This is
  precisely why the event budget defers rather than drops.
- *Compress.* Compressed length is a function of content, which is the length
  channel with a fig leaf. Padding after compression would work and would save
  nothing, since the padding target is what determines the bandwidth.

**The consequence for the transport, which is now a gain rather than a cost.**
1110 bytes in two datagrams fit any path MTU, so state travels on QUIC datagrams
and a lost packet costs the tick it belonged to instead of blocking every frame
behind it. `RISKS.md` R6's original hedge — "datagrams for state, reliable
streams for session commands" — is restored, and it is exactly what the code
does: `Accepted` and `Rejected` are sent once and must arrive, so they keep the
bidirectional stream, and so do the client's own frames, where head-of-line
blocking costs one tick's intention that the sequence rule already treats as
droppable.

What that costs instead is stated rather than hidden: **state delivery is
unreliable now.** A client can miss a tick. `ShardAssembler` abandons a frame
whose shard never arrived the moment a newer frame starts, and counts it; a view
older than the one already applied is discarded and counted. M3's exit criterion
had to be weakened to match — see `MILESTONES.md` — and that weakening is the
honest price of removing head-of-line blocking.

#### Handles a recipient is given rather than told

Champion and tower handles are public: a champion's handle *is* its seat and a
tower's position follows from the rules. Projectile handles are not. They come
from a counter global to the match, so a client shown `1005` and later `1009`
has learned that three skillshots were cast where it could not see them — a
wider channel than the ordering one M2 closed, because that leaked the *order*
of two legitimate sightings and this leaks a count of events the recipient was
never shown.

`protocol::HandleSpace` gives every recipient a naming of its own, allocated on
first sight. **The design that was rejected is the tempting one:** a map with a
free list, releasing a handle when its projectile expires. It closes the
counting channel and opens a recycling one — a handle that comes back is
observable, and a full free list counts the projectiles in flight including the
invisible ones. So the local counter is monotone and never reuses; the map is
pruned to the arena each tick, which bounds memory without touching the counter.

Three consequences for reconciliation, all real:

- **Handles are session-local.** Two clients cannot compare projectile handles,
  and a handle means nothing outside the session that issued it. Everything that
  has to correlate across sessions — the recorded log, the resimulator, the
  input-log digest — correlates on `Input`s, which name no projectile.
- **A reconnecting session gets a new space.** It is resynchronised like a
  joining one, by being sent a `PlayerView` and nothing else, so it has already
  thrown away the world it had; new handles are consistent with that rather than
  an extra cost. M4 inherits the constraint.
- **Two sessions agree only if they saw the same history.** The three seats of a
  team receive the same visible set and the list is ordered by handle, so they
  allocate in the same order — which is what lets M3's exit criterion compare
  their reconciled worlds directly. A client that joined late is *supposed* to
  disagree about the handles of projectiles it never saw.

At exhaustion — about a day of continuous play at one skillshot per player per
eight seconds — a projectile with no handle is omitted from that recipient's
view. A degradation, and a function of that recipient's own history rather than
of anything hidden.

### `server` and `client`

```rust
// server: the authority has no clock, no socket and no runtime.
impl Match {
    pub fn join(&mut self) -> (Option<Seat>, ServerFrame);
    pub fn deliver(&mut self, seat: Seat, bytes: &[u8], received_at_ms: u64)
        -> Result<(), Violation>;
    pub fn tick(&mut self) -> Vec<(Seat, ServerFrame)>;   // one per occupied seat, always
    pub fn recording(&self) -> Recording;
}
pub mod net { /* quinn: the clock, the sockets, the certificate */ }
```

`Match` is driven rather than driving, and that is what makes the authority a
function of its inputs — the thing the replay resimulates, the thing M7's
exploit suite drives, and the thing the traffic properties are stated over. The
clock and the sockets are in `net`.

The seat comes from the session and never from the message; the tick comes from
the server. `Surrender` frees the seat and does **not** decide the match:
whether a team that concedes loses is a rule, rules live in `sim` where a replay
resimulates them, and a match outcome invented in the session layer is one no
verifier could reproduce.

The client at M3 is headless: input scripts in, digests out. Its reconciled
local world is what it was told, with `own` folded into the entity list at a
teammate's fidelity, which is what makes the three seats of a team comparable.

**What M3 discovered that M4 has to answer.** Client-side prediction cannot be
built on this protocol. Prediction needs the client to know *which of its inputs
the server applied to which tick*, and nothing tells it: the server buckets an
intention into whichever tick it is about to run, and `PlayerView` carries no
acknowledgement — no last-applied sequence number, and deliberately no standing
order, because an order names an `EntityId` the player may no longer see. M4
needs one more field or one more message, and choosing its shape is M4's
decision rather than one M3 should make by accident.

### `replay`

At M3 this is a container, a reader that is total on hostile bytes, and
`resimulate`. **No signature and no version stamp**: a recording proves that
some server wrote the file and nothing about who, and two builds that reordered
a rule resimulate differently with nothing in the file to say which was right
(`RISKS.md` R13). A digest mismatch at M3 therefore means "these bytes do not
describe that match" and cannot yet distinguish tampering from a build
difference. It is a development artefact, not evidence; M5 is where that
changes. `rules_hash` *is* carried, because its absence is a silent failure
rather than a missing feature (`RISKS.md` R2).

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

   What guards it is measured rather than asserted, and the measurement was
   less flattering than the argument. Putting the truncating comparison back
   turns `everything_inside_vision_is_named` red — the laxer specification
   admits a champion the view withholds — but **only at the `properties` job's
   raised case budget**; at proptest's development default of 256 the
   divergence passes unnoticed. And `vision_flips_exactly_at_the_radius`, the
   property that looks like it should be the one, did not catch it at any
   budget: its two champions were separated along `x` alone, so the squared
   distance was a perfect square and the integer square root exact on it — the
   property swept a boundary along the one axis where the question has an easy
   answer.

   **That is fixed, and it moves the guard off CI's budget.** The property
   draws an offset vector rather than a separation, and its generator aims most
   of its draws at the shell one raw unit thick outside the circle, where a
   truncating comparison and an exact one disagree; it holds the rule *and* the
   re-derived specification to the exact criterion. Truncation in
   `sim::view::can_see` now fails it after three cases at the development
   default, shrinking to `FxVec2 { x: 0.00001, y: 12.00000 }` against a radius
   of `12.0` — the counter-example this document used to name from memory.
   Truncation in the specification fails it after six. Completeness at CI's
   budget remains the second guard, and the obligation stated at the top of
   that module is still the mechanism rather than the backstop: a change to the
   rule changes the specification in the same commit.

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
9. Every `View` message has the same encoded size, travels as the same number
   of equally sized datagrams, and the server emits exactly one per connected
   player per tick.

   The size half is carried by the *types*: `ServerFrame` wraps
   `[u8; SERVER_FRAME_BYTES]` and `ServerFrame::shards` returns
   `[ServerShard; SERVER_SHARDS]`, so there is no encoder that could return a
   shorter frame, no bucketing scheme that would compile, and no packet count
   that could follow the content. The cadence half cannot be a type and is
   `server/tests/traffic.rs`, along with the property that carries the most: two
   states a player cannot tell apart produce byte-identical frames for that
   player — the transport's version of the side-channel property in
   `sim/tests/view_properties.rs`, covering the padding, the framing, the
   per-recipient handle space and the per-recipient event backlog.

   All of it was exercised rather than trusted. Emitting a view only when its
   content changed turns the cadence property red at `tick 1: 0 frames for 9
   seats`; padding derived from the payload is refused by the decoder's padding
   check; a sender that skipped a shard carrying nothing but padding — a packet
   count that follows content — leaves the exit criterion at `Blue0 reached 0 of
   10 checkpoints`, with 999 frames abandoned.

   **The known blind spot is closed, and by the third team rather than by a
   test.** Naming every projectile in the arena instead of only the ones the
   recipient was shown — the leak the handle space exists to close — used to
   pass the byte-equality property at 4096 cases, because its antecedent is full
   entitlement equality and on a two-team map a fork nobody could tell apart was
   almost always a fork in which nothing had happened. With three teams a whole
   enemy team can act at a vertex a lane away while the observer's entitlement
   is untouched, so hidden activity and equal entitlement stopped being
   opposites: the same mutation now fails on the property's **first case**, at
   `Blue0 was sent different bytes 1 ticks after a fork it cannot tell apart`.
   The scripted scenario that was written to cover the gap stays, because a
   property that happens to reach a channel is evidence about a generator and a
   state built to expose it is evidence about the channel.

   None of this is a delivered defence. `SCOPE.md` reserves that word for a
   class with a matching exploit failing against it in CI, which is the M7
   traffic-analysis exploit: recovering the visible-entity count from a recorded
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
