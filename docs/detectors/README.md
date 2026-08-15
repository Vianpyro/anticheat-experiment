# DETECTORS

`docs/MILESTONES.md` M8's page per detector, and the account of the candidate
signals that have none.

**Read this first, and it is the whole of the milestone in one line: not one
threshold in this repository has been calibrated, and none can be until nine
people have played twenty matches.** Two of the five candidate signals below were
recorded here as *not buildable* and are now *buildable and not built*: the
quantity they need reaches the corpus since `docs/SCHEMA.md` §11 added the
telemetry companion. Nothing about the sentence above changes — a signal that
exists and a threshold that has been chosen are different things, and the second
still waits on nine people. The detectors exist, they respond to the
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

## And a fourth, which arrived with the lobby

**A detector that reads a distance or a speed abstains on a seat whose
calibration state is not `sufficient`.** `docs/SCHEMA.md` §4e is the schema and
`docs/RISKS.md` R17 is the reason: nine people on nine mice means every hand
appears with exactly one device, so a distance measured in raw device counts is a
distance about a person *and* their hardware. The lobby measures the conversion —
device counts per world unit, fitted against geometry the build fixes — and
`anticheat::SeatFacts::calibration` is where a detector reads whether the corpus
has enough of it.

The treatment is M8's own, one level down: `Reading::abstained` answers rather
than `Reading::score`, exactly as a detector with no calibrated threshold answers
`None` for everybody. **Nothing is refused and no match is blocked** — an
insufficiently calibrated seat is a seat no distance-shaped statistic has an
opinion about, and that is the whole of the consequence.

**Neither detector family in this crate reads it**, because both read only
*times*: a reaction is a difference of ticks and a clock divergence is a
difference of milliseconds, and neither is a distance. The rule is written here
before the detector that needs it exists, in the same register as §11f's
polling-rate rule and for the same reason — the covariate cannot be added to a
corpus after it is recorded.

And the limit, because a rule invites a reader to conclude more: the conversion
is a **scale** and not an identification. `device_cpi` is still a declaration,
the corpus still cannot say which part of a *style* is the device's, and the
`3/9 ≈ 33%` bound is unmoved by any of it.

## The five candidate signals, and the verdict on each

`docs/MILESTONES.md` M8 named five. Three are built. **Two cannot be built, and
one of those is not a question of calibration at all** — the quantity it needs is
not in the corpus, at any resolution.

| Candidate | Verdict |
| --- | --- |
| input inter-arrival distribution and quantisation | **Buildable, not built.** The quantity is in the corpus since `docs/SCHEMA.md` §11; nothing here reads it yet. See below |
| reaction latency floor | Built — [`reaction-floor`](reaction-floor.md) |
| aim-correction trajectory curvature | **Buildable, not built**, for the same reason and with one caveat of its own. See below |
| claimed-versus-observed timestamp drift | Built — [`clock-divergence`](clock-divergence.md) |
| account progression coherence across matches | **Not buildable, and no format fixes it.** Needs a corpus spanning months of the same people, which is the calendar M6 is bound by; and its null model — "a person's skill moves slowly" — cannot be stated from nine people |

And one M8 did not name, which fell out of the reaction extraction and is worth
a page of its own: [`reaction-dispersion`](reaction-dispersion.md).

**"Buildable" is a much weaker word than "built", and the two sections below are
kept rather than deleted** because what they were right about is the part worth
having. They said these two detectors were blocked by a *format* rather than by a
threshold nobody had chosen, and they were right; the format changed. What has not
changed is that no corpus has been recorded, so a detector written against this
would have a threshold chosen on nothing — which is the reason M6 gives for
building none, and it applies here exactly as it applies everywhere else on this
page.

### Why "input inter-arrival distribution" had no distribution to read, and what changed

The client records **every device event**, unconditionally, at the device's own
125 Hz to 1 kHz, with a per-event timestamp — `docs/RISKS.md` R14 is the entry
that rebuilt the capture path to make that true, and it measured the residual the
client itself adds at 16 µs of standard deviation.

**That stream did not reach the corpus, and now it does.**
`replay/src/manifest.rs` kept it out of the artefact resimulation is a function of
— `sim` consumes one intention per tick at 30 Hz, and folding a kilohertz stream
into that file would have made the resimulation a function of something no rule
reads. That argument was and is correct, and it was never an argument for keeping
the stream out of the *corpus*: `docs/SCHEMA.md` §11 puts it in a second sealed
file the replay's manifest commits to by digest, which leaves the resimulation
exactly where it was.

What is in the corpus now, per seat per match, is every device event with its own
timestamp: the distribution rather than §4b's four summary numbers. What is left
at the *intention* rate still is not the hand — a client sends exactly one
intention per tick whatever the player is doing, which
`docs/ARCHITECTURE.md`'s traffic-shape invariant makes a property of the protocol
— so a detector on this signal reads the companion and not the replay's log.

**What this does to `evdev`, and the answer is "not yet, and the device decides".**
The paragraph that used to sit here reasoned that the corpus's own timestamps are
whole milliseconds and the finest quantity any detector reads is a 33.3 ms tick,
so a 16 µs residual is three orders below anything in scope. That arithmetic was
right and it was arithmetic about a *format*: it concluded such a detector "cannot
exist, because the stream is not in the corpus", which was a statement about a
recording policy wearing the clothes of a statement about the system.
`docs/RISKS.md` R14 now says so in its own words.

The live version of the question is the polling rate. At 125 Hz the gap between
two device events is 8 ms and the client's residual is a fraction of a per cent of
it; at 1 kHz the gap is 1 ms, a worst pass of the capture loop is five reports
stamped microseconds apart, and the recorded distribution acquires a
burst-and-stall structure belonging to the client's scheduler. So: **a detector
here stratifies by declared polling rate or its page says it did not**
(`docs/SCHEMA.md` §11f), and R14 reopens on a detector that needs a 1 kHz seat's
distribution and cannot be stated over a stratum instead.

### Why an aim-curvature detector had no trajectory to read, and what changed

`docs/RISKS.md` R14 closed the *resolution* half of this: a device count is 0.05
world units where a character cell was 1.158 across and 4.111 down, so aim is no
longer quantised to a grid the renderer chose. R14 recorded that as a permission
— "a curvature detector at M8 is now a detector that may be written against this
corpus" — and it was right about what it said. It was a statement about
resolution.

**The blocker was the rate and the send policy, not the resolution.** Two facts,
neither of which is a defect:

1. The aim path lives in `client::input::InputTrace`, which is the kilohertz
   stream above.
2. The aim reaches the wire **only at the moment of a click**.
   `client::play::Play::intention` returns the *standing* order repeated, and the
   standing order changes when a control is pressed. So a replay holds the aim
   point at the instants a player committed to something — a few per second at
   most — and nothing in between.

The second is unchanged and will stay unchanged: `docs/SCHEMA.md` §11 does not
touch the send policy, because touching it would change what `sim` consumes and
therefore the digests, the format and every replay recorded under it, and it is
unnecessary since the companion carries the quantity. A curvature statistic over
the *log's* points is still the curvature of a click sequence.

The first is what changed. The companion holds the raw deltas the aim is
integrated from, in the device's own units, at the device's own rate, with
`world_units_per_count_e6` beside them — so the trajectory is reconstructible and
it is reconstructible at 0.05 world units per count rather than at a click.
`docs/SCHEMA.md` §4d.3 is right that shape statistics are the strongest position
available, being scale-invariant and therefore immune to the per-participant CPI
declaration, and there is now a shape to compute one over.

**What the companion deliberately does not give it**, since a page that only
reported the good news would be this project handling its reader: the deltas are
quantised by nothing here, but they are quantised *upstream* by whatever the
platform's unaccelerated path does — Wayland reports in 1/256 of a count, X11 in
FP16.16, Windows in whole counts for raw mouse input — and `platform` is recorded
per seat precisely because that differs. A curvature detector pooling two
platforms has a covariate in it, in the same register as the polling rate above.

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
