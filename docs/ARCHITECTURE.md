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
| `replay` | The replay container: format, signing, verification, resimulation. From M4, the corpus on disk and the commands that withdraw a participant from it and audit the result. From M6, the corpus's schema — the session record, the consent version, and the frozen train/holdout split. From M8, the telemetry companion: the device-event stream, sealed, and the commitment that binds it to a replay | `sim`; externally, an audited signature crate and a source of entropy for `keygen` | `server`, `client`, `anticheat`, any runtime |
| `server` | Authority. Tick loop, the clock, sockets, sessions, fog application, telemetry capture, replay recording | `sim`, `protocol`, `replay`, `anticheat`, a runtime | `client`, `cheat-client` |
| `client` | Presentation. Rendering, **input capture**, the lobby and the device measurement hidden in it, prediction, reconciliation; and the playtest bot that fills a seat nobody is sitting in | `sim`, `protocol`, a runtime, a window library and a framebuffer; plus `server` and `replay` as dev-dependencies for the M3 and M4 exit harnesses | `server`, **`anticheat`**, `replay`'s signing keys, **`cheat-client`** |
| `anticheat` | Detection. Feature extraction from telemetry, detectors, thresholds, evidence bundles | `sim`, `replay`; plus `cheat-client`, `server` and `protocol` as dev-dependencies for the detector suite | `server` (it is called by the server, not the reverse), `client`, any network or filesystem I/O outside `src/bin` |
| `cheat-client` | The attacker, and the exploit suite | `protocol` only, plus `server` as a dev-dependency for the in-process harness | `sim` internals, `client`, `anticheat` |

Three of these deserve their reason stated:

**`client` must not depend on `anticheat`.** Shipping detector logic to the
machine you assume is compromised hands the attacker your thresholds. All
detection runs server-side or offline over recorded telemetry. This is why
`anticheat` does no I/O — it is a pure function from telemetry to scores, which
also makes it replayable and testable without a server.

**And `cheat-client` must not depend on `anticheat` either, which is the same
rule read one crate over.** `cheat-client` *is* the machine this project assumes
is compromised, so the edge M8 needed had to point the other way: `anticheat`
takes `cheat-client` as a dev-dependency, plays its bots, and scores the log the
server wrote. That is the allowance `client` and `cheat-client` already hold for
`server`, with the same justification — a dev-dependency does not ship, and the
enforced claim is about the *normal* graph. The consequence is that
`cargo test -p cheat-client` is the attack account and `cargo test -p anticheat`
is the detection account, and only one of them can see both sides. `ci` asserts
both directions.

`anticheat`'s "no I/O" is enforced by a grep over `anticheat/src` excluding
`src/bin`, because the operator's tool is exactly where a directory walk belongs
and the rule is about the library other crates link.

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
    View {                            // already culled, in this recipient's handles
        view: PlayerView,
        applied_through: Option<u32>, // this recipient's own last applied input
    },
}

pub struct ClientFrame([u8; CLIENT_FRAME_BYTES]);      // 24
pub struct ServerFrame([u8; SERVER_FRAME_BYTES]);      // 1102
pub struct ServerShard([u8; SERVER_DATAGRAM_BYTES]);   // 558, and there are two
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
shards of 558 bytes, every tick, per player, whatever the match is doing. That
is the padding scheme in one line — *constant cadence, constant count, constant
size* — and it is a stronger statement than the one it replaces. The old frame
travelled on a reliable stream and the invariant was carried by an argument
about QUIC's packetiser ("a constant number of bytes at a constant period is
packetised into a constant number of packets"); it is now a fact about
`protocol::ServerFrame::shards`, which returns a fixed-size array of a newtype
over a fixed-size array. An observer counts two packets of 558 bytes per player
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
| plus M4's input acknowledgement, `APPLIED_BYTES` | 5 |
| plus a 3-byte frame header, rounded to 2 shards | **1102** |
| on the wire: 2 × (7-byte shard header + 551) | **1116** |

**What it costs, measured.** Over the thousand-tick nine-player fixture in which
the teams walk their lanes and fight, the encoded views come out at a median of
95 bytes, a mean of 107, a 95th percentile of 140 and a maximum of 190 — against
a bound of 1093.

| | Per player | Per match (9) |
| --- | --- | --- |
| Padded, at 30 Hz | 33.5 kB/s — **268 kbit/s** | 301 kB/s — 2.41 Mbit/s |
| Unpadded, at the measured mean | 3.2 kB/s — 26 kbit/s | 29 kB/s — 231 kbit/s |
| Inflation | **10×** | 10× |

Upstream is negligible either way: a client frame is 24 bytes, one per tick, 5.8
kbit/s per player.

For comparison, the frame this replaced was 1501 bytes at six players: 360
kbit/s each and 2.16 Mbit/s for the match. Per player the new scheme is
**cheaper** — 268 against 360 — and the match total rises only because there are
half again as many players in it.

M4 added five bytes to it, and the arithmetic is worth stating rather than
absorbing: the input acknowledgement prediction needs is a tag byte and a
four-byte sequence number, present whether or not there is anything to
acknowledge. It moved the frame from 1096 to 1102 and the datagram from 555 to
558, which is two kbit/s per player and no change at all to the shard count or
to any claim resting on it.

**Is 268 kbit/s acceptable?** For this project, yes, and the comparison worth
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

**What a replay records is what the rules produced, never what a client was
delivered.** Deferral creates two sequences of events and they are not the same
sequence: the rules emit an event on tick *T*, and the recipient whose frame was
already full is told about it in the frame for *T + 1*. The question of which
one a replay records costs thirty seconds to answer here and a painful debugging
session at M5, where resimulation compares a recorded match against a `sim` that
produces events in rule order — a log written in delivery order would be offset
by exactly one tick on every busy tick of every match, and the symptom would be
a digest mismatch reported against a recording that was perfectly faithful, in
the one milestone whose subject is telling tampering apart from honest
disagreement.

The answer is **produced**, and it is structural rather than a rule somebody
follows. A `Recording` carries the seed and the input log and **no events at
all**; resimulation derives them by running the same `step` the server ran, so
there is no field for delivery order to get into. `Match::recording` is built
from the seed and the log, and the backlog lives in a `Session` downstream of
it, fed from an already-culled `PlayerView` — it never sees a `State`, and
nothing it does can reach the recording.

`server/tests/produced_not_delivered.rs` demonstrates that the two sequences
really do differ rather than leaving it hypothetical: nine champions walk to the
middle of the map and every seat casts both abilities on one tick, producing 38
events of which one frame carries 16, 16 more arrive on the next frame and 22
are still owed — and resimulating the log reproduces all 38 on the tick that
produced them. A second test drives two matches with identical accepted inputs
and different audiences and requires byte-identical recordings; a `recording`
that consulted the session table fails it.

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
1116 bytes in two datagrams fit any path MTU, so state travels on QUIC datagrams
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

The client at M3 was headless: input scripts in, digests out. That mode is still
there behind `--headless`, because it fills a seat where there is no display and
because it is what the exit-criterion harnesses drive; its reconciled local world
is what it was told, with `own` folded into the entity list at a teammate's
fidelity, which is what makes the three seats of a team comparable. The playable
client is a window, and what it draws is the smaller half of it — see "The
client's input path, and the renderer it chose" below.

**What M3 discovered, and the shape M4 gave it.** Prediction needs the client to
know *which of its inputs the server applied to which tick*, and at M3 nothing
told it: the server buckets an intention into whichever tick it is about to run,
and `PlayerView` carries no acknowledgement — no last-applied sequence number,
and deliberately no standing order, because an order names an `EntityId` the
player may no longer see.

M4's answer is one field, and **it is outside `PlayerView`**:
`ServerMessage::View` carries `applied_through: Option<u32>` beside the view.
The placement is the decision rather than the field. A view is what
`sim::view::view_for` computes from a `State`, and that function has no session
to ask; an acknowledgement is a fact about a connection. Putting it inside the
view would have meant either a projection taking a session argument — a second
argument to the most sensitive function in the project — or a view type with a
field its own constructor cannot fill, which is how a type stops meaning what
its name says. It stays out of `State`, out of the digest, and out of the
projection.

It is not a channel: it is a number this recipient wrote and sent, it says
nothing about the world or about any other seat, and its width is constant in
both cases so the frame's size does not follow it. What it does change is the
statement of the byte-equality property in `server/tests/traffic.rs`, which is
now explicit that a frame is a function of what the recipient is entitled to
know about the world **and** of what the recipient itself said — both things the
recipient already has. The property's fork is constructed so that the two
branches never differ in which seats spoke, which keeps every seat eligible for
the comparison rather than retiring the ones a fork happened to separate.

### The client's input path, and the renderer it chose

`docs/RISKS.md` R14 recorded that a terminal quantises aim to a character cell
and priced it: no aim-curvature detector at M8, everything timing-shaped
untouched. The second half was wrong, and reading a real SGR trace is what said
so. Three things, of which R14 named one:

- **The quantisation is anisotropic.** A terminal is about 190 columns by 45
  rows, and a cell is about twice as tall as it is wide. Over the window the map
  was drawn to, one cell was **1.158 world units across and 4.111 down** — the
  vertical resolution 3.55 times the coarser. That is a *directional* bias in
  every aimed input, not merely a coarse one, and no analysis recovers a
  direction from a grid that had a preferred one.
- **The sampling rate was a function of pointer speed.** A terminal reports the
  pointer only when it crosses into a new cell, so a fast sweep produces an
  event per cell crossed and a slow creep produces almost none. Inter-arrival
  times recorded that way measure how fast the pointer was moving. R14 claimed
  the timing statistics — which is most of M8's list — were untouched; they were
  contaminated at the source.
- **There is no device timestamp in the trace at all.** A terminal escape
  sequence carries coordinates and buttons and nothing else, so time could only
  ever come from the moment the client read the byte, tty and scheduler latency
  included.

The first is a resolution problem with a resolution fix. The second is not: it
is a property of *what the device reports*, and no renderer that reports
positions on a grid can be fixed by making the grid finer. That is why the
client changed rather than the camera.

#### The library, chosen on control of the input device

The dominant criterion is access to the pointing device, because the capture is
what motivated the change; the rendering is a means. Availability was **read out
of the sources rather than assumed**, which is the only reason the table below
disagrees with the assumption this work started from.

| Option | Raw device motion | Device timestamp | What it costs |
| --- | --- | --- | --- |
| **`winit` + `softbuffer`** | Yes, on X11, Wayland and Windows | **No** — see the next table | 79 crates against the client's existing 70, no system development package on any platform, no GPU at run time, and `client::draw::rasterize` stays a pure function of a slice so the renderer keeps its tests in a CI job with no display |
| `winit` + `wgpu` | Identical — the same input layer | Identical | 44 more crates, a shader pipeline and a working adapter at run time, and a renderer that cannot be tested in CI because CI has no GPU |
| SDL2 / SDL3 | Yes, in relative mouse mode | A `timestamp` field that **looks** like one and is not: SDL3's headers say it is "populated using `SDL_GetTicksNS()`", which is SDL's own clock read when SDL generated the event from the OS queue — a read time in nanosecond units | A C library to find or vendor on three platforms, and a field whose name invites precisely the silent substitution this work exists to refuse |
| A higher-level framework (`macroquad`, `ggez`) | Pointer position, not raw motion | No | Fails the dominant criterion outright |
| `evdev` on Linux beneath the window library | Yes | **Yes** — the kernel's `input_event.time`, switchable to `CLOCK_MONOTONIC` with `EVIOCSCLOCKID` | A second input stack; a `/dev/input` read permission each participant must be granted; and no Windows counterpart, so the corpus's timestamps would mean different things on the two platforms it is recorded on. Worse for M8 than a uniform second best |
| Bevy | — | — | Excluded, and not on these grounds: a breaking-release cadence a solo project revisited intermittently cannot absorb, and a client-side ECS invites crossing the sim/render boundary that is this project's principal asset |

**Decided: `winit` + `softbuffer`.** `winit` because it is the layer that
surfaces unaccelerated device motion on every platform in the matrix —
`XI_RawMotion`, `zwp_relative_pointer_v1`, `WM_INPUT`. `softbuffer` because the
scene `SCOPE.md` fixes is nine discs, six towers and some projectiles, and the
deciding argument is not the crate count but that a CPU rasteriser is a pure
function of a slice: the renderer's assertions run in `ci` beside everything
else, on a runner with no display and no GPU. A `wgpu` renderer would have moved
those assertions to "nobody checks".

**Reopening criteria**, any one of which is sufficient:

- **`winit` gains an event timestamp.** The platform data exists on three of the
  four backends and `winit` discards it; the day it stops,
  `client::input::CLOCK` becomes `Device` where it can be. This has to happen
  **before M6** or not at all, because a corpus half of whose timestamps are
  device times and half dequeue times is a corpus with a covariate nobody can
  remove.
- **The scene stops being a fixture.** The CPU rasteriser is sized for what
  `SCOPE.md` freezes. Anything that genuinely needs a GPU reopens the `wgpu`
  comparison, and the crate count is then the smaller half of the argument.
- **A pointing device rather than a motion device is wanted.** This client
  integrates raw deltas, so the aim is a first-order integral of the device. A
  detector that needs absolute pointing — a tablet, a touchscreen — is a
  different measurement and a different capture path.

#### Device timestamps, per platform, read rather than assumed

This work began on the premise that `winit` exposes device events *and their
timestamps*. The first half is true. The second is false on every platform, and
the finding is recorded per platform because "the library does not do it" and
"the platform cannot do it" are different facts with different futures.

| Platform | Does the platform have a device timestamp? | Does it reach the client? |
| --- | --- | --- |
| Linux / Wayland | **Yes.** `zwp_relative_pointer_v1::relative_motion` carries `utime_hi`/`utime_lo`, microseconds, taken by the compositor from `libinput`, which takes it from the kernel's `evdev` event | **No.** `winit` destructures that event as `{ dx_unaccel, dy_unaccel, .. }`; the `..` is the timestamp |
| Linux / X11 | **Partly.** `XIRawEvent.time` is an X server millisecond stamp taken when the server processed the event: nearer the device than this process is, and not the device's | **No.** `winit` reads it only to remember the connection's last-seen server time, for selections and activation |
| Windows | **No.** `WM_INPUT`'s `RAWMOUSE` has no timestamp field at all. `GetMessageTime()` exists and is a millisecond queue-post time | **No.** `winit` does not surface it either |
| macOS | **Yes.** `NSEvent::timestamp`, process uptime, taken when the event entered the window server | **No.** `winit` does not surface it |

Verified against `winit` 0.30.13 and 0.31.0-beta.2: the string `timestamp` does
not occur anywhere in the public event API of either.

So the client stamps each sample when it **dequeues** the event from the
platform, and `client::input::CLOCK` is a `Clock::Dequeue` constant that says so
in a type rather than in a comment. Two things about that are the substance:

- It is taken **per event, in the callback**, not once per rendered frame.
  Stamping at frame time would give every event in a frame the same time and
  write the renderer's jitter into the record — which is the exact signal M8's
  timing detectors have to separate a bot's regularity from.
- It is a substitution and it is named as one. `Clock` has a `Device` variant
  that nothing currently produces, so a later build that gets a real device
  timestamp records *which* it had, and a corpus spanning the change can be
  split rather than silently pooled.

#### One sample per device event, and never one per change

`InputTrace::moved` appends unconditionally. The two designs the task allows are
"record every device event as it arrives" and "sample at a fixed interval", and
the first is taken: it is the device's own cadence, it needs no second clock,
and fixed-interval sampling would be a *resampling* of a stream the client
already holds in full — aliasing anything faster than its interval and able only
to lose information. The forbidden third option, "record when the position
changed", is the terminal's failure with a window in front of it.

The consequence worth stating plainly: the recorded rate is now the **mouse's
report rate**, 125 Hz to 1 kHz, and it is the same whether the hand is creeping
or sweeping. A stationary hand records nothing, which is not speed dependence —
it is the absence of motion to report.

#### Two paths from one event, and a projection that runs one way

The device delta goes to `InputTrace` verbatim — the platform's `f64` pair,
unscaled and unrounded — and to `Aim`, which integrates it into the fixed-point
world point `sim` consumes. The record is in the device's own counts, so a
player's sensitivity setting does not scale their contribution to the corpus;
the aim is in world units, because the rules are integers.

The structural half is that **`client::draw` has no inverse projection.**
`Viewport::pixel` maps world to pixel; nothing maps pixel to world. The terminal
client's `Camera::world` was exactly that inverse and it was the function R14
was about. `client/tests/capture.rs` asserts what the absence buys — the same
device events under a 640×480 and a 3840×2160 window produce a byte-identical
trace and an identical aim — and the aim's clamp is to `RULES.map_half_extent`,
a rule constant, rather than to the window, which would have made a recorded aim
a function of a monitor.

The renderer is letterboxed for the same reason: a world distance is the same
number of pixels whichever way it points, at any window shape, so the terminal's
3.55:1 anisotropy cannot come back through the display.

#### A platform artefact the measurement found

`CursorGrabMode::Confined` — the obvious way to stop an invisible OS pointer
wandering off the window — makes X11 deliver **every raw motion event twice**,
about five microseconds apart. Measured against `winit` alone with none of this
workspace involved: 50 synthesised device motions produce 50
`DeviceEvent::MouseMotion` without the grab and 100 with it.

It is invisible on screen and it is a second mode near zero in every
inter-arrival distribution, so a corpus recorded under it would have calibrated
M8's timing detectors on an X11 grab. The client does not take a grab, and the
cost — the hidden OS pointer drifts, and a click after it has left the window
goes elsewhere — is the cheaper of the two.

Filtering the duplicates was the tempting fix and is the same mistake in a
better disguise: a predicate on the contents of the record is what the cell
crossing was. So the trace keeps everything it is given and
`InputTrace::stats().coincident` reports whether what it was given was sound.

#### Nothing below the client changed

`Input`, `Action`, `ClientFrame`, `ServerFrame`, `State::digest()`, `rules_hash`
and the recording format are **byte-for-byte what they were**. The aim is still
an `FxVec2` in Q15.16 and the wire still carries 24 bytes up and 1102 down. What
changed is the resolution of what reaches that type — a device count is 0.05
world units where a character cell was 1.158 and 4.111 — and R14's own hedge is
why that was free: it said the quantisation lived in one function and everything
downstream carried full precision, and it did.

#### The lobby, and why the menu is where the device is measured

`docs/RISKS.md` R17 is the risk this answers and `docs/SCHEMA.md` §4e is the
schema. What belongs here is the shape of the thing.

**The confound.** The corpus is nine people on nine mice — `docs/SCOPE.md` fixes
both numbers — so every hand appears with exactly one device and no analysis can
separate a person's style from their hardware's response. That is not variance
more matches absorb; it is a variable the design does not identify. The parade is
not to standardise the hardware, which a production anti-cheat cannot do, but to
measure its contribution so that a statistic reading a distance or a speed works
in normalised units rather than in raw device counts.

**There is no calibration screen, and the lever is the geometry.** `client::lobby`
lays five elements out at positions the build fixes: the pseudonym check and the
consent confirmation at opposite top corners, champion select and `Ready` at
opposite bottom ones, and a training dummy that moves through a fixed table of
stations each time it is hit. `Ready` is **inert** until the other three have been
visited, so three long crossings happen because the interface requires them and
not because anybody is asked to make them; the dummy is what fills the wait for
the last player to connect, and its table is what sweeps eight octants and a
distance ratio above four — which one traversal of a static menu cannot do.

A click on an element is therefore a movement whose **endpoints are known
exactly** and whose **cost in device counts is measured**. That pair is the whole
measurement, and `client/src/lobby.rs` carries the field-by-field account.

**The binding constraint is that the lobby is driven by the game's own cursor.**
It integrates raw device deltas through `client::input::Aim` — the same
integrator, the same world units, the same clamp to `RULES.map_half_extent` — and
never by the operating system's pointer. A menu that reacted to the OS pointer
would be measuring the *accelerated* pointer, which is the quantity
`docs/SCHEMA.md` §4d refuses everywhere else in this client, and the scale
recovered from it would not be the scale the match is played at: a number worse
than no number, because it would have the shape of a calibration. Invariant 18
below is the test.

**What the client computes is nothing.** A session's reaches are folded into the
**sufficient statistics of a least-squares fit** rather than into its answer, so
two sessions of one participant pool by addition and the estimate is computed once
by `replay::calibration`, on the side this project does not assume is lying. That
is also what makes estimation *accumulate*: `Corpus::profile_of` is a fold over
the matches on disk, computed when somebody asks, in the register
`replay::split::split_of` is a function rather than a file.

**And it measures the mouse, not the inch.** The slope recovered is device counts
per **world unit** — the conversion a distance-shaped statistic needs in order to
stop being a count. `device_cpi` stays a declaration and stays in
`docs/SCHEMA.md` §4c's unknown column: a mouse reports counts and nothing in any
stream this project records says what physical distance produced them.

#### The fallback that was recorded and not taken

SGR-Pixels mode (`CSI ? 1016 h`) makes an xterm-compatible terminal report the
pointer in pixels rather than in cells, which would improve both the
quantisation and the speed-dependent cadence. It is recorded here because it is
the fallback if a terminal client is ever kept for development, and it is **not
taken**, because no terminal renderer is kept: `--headless` is what fills a seat
where there is no display and it needs no pointer at all, and a second renderer
would be a second capture path to keep honest for a corpus that has one.

The statement that outlives the decision, and it is the one that matters: **a
trace recorded through a terminal is unusable for the corpus in either mode.**
In cell mode for the reasons above. In pixel mode because the resolution becomes
the display's pixel grid rather than the device's counts, the report is still
triggered by a change of position rather than by a device event — so the cadence
still follows speed below one pixel — the coordinates are still integers clamped
to the terminal's window, and support varies by terminal, so the corpus's
resolution would be a property of which terminal each participant happened to
run.

### `replay`

**One file format, and it is signed.** At M3 and M4 this crate held an unsigned
container: it proved that some server wrote the file and nothing about who, and
two builds that reordered a rule resimulated differently with nothing in the file
to say which was right (`RISKS.md` R13). It was a development artefact and said
so. M5 replaced it rather than adding beside it — `Recording` survives as the
authority's in-memory product with **no encoding at all**, and `seal` is the only
path to a disk.

Keeping both was the alternative and it was rejected on one argument: a reader
that accepts a signed and an unsigned container accepts the weaker one, and a
corpus holding both holds files nobody can tell apart at a glance. The unsigned
one is precisely the artefact somebody would later hand you as evidence.

The `replay` binary is the tool: `replay verify <replay> <keys> [<telemetry>]` is
M3's separate process and M4's exit criterion, `replay keygen` and `replay inspect`
are the operator's, and `replay withdraw` / `replay audit` are the consent
regime's teeth. The companion is a **third argument rather than a file `verify`
goes looking for**, and both halves of that are decisions: a replay is verifiable
without one, so searching a directory would turn a legitimate absence into a
question about where somebody put a file; and accepting a companion found beside a
replay would be accepting a binding the replay never made. Handed none, `verify`
prints which of the two legitimate states the replay is in and checks nothing
else. M6 adds the three that operate a recording session — `replay enrol`,
`replay store` and `replay census`. They share a binary because
`docs/ENGINEERING.md` prefers five automations understood to fifteen endured and
this document refuses a crate for a handful of commands; they operate on
directories of the thing this crate defines. `docs/CONSENT.md` and
`docs/SCHEMA.md` are what they implement.

**`census` prints and stores nothing**, and that is the same decision as the
absent index. A stored summary is derived from the corpus, can disagree with it,
and outlives a withdrawal that changed what it summarised. Recomputing it costs
milliseconds on a corpus of dozens and is correct by construction. What it prints
is a page rather than a line because `docs/RISKS.md` R8 requires the two
confidence bounds to travel together, and it prints the sentence refusing "0%
false positives" beside them on every run.

**The session record is a second file in a match directory and it is not a second
format.** `replay::session` carries it: one entry per seat, no pseudonym, holding
what a *replay* structurally cannot — the hardware a participant declared, the
sensitivity the build applied, the platform, and whether the client kept up with
the tick (`docs/RISKS.md` R16). It cannot go in the manifest, which M5 froze and
whose every field is something the authority knows; the server has no idea what
mouse anybody is using, and an operator-filled field inside the server's signature
would be the server attesting to something it did not observe.

It crosses from `client` as **text rather than as a type**, because
`docs/ARCHITECTURE.md` forbids `client` a normal dependency on `replay` and that
rule stands: `replay` owns the signing key. The coupling that creates is closed in
`client/tests/session_part.rs`, where a test binary links both — a dev-dependency,
which the enforced `--edges normal` claim excludes — and requires the writer and
the reader to agree field for field.

**`verify` takes a key registry and there is no default.** A verification with no
registry establishes nothing — a signature is internally consistent by
construction, so "verified" without "verified as whose" is a word doing no work.
The registry is a required argument, and there is deliberately no `--insecure`.

```rust
pub struct Manifest {                  // this is what gets signed, not the log
    pub match_id: MatchId,             // 16 bytes; no uuid crate for a 16-byte array
    pub server_identity: VerifyingKey,
    pub seed: u64,
    pub rules_hash: Digest,
    pub sim_version: [u16; 3],         // the `sim` crate version: major, minor, patch
    pub sim_commit: SimCommit,         // the commit that build came from, or Unknown
    pub started_at_unix_ms: u64,
    pub participants: [Option<Pseudonym>; PLAYER_COUNT],   // per seat
    pub ticks: u32,
    pub inputs: u64,                   // what makes "truncated" its own error
    pub input_log_digest: Digest,
    pub outcome: Outcome,              // the claim a forged replay exists to make
    pub final_state_digest: Digest,
}
```

**The signature covers the manifest; the manifest covers the log by carrying its
digest.** That is R4's three failure modes answered at once — a genuine log
cannot be resubmitted under another match identity, no party without a registered
identity can mint replays, and a replay is distinguishable from a copy of itself.
The signed bytes are the magic, the format and the manifest, so a file cannot be
re-labelled as another format's and re-parsed under different rules while keeping
a signature that verifies. `replay::signed_bytes` is public, because "what is
signed" is the question M5 exists to answer and a private function is the weakest
place to answer it.

**Two fields are decisions rather than fields.** `outcome` is in the manifest
because it is the claim a replay is *submitted* to make — exploit class 2 is
result forgery — and being a field is what lets resimulation contradict it, which
is what gives "altered outcome record" an error of its own. `inputs` is the log's
length, and it is what makes a shortened log a distinct answer from a different
one.

**And the absences are decisions too**, at more length in `replay/src/manifest.rs`
because M5 freezes a format and whatever is missing is missing from the whole
corpus. No events and no frames: a replay carries the seed and the log and
resimulation derives the events, so there is no field for delivery order to get
into. No telemetry above one intention per tick: `sim` consumes one per tick at
30 Hz, so `client::input::InputTrace`'s kilohertz stream is a separate artefact
beside a replay rather than inside the one resimulation is a function of — folding
it in would have made the resimulation a function of something no rule reads. No
player identity beyond the pseudonym. No score or derived summary, because a
field restating a derivable fact is a field that can disagree with it. And no
client version, because the client is assumed compromised and the build that
matters is the one that resolved the match.

**The one field the freeze had to gain, and the shape it was given.** M8 keeps
the device stream after all, and the manifest carries its **digest** rather than
its contents: `telemetry: Commitment`, one tag byte and thirty-two, at a constant
width present or absent. Every absence above survives — the stream is still not
in the replay, and a resimulation is still a function of the seed and the log
alone — and what the commitment adds is that a companion cannot be substituted
and a replay stays verifiable without one. The container format is 2 and there is
no reader for 1: no corpus holds one, and a build that read both would be the
two-formats mistake this document already refuses, arriving through a version
number instead of a second magic.

### The telemetry companion

```rust
pub struct TelemetryManifest {         // signed, exactly as the replay's is
    pub match_id: MatchId,
    pub server_identity: VerifyingKey, // must be the key that sealed the replay
    pub started_at_unix_ms: u64,
    pub seats: [Option<SeatTrace>; PLAYER_COUNT],
    pub stream_digest: Digest,         // the body: every seat's records, in seat order
}

pub struct Telemetry { pub manifest: TelemetryManifest, pub signature: Signature, pub log: TelemetryLog }

pub enum Event {                       // one record, 25 bytes whatever it holds
    Moved { dx: f64, dy: f64 },        // the device's own units, by their bits
    Pressed { control: Control, down: bool },
    Viewed { tick: Tick, seq: u32 },   // the only record that is not the hand
}

/// The companion is sealed first; the replay then commits to its digest.
pub fn seal(log: &TelemetryLog, session: &SessionFacts, key: &SigningKey) -> Telemetry;
pub fn verify(replay: &Replay, telemetry: &Telemetry, keys: &KeyRegistry)
    -> Result<TelemetryVerified, TelemetryError>;
```

`docs/SCHEMA.md` §11 is the schema, the field list and the size budget. Three
things belong here because they are about the code rather than about the corpus.

**`TelemetryLog` has no encoding and `seal` is the only path to a disk**, which is
`Recording`'s arrangement one file over and for the same reason: a second,
unsealed container is precisely the artefact somebody hands you as evidence. The
one exception is a client's `*.telemetry-part`, and it is forced rather than
chosen — `client` may not link `replay`, which owns the signing key, so a client
cannot sign anything at all. A part names one seat, is consumed at sealing, and is
**not a corpus artefact**: `Corpus::audit` reports a match directory holding a
file `docs/SCHEMA.md` §1 does not name, which is the only check that can reach an
artefact carrying no pseudonym.

**`TelemetryError` has one variant per check and the checks run in order**, which
is `VerifyError`'s arrangement and buys the same property: nine tamper cases are
nine answers rather than one repeated. The first and the last are the two a replay
has no analogue for — `NotCommitted`, because absence is *signed* and therefore
cannot be quietly upgraded, and `Substituted`, because an attacker holding an
accepted key can seal a second internally perfect companion for the same match and
the only thing that refuses it is that the replay named other bytes first.

**The order of sealing is fixed by the direction of the commitment.** The
companion is sealed, then the replay commits to its digest. That is why the
assembly lives in `moba-server` — the process holding both the key and the
recording — and why a companion whose parts have not all arrived produces
`Commitment::Absent` rather than a file covering some of the seats.

**Sealing happens outside `Match`.** The authority has no clock, no socket and no
identity — that is what makes it a function of its inputs and what every
traffic-shape property is stated over — so a signing key inside it would be the
first secret in the one component that is supposed to have none. `Match` produces
a `Recording`; whoever holds the key produces a `Replay` from it and the session
facts it cannot know.

`rules_hash` covers the constants; `sim_version` and `sim_commit` cover the code
that reads them, which is the gap `RISKS.md` R13 is about — two builds can agree
on every constant and still resolve a tick differently. The version is enforced
mechanically: `sim` owns its version rather than inheriting the workspace's, and
CI refuses a pull request that touches `sim/` without raising it. The commit is
what makes a mismatch investigable rather than merely reportable, and it is
allowed to be absent, because a locally built server is a real case and a
manifest that lies about provenance is worse than one that admits it:

The commit is stamped by `replay`'s build script, from `git rev-parse HEAD` and
`git status --porcelain`, so a binary carries the commit it was *built* from
rather than whatever the machine it runs on has checked out. No `.git`, no
variable, and `Unknown`. Two `std::process` calls rather than a dependency, which
is `docs/ENGINEERING.md`'s bar met exactly.

```rust
pub enum SimCommit { Sha([u8; 20]), Dirty([u8; 20]), Unknown }

pub struct Replay { pub manifest: Manifest, pub signature: Signature, pub inputs: Vec<TimedInput> }

pub struct TimedInput {
    pub input: Input,
    pub claimed_at_ms: u64,            // untrusted
    pub received_at_ms: u64,           // server-observed: the only real clock
}

pub struct Build { pub rules_hash: Digest, pub sim_version: [u16; 3] }

/// Resimulates and checks the seal. The only defined way to assert anything
/// about a replay somebody handed you.
pub fn verify(replay: &Replay, keys: &KeyRegistry, build: &Build) -> Result<Verified, VerifyError>;
```

`Build` is a parameter rather than two ambient constants because "this replay is
from another build" has to be testable without changing the build under test.
Every caller in this workspace passes `Build::current()` except two: the tamper
suite, which constructs a mismatch, and the cross-platform fixture, whose version
field is a constant of the fixture.

**`VerifyError` has one variant per check and the checks run in order**, which is
what makes M5's six tamper cases six answers rather than one reported six times:
unknown key, signature, rules hash, sim version, truncated, input log, final
digest, outcome. Each catches the attacker who stopped one step short of the
next, and the naive attacker — who edits and cannot re-sign — is caught by the
first two before any of the rest run. "This replay is from another build" is its
own case and not a digest mismatch: the two have different answers, and a
verifier that conflates them teaches its reader to distrust the loud one.

**A retired key still verifies.** Retirement says what may be *sealed* from now
on; a verifier reports it and does not act on it. `RISKS.md` R4: rotating without
keeping the retired key published orphans every replay signed with it, which is
destroying evidence by housekeeping.

**What the whole apparatus does not establish** is in
`replay/src/container.rs`'s header at length, and the short version belongs here
too because it is the sentence a reader will otherwise supply for themselves:
resimulating a fully authoritative server's own inputs proves the server did not
corrupt itself, and catches **nothing about how anybody played**. A bot's inputs
resimulate exactly. And the comparison is `sim` against `sim` — a mutation inside
`step` moves both sides and reddens nothing here, while a mutation in
`Match::recording` or in the container's encoding moves one and reddens
immediately. It is a check on the recording, not on the rules; the committed
tri-platform digests are what covers the rules.

### `anticheat`

```rust
/// Everything a detector may look at: a sealed replay's log, the session record
/// beside it, and what each seat was **shown**, re-derived by resimulation.
pub struct MatchTelemetry { pub inputs: Vec<TimedInput>, pub seats: [Option<SeatFacts>; 9], .. }

pub trait Detector {                   // more than one implementation, so a trait is earned
    fn name(&self) -> &'static str;
    fn null_model(&self) -> &'static str;      // one sentence, or nobody will check it
    fn tail(&self) -> Tail;                    // which side a reviewer looks at
    fn calibration(&self) -> Calibration;      // Uncalibrated, or Fixed with its basis
    fn read(&self, t: &MatchTelemetry, seat: Seat) -> Reading;
}

pub struct Reading { pub score: Option<Score>, pub abstained: Option<String>, .. }
pub struct Finding { pub reading: Reading, pub calibration: Calibration, pub tail: Tail }

impl Finding {
    /// Whether a **person should look at this**. `None` while no corpus has
    /// fixed a threshold, which is every detector in this repository.
    pub fn for_review(&self) -> Option<bool>;
}
```

Detectors return findings. Nothing in this crate bans, disconnects, or notifies —
acting on a finding is a human decision, per `SCOPE.md`.

**Three things about that signature are decisions M8 had to take, and the first
is a correction to what this document used to sketch.**

*There is no `fn threshold(&self) -> Score`.* A signature that returns a
threshold unconditionally is a signature in which "there is no threshold" cannot
be expressed, and that is the only thing M8 has to be able to say: M6 is built
and not reached, so no threshold in this repository has been calibrated. A
`Fixed` threshold cannot be constructed without a `CorpusBasis`, and a
`CorpusBasis` cannot be obtained except from `Evaluation::basis`, which refuses
synthetic play, an empty corpus, and fewer than nine distinct participants.

*The score is an `Option` and abstention is a first-class answer.* Both reaction
detectors are on the **low** tail, so a seat that produced nothing would score
zero — the same number a bot answering instantly produces — if an absence were
scored rather than declined. A detector that scores silence flags the quietest
person in the corpus.

*`AccountHistory` is gone from this sketch.* Progression coherence is the one M8
candidate signal that needs a corpus spanning months of the same people, and a
parameter for a detector nobody has written is an abstraction with no
implementation. It comes back with the detector or not at all.

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
6. **The attacker links the protocol and nothing of the victim.** `cargo tree -p
   cheat-client --edges normal` shows no path at all to `client`, `server`,
   `replay` or `anticheat`, and `--depth 1` shows no direct edge to `sim`: its
   only workspace dependency is `protocol`, plus `ed25519-dalek`, because
   `cheat_client::forge` has to sign. Checked in `ci`, and exercised — a direct
   `sim` dependency added to the manifest is caught.

   **This used to read "no path to `sim`, `client`, or `anticheat`", and the first
   third of that was never true and could not be.** `protocol`'s own message types
   are stated in `sim`'s — a `View` carries a `PlayerView`, an `Input` carries an
   `Action` — so anything that speaks this protocol reaches `sim` through it.
   Nothing checked the claim, so nothing noticed for four milestones; M7 is when
   the crate gained content and the claim had to become true or go. `protocol`
   re-exports the wire's vocabulary so that the attacker's manifest names one
   workspace crate, and what the rule was always about survives intact: the
   attacker gets the surface a third party reading the wire format would have, and
   no `State`, no `step`, no `view_for`, no `Rules`.

   `sim`, `server` and `replay` are **dev**-dependencies of the exploit harness,
   which is the division that makes an exploit mean anything: an exploit asserting
   "the attacker did not learn where Red0 was" is an assertion about where Red0
   actually was, and only the world holds that. The judge needs the truth; the
   attacker must not have it, and that is two dependency lists rather than a rule
   somebody follows.

6a. **No production crate links the attacker.** The mirror of the rule above, and
   asserted for the same reason it needed rewriting: an exclusion nobody runs is
   an exclusion nobody has. `ci` checks `server`, `client`, `sim`, `protocol`,
   `replay` and `anticheat`, and `SECURITY.md` and `docs/RISKS.md` R7 are what it
   is enforcing.
7. `cargo tree -p client` shows no path to `anticheat`.
8. Every detector in `anticheat` has an exploit in `cheat-client` that runs
   against it in CI.

   **It said "fails against it" until M8, and the word had to change rather than
   the invariant.** Nothing *fails* against a detector: a detector emits a score
   and an evidence bundle and refuses nobody, and at M8 it cannot even say
   whether a reading is worth a look, because no corpus has fixed a threshold.
   What the pairing asserts instead is the same discipline pointed the other way
   — **each detector responds to its own exploit and is quiet against the same
   match played without the behaviour.** A detector that fired on an exploit
   without ever having been quiet proves exactly as little as an exploit that
   failed against a defence without ever having worked, and both are
   `docs/RISKS.md` R15.

   The controls are therefore part of the invariant and not decoration, and so
   is the third arm: `cheat_client::bot::Reflexes::Jittered` is caught by
   neither reaction detector, and `anticheat/tests/detectors.rs` asserts that
   green because `docs/SCOPE.md`'s ceiling is a limit this project states rather
   than defends.

   **And since M7, every exploit is run twice.** Once against a weakened version
   of the defence that does not stop it, and once against the one this project
   ships; the test is red if either half comes out wrong. The first half is
   `docs/RISKS.md` R15 applied to attacks — an exploit that fails against the real
   defence without ever having worked proves nothing, because it looks exactly
   like a defence that holds and there is no red to tell them apart.

   The weakened version is **never a Cargo feature**, for the reason invariant 10
   gives about `Serialize`: features are additive and unified, so a `no-culling`
   on `sim` would be a switch any crate in the graph could throw for the server
   binary. It is a surrogate built in the exploit harness — an omniscient
   projection for the culling, an unpadded transport for the traffic shape, a key
   registry that trusts the forger for the signature — so that both configurations
   run in one test binary and the *pairing* is exact: the same attacker, the same
   world, the same tick.

   Every one of them was exercised by mutation rather than argued, and the pass
   found its defect in the exploit suite rather than in the defences: the maphack
   read "what the fog shows" out of `view_for`'s own output, so a projection that
   leaked everything satisfied it by leaking consistently. That is invariant 5's
   trap one crate over, and the fix is the same — a re-derived predicate in
   `cheat-client/tests/harness/entitlement.rs`, carrying the same obligation that
   a change to the rule changes it in the same commit.
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
12. **The client's input capture is a function of the device event stream and of
   nothing the renderer knows.** The same device events, driven through two
   clients differing only in window size — 640×480 against 3840×2160, a factor
   of six in pixels per world unit — produce a byte-identical `InputTrace` and
   an identical aim. Asserted in `client/tests/capture.rs`, alongside the two
   properties that make the first one worth having: every device event produces
   exactly one sample even when it moves the drawn aim by nothing, and the trace
   is unchanged by how many frames were drawn between the events.

   This is `RISKS.md` R14's successor stated as a property rather than as a
   choice of renderer. A windowed client that read its aim off the drawn cursor
   would have rebuilt R14 in pixels; one that sampled on redraw would have
   rebuilt the speed-dependent cadence at the frame rate. Both mutations turn
   this red — see the pull request that introduced it for the messages, and for
   the third mutation that passed until the fixture stopped saturating the clamp.

   Two supports rather than one, because a test is a claim about the code as
   written: `client::draw` exposes no inverse projection at all, so there is no
   screen-space quantity for a capture to derive from; and `client::input::CLOCK`
   names in a type which clock a timestamp came from, so a platform that starts
   supplying a device time is a visible change rather than a silent one.

   And the residual that leaves — a dequeue stamp carries the delay between the
   device and this process — is measured rather than feared since M5.
   `client/tests/jitter.rs` runs the capture loop while it rasterises real frames
   and talks to a real server over QUIC, and isolates the delay the loop *adds*
   by differencing against a timestamp the event source read from the same clock:
   in `release`, a standard deviation of 0.016 ms and a worst case of 0.26 ms over
   1200 samples. The isolation is not a refinement — the recorded inter-arrival
   is the sum of the client's promptness and the source's regularity, and on a
   host with a coarse sleep granularity the second term is all of it, which is
   what the first Windows run of that test reported. `RISKS.md` R14 carries the
   table and what it does not cover.

13. **A replay is one file format, it is signed, and what is signed is the
   manifest.** `Recording` has no encoding; `replay::seal` is the only path to a
   disk; `verify` requires a key registry and has no permissive default. Eight
   checks run in a fixed order and each has its own `VerifyError`, which is what
   makes M5's six tamper cases six answers — `replay/tests/tamper.rs` runs them
   against an attacker who *can* re-sign, because every edit is a signature
   failure otherwise and the table would be one answer six times.

   Two things this invariant is careful not to claim. It says nothing about how
   anybody played: resimulating an authoritative server's own inputs catches a
   broken server and not a cheating client (`SCOPE.md`, class 2), and the
   matching exploit is M7's. And the comparison is `sim` against `sim` — verified
   by mutation, not asserted: doubling a champion's displacement inside `step`
   leaves the whole M5 suite green and turns the tri-platform fixture red at
   `divergence first visible at tick 100`, while mis-stamping a log entry's tick
   in `Match::recording` does the reverse. It is a check on the recording, not on
   the rules.

14. **A replay sealed on one target is byte-identical on the other two, and
   verifies there — and so is its telemetry companion.** The layer M5 adds above
   `State::digest`: a manifest's encoding, a log's encoding and a signature over
   them are three new places a platform can differ, and a log recorded on one
   machine and verified on another is what a replay is for.
   `replay/tests/sealed.rs` carries bytes sealed on Linux and committed, and the
   `determinism` workflow requires byte equality with them on all three targets.
   Encoding the seed little-endian turns it red with both hex strings printed.

   **The companion is a file and the three places are the same three**, so it is
   in the same fixture and the same job. What is new in the middle one is an
   `f64` pair per record written by `to_bits`: exactly specified by IEEE-754 and
   therefore precisely the shape of claim `RISKS.md` R1's negative control exists
   to distrust rather than assume, which is why the fixture's deltas are at the
   ends of the domain rather than whole numbers. The same test also executes the
   substitution: a second companion for the same match, honestly sealed by the
   fixture's own key, refused because the committed replay named other bytes.

15. **A match the consent regime cannot account for does not enter the corpus,
   and the refusal is at the door.** `Corpus::store` refuses eight ways —
   `replay/src/corpus.rs` carries the table — and the two that are new in kind
   rather than in detail are worth naming here. A participant whose consent record
   is from another version of `docs/CONSENT.md`, **or has no version at all**,
   is refused identically: a record written before the field existed does not
   decode, so "absent" and "stale" cannot be told apart by a corpus assembled
   under an older regime. And a seat whose client recorded **zero device events**
   is refused, which is the one mechanical thing a file can say about synthetic
   play — narrow by construction, since a bot moving a real mouse records as many
   samples as a person, and `docs/SCOPE.md`'s ceiling is where that stops.

   Nothing is written until every check has passed. A corpus that half-stored a
   match it then refused would be holding telemetry it had already decided it may
   not hold.

16. **The corpus holds no derived artefact, and the two things M6 could have made
   one of are a file that is primary and a rule that is a function.** The session
   record beside each replay carries what a replay structurally cannot — the
   hardware, the sensitivity, the platform, and whether the client kept the tick —
   and it is primary rather than derived, indexed by **seat and never by
   pseudonym**, and filed inside the directory a withdrawal removes whole. The
   train/holdout split is `replay::split::split_of(match_id)`, a pure function of
   a frozen salt and the identifier, stored nowhere.

   Both decisions are the same decision, and `docs/CONSENT.md` records why: the
   way a destruction promise fails is a derived artefact outliving what it
   described. A split *file* has a second failure of its own — a withdrawal would
   leave a line about somebody's participation after they asked for it to be
   destroyed, and a date-ordered rule would reshuffle a holdout a threshold had
   already been chosen against.

   What keeps a future one out is the audit's crudeness, extended: a match
   directory whose replay **or** whose session record fails to read is reported
   unconditionally, for every pseudonym, because a seat record with no manifest in
   front of it describes somebody's session and nobody can say whose.
   `replay/tests/withdrawal.rs` breaks the withdrawal on both files and plants an
   index besides.

17. **The device stream is a sealed file the replay commits to by digest, and a
   withdrawal reaches it.** `RISKS.md` R3 and `docs/SCHEMA.md` §11. It is the
   richest personal information in this corpus and it names **no pseudonym**,
   which is deliberate — the signed manifest stays the one naming of a person —
   and is exactly what makes it unreachable by the audit's byte search. Two
   clauses close that:

   - `Corpus::accountable` requires the telemetry state to be **coherent**: the
     replay commits to a companion and the companion is there and is that one, or
     it commits to none and there is none. A stream with no manifest in front of
     it is the same orphan the session record was, in a richer form.
   - …and requires the match directory to hold **nothing else**, which is
     `docs/SCHEMA.md` §1's rule enforced for the first time. The case it is
     really about is a client's `*.telemetry-part` left behind by an interrupted
     collection: one seat's hand movements, naming nobody, that no search for a
     pseudonym in any corpus would ever report.

   Both were exercised by breaking the destruction on purpose —
   `an_audit_catches_a_withdrawal_that_left_the_telemetry_companion_behind` removes
   the two files a search *could* find and leaves the stream, and the audit
   reports the directory for every pseudonym including one the corpus has never
   held.

   And the two files that describe one seat cannot drift: `Corpus::store` refuses
   a session record and a companion that disagree about a seat's device-event
   count, its motions, its clock, its platform or its sensitivity. Neither is
   derived from the other — the summary is what survives when there is no
   companion — so nothing but that refusal would notice.

18. **The lobby is driven by the integrated cursor, and the window cannot reach
   it.** The same device events, driven through two clients differing only in
   window size — 640×480 against 3840×2160 — produce a byte-identical
   `InputTrace`, an identical cursor and **identical calibration observations**.
   Asserted in `client/tests/lobby.rs`, which is invariant 12 restated over the
   menu and exists for the reason that one does: a measurement taken through a
   display inherits the display.

   The two mutations that break it are the two natural ways to write a menu.
   Driving the lobby from a **system pointer** — one device count moving it one
   pixel — makes the synthetic hand miss every element, because it steers by the
   cursor a player sees; **measuring the movement in pixels** rather than in the
   device's own counts makes two clients report scales a factor of six apart.
   `client::lobby` holds no viewport and `client::draw` still exposes no inverse
   projection, so there is no screen-space quantity for either to derive from.

   **And a third mutation passes, which is recorded rather than left to be
   discovered.** Quantising the resolved cursor to the pixel the renderer would
   draw it on — R14's own failure, one order finer — changes nothing, because the
   measurement never reads the cursor as a *quantity*: a reach's distance is two
   positions the build fixes and its cost is the raw deltas, and the cursor
   decides only *which* element was clicked, over radii of six and eleven world
   units. That is a robustness of the design rather than a hole in the test, and
   it is why the property is stated over the observations rather than over the
   cursor.

   Two properties travel with it, because the measurement is worth nothing if it
   is not the thing it claims to be. A simulated crossing recovers the scale this
   build actually applies — 20 device counts per world unit — to within **0.04%**,
   with the arrival cost landing in the intercept where it belongs rather than in
   the slope; and the dummy's station table is asserted to reach eight octants and
   a distance ratio above four, which is `docs/RISKS.md` R15 applied to an
   interface.

19. **An insufficiently calibrated seat is marked and never refused.**
   `Corpus::store` gained no check for it, deliberately: a seat whose device is
   unknown is filed as `partial` or `absent` and the match is stored. What the
   state governs is a *reading* — a detector depending on the scale answers `None`
   for it, which is the treatment M8 already gives an uncalibrated threshold — and
   `docs/SCHEMA.md` §4e carries the rule. Blocking a player for a calibration
   reason is the shortest path to an anti-cheat that degrades the experience of
   honest players, which is `docs/SCOPE.md`'s standing position about sanctions
   arriving one level down.

20. **Every separable consent is applied by a value that cannot be built without
   the check, and never by a rule anybody follows.** `docs/CONSENT.md` offers four
   permissions a participant may refuse on their own, and `replay::permit` is
   where each one stops being a field:

   - `replay::Publishable` is the **only** value this workspace writes to a
     publication directory, and `Publishable::of` is its only constructor. So
     publishing a match somebody refused is not a mistake to avoid — it is a value
     that does not exist.
   - `replay::TrainingSet` is the **only** value that yields corpus matches for
     training, and `TrainingSet::load` is its only accessor. A trainer's signature
     is `fn(…, &TrainingSet)`; a caller holding a `Corpus` and a list of
     identifiers cannot reach the data.
   - `Corpus::attribution` is the **only** path from a pseudonym to a person in
     this workspace, and it refuses without `named-attribution`.

   The predicate underneath all three is `all`, never `any`: a match is one
   interleaved log, so one refusal withholds the whole of it — the rule
   publication already had, generalised. The permissions are read off the disk at
   the moment of use and cached nowhere, which is what makes a partial withdrawal
   an edit to one file with nothing to invalidate.

   Exercised by mutation in `replay/tests/permissions.rs`: one bit in one consent
   record, everything else identical, and the match stops being publishable and
   the training set stops containing it.

21. **A consent record answers every question this build knows how to ask, and a
   silence is not an answer.** `Permissions::decode` refuses a record that omits a
   line for any `Purpose`, and `ConsentRecord::decode` refuses one that omits the
   age answer — the same equivalence invariant 15 draws between an absent consent
   version and a stale one, one level down.

   The consequence is the property: **adding a fifth `Purpose` invalidates every
   consent record already written**, so everybody is asked again and there is no
   path by which an old signature quietly covers a new use. `docs/RISKS.md` R3's
   "an old consent cannot silently authorise a new collection" becomes true of
   *purposes* and not only of *fields*.

   And two withdrawals rather than one, with two audits that are not the same
   check. `Corpus::audit` reads every byte under the root for a name;
   `Corpus::audit_purpose` runs the *use's own gate* over the matches the
   participant is in, because re-reading the record a revocation just wrote would
   be the command agreeing with itself. Empty is the only acceptable answer to
   either.

22. **A match a program played does not enter the corpus, and the refusal is a
   value that cannot be built.** `replay::Attested` is the only value
   `Corpus::store` accepts and `Attested::of` is its only constructor, which is
   invariant 20's shape one level below a purpose: it refuses a match in which a
   seat the **input log** shows playing has no `SeatRecord::Human` behind it.

   The choice of what it reads is the whole of it. `Corpus::store` already
   compared the session record against the manifest's participant list, and both
   of those are written by the operator — so a playtest filed as "one person
   played" produces two files that agree perfectly about a match whose other
   eight seats were `client::bot`. The input log is what the *authority*
   observed and is covered by `input_log_digest` inside the signature, so a seat
   that played cannot be un-played by the way somebody files it. A session record
   is the other half: it exists only because a client's capture path wrote a
   part, and `SeatRecord::decode_part` refuses a part claiming any provenance but
   a person's.

   It is deliberately one-directional — a seat with a record that never played is
   not refused here, because that is a broken client rather than synthetic play
   and the manifest comparison already covers the operator's side. And it
   establishes exactly one thing more than M6 did: a seat with **no device behind
   it at all**. A bot moving a real mouse would write a part like anybody's, and
   `docs/SCOPE.md`'s ceiling is where that stops.

   Exercised end to end in `client/tests/playtest_bots.rs`: a match one person
   and two bots played over the real transport is refused by name, the two
   operator-side files are asserted to agree seat for seat so that the refusal is
   demonstrably not the one that already existed, and the same pipeline stores
   the match with no bot in it.

## Directions this architecture leaves open, and does not build

Recorded because they will be obvious to whoever reads `client::lobby` next, and
because both of them belong to a sub-project `docs/SCOPE.md` puts third in the
queue — matchmaking — which is far enough away that an implementation written now
would be stale before it was useful. What is worth keeping is the *observation*,
not the code.

**Calibration during matchmaking.** The wait this pass exploits is the wait for
eight named people in a scheduled session. A matchmade queue has a different and
larger one: a player sits in it for a minute or two with nothing to do, and the
natural things to put there — a warm-up server, a practice range, a dummy at a
known distance — are the same instrument as the lobby's, at a scale where a
device profile could be estimated properly rather than accumulated over evenings.
Nothing about `client::lobby`'s design forecloses it: the geometry is a table of
constants and the observations pool by addition, so a queue's crossings would fold
into the same profile as a lobby's.

**The first seconds of a match are already a calibration trajectory.** A champion
starts at its base and the player's first order is almost always a walk to a lane,
which is a movement of **known geometry** — the base and the lane are constants of
`sim::rules` — that nobody had to provoke and that every match in the corpus
already contains. It is not free: the aim reaches the wire only at the instant of
a click, so what a replay holds is the two endpoints rather than the path, and the
path is in the telemetry companion under a different clock. Reconstructing it
means aligning a `Viewed` anchor against the first `Move` and integrating the
deltas between them, which is real work and is exactly the sort of thing that
should be written against a corpus that exists rather than one that does not.

Both are recorded here and in no issue tracker, for the reason
`docs/ENGINEERING.md` gives about automation: an idea that has to be maintained
somewhere is an idea that goes stale where nobody is reading.

## Deliberate non-abstractions

One champion means a concrete `Champion` struct, not a trait. One transport
means concrete types, not a `Transport` trait. Two message directions mean two
enums, not a codec framework. One client means one renderer: no `Renderer`
trait, no backend abstraction, and `client::draw::Mark` is a flat enum the
rasteriser matches exhaustively rather than a display list anything else could
implement. `Detector` is a trait because there will be five of them and the
server iterates over a collection — that is the bar an abstraction has to clear
here.
