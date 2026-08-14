# reaction-dispersion

**Exploit class 3.** How much this seat's reaction latencies varied.

`anticheat::detectors::ReactionDispersion`. **Uncalibrated, and the one detector
here that may be withdrawn rather than thresholded.**

## The null model, in one sentence

> A human reaction time is a random variable, not a constant: the same person
> answering the same stimulus twice takes two different amounts of time, and the
> trial-to-trial variability is irreducible. A scripted delay has none.

## Why it exists beside the floor

[`reaction-floor`](reaction-floor.md) catches a bot that answers faster than a
nerve and is blind to one that waits. A bot that waits a plausible constant — 233
milliseconds, every single time — passes the floor and is caught here. The two
read the same extracted pairs and are two detectors rather than one because they
fail differently, catch different variants, and would need different thresholds.

## What it reads

The same pairs [`reaction-floor`](reaction-floor.md) describes: the tick a view
first showed an enemy champion to this seat, and the tick of the first order
naming it. The score is the **mean absolute deviation from the median**, in
hundredths of a tick.

### Why not the standard deviation, and why not the MAD

A standard deviation needs a square root and this crate has no floats — a
detector's score is published, and a float is a number that can come out
differently on two of this project's platforms (`anticheat/clippy.toml`).

The **median** absolute deviation is the robust choice everywhere else and is the
wrong one here, for a reason specific to the quantisation. Latencies are whole
ticks and a plausible human range spans three or four of them, so more than half
the values often equal the median exactly — and the median of the deviations is
then **zero for a player with real spread**. A statistic reporting "no variation
at all" about somebody who varied is a false-positive generator, and false
positives are this detector's entire cost. `anticheat/src/features.rs` carries a
unit test on exactly that case: latencies `[6, 6, 6, 5, 9]` give a MAD of 0 and a
mean absolute deviation of 80 hundredths.

### When it abstains

Fewer than five answered appearances. Higher than the floor's three, because a
spread is a statement about a distribution and three points do not describe one.

The abstention matters more here than anywhere else in this crate. This detector
is on the **low** tail, so a seat with no reactions would score zero — the same
number a perfectly scripted bot produces — if an absence were scored rather than
declined.

### What it does not read

No distance, no speed, no aim, no device event.

## The resolution problem, stated rather than worked around

**A tick is 33.3 ms and a human reaction spread is of the same order.** Published
trial-to-trial standard deviations for simple reaction time are in the tens of
milliseconds; call it 40 ms, which is 1.2 ticks. This record quantises latencies
to whole ticks.

So the separation this detector is asked to make — between a person whose spread
is about one tick and a script whose spread is exactly zero — is barely more than
one unit wide, and every source of noise in the pipeline is measured in the same
units:

| Source | Size |
| --- | --- |
| the quantisation of a latency | 1 tick, 33.3 ms |
| a human trial-to-trial spread | ≈ 1.2 ticks |
| a lost datagram shifting a stimulus | 1 tick per loss |
| the client's own capture residual (`docs/RISKS.md` R14) | 16 µs — 0.0005 ticks |

The last row is the reassuring one and it is also the point: **the limit is the
record's resolution, not the client's.** A per-platform input stack would improve
the fourth row by nothing anybody could spend, which is why `evdev` stays refused
(`docs/RISKS.md` R14's third clause, and `docs/detectors/README.md` carries the
full argument).

**What would improve it is a millisecond-resolution stimulus time, and the corpus
does not carry one.** The stimulus is a tick number because that is what the log
holds; the server's tick *times* are recorded nowhere. Recovering them
approximately from the `received_at_ms` of the inputs bucketed into a tick is
possible and would fold the network round trip into the answer, which is a
different measurement rather than a better one.

## The exploit, and the control

| | Variant | Spread, nine seats |
| --- | --- | --- |
| **exploit** | `Reflexes::Scripted(7)` | **0**, every seat |
| control | `Reflexes::Jittered { centre: 8, spread: 2 }` | **116–118** hundredths of a tick |
| also caught | `Reflexes::Immediate` | 0 (a bot with no delay has no spread either) |

Measured over the 1600-tick match in `anticheat/tests/harness/played.rs`, 11 to
12 answered appearances per seat.

A zero is an unambiguous sentence — *every answer took exactly the same number of
ticks* — and it is the strongest thing this detector says. Everything above zero
is where the resolution problem lives.

## The threshold

**There is none**, and this is the one detector whose blocked-on clause allows
for it never getting one. `Calibration::Uncalibrated`, blocked on:

> a corpus, and a decision this project cannot take without one: a human spread
> of about 40 ms is 1.2 ticks, and this record quantises to whole ticks, so the
> honest separation between a person and a constant delay is barely more than one
> unit wide. What would settle it is nine people's measured spreads — and if they
> come out under a tick, this detector is withdrawn rather than thresholded.

**Withdrawn rather than thresholded** is a commitment made here, before the data,
for the reason `docs/RISKS.md` R8 gives about detectors in general: a threshold
chosen because a detector exists is a threshold nobody has to defend. If nine
people's spreads land at or below one tick, there is no value that separates them
from zero, and the answer is to delete the detector rather than to pick the
number that flags the fewest of them.

## The distributions

| Corpus | Scored | Distribution |
| --- | --- | --- |
| human, `in-person` | — | **there is no corpus** |
| human, `remote` | — | there is no corpus |
| human, `unsupervised` | — | there is no corpus |
| bot, `Scripted(7)` | 9 | `[0 × 9]` hundredths of a tick |
| bot, `Jittered { 8, 2 }` | 9 | `[116, 116, 116, 118 × 6]` |
| bot, `Immediate` | 9 | `[0 × 9]` |

Observed false positives: **not measurable.** Observed false negatives: **not
measurable.**

## Both bounds

| For anything driven by… | `N` | Upper bound |
| --- | --- | --- |
| a person's style | 0 people | nothing at all (no observations) |
| a match's circumstances | 0 matches | nothing at all (no observations) |

At nine people and twenty matches they would be `3/9 ≈ 33%` and `3/20 ≈ 15%`.
Trial-to-trial variability is a property of a hand, so the applicable one is the
**people** bound, `3/9 ≈ 33%`, and no number of matches improves it. No claim of
the form "0% false positives" is supportable at any corpus size this project can
reach.

## The supervision strata

None, because there is no corpus.

## What this detector does not reach

**The ceiling.** `Reflexes::Jittered` varies, so this is quiet against it, and
`anticheat/tests/detectors.rs` asserts that green. Adding plausible variability
to a scripted delay is a few lines in any bot, which is precisely why
`docs/SCOPE.md` puts statistically-human synthetic timing outside the adversary
model rather than promising to defend it.

**A player with fewer than five reactions in a match.** It abstains.
