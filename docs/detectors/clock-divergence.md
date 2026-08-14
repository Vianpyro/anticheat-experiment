# clock-divergence

**Exploit class 4.** How fast a client's own clock ran against the server's.

`anticheat::detectors::ClockDivergence`. **Uncalibrated.**

## The null model, in one sentence

> Two clocks measuring the same seconds run at the same rate: an honest client's
> claimed timestamps differ from the server's observations by an offset and by
> the drift of a quartz crystal, which is tens to hundreds of parts per million
> and does not accumulate into a trend.

## What it reads

`replay::TimedInput`'s two timestamps, and nothing else. `docs/SCHEMA.md` §3:
`claimed_at_ms` is what the client said and is attacker-controlled by definition;
`received_at_ms` is what the server observed and is the only clock in this system
that is evidence of anything.

The statistic is a **rate**, in parts per million:

```text
    (claimed_last − claimed_first) − (received_last − received_first)
    ────────────────────────────────────────────────────────────────  × 10⁶
                 (received_last − received_first)
```

The score is its magnitude; the signed value is in the evidence, because a clock
running fast and a clock running slow are different attacks.

**It is a difference of two spans, and that is the decision in it.** Two machines
do not agree on the epoch, so every honest client's claimed timestamps are the
server's plus some arbitrary constant — a detector that read the *offset* would
flag every participant in the corpus on the first run. Differencing removes the
constant exactly. `anticheat/tests/detectors.rs` gives its control an epoch a
trillion milliseconds away from the server's and requires the score to stay under
the measurement's own resolution; a version reading the offset was written on
purpose and reports `1 786 000 026 350 ppm`.

### Its resolution, which is a function of match length

Both timestamps are whole milliseconds and there are two of them, so a span of
`S` milliseconds cannot resolve a rate error below about `2 000 000 / S` ppm.

| Match length | Resolution |
| --- | --- |
| 33 s (a 1000-tick fixture) | ≈ 60 ppm |
| 53 s (the exploit suite's 1600 ticks) | ≈ 37 ppm |
| 17 min | ≈ 2 ppm |

The evidence bundle prints it beside the score, because a score under its own
resolution is noise with a number on it.

### What it does not read

No distance and no speed, so `docs/SCHEMA.md` §4d.2 does not apply: this detector
never divides by `device_cpi` and a participant who misreported their mouse is
scored exactly as one who did not. It reads no device event, no aim and no
position.

## The exploit, and the control

| | Variant | Score |
| --- | --- | --- |
| **exploit** | `ClaimedClock::Scaled { numerator: 1, denominator: 2 }` | **500 000 ppm**, nine seats of nine |
| control | `ClaimedClock::Honest { offset_ms: 1 786 000 000 000 }` | **0 ppm**, nine seats of nine |

Measured over the 1600-tick match in `anticheat/tests/harness/played.rs`, whose
resolution is 37 ppm. Both arms are the same match with the same seed; the only
difference is what the bots claimed the time was.

`cheat-client::bot::ClaimedClock` is the exploit and `cheat-client/tests/clock.rs`
is M7's half of it: four different claimed clocks produce one identical world
digest, because **no rule reads the field**. The divergence has been recorded and
inert since M7; this is the detector over it that M7 said was M8's.

## The threshold

**There is none.** `Calibration::Uncalibrated`, blocked on:

> the spread of real clock drift across nine participants' machines over a
> recorded session. The null model bounds it at hundreds of ppm from the physics
> of a crystal; what a threshold needs is what an unsynchronised laptop with a
> sleeping scheduler actually does, and this project has never watched one.

The null model's own bound is not a threshold and must not be used as one. A
crystal is tens of ppm; an NTP-disciplined machine is better; a laptop that
suspended mid-match is worse than either by an amount nobody here has measured.
The gap between "physics says hundreds" and "these nine machines did X" is
exactly the gap a corpus closes.

## The distributions

| Corpus | Scored | Distribution |
| --- | --- | --- |
| human, `in-person` | — | **there is no corpus** |
| human, `remote` | — | there is no corpus |
| human, `unsupervised` | — | there is no corpus |
| bot, `Scaled { 1/2 }` | 9 | `[500000 × 9]` ppm |
| bot, `Honest` | 9 | `[0 × 9]` ppm |

Observed false positives: **not measurable.** Observed false negatives: **not
measurable.** Both require a corpus of people, and a control bot is a bot.

## Both bounds

| For anything driven by… | `N` | Upper bound |
| --- | --- | --- |
| a person's style | 0 people | nothing at all (no observations) |
| a match's circumstances | 0 matches | nothing at all (no observations) |

At the corpus `docs/MILESTONES.md` M6 proposes — nine people, twenty matches —
they would be `3/9 ≈ 33%` and `3/20 ≈ 15%`. No claim of the form "0% false
positives" is supportable at any corpus size this project can reach
(`docs/RISKS.md` R8).

**Which of the two applies here is a judgement, and it is the pessimistic one.**
A clock's drift is a property of a *machine*, and a participant plays every match
on the same machine — so this behaves like a person's style, `N` is the number of
people, and the bound is the 33% one. It is not a match's circumstances just
because it is measured per match.

## The supervision strata

None, because there is no corpus. When there is one, this detector's distribution
is computed per stratum-half and never pooled; `anticheat::evaluate` offers no
call that would pool them.

## What this detector does not reach, stated because a table invites more

**A clock that lies without changing its average rate.** One that jitters, or
steps back and forward by the same amount, or runs true for ninety per cent of a
match and freezes for the rest, leaves the rate error near zero. The evidence
bundle counts consecutive intentions whose claimed timestamp went *backwards*,
which catches the crudest form of it, and that count is reported and **not
scored** — a statistic nobody has a null model for is a statistic that does not
belong in a score.

**And a client that simply reports the server's own clock back.** Nothing obliges
an attacker to lie in this field at all; `docs/SCOPE.md` records that only
server-observed time is evidence, and this detector reads a lie rather than
detecting a bot. A bot with an honest clock is not touched by it, which is why
the two reaction detectors exist.
