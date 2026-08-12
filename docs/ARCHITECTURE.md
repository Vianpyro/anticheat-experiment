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
#![deny(clippy::float_arithmetic)]

pub struct Tick(pub u32);
pub struct PlayerId(pub u8);        // 0..6
pub struct EntityId(pub u16);
pub type Fx = /* fixed-point scalar, i32 with a fixed fractional width */;
pub struct FxVec2 { pub x: Fx, pub y: Fx }

pub struct State { /* tick, rng, [Champion; 6], towers, projectiles, outcome */ }

pub struct Input {
    pub tick: Tick,               // tick this input applies to
    pub seq: u32,                 // per-player, monotonic; protocol-level identity
    pub player: PlayerId,
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

impl State {
    /// The only way to compare states. `State` is deliberately not serializable;
    /// see docs/RISKS.md R5.
    pub fn digest(&self) -> Digest;   // [u8; 32]
}

pub fn new_state(seed: u64) -> State;
```

### `sim::view` — the visibility projection

Separate module, separate call, computed from the full world state. `step` never
reads it, and `State` carries no per-player visibility.

```rust
/// Strict culling: an entity outside vision is absent from the result, not
/// flagged. Derived signals (damage events, cast events, sound cues) are culled
/// on the same rule.
pub fn view_for(state: &State, player: PlayerId) -> PlayerView;

#[derive(Serialize)]                  // the only serializable state type
pub struct PlayerView {
    pub tick: Tick,
    pub own: ChampionView,            // full detail, always
    pub visible: Vec<EntityView>,     // only what this player can see
    pub events: Vec<VisibleEvent>,    // culled on the same rule
}
```

`PlayerView` is the sole state type crossing the wire. Because `State`
implements no serialization anywhere in the workspace, "the server accidentally
sends the whole world" is a compile error rather than a bug class.

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
    pub sim_version: u16,
    pub started_at: SystemTime,
    pub participants: Vec<PlayerPseudonym>,
    pub input_log_digest: [u8; 32],
    pub final_state_digest: [u8; 32],
}

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
case is rejected for the right reason.

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

1. `sim` compiles with `forbid(unsafe_code)` and denies float arithmetic; a
   `clippy.toml` disallowed-types list blocks `std::time`, `HashMap`, and `rand`.
2. Identical seed and input log produce an identical `State::digest()` on
   x86-64 Linux, x86-64 Windows, and aarch64 macOS.
3. No `Serialize` impl exists for `State` or its components; only the view types
   have one. Checked in CI.
4. For every tick and player of the reference fixture, `view_for` output
   contains no entity outside that player's vision.
5. `cargo tree -p cheat-client` shows no path to `sim`, `client`, or `anticheat`.
6. `cargo tree -p client` shows no path to `anticheat`.
7. Every detector in `anticheat` has an exploit in `cheat-client` that fails
   against it in CI.
8. Every `View` message has the same encoded size, and the server emits exactly
   one per connected player per tick. Checked by the M7 traffic-analysis
   exploit, which must fail to recover the visible-entity count from a recorded
   session's message sizes and arrival times.
9. No `Serialize` impl for `State` exists behind any Cargo feature either — the
   only sanctioned constructors are `#[cfg(test)]`-gated, and no reconnection
   path transports state.

## Deliberate non-abstractions

One champion means a concrete `Champion` struct, not a trait. One transport
means concrete types, not a `Transport` trait. Two message directions mean two
enums, not a codec framework. `Detector` is a trait because there will be five
of them and the server iterates over a collection — that is the bar an
abstraction has to clear here.
