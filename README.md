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
| `replay` | Replay container, signing, verification, resimulation; the corpus, and the sealed telemetry companion a replay commits to |
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

## Checking a release you downloaded

Every asset on a release is covered by a build provenance attestation. It is not
a signature by anybody here — this project holds no signing key for its releases
and deliberately does not — so what it establishes is narrower and more useful:
**which workflow, in which repository, at which commit, built this file.**

```sh
gh attestation verify moba-server-v0.2.0-x86_64-unknown-linux-gnu.tar.gz \
  --repo Vianpyro/moba \
  --signer-workflow Vianpyro/moba/.github/workflows/release.yml
```

The `--signer-workflow` flag is the part worth not dropping. Without it the
command answers "something in this repository built it", which a pull-request
workflow also satisfies; with it, the answer is that the release pipeline did.

The server image is verified the same way, by digest rather than by tag, because
a registry tag is a mutable pointer:

```sh
gh attestation verify oci://ghcr.io/vianpyro/moba-server@sha256:… \
  --repo Vianpyro/moba \
  --signer-workflow Vianpyro/moba/.github/workflows/release.yml
```

Each release also carries a CycloneDX SBOM per binary per platform
(`moba-<crate>-v<version>-<target>.cdx.json`) listing every crate linked into
that binary, and a `SHA256SUMS` over all twelve files. The SBOMs are covered by
the attestation as well, since an SBOM nobody can check is a place to be wrong
about what a binary contains.

## Playing it before nine people are free

A match is nine seats. `moba-bots` fills the ones nobody is sitting in, so that
one or two people can start one:

```sh
cargo run -p server --bin moba-server -- 3000 33 --players 9   # prints an address and a certificate
cargo run -p client --bin moba-client -- <address> <certificate>
cargo run -p client --bin moba-bots -- <address> <certificate> 8
```

The bots compose one intention per tick from the view the server sent them, over
the ordinary protocol; they synthesise no device input of any kind. It is a
**playtest tool**: it does not satisfy M4's exit criterion, which asks for three
humans on two operating systems, it produces no corpus data — a match with a bot
seat in it is refused at the corpus door by `replay::Attested` — and it
calibrates nothing. `docs/MILESTONES.md` M4 and `docs/RISKS.md` R7 carry the
reasoning, including why it is in `client` and not in `cheat-client`.

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
- `docs/detectors/` — one page per behavioural detector: its null model, the
  exploit it responds to, the control it stays quiet against, and what the corpus
  would have to be before its threshold could be a number. **None of them is
  calibrated**, and the index says which of M8's candidate signals are buildable,
  which are not, and why.
- `docs/SCHEMA.md` — what the human corpus holds, field by field, including the
  telemetry companion and what it costs to keep.

## Contributing and security

`CONTRIBUTING.md` for scope and process; `SECURITY.md` for how to report a
vulnerability in the server, and for the boundary around the cheat client — it
targets this project only, and contributions targeting other games are refused.

## License

MIT. See `LICENSE`.
