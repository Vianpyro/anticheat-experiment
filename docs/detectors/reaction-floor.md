# reaction-floor

**Exploit class 3.** The shortest interval between an enemy appearing and this
seat naming it.

`anticheat::detectors::ReactionFloor`. **Uncalibrated.**

## The null model, in one sentence

> A person cannot answer something before they have seen it, and the interval
> between a stimulus reaching a screen and a hand reaching a button is bounded
> below by visual and motor latency — a property of people rather than of this
> game, and one no amount of practice takes to zero.

## What it reads

A **pair**: the tick a view first showed an enemy champion to this seat, and the
tick of the first order from this seat that *names* that champion.

Only two of the game's five actions can be an answer to somebody rather than to
somewhere. `Attack(id)` and `Targeted(id)` carry a handle; `Move` and
`Skillshot` carry a point and `Idle` carries nothing. A handle is the thing a
player could not have composed without having been shown it, which is what makes
the interval a reaction rather than a coincidence of walking.

The score is the **minimum** latency over the match, in ticks. One tick is
33.3 ms and that is the resolution.

### Where the stimulus comes from, and why it is `view_for`

A replay records no views. `docs/ARCHITECTURE.md` is explicit that a recording
carries the seed and the log and nothing else, so that there is no field for
delivery order to get into — so "when did this seat first see that enemy" is
re-derived by running the same `step` the server ran and applying
`sim::view::view_for` to each tick.

**That is deliberately not `docs/ARCHITECTURE.md` invariant 5's situation**, and
the two look identical. Invariant 5 forbids a *test* of the projection from
calling the projection's own predicate, because a projection that leaks
everything satisfies a test that agrees with it. Nothing here tests the
projection. This detector's claim is *this player could not have known before the
server told them*, and what the server told them **is** `view_for`'s output — so
re-deriving a second visibility rule would be modelling a game the players did
not play.

### The error a resimulation cannot recover, and which way it runs

State travels on QUIC datagrams (`docs/RISKS.md` R6), so **a client can miss a
tick**. The resimulation says an enemy was in the view for tick `v`; the player
may have seen it first at `v + 1` because the frame for `v` never arrived. Every
latency here is therefore an **under**estimate.

Under-estimating a reaction latency is the direction that produces a false
positive rather than a miss, and `docs/SCOPE.md` is explicit that a false
positive is the expensive one. The loss count is a client-side number that the
corpus does not carry; on loopback it is zero and `client/tests/m3_exit.rs`
prints it, over a network it is not. **This is one of the two reasons the
threshold below cannot be fixed even once a corpus exists that was recorded
without measuring loss.**

### What it does not read

No distance, no speed, no aim, no device event. `docs/SCHEMA.md` §4d.1: timing
statistics are unaffected by the per-participant scale factor, so this detector
never divides by `device_cpi` and a participant who misreported their mouse is
scored exactly as one who did not.

### When it abstains, and why abstention is an answer

Fewer than three answered appearances, and the reading carries the reason.

A "floor" over one sample is that sample. More importantly, this detector is on
the **low** tail: a seat that produced no reactions at all would score zero — the
same number a bot answering instantly produces — if the absence were scored
rather than declined. **A player who fights with skillshots names nobody and
lands here**, and that is a property of the game rather than of the player.
`anticheat/tests/calibration.rs` asserts that a match with no reactions in it
produces nine abstentions and no scores, and removing the abstention turns it
red.

The evidence bundle also counts **orders naming an enemy this seat had never been
shown**. Reported, scored by nothing: it is class 1's shape rather than class 3's
— the rules do not require an attack order's target to be visible, so it is
reachable — and a reviewer reading a floor is entitled to know the seat also
named somebody it could not see.

## The exploit, and the control

| | Variant | Floor, nine seats |
| --- | --- | --- |
| **exploit** | `Reflexes::Immediate` | **0 ticks** (0 ms) |
| control | `Reflexes::Scripted(7)` | **7 ticks** (233 ms) |
| the ceiling | `Reflexes::Jittered { centre: 8, spread: 2 }` | 6 ticks (200 ms) |

Measured over the 1600-tick match in `anticheat/tests/harness/played.rs`: 306
enemy sightings, 11 to 12 answered appearances per seat.

Zero is achievable and is not an artefact: the view carrying tick `v` was
produced by the step that consumed the inputs stamped `v − 1`, so the server's
next tick is `v`, and an intention composed from that view is stamped `v`. A
reflex bot's answer is in the same tick as the sighting.

**The gap between the exploit and the control is 7 ticks — 233 ms — and it is a
gap, not a threshold.** Note where the ceiling sits: at 6 ticks it is *below* the
control, so a threshold placed to catch the exploit has to sit under 6 to spare a
bot with plausible reflexes, and where it actually belongs is under whatever nine
people's own floors turn out to be. That is the whole of why this page carries no
number.

## The threshold

**There is none.** `Calibration::Uncalibrated`, blocked on:

> nine people's own floors, measured through this client and this transport. The
> literature's number is about a laboratory and a button; what a threshold needs
> is what these participants do through a 33 ms tick and a lossy datagram path,
> and the loss is the half a resimulation cannot recover.

A published simple-reaction-time figure is not a substitute and must not be used
as one. It is measured on a prepared subject watching one stimulus with a finger
on a key; a player is watching nine champions, deciding *whether* to answer, and
moving a mouse to a target first. Every one of those adds time, none of them is
in the literature's number, and the difference between them is what a threshold
is made of.

## The distributions

| Corpus | Scored | Distribution |
| --- | --- | --- |
| human, `in-person` | — | **there is no corpus** |
| human, `remote` | — | there is no corpus |
| human, `unsupervised` | — | there is no corpus |
| bot, `Immediate` | 9 | `[0 × 9]` ticks |
| bot, `Scripted(7)` | 9 | `[7 × 9]` ticks |
| bot, `Jittered { 8, 2 }` | 9 | `[6 × 9]` ticks |

Observed false positives: **not measurable.** Observed false negatives: **not
measurable.**

## Both bounds

| For anything driven by… | `N` | Upper bound |
| --- | --- | --- |
| a person's style | 0 people | nothing at all (no observations) |
| a match's circumstances | 0 matches | nothing at all (no observations) |

At nine people and twenty matches they would be `3/9 ≈ 33%` and `3/20 ≈ 15%`. A
reaction floor is a property of a hand, so the applicable one is the **people**
bound, `3/9 ≈ 33%`, and it does not improve with matches. No claim of the form
"0% false positives" is supportable at any corpus size this project can reach.

## The supervision strata

None, because there is no corpus. `docs/SCHEMA.md` §5a's rule applies with
particular force to this detector: what makes a match human is that somebody was
watching, and a reaction floor is exactly the statistic a mouse-moving bot in an
unsupervised session would be under-reporting.

## What this detector does not reach

**A bot that waits.** `Reflexes::Scripted(7)` is not touched by it, which is what
[`reaction-dispersion`](reaction-dispersion.md) exists for, and
`Reflexes::Jittered` is not touched by either — the ceiling, executed.

**A player with no reactions in the match.** It abstains rather than scoring
them, so a whole style of play is outside it. That is a false-negative property
of the game, and the honest form of it is that this detector reads right-clicks.
