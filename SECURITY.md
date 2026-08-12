# Security policy

This repository contains both a small MOBA and the exploits against it. That is
deliberate: a defense is only considered delivered here once the matching
exploit exists in the repository and fails against it in CI. This document says
what that means for you, and what it does not.

## The cheat client targets this project only

`cheat-client/` speaks this project's own protocol and nothing else. It contains
no memory scanner, no code injector, no hooking or detouring library, no
signature scanner, and no dependency on any other game. It cannot be pointed at
another target, because there is nothing generic in it to point.

Every exploit in it is expressed as a test assertion — "the attacker learns X",
"the server accepts Y" — rather than as a usable tool.

**Contributions that target another game are refused.** So are contributions
that generalise the cheat client into reusable cheating technique: process
injection, anti-debug bypass, kernel drivers, anti-cheat evasion for third-party
products. This applies regardless of how the contribution is framed. Such pull
requests are closed without review.

## Reporting a vulnerability in the server

Report privately through GitHub's private vulnerability reporting:

**<https://github.com/Vianpyro/moba/security/advisories/new>**

Please include what you observed, the build or commit you observed it on, and
the shortest reproduction you have — a failing test in the shape of the ones in
`cheat-client/` is ideal but not required.

Expect an acknowledgement within 7 days and an assessment within 30. This is a
solo project with no on-call rotation and no bug bounty; the timeline is a
best effort and the reward is credit in the fix commit if you want it.

Please do not open a public issue for something that lets a client obtain
information or an outcome it should not have, until it is fixed.

## What counts as a vulnerability here

The project's starting axiom is that **the client is compromised and lying**
(`docs/SCOPE.md`). So the bar is set by what the server does, not by what the
client can be made to do.

In scope — please report:

- A client learning anything about the world state that its own vision does not
  cover, including through message sizes, message counts, or timing.
- A client obtaining an outcome the rules do not permit: a state transition the
  server should have rejected, an input accepted out of sequence or replayed, a
  session command that is not idempotent.
- A replay that verifies but was not produced by a genuine match, or a genuine
  replay that can be altered and still verify.
- Anything that lets one player affect another player's session, or the server
  process, beyond the rules of the game.
- Ordinary server-side software vulnerabilities: memory unsafety, resource
  exhaustion from a single unprivileged session, panics reachable from the wire.

Out of scope — these are not vulnerabilities in this project:

- Anything that requires modifying the client. That is the assumption, not the
  finding.
- Anything that requires a modified or self-hosted server, or access to the
  machine running it.
- Hardware input injection producing statistically human timing. This is the
  stated ceiling of behavioral detection (`docs/SCOPE.md`), not a defect.
- The absence of client-side anti-tamper, obfuscation, or a kernel driver. These
  are excluded on evidence, and their absence is a design decision documented in
  `docs/SCOPE.md`.
- Detection false negatives from the detectors' stated error bounds. The bounds
  are published precisely so they can be relied on rather than reported.

## Supported versions

Only `main`. There are no released versions yet, no backports, and no security
support for forks or for any deployment other than the author's.
