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
- Collusion between human players (win trading, intentional feeding). Social
  problem, detected socially.

## In scope

### Game (MVP, frozen)

3v3, one shared champion, one lane. Fog of war. One skillshot, one targeted
spell, a basic-attack range. Towers. No items, levels, minions, jungle, or
matchmaking. Tick-based authoritative server, client-side reconciliation.

The MVP is frozen. Additions to the game are only in scope when a specific
anti-cheat experiment requires them, and the milestone that requires them says so.

### Structural invariants

These are not features, they are properties the codebase must never lose:

1. `step(&State, &[Input]) -> State` is pure: no clock, no I/O, no async, no
   floating point, explicit seed, fixed-point arithmetic.
2. Per-player visibility is a projection, `view_for(&State, Seat) ->
   PlayerView`, computed server-side and applied before serialization. An entity
   outside vision is **absent from the message**, not flagged invisible. The
   seat is a six-valued type rather than an integer, so "whose vision is this"
   has no answer outside the match to get wrong; the byte that names it is
   validated at the protocol boundary or refused there.
3. `State` is not serializable anywhere in the workspace. Only `PlayerView`
   crosses the wire. This makes the maphack defense a compile-time property
   rather than a code-review habit.
4. Every playable build records replays (seed + timestamped inputs) from the
   first playable build onward.

### Anti-cheat (the actual project)

Five exploit classes. Each defense is only considered delivered once the
matching exploit exists in the repository and fails against it in CI.

| # | Exploit class | Defense in scope |
| --- | --- | --- |
| 1 | Maphack: reading information not visible | Strict server-side culling; message-size padding against traffic analysis; culling of *derived* signals (sound, damage events, cast events) |
| 2 | Result forgery: unplayed match, edited replay | Signed replays; server-issued match records; offline resimulation of the input log |
| 3 | Synthetic input and botting | Server-arrival-time telemetry; behavioral statistics calibrated on a human corpus; account-progression coherence over time |
| 4 | Time manipulation: slowdown, clock desync | Divergence between client-claimed and server-observed input timestamps as a first-class signal |
| 5 | Protocol abuse: replay, concurrency, out-of-sequence | Monotonic input sequence numbers, idempotent session commands, per-player rate limits |

Note on class 5: a QUIC/TLS transport already defeats naive packet replay and
reordering. The in-scope work is the application-level residue — session
commands and input sequencing — and the documentation must say which layer is
doing the work rather than claim credit for the transport.

Note on class 2: against a fully authoritative server, resimulating the server's
own inputs proves the server did not corrupt itself; it does not catch a
cheating client. Resimulation earns its place at the surfaces where a
**client-supplied artifact asserts an outcome**: replay files, submitted match
records, third-party verification of a published replay, and offline reanalysis
at higher-than-realtime cost. That is where it is scoped, and where the tests
live.

### Cheat client

A first-class crate, not a test utility. It speaks the protocol directly, never
links the real client's internals, and each exploit is expressed as a headless
assertion — "the attacker learns X" or "the server accepts Y" — so the whole
suite runs in CI without rendering.

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
| Automatic bans | A false positive is worse than a missed cheater. Detectors emit scores and evidence bundles; acting on them is a human decision, out of scope for automation |
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
