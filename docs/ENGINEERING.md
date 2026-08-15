# ENGINEERING

The operating rule for everything below: **five automations understood beat
fifteen endured.** A solo project revisited every few months does not fail from
too little automation, it fails when a workflow nobody remembers blocks a merge.
Every automation here must be explainable in one sentence and removable in one
commit.

## Platform matrix

| Target | Where it appears | Why |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | Every CI job; release binaries; container | Primary development and server target |
| `x86_64-pc-windows-msvc` | Every CI job; release binaries | Primary player platform, and a genuinely different toolchain |
| `aarch64-apple-darwin` | Determinism job only; release binaries for client and server | The second CPU architecture. This is what catches determinism leaks that x86-only CI hides — it is in the matrix for a security reason, not for macOS support |

Not supported: 32-bit, musl, BSD, WebAssembly, Linux client packaging beyond a
tarball. Each would add a build target for no additional evidence about the
thing being demonstrated.

## Branches

Trunk-based. `main` is always releasable and always protected. Work happens on
short-lived branches named `category/slug`, already enforced by the existing
branch-name workflow — keep it, it is cheap and it makes the generated changelog
legible.

Squash merge only, so `main`'s history is one commit per change and the
conventional-commit type on the squash commit is what the changelog consumes.

Branch protection on `main`: required status checks, linear history, no force
push. **Not** required reviews — a solo developer cannot approve their own PR,
and a rule you must click "bypass" on every week is a rule that trains you to
ignore rules. The PR exists to run CI and to give the change a description, not
to simulate a reviewer.

Conventional commits are already documented in `.copilot/commit-message-instructions.md`.
Enforce them on the PR title only (that is what becomes the squash commit),
with a check that runs in seconds. No commit hooks: a hook that rejects a
work-in-progress commit message is friction with no payoff, since the individual
commits are squashed away.

## Workflows

Seven, with their triggers and their permissions. Every workflow declares
`permissions: contents: read` at top level and elevates per job only where
required.

| Workflow | Trigger | Jobs | Permissions | Budget |
| --- | --- | --- | --- | --- |
| `ci` | PR, push to `main` | `check` (fmt + clippy `-D warnings` + test, matrixed over Linux and Windows, plus the Linux-only additions below), `consent-version` | `contents: read` | < 5 min wall, warm |
| `pr-hygiene` | PR, push to any branch but `main` | `branch-name` (skipped for pull requests from a fork, whose branch names are the fork's business), `pr-title` (Conventional Commits, every pull request) | `contents: read` | seconds |
| `determinism` | PR and push touching `sim/`, `replay/`, the fixtures, the lockfile or the toolchain pin | `fixture` (the fixtures on Linux x86-64, Windows x86-64, macOS aarch64, under `--release`, each compared against digests committed in the repository, plus the replay and its telemetry companion sealed on Linux and committed, which every target must reproduce byte for byte, verify, and check the binding between), `properties` (the same three targets with a raised `PROPTEST_CASES`), and `sim-version` (a PR touching `sim/` must raise the crate version — `RISKS.md` R13) | `contents: read` | < 6 min |
| `supply-chain` | PR touching a manifest, the lockfile or `deny.toml` (licenses, bans, sources) and weekly cron (advisories) | `cargo-deny` | `contents: read` | < 2 min |
| `coverage` | Weekly cron, manual dispatch | `cargo llvm-cov`, uploaded as an artifact | `contents: read` | untimed |
| `release-plz` | Push to `main` | `release-pr` (computes the bump from conventional commits, writes `CHANGELOG.md`, opens a pull request; it does not tag, publish or push to `main`) | `contents: write`, `pull-requests: write` — this job only | < 2 min |
| `release` | Tag `v*` | `draft`, `build` (client and server on the three targets), `checksums`, `container`, `publish` | `contents: write` on `draft`, `build`, `checksums` and `publish`; `packages: write` on `container` — per job, never at the top | < 20 min |

`ci` runs fmt, clippy and the tests in one matrixed job rather than splitting a
Linux-only `check` from a two-platform `test`, because M0's exit criterion asks
for all three green on both platforms. On an already-compiled workspace the two
extra Windows steps cost seconds, and a Windows-only clippy finding — from a
`cfg`-gated path, most likely — is exactly the kind of thing a Linux-only lint
job would let through.

`check`'s Linux-only steps, each of which reads the working tree or the resolved
dependency graph rather than the build, so running them twice would buy nothing:
the property suites outside `sim` at a raised case budget; a grep for a
serialization derive in `sim` (`RISKS.md` R5); `cargo tree -p sim --edges normal`,
which must print `sim` and nothing else; `cargo tree -p client --edges normal`,
which must show no path to `anticheat` or `server`; the two graph checks M7 added
around the attacker (`ARCHITECTURE.md` invariants 6 and 6a) — it links nothing of
the victim, and no production crate links it; and the checks that no recording,
consent record or signing key is tracked in git.

**The `exploits` job planned for M7 is a step in `check` instead, and that is a
deliberate demotion.** `cargo test --workspace` already runs the exploit suite, on
both platforms — which is more than the planned Linux-only job — so a separate job
would re-run tests that have just run and return the same information, which is
exactly the second automation R11 says to refuse. What the step buys is
`--nocapture`, so the exploit-by-exploit account is in the run summary rather than
only in a failure: `RISKS.md` R15's hedge is that the number is printed even when
it passes, because the value of a counter is that a reader sees it and asks the
question.

**And it is the one report step that is not Linux-only, since M8.** It was, and
that was R15's own failure committed on a report rather than on a fixture: a
counter whose whole value is that a reader sees it was printed on one of the two
platforms this project supports. `RISKS.md` R16's live question — whether
`windows-latest` bunches a client's frames more often than `ubuntu-latest` at the
same tick period — was therefore a question no run log could answer, while the
criterion that measures it stayed green on both. So the step runs on both and
carries `client/tests/m4_exit.rs` with it, because that is where the bunching
count lives. It costs about thirty-five seconds a platform, which is the price of
a number R16 has been re-diagnosed twice for want of.

The determinism job compares against committed digests rather than shipping each
job's result to a fourth job that compares them. That is strictly stronger and
much simpler: three jobs checking the same constant already detect any
disagreement between platforms, and the constant additionally detects drift over
time on a single platform — which cross-job comparison cannot see at all, since
three jobs that have all drifted the same way still agree with each other. It
also means a compiler upgrade that perturbs the simulation arrives as a failing
test with a diff, which is exactly the reviewable event the pinned toolchain
exists to produce.

Advisories run on a schedule rather than on PRs deliberately: a CVE published in
a transitive dependency has nothing to do with the PR in front of you, and a red
check you did not cause is the fastest way to learn to ignore red checks.

Caching: `Swatinem/rust-cache`, keyed on OS plus the pinned toolchain plus
`Cargo.lock`. No `sccache` — it is a second cache layer to reason about for a
workspace this size.

Every third-party action is referenced by commit SHA with the tag in a trailing
comment, since M3. `RISKS.md` R12 is why, and why not earlier: the pins and
Renovate, which is what keeps them current, land in one change or not at all.
The count is three. It was two for six milestones — `supply-chain` installs
`cargo-deny` with `cargo install --locked` rather than reaching for an action —
and the third is `release-plz/action`, taken because `release-plz` links `cargo`
itself and compiling it on every push to `main` to open a pull request costs
minutes where `cargo-deny` costs seconds. `release` adds none: it publishes with
`gh` and builds its image with `docker`, both already on the runner, which is
three actions avoided in the one workflow that holds write tokens.

### Changes to what exists today

Remove `super-linter`. It duplicates rustfmt and clippy on a Rust workspace, it
is slow, and it holds `contents: write` so it can push commits onto your PR
branch. An automation that rewrites your branch while you work is a
supply-chain surface and a source of confusing history, in exchange for
formatting Markdown. If Markdown linting is wanted, run a linter in check mode
and fix the file yourself.

Keep the branch-name check. It now lives in `pr-hygiene` alongside the PR-title
check, since both are the same kind of thing — a few lines of `bash` reading the
event payload, no checkout, no third-party action — and two workflows that each
run for three seconds are one workflow more than the project needs to remember.

Convert the devcontainer to a Rust base image —
but the devcontainer installs the toolchain, it never defines it.
`rust-toolchain.toml` is the single source of truth, so that a build inside the
container, on the host, and in CI are the same build.

## Toolchain and dependency policy

`rust-toolchain.toml` pins an exact stable version with the components CI needs.
Pinned, not `stable`: a compiler upgrade that changes optimization behavior is
exactly the kind of event that can perturb a determinism claim, and it should
arrive as a reviewable commit rather than on a Tuesday.

`Cargo.lock` is committed — the workspace produces binaries, and reproducing a
bug from six months ago requires it. CI builds with `--locked`.

Dependencies in `sim` are near-zero by policy (see `ARCHITECTURE.md`). Elsewhere,
a new dependency needs a reason that a few lines of code would not satisfy.

### Renovate

Arrives at M3, when there are dependencies to update. Configuration:

- **One grouped PR per week**, not one PR per dependency. The value for an
  intermittent project is returning after two months to a single
  CI-validated update, not to a wall of forty PRs.
- `minimumReleaseAge: 7 days`. A quarantine delay against a compromised
  release reaching your lockfile in the first hours after publication, which is
  when that class of incident is discovered.
- **Auto-merge** patch and minor updates of `dev-dependencies` when CI passes.
  Blast radius is the test suite, and the test suite is what is validating them.
- **Manual review** for every production dependency, any major bump, and any
  change to `sim`'s dependencies — no exceptions, since `sim` is where a silent
  behavioral change is most expensive (`RISKS.md` R9).
- `rangeStrategy: bump`, lockfile maintenance monthly.
- **`helpers:pinGitHubActionDigests`**, which rewrites every third-party action
  reference from a mutable tag to a commit SHA and then keeps those SHAs
  current. This is the second half of `RISKS.md` R12: the pins and the
  automation that maintains them land in the same milestone, because a SHA pin
  with nothing to bump it is an unpatched action wearing a security measure.

The lighter alternative, if Renovate ever becomes noise: delete it and run
`cargo update` by hand once a month, with `cargo-deny` on the schedule to tell
you when that is urgent. Say so explicitly rather than letting it rot.

### Licensing

**MIT alone, not the `MIT OR Apache-2.0` dual license that is the Rust
ecosystem's default.** The holder is Vianney Veremme; the year is the year of
first publication and does not get bumped annually.

The dual license is a convention with two specific purposes, and neither one
reaches this repository:

- *License compatibility for downstream crates.* This is the real reason the
  convention exists — a crate published to crates.io is linked into other
  people's dependency trees, and offering Apache-2.0 lets Apache-licensed
  projects consume it under their own terms. Every crate here is
  `publish = false`, permanently and for stated reasons (`MILESTONES.md` M9).
  Nothing in this workspace will ever appear in someone else's dependency tree.
- *Apache-2.0's explicit patent grant and its contributor patent-retaliation
  clause.* These matter when contributions arrive from parties who hold patents,
  which is to say from companies. This is a solo portfolio project about
  anti-cheat engineering, there is no patentable subject matter here, and MIT's
  broad "deal in the Software without restriction" grant is not seriously argued
  to withhold patent rights for a project of this shape.

Against that, the dual license costs two license files, an SPDX expression
every reader has to parse, a per-file header convention, and a paragraph in the
README explaining a choice that changes nothing for anyone. One license is one
fewer thing to be correct about.

This is a decision that is expensive to reverse in exactly one direction:
relicensing later requires the agreement of every copyright holder, so it gets
harder with each outside contribution. Adding Apache-2.0 as an *option* is not
a relicensing and stays available — the current holder can offer additional
terms at any time. Removing MIT would not be. The asymmetry runs the safe way,
which is why the choice is recorded here rather than escalated to `RISKS.md`.

`cargo-deny`'s license allow-list at M3 governs *dependency* licenses and is a
separate question from this one; it will need to admit the ecosystem's usual
`MIT`, `Apache-2.0` and `Unicode-3.0` at minimum.

### Supply chain

`cargo-deny` covers licenses (allow-list), security advisories, duplicate
versions, and source registries. It is the only tool here — its advisories check
reads the same RustSec database as `cargo-audit`, so running both is a second
automation returning the same information.

Reproducible builds are **out of scope**. In Rust, achieving them means
controlling build paths, debug info, and embedded metadata across three
platforms; the effort is large, the failure modes are subtle, and almost nobody
verifies a reproducible build by hand. The property people actually check is
provenance, so the release publishes a signed provenance attestation via GitHub's
OIDC-backed attestation action instead — a few lines, verifiable with one
command.

## Release pipeline

**Two halves with a person between them**, and the person is the point. This
section describes what is in the repository; `MILESTONES.md` M9 records which
parts of its exit criterion are not there yet.

The first half is `release-plz`, in release-pull-request mode. On every push to
`main` it reads the conventional commits since the last `v*` tag, works out
which crates their file paths belong to, computes the next version, writes
`CHANGELOG.md`, and **opens a pull request**. It does not tag, does not publish,
and does not push to `main` — `release-plz.toml` switches each of those off
individually rather than leaving them unused, and the workflow only ever invokes
`release-pr`. That is the "pushing the release tag" row of the table below
holding: the tool proposes a version, a human merges it, and a human pushes
`git tag v0.2.0`. It is also R11 holding — the automation that rewrites your
branch while you work is the one this project deleted.

Two consequences of that shape, both of which will otherwise be discovered at an
inconvenient moment:

- Every crate is `publish = false`, so release-plz cannot ask crates.io what was
  last released and `git_only = true` points it at the git tags instead. Without
  that line it does nothing at all — no error, an empty run. And the tag pattern
  has to be `v{{ version }}`, because its default in a workspace is one tag per
  crate and this project pushes one tag per release.
- GitHub does not run workflows on a pull request opened by `GITHUB_TOKEN`, so
  the release pull request arrives with no checks on it. The alternative is a
  personal access token, which would be this repository's first secret and a
  credential a compromised action could read (`RISKS.md` R12) — in exchange for
  CI on a change to a version string and a changelog. The token stays and the
  cost is a manual step in the table below.

The second half is `release`, triggered by that tag and by nothing else. Its
first job checks that the tag and the workspace version agree — the one mistake
the manual step makes possible is tagging before merging the release pull
request — lifts this version's section out of `CHANGELOG.md`, and opens the
GitHub Release **as a draft**. Then:

1. Build the client and server for Linux x86-64, Windows x86-64, and macOS
   aarch64, natively on each, with `--locked`. Each archive holds the binary,
   `LICENSE` and `README.md`, copied by name.
2. Emit SHA-256 checksums for every artifact, into one `SHA256SUMS` computed
   from an enumerated list of the six expected archives, so that a release
   missing a platform fails rather than looking finished.
3. Build the server container: distroless base, non-root user (uid 65532, read
   back out of the built image and checked), `linux/amd64` only. Publish to
   `ghcr.io`.
4. Generate an SBOM (CycloneDX) for the workspace and attach it to the release.
   **Not delivered** — `MILESTONES.md` M9.
5. Generate a provenance attestation for the binaries and the image. **Not
   delivered**; `id-token: write` is therefore granted to no job in this
   repository, because a write permission held ahead of the thing that uses it
   is a permission nobody is watching.
6. Undraft the release, last, so that a run which dies halfway leaves an
   unpublished draft rather than a release advertising three of its six
   binaries.

**Nothing in this pipeline packages a directory.** Every file that enters an
archive is named on the line that copies it, the checksummed set is a written-out
list, and the container's build context is a directory the workflow creates and
puts exactly one file into. `RISKS.md` R3 and R4 are about a recording, a consent
record or a signing key that cannot be un-published, `ci` refuses those shapes as
*tracked* files, and this is the same rule pointed at the working tree: even a
checkout that contained one could not ship it. A `COPY . .` in a multi-stage
Dockerfile is exactly the tool that would.

The image holds a **sibling build** of the same commit and the same lockfile
rather than a byte-for-byte copy of the published Linux binary: reproducible
builds are out of scope (above), and reading an asset off a draft release needs
push access, so the alternative would be handing the registry job a write token
it otherwise has no use for. If that ever needs to be an identity, publish the
image's digest beside the checksums.

`linux/arm64` is deliberately absent: cross-building it roughly doubles release
time to serve a user base that does not exist. Add it the day someone asks, or
the day you want the server on a single-board computer. Non-root and distroless
stay, because they are nearly free and their absence would be the wrong signal
in a security project. The image runs a server that binds `127.0.0.1:0`, which
is what `server/src/main.rs` does today, so it is a way to run the authority
reproducibly rather than a way to host it, and the Dockerfile says so instead of
carrying an `EXPOSE` for a port that does not exist.

**One known limitation, upstream and open.** `release-plz` decides whether a
crate changed by running `cargo package` against the previous tag, and
`cargo package` refuses a workspace whose internal dependencies are path-only —
then, once they carry a version requirement, resolves them from crates.io, where
`sim`, `replay`, `protocol`, `client` and `server` are all names belonging to
somebody else. That is release-plz issue #2595, open since January 2026. The
consequence is bounded and worth stating plainly: the first release pull request
works, and a later one may fail with a `cargo package` error rather than
silently producing a wrong version. The fallback is the manual one — raise
`[workspace.package] version` and add the changelog section by hand in a pull
request, which is what the release workflow reads anyway.

## What stays manual, and why

| Manual step | Why it is not automated |
| --- | --- |
| Pushing the release tag | A human decides that a version exists. Auto-tagging on merge turns every merge into a release. `release-plz` therefore stops at a pull request, and `release.yml` refuses a tag whose version disagrees with the merged manifest |
| Running CI on the release pull request | GitHub does not run workflows on a pull request opened by `GITHUB_TOKEN`, so it arrives with no checks. Close and reopen it to run them, or merge it knowing CI last ran on the commits it describes. The alternative is a stored personal access token — this repository's first secret, readable by any action in the job — in exchange for checks on a version string and a changelog (`RISKS.md` R12) |
| Choosing the first version | With no previous tag, `release-plz` proposes the version the manifests already declare, and `0.0.0` is not a release. Set `[workspace.package] version` by hand once; after the first tag it computes the bumps |
| The release notes headline | The changelog is generated; the narrative of what changed is not a thing a tool knows |
| Approving production-dependency updates | The blast radius is the running server and, for `sim`, the determinism guarantee |
| Committing a proptest counter-example | The seed is printed into the run summary of the job that found it and pasted into `sim/proptest-regressions/properties.txt`. A bot pushing it would need write permissions on the branch, which is the automation `RISKS.md` R11 exists to refuse; the paste costs seconds and the case is permanent afterwards |
| Rotating the replay signing key | Rotation without publishing the retired key orphans every replay signed with it (`RISKS.md` R4). Rare, consequential, and better done deliberately |
| Admitting a participant to the human corpus | Consent is a person-to-person act, and the consent record is what makes the corpus lawfully usable (`RISKS.md` R3). `replay enrol` files the answers; it does not collect them |
| Showing a participant their own device stream before they sign | `replay disclose` renders the page, and the operator sits beside them while they read it. What it demonstrates is the one claim in `docs/CONSENT.md` a participant cannot evaluate from prose, and the crossing it reads is deliberately never stored |
| Naming somebody in a report, a talk or an acknowledgement | `Corpus::attribution` refuses to hand out a name without `named-attribution`, and that is the whole of what a program can reach. A name somebody already knows passes through no gate — `docs/CONSENT.md` states that to the participant rather than implying otherwise, and it is the one participant choice this project keeps by a promise as well as by a control |
| Acting on a detector finding | By design, permanently. Detectors emit scores and evidence; a ban is a human judgment. This is a scope decision, not a missing feature |
| Choosing a detector threshold | The threshold and its justification are the deliverable. A tuner that picks it optimizes a number nobody has to defend |

## Definition of done

A change is done when it is on `main`, CI is green, and — where the change is a
detector — the exploit it responds to exists in `cheat-client`, **and so does the
control it must stay quiet against**, and both run against it in CI. That last
clause is the project's actual quality gate; the rest is hygiene.

It said "the exploit it defeats … and fails against it" until M8, and the wording
had to change rather than the gate. Nothing *fails* against a detector: a
detector emits a score and an evidence bundle and refuses nobody, and while no
corpus has fixed a threshold it cannot even say whether a reading is worth a
look. The control is what carries the weight the word "fails" used to — a
detector that fires on an exploit without ever having been quiet proves exactly
as little as an exploit that fails against a defence without ever having worked
(`RISKS.md` R15).
