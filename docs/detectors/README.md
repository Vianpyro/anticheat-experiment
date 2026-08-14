# DETECTORS

`docs/MILESTONES.md` M8's page per detector, and the account of the candidate
signals that have none.

**Read this first, and it is the whole of the milestone in one line: not one
threshold in this repository has been calibrated, and none can be until nine
people have played twenty matches.** The detectors exist, they respond to the
exploits they were born with, they are quiet against the controls, and they emit
a score and an evidence bundle. What they do not emit is a decision, and
`Finding::for_review` answers `None` rather than `false` so that they cannot.

---

## What may be claimed, and it is two numbers rather than one

`docs/RISKS.md` R8's rule of three: zero false positives observed over `N`
independent trials supports an upper bound of about `3/N` at 95% confidence.
`docs/SCHEMA.md` §8 fixes what counts as `N`, and there are two answers.

| For anything driven by… | `N` is | At 9 people, 40 matches | At 9 people, 20 matches |
| --- | --- | --- | --- |
| a person's style | distinct **people** | `3/9 ≈ 33%` | `3/9 ≈ 33%` |
| a match's circumstances | **matches** | `3/40 ≈ 7.5%` | `3/20 ≈ 15%` |

**Both appear together on every page here, and in every number this project
publishes.** A reader shown one has been shown the friendlier one, and the
friendlier one is always whichever the author is quoting. The `9 × matches`
scored player-matches are not independent — nine share a match and a few dozen
share a person — and are never `N`.

The people bound does not improve with matches. Recording a hundred more matches
from the same nine people moves `3/9` by nothing at all.

**No number in this repository is written as "0% false positives", at any corpus
size this project can reach.** `replay census` prints the sentence saying so on
every run and `anticheat report` prints it again.

## Three things M6 fixed, which this milestone inherits and may not improve on

- **Detectors flag for review. Nothing here sanctions anybody automatically** —
  not a ban, not a suspension, not a queue restriction, not a silent
  match-quality adjustment. A 33% upper bound means one flagged player in three
  could be innocent and the corpus cannot rule it out. A detector ships as a
  score and an evidence bundle; a human decides. There is no verb in `anticheat`
  that does anything to anybody.
- **No claim about a player this project has never recorded.** Nine people is
  nine hands, and a null model for human behaviour is a distribution over humans;
  nine draws do not characterise one. No page here says "this detector achieves X
  on players in general", at any corpus size.
- **A distribution is built over one supervision stratum, or the page says it was
  not.** `docs/SCHEMA.md` §5a: authenticity comes from an operator having been
  present, not from anything in a file. `anticheat::evaluate` has no function
  that returns a distribution over more than one stratum, and the split between
  train and holdout is a fourth axis beside the three §5, §5a and §6 name.

## The five candidate signals, and the verdict on each

`docs/MILESTONES.md` M8 named five. Three are built. **Two cannot be built, and
one of those is not a question of calibration at all** — the quantity it needs is
not in the corpus, at any resolution.

| Candidate | Verdict |
| --- | --- |
| input inter-arrival distribution and quantisation | **Not buildable.** See below |
| reaction latency floor | Built — [`reaction-floor`](reaction-floor.md) |
| aim-correction trajectory curvature | **Not buildable.** See below |
| claimed-versus-observed timestamp drift | Built — [`clock-divergence`](clock-divergence.md) |
| account progression coherence across matches | Not built. Needs a corpus spanning months of the same people, which is the calendar M6 is bound by; and its null model — "a person's skill moves slowly" — cannot be stated from nine people |

And one M8 did not name, which fell out of the reaction extraction and is worth
a page of its own: [`reaction-dispersion`](reaction-dispersion.md).

### Why "input inter-arrival distribution and quantisation" has no distribution to read

The client records **every device event**, unconditionally, at the device's own
125 Hz to 1 kHz, with a per-event timestamp — `docs/RISKS.md` R14 is the entry
that rebuilt the capture path to make that true, and it measured the residual the
client itself adds at 16 µs of standard deviation.

**That stream does not reach the corpus.** `replay/src/manifest.rs` keeps it out
of the artefact resimulation is a function of, deliberately and permanently: `sim`
consumes one intention per tick at 30 Hz, and folding a kilohertz stream into the
file a resimulation is a function of would have made the resimulation a function
of something no rule reads. `docs/SCHEMA.md` §3 says the same thing from the
schema's side, and §4b is what does reach the corpus — four summary numbers per
seat per match: `samples`, `motions`, `coincident`, `median_gap_ns`.

So what exists is a *summary*, not a distribution. `median_gap_ns` read against
the declared `device_polling_hz` is a one-number consistency check on a
declaration, which `docs/SCHEMA.md` §4b already describes and which is not a
behavioural detector.

What is left at the *intention* rate is not the hand. A client sends exactly one
intention per tick whatever the player is doing — `docs/ARCHITECTURE.md`'s
traffic-shape invariant makes that a property of the protocol rather than of the
player — so the inter-arrival time of the inputs in a replay is the tick period
plus the network, for a bot and for a person alike.

**This does not reopen `evdev`.** R14 left one condition for reopening it: a
detector at M8 turning out to depend on a quantity at the scale of a millisecond.
Nothing here does, and the reason is worth stating precisely rather than
asserting. The corpus's own timestamps are **whole milliseconds**, and the finest
quantity any detector here reads is a tick, which is 33.3 ms. The capture path's
16 µs residual sits sixty times below the field it is written into and three
orders below the tick. **The binding resolution is the record's, not the
client's**, so a per-platform input stack would buy nothing any detector in scope
could spend.

The thing that *would* reopen it is a detector over the device stream itself —
and that detector cannot exist, because the stream is not in the corpus. Naming
it rather than assuming it absent is the obligation R14's own third clause
imposes.

### Why an aim-curvature detector has no trajectory to read

`docs/RISKS.md` R14 closed the *resolution* half of this: a device count is 0.05
world units where a character cell was 1.158 across and 4.111 down, so aim is no
longer quantised to a grid the renderer chose. R14 recorded that as a permission
— "a curvature detector at M8 is now a detector that may be written against this
corpus" — and it was right about what it said. It was a statement about
resolution.

**The blocker is the rate and the send policy, not the resolution.** Two facts,
neither of which is a defect:

1. The aim path lives in `client::input::InputTrace`, which is the kilohertz
   stream above, and it is not in the corpus.
2. The aim reaches the wire **only at the moment of a click**.
   `client::play::Play::intention` returns the *standing* order repeated, and the
   standing order changes when a control is pressed. So a replay holds the aim
   point at the instants a player committed to something — a few per second at
   most — and nothing in between.

A curvature statistic over those points is the curvature of a click sequence, not
of a hand. `docs/SCHEMA.md` §4d.3 is right that shape statistics are the
strongest position available, being scale-invariant and therefore immune to the
per-participant CPI declaration — and there is no shape in the corpus to compute
one over.

**What would change that is a second artefact beside the replay**, carrying the
device trace, which is a new collection of personal information and therefore a
new purpose, a new consent version and a new retention decision
(`docs/CONSENT.md` §2 already promises the finer stream "will be a separate thing
to be asked for separately"). That is a decision for a milestone with people in
it, not a gap this one can close.

## What the exploit suite establishes, and the half no arrangement of bots pays

`cheat-client::bot` carries five variants. Two are attacks, two are controls, and
one is the ceiling:

| Variant | reaction-floor | reaction-dispersion | clock-divergence |
| --- | --- | --- | --- |
| `Reflexes::Immediate` | **the exploit** | responds too | — |
| `Reflexes::Scripted(7)` | control | **the exploit** | — |
| `Reflexes::Jittered { 8, 2 }` | quiet | quiet | — |
| `ClaimedClock::Scaled { 1/2 }` | — | — | **the exploit** |
| `ClaimedClock::Honest` | — | — | control |

`anticheat/tests/detectors.rs` runs all of them, on both platforms, on every pull
request. Each assertion is of the form **this detector responds to this behaviour
and does not respond to its absence** — which is `docs/RISKS.md` R15 pointed at
detection, and the exact mirror of the rule M7 applied to attacks.

**It is not a false-positive measurement and no reading of it is.** A control bot
is a bot. The false-positive half is what the absent corpus owes, and there is no
arrangement of synthetic play that pays it —
`anticheat::evaluate::Evaluation::basis` refuses synthetic groups by name, and
`anticheat/tests/calibration.rs` exercises the refusal against a synthetic group
that actually scored.

### The ceiling, executed

`Reflexes::Jittered` defeats both reaction detectors: its floor is a plausible
human interval and its spread is a plausible human spread. That green is the
honest half of this milestone.

It is a **lower bound on the ceiling and is named as one.** `docs/SCOPE.md` puts
hardware input injection producing statistically human timing outside the
adversary model outright, and `docs/RISKS.md` R7 records why this project will
not build the thing that would test against it: what turns a bot into a *tool* is
a layer that synthesises device input, because such a layer drives the operating
system rather than a protocol and is game-independent by construction. So M8's
variants add noise to a **decision** and never to a device, and this project can
measure its detectors against a bot that plays through the wire and cannot
measure them against the ceiling at all.

## Running it

```console
$ anticheat report <corpus>
```

Prints, per detector: the null model, the calibration, and the score
distribution over **each** stratum-half separately, followed by the two bounds
for each and the sentence refusing a rate of zero. On an empty corpus it prints
that there is no corpus, three `UNCALIBRATED` lines, and no bound — which is a
working instrument rather than a satisfied criterion, exactly as
`replay census` is.

## The pages

- [`clock-divergence`](clock-divergence.md) — how fast a client's own clock ran
  against the server's.
- [`reaction-floor`](reaction-floor.md) — the shortest interval between an enemy
  appearing and a seat naming it.
- [`reaction-dispersion`](reaction-dispersion.md) — how much those intervals
  varied.
