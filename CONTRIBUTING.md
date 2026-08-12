# Contributing

This is a solo portfolio project with a narrow, documented subject. Issues and
pull requests are welcome, but the scope is deliberately closed: read
`docs/SCOPE.md` before proposing anything, and `SECURITY.md` before touching
`cheat-client/`.

## What is likely to be accepted

- Bug fixes, with a test that fails without the fix.
- A demonstration that a defense does not hold — ideally as a failing assertion
  in `cheat-client/`. This is the most valuable contribution the project can
  receive.
- Corrections to the documents in `docs/`, especially where a claim is stronger
  than the evidence behind it.

## What will be refused

- Game features. The MVP is frozen; the game is a test fixture, not the subject.
  An addition is in scope only when a specific anti-cheat experiment requires
  it, and the milestone that requires it says so.
- Anything from the exclusion list in `docs/SCOPE.md`: client-side anti-tamper,
  automatic bans, machine-learned classifiers, rollback netcode, an ECS inside
  `sim`, plugin systems.
- Anything targeting another game, or generalising the cheat client into
  reusable cheating technique. See `SECURITY.md`.
- New dependencies without a reason that a few lines of code would not satisfy.
  In `sim`, the bar is higher still: a dependency there can smuggle in
  nondeterminism (`docs/RISKS.md` R9).

## Before you push

The three commands CI runs, in the order it runs them:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

The toolchain comes from `rust-toolchain.toml`; `rustup` installs it for you on
the first `cargo` invocation. Do not install a different toolchain and do not
change the pin as part of an unrelated change — a compiler upgrade is a
reviewable commit of its own (`docs/RISKS.md` R1).

`Cargo.lock` is committed. If your change alters it, say why in the pull
request.

## Branches, commits, merges

Branches are named `category/slug`, lowercase, e.g. `security/pad-view-messages`
or `docs/law-25-retention`. Allowed categories are listed by the failing check.

Pull requests are **squash merged**, so the pull request title becomes the
single commit on `main` and is what the generated changelog consumes. It must
follow Conventional Commits, per `.copilot/commit-message-instructions.md`:

```
<type>(<optional scope>): <imperative, lowercase, no trailing period>
```

Individual commits inside a branch are not checked — they are squashed away.
Write them for the reviewer, not for the linter.

There is no required-review rule. A solo developer cannot approve their own pull
request, and a rule you bypass every week is a rule that trains you to ignore
rules. The pull request exists to run CI and to give the change a description.

## Definition of done

A change is done when it is on `main` and CI is green. Where the change is a
detector, it is done when the exploit it defeats exists in `cheat-client/` and
fails against it in CI — a detector without that exploit is not a delivered
detector.
