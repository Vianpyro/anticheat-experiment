# moba

A minimal MOBA used as a substrate for anti-cheat engineering. The game is the
test fixture; the anti-cheat is the subject. Both the attack and the defense
live in this repository.

The starting axiom is that **the client is compromised and lying**. Every
defense here must hold when the attacker controls the client binary, its memory,
its clock and its input stack — so there is no obfuscation, no anti-tamper and
no kernel driver, and their absence is a design decision rather than an omission
(`docs/SCOPE.md`).

A defense is only considered delivered once the matching exploit exists in this
repository and fails against it in CI.

## Status

**M1 — deterministic simulation core.** `sim` holds the rules: a fixed-point
type with stated overflow semantics, a seeded generator, `State`, `Input`,
`step`, and a `State::digest()` whose encoding stops compiling if a field is
added and not covered. It depends on nothing. Two fixtures run on x86-64 Linux,
x86-64 Windows and aarch64 macOS against digests committed here.

No protocol, no server, no rendering and no exploits yet. See
`docs/MILESTONES.md` for what lands when.

## Workspace

Seven crates, whose boundaries make two security properties structural rather
than procedural: the client cannot receive information it should not see, and
the detection logic never ships to the attacker.

| Crate | Owns |
| --- | --- |
| `sim` | The rules. `State`, `Input`, `step`, `view_for`, fixed-point math. Depends on nothing in the workspace |
| `protocol` | The wire, and the only trust boundary |
| `replay` | Replay container, signing, verification, resimulation |
| `server` | Authority: tick loop, the clock, sessions, fog application, telemetry |
| `client` | Presentation. Never links `anticheat` |
| `anticheat` | Detection. A pure function from telemetry to scores and evidence |
| `cheat-client` | The attacker, and the exploit suite. Speaks `protocol` and nothing else |

`docs/ARCHITECTURE.md` has the dependency rules and the reason for each.

## Building

The toolchain is pinned in `rust-toolchain.toml` and `rustup` installs it on the
first `cargo` invocation. Nothing else is required.

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Those three commands are exactly what CI runs, on Linux and on Windows. The
determinism fixtures additionally run on aarch64 macOS, under `--release`:

```sh
cargo test -p sim --release --locked --test determinism -- --nocapture
```

## Documents

The documents in `docs/` are the specification. They are meant to be read in
this order:

- `docs/SCOPE.md` — what is in, what is out, and why each exclusion is an
  argument rather than a preference.
- `docs/ARCHITECTURE.md` — the seven crates, the central types, and the
  invariants that are lints or tests rather than conventions.
- `docs/RISKS.md` — the decisions that are irreversible, when each must be
  taken, and the cheapest hedge available now.
- `docs/MILESTONES.md` — M0 to M9, each with an exit criterion that is a command
  rather than an inspection.
- `docs/ENGINEERING.md` — toolchain, workflows, supply chain, release, and what
  stays manual on purpose.

## Contributing and security

`CONTRIBUTING.md` for scope and process; `SECURITY.md` for how to report a
vulnerability in the server, and for the boundary around the cheat client — it
targets this project only, and contributions targeting other games are refused.

## License

MIT. See `LICENSE`.
