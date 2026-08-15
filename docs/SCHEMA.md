# SCHEMA

What the human corpus holds, field by field; what it excludes and how; what a
claim made on it may say; and the procedure that destroys a participant's part of
it.

`docs/MILESTONES.md` M6 asks for "a documented telemetry schema" and "a
pseudonymous player identity scheme". This is both, plus the two things that turn
out to be inseparable from them: the covariates without which a difference of
hardware reads as a difference of style, and the arithmetic that bounds what any
of it can support.

`docs/CONSENT.md` is what a participant reads. This document is what an operator
and a detector author read, and nothing may be in one and not the other: **every
field named here is personal information, `docs/CONSENT.md` names it, and
withdrawal destroys it.**

---

## 1. Where the corpus lives

```text
<corpus>/
  participants/<pseudonym>.consent      one consent record per participant
  identities/<pseudonym>.identity       the mapping to a person. The sensitive file
  matches/<match_id>/match.replay       the sealed replay (docs/ARCHITECTURE.md)
  matches/<match_id>/match.session      the session record (§4)
  matches/<match_id>/match.telemetry    the device-event stream, sealed (§11)
  withdrawals/<pseudonym>.withdrawn     a tombstone, and nothing else
```

Outside the repository, always. `.gitignore` refuses every one of these shapes
and `ci` fails a pull request that tracks one, because `docs/RISKS.md` R3 is about
an irreversibility git makes literal.

`match.telemetry` is present exactly when the replay beside it commits to one,
and absent exactly when it commits to `Commitment::Absent`. Both are legitimate
states and neither is a default: §11 has the account.

**There is no other file.** No index, no cache, no summary, no participant list,
no split file, and no client's part left over from a collection. `docs/CONSENT.md`
records why — a derived artefact is what outlives the thing it was derived from —
and `Corpus::audit` is the guard, in two ways rather than one. It reads every
byte of every file under the root, so an artefact carrying a pseudonym is
reported the first time somebody withdraws; and since M8 it also reports a match
directory holding **any file this list does not name**, which is the only check
that reaches an artefact naming nobody at all. A `seat-3.telemetry-part` left
behind by an interrupted collection is one seat's hand movements with nothing to
say whose, and no search for a name in any corpus would ever find it.
`replay/tests/withdrawal.rs` plants both to prove it.

## 2. The pseudonymous identity scheme

A participant is one **pseudonym**, stable across every match they play in and
for the life of the corpus.

| Property | Value |
| --- | --- |
| Character set | `A–Z a–z 0–9 _ -`, at most 32 bytes (`replay::Pseudonym`) |
| Chosen by | the operator, from a list of colour names, at enrolment |
| Stable across matches | **yes**, and that is the point: the corpus's `N` for anything a person's style drives is the number of distinct pseudonyms |
| Mapping to a person | one line in `identities/<pseudonym>.identity`, destroyed by withdrawal |
| Written where | inside the replay's signed manifest, one slot per seat, and **nowhere else** |

Two constraints are load bearing rather than tidy. The character set is what makes
`Corpus::audit`'s byte search sound — a pseudonym containing a space, a newline or
a path separator could be split across a line or collide with unrelated text — and
a free-form field is where a real name ends up by accident. And "nowhere else" is
what keeps a withdrawal to one thing to delete: the manifest is inside the
signature, so it cannot drift, and the session record beside it is indexed by
**seat**, never by pseudonym.

**Pseudonymisation is a security measure and not a change of category.**
`docs/RISKS.md` R3 is explicit: input timing is distinctive, "the mapping is in
another file" is a thin claim, and this corpus is treated as personal information
throughout.

## 3. Per input — what a replay already holds

Frozen at M5 and unchanged here. `replay::TimedInput`, one entry per accepted
intention, roughly one per thirtieth of a second per player:

| Field | What it is | Trusted? |
| --- | --- | --- |
| `tick` | the tick the server applied it to | the server's |
| `seq` | per-player, strictly increasing | the client's, validated |
| `player` | the seat, written by the server from the session | the server's |
| `action` | one of the five, with its coordinate or target | the client's claim |
| `claimed_at_ms` | when the client says it produced the input | **never** |
| `received_at_ms` | when the server observed it arrive | the only real clock |

M6 asked for the two timestamps and they were already there. What is **not** here
and will not be: anything at a higher rate. `sim` consumes one intention per tick
at 30 Hz, and folding a kilohertz stream into the artefact a resimulation is a
function of would make that resimulation a function of something no rule reads
(`replay/src/manifest.rs`).

**That is a statement about the replay and no longer a statement about the
corpus.** Since M8 the device stream is kept — in a separate sealed file the
replay's manifest commits to by digest, described in §11 — and what reaches the
corpus from `client::input::InputTrace` is both the summary in §4b *and* the
stream itself. The two hold the same numbers about each seat and neither is
derived from the other, which is why `Corpus::store` refuses them disagreeing.

## 4. Per seat, per match — the session record

`matches/<id>/match.session`, one file per match, one entry per seat, written by
`replay store` from the parts the clients wrote. It answers the question a replay
structurally cannot: **what was this recorded on?**

The reason it exists is one sentence. A mouse at 400 counts per inch and one at
1600 describe the same hand differently; without the number, a difference of
equipment is read as a difference of style, and every behavioural statistic at M8
inherits the confusion.

### 4a. Asked of the participant — not measurable, and stated as such

| Field | What it is | Why it cannot be measured |
| --- | --- | --- |
| `device_profile_id` | **which device this participant is playing on**, as an opaque label the operator keeps stable while the hardware does not change | nothing can tell one mouse from another, and `docs/CONSENT.md` promises no model, serial or manufacturer is collected. What it buys is the one thing a per-match record cannot otherwise express — that two sessions were played on the *same* device — and §4e is what needs it |
| `device_cpi` | counts per inch the mouse is configured at | a mouse reports counts. Nothing in the stream says what physical distance produced them |
| `device_polling_hz` | the device's report rate | the client observes an *arrival* rate, which is the report rate plus the platform. §4b records the observation beside the declaration |
| `pointer_acceleration` | whether the OS's acceleration was left on | the client sees deltas after the platform has applied it, and cannot invert a curve it does not know |

All four are **declarations**. They are as good as the participant's answer and
no better, and no analysis may treat them as measurements.

**The label is a linkage key and is treated as one.** It is stable across a
participant's sessions, which is exactly what makes it able to tie two matches to
one person — and it names nobody, so `Corpus::audit`'s byte search for a
pseudonym structurally cannot find one left behind. What destroys it is that it
lives **inside the match directory** a withdrawal removes whole, and
`replay/tests/withdrawal.rs` asserts both halves: the label is in the corpus
before, and it is in no byte of it after. It is constrained to `Pseudonym`'s
character set for `audit`'s sake, and it is chosen by the operator from a list
for the same reason a pseudonym is — a free-form field is where a real name ends
up. They are collected
together or not at all: a corpus holding hardware for some sessions and not others
has a covariate present on a subset chosen by whoever remembered a flag, which is
worse than not having it.

### 4b. Measured by the client

| Field | What it is |
| --- | --- |
| `platform` | `linux`, `windows`, `macos` or `other` |
| `clock` | `dequeue` or `device` — which clock the sample timestamps came from (`client::input::CLOCK`) |
| `world_units_per_count_e6` | the build's sensitivity, in millionths of a world unit per device count |
| `samples`, `motions` | device events recorded, and the motions among them |
| `coincident` | consecutive identical motions closer together than a device produces — a platform delivering one event twice (`docs/RISKS.md` R14) |
| `median_gap_ns` | the median inter-arrival time. **Read against `device_polling_hz`**: it is the only check available on a declaration |
| `budget_ns`, `passes`, `passes_over_budget`, `worst_overrun_ns`, `worst_pass_ns` | what the capture loop cost (§5) |

`platform` and `clock` are here because `docs/ARCHITECTURE.md`'s device-timestamp
table is per platform: what a timestamp *is* differs between them, and a corpus
that pooled two platforms without recording which is which would have a covariate
nobody can remove afterwards. `clock` has a `Device` value nothing produces today,
so a corpus spanning a build that gains one can be split rather than pooled.

### 4c. Unknown, and staying unknown

Naming these is part of the schema, because a reader who finds the hole before the
document does is entitled to conclude the rest was written the same way.

- **The true CPI.** `device_cpi` is a declaration. If it is wrong, every distance
  in that participant's record is wrong by a constant factor.
- **The OS acceleration curve.** Acceleration is *refused*, not measured — see
  §4d for what that buys and what it does not.
- **A device timestamp.** No platform in `docs/ENGINEERING.md`'s matrix hands this
  client one through `winit`; the stamp is the dequeue time and `clock` says so.
  `docs/RISKS.md` R14 carries the measured residual: 16 µs of standard deviation
  in `release`, against signals whose human spreads are tens of milliseconds.
- **Everything below the client.** The kernel input stack and the compositor are
  not in any loop this project measures, so every latency figure here is a lower
  bound on the real one.
- **The hand.** Grip, posture, mousepad, desk height, whether somebody was
  standing up. None of it is collected and none of it is knowable.

### 4d. What refusing pointer acceleration means for comparability

`pointer_acceleration` must be `off`. A session that declares otherwise is refused
by `Corpus::store` and by the client before it connects — refused rather than
flagged, and it is the only declaration treated that way.

**Why refused.** Acceleration makes the map from device counts to world units a
function of the pointer's *speed*. A trajectory recorded through it is the
operating system's curve as much as the hand's, and unlike a sensitivity — which
is one number and can be divided out — a curve cannot be recovered from the record
by any covariate this schema holds. It is R14's failure in a new place: a
transformation applied before the sample exists, which no precision downstream
undoes.

**What that leaves, stated plainly.** With acceleration off and CPI declared, two
participants' records are comparable **up to a per-participant scale factor that
the corpus knows only as a declaration**. Three consequences, and they are not the
same size:

1. **Timing-shaped statistics are unaffected.** Inter-arrival distributions,
   reaction latency floors, claimed-versus-observed drift: none of them reads a
   distance. This is most of `docs/MILESTONES.md` M8's list.
2. **Distance- and speed-shaped statistics are comparable only after dividing by
   a scale**, and §4e is what changed about which scale. It used to be
   `device_cpi`, a declaration nobody checked, with the consequence that a
   participant who misreported it by a factor of two is a participant a
   speed-thresholding detector scores as an outlier for a reason that has nothing
   to do with how they played. There is a **measured** conversion now — device
   counts per world unit, fitted against geometry the build fixes — and the rule
   that goes with it is that a detector reading a distance or a speed uses it and
   abstains on a seat that has none. What is still a declaration, and still
   unchecked, is `device_cpi` itself: the measured scale converts counts to *world
   units*, never to inches, and §4c keeps the inch in the unknown column.
3. **Shape-shaped statistics — curvature, smoothness, the ratio of one distance to
   another — are scale-invariant and therefore comparable without the
   declaration.** This is the strongest position available, and it is the one a
   curvature detector at M8 should be built from.

The residual nobody can close: acceleration being *off* is itself a declaration.
The corpus cannot tell an accelerated session from an unaccelerated one, because
the only difference is in a transformation applied upstream of the first byte this
project sees.

### 4e. Calibration — what the lobby measured, and how well the device is known

`docs/RISKS.md` R17 is the risk and `client::lobby` is the instrument.

**The confound.** Nine participants and nine devices: every hand appears with
exactly one mouse, so no analysis in this corpus can separate a person's style
from their hardware's response. That is not variance more matches absorb, it is a
variable the design does not identify. The parade is not to standardise the
hardware — a production anti-cheat does not choose its players' mice — but to
**measure its contribution**, so that a statistic reading a distance or a speed
reads normalised units rather than raw device counts.

**Where the measurement comes from.** The lobby, and there is no calibration
screen. Every element in it stands at a position the build fixes, `Ready` is
inert until the pseudonym, the consent version and champion select have each been
visited, and a training dummy moves through a fixed table of stations while the
last player connects. So a click is a movement whose **endpoints are known
exactly** and whose **cost in device counts is measured**, and the traversal that
produces them is the traversal that starts the match.

**What the record holds, per seat.** Sufficient statistics, not an estimate:

| Field | What it is |
| --- | --- |
| `calibration.reaches` | clicks on an element of known position with a measured crossing behind them |
| `calibration.octants` | a bit per compass octant covered, of eight |
| `calibration.clamped` | legs discarded because the cursor reached the map clamp during them |
| `calibration.min_distance_e3`, `…max_distance_e3` | the shortest and longest reach, in thousandths of a world unit |
| `calibration.sum_distance_e3`, `…sum_counts_e3`, `…sum_distance_sq_e3`, `…sum_distance_counts_e3`, `…sum_counts_sq_e3` | the five sums a least-squares fit of counts against distance needs |
| `calibration.fast_reaches`, `…fast_motions`, `…fast_ns` | the reaches crossed fast enough for a report rate to be readable, and what they cost |
| `calibration.quantum_e6` | the finest non-zero delta component observed, in millionths of a count |

**Sums rather than a fit, and that is the whole of what makes estimation
accumulate.** They add across sessions by `+`, so a participant's device profile
is the sum of their sessions on one device and nothing has to be stored to make
it so. `Corpus::profile_of` computes it from the matches on disk when somebody
asks, in exactly the register `replay::split::split_of` is a function rather than
a file (§7) and `census` prints rather than writes: a stored profile would be a
derived artefact able to disagree with the corpus and able to outlive a
withdrawal.

**What is estimated from them, and what is not.** `replay::calibration::Estimate`
fits `n = a·d + b`. The slope `a` is **device counts per world unit** — the
conversion a distance-shaped statistic needs in order to stop being a count,
measured against geometry the build fixes rather than taken from a number the
client wrote about itself. The intercept `b` is the fixed cost of arriving at a
target: the landing slop and the overshoot correction, which are **style**, and
which are in the model in order to be kept *out* of the slope. A ratio taken from
one movement cannot separate the two, which is the argument for a regression over
a spread of distances rather than a direct measurement.

**`device_cpi` is not recovered and does not become measurable.** A mouse reports
counts; nothing in any stream this project records says what physical distance
produced them, and no menu geometry changes that. §4c keeps the true CPI in the
unknown column exactly where it was.

**Sufficiency**, pooled across a participant's sessions on one device, and every
clause is the antecedent of something the estimate claims:

| Clause | Value | Why |
| --- | --- | --- |
| reaches | **16** | the fit has two parameters and its residual is comparable with a button's radius |
| octants covered | **6** of 8 | a measurement aligned on one axis has hidden an anisotropy in this project once already (`docs/RISKS.md` R14) |
| longest reach ÷ shortest | **4** | below that the slope and the intercept are not separately identified and the slope absorbs the arrival cost |
| fast reaches | **4** | the report rate is the one quantity a slow session cannot produce: a creeping hand reports at the same rate and spends most of every interval stationary |

**The state, per seat, frozen at filing time**, which is the second field this
milestone adds to the record:

| Value | What it asserts |
| --- | --- |
| `sufficient` | every clause above is met, counting this session and the participant's earlier ones on the same device |
| `partial` | something was measured and it is not enough yet. **The ordinary state of a first session**, and named rather than treated as a failure: a corpus's first evening is a calibration evening |
| `absent` | nothing was measured. A client that never crossed a lobby, a session somebody joined late |
| `mismatched` | something was measured and it does not match the profile that device is on record as. **Not an accusation** — a mouse replaced between two evenings produces exactly this — it is the corpus declining to pool two devices under one profile |

It is written by whoever *files* the match and not by the client: rating a seat
needs the participant's earlier sessions, which a client has never seen and which
`docs/SCOPE.md` assumes it would lie about. A client's part therefore carries the
observations and never a state, and `client/tests/session_part.rs` asserts that a
part rates itself no higher than `partial`.

**Frozen rather than recomputed**, and that is the point of it being a field. §8
requires a distribution to say which stratum it was computed over, and a stratum
re-derived from the whole corpus on every read is a stratum that quietly changes
under a published number. The observations stay beside it so the decision can be
audited; what is fixed is the decision.

**Estimating and verifying are different operations and cost different amounts.**
Estimating needs many movements in many directions over a spread of distances, and
no single evening owes anybody that; verifying that a device has not changed needs
a handful. So the last person to join does not have to be calibrated that night —
they already are, by their earlier sessions; a participant who spends the wait
doing something else loses nothing and merely defers the refinement of their own
profile; and the first session of a participant is explicitly a calibration
session.

**Nothing here blocks anything, and that is a decision rather than an
implementation detail.** An insufficiently calibrated seat never stops a match
starting and never stops a match being stored. It is *marked*, and the rule for
whoever reads the corpus is:

> A detector that depends on the scale returns `None` for a seat whose state is
> not `sufficient`, through `anticheat::Reading::abstained` rather than by
> scoring it anyway.

That is the treatment M8 already gives an uncalibrated *threshold*, one level
down, and the reason is `docs/SCOPE.md`'s standing one: blocking a player for a
calibration reason is the shortest path to an anti-cheat that degrades the
experience of honest players. `replay census` prints the four counts, and
`anticheat::SeatFacts::calibration` is where a detector reads the state.

**Neither of M8's two detector families reads it**, because both read only
*times*. The rule above is stated for a detector that does not exist yet, in
exactly the register §11f's polling-rate rule is: written before the first
recording, because the covariate it is about cannot be added to a corpus
afterwards.

## 5. The tick budget, and sessions that fell behind

`docs/RISKS.md` R16. Every recording session reports **how many passes of the
client's capture loop exceeded one tick, and the worst overrun**, and a session
with a non-zero count is **degraded**.

A degraded session is not refused and is not deleted. It is *marked*, and the rule
is:

> A degraded session is never pooled into a distribution with sessions that are
> not. It is counted separately, wherever it is counted at all.

The reason is the same one R14 spent a milestone on. A client that falls behind
does not lose data; it writes a **delay** into the record, and an intention decided
one pass late looks exactly like a hand that hesitated. A detector calibrated on a
corpus with those in it has been calibrated on somebody's scheduler.

One seat over budget makes the whole match degraded, and that is deliberate: a
match is one interleaved log, the nine seats in it are not independent
observations, and "seat 4 only" is an invitation to the partial pooling this rule
exists to refuse.

`replay census` prints the count and the worst overrun across the corpus.

## 5a. Supervision, and why it is in the schema rather than in a habit

**What makes a match in this corpus a human match is not a property of any file.
It is a fact about a person: somebody was watching.** §6 below states the one
mechanical thing a file *can* say — a seat that recorded zero device events is
refused, which catches a scripted or headless client — and states its narrowness
in the same breath: a bot that moves a real mouse records exactly as many samples
as a person and is not reachable from anything in this directory. That is
`docs/SCOPE.md`'s ceiling of behavioural detection, and M7's `cheat-client`
executes both halves of it (`cheat-client/tests/botting.rs`): the bot plays a
whole match, the server accepts every frame, the replay verifies, and the only
thing that catches the crude version is the sample count.

So the guarantee is the operator, and a guarantee that lives in somebody's memory
of an evening six months ago is not one. Every session record therefore carries
**one** of:

| Value | What it asserts |
| --- | --- |
| `in-person` | The operator was physically present for the whole session |
| `remote` | The operator was on a live call with the participants throughout, but not in the room |
| `unsupervised` | Nobody was watching; participants recorded on their own |

Four rules go with it.

**It is the operator's observation, not a measurement and not a declaration.** No
client can measure whether somebody was in the room, and a participant's own
self-report would mean nothing if they were the one cheating. So it is not in a
session *part* — which is what a client writes — it sits beside `recorded_on`,
which is the other thing an operator fills in, and `replay store` takes it as an
argument.

**A mixed session takes the weakest of the three**, for the reason §5 gives about
degradation: a match is one interleaved log, its seats are not independent
observations, and "seven of the nine were in the room" invites exactly the partial
pooling this schema refuses.

**Absence does not decode.** A session record with no supervision line is not read
as supervised, or as anything else — it is refused, which is the same equivalence
`docs/RISKS.md` R3 draws between an absent consent version and a stale one. A
corpus assembled before this field existed must not be readmitted by the silence
of its own files.

**And a distribution over more than one stratum says so.** `replay census` prints
the three counts on every run with that sentence beside them. What M8 does with
them is M8's decision — calibrate on the best-attested stratum and test on the
rest, or exclude the weakest and report the smaller `N` — but it is a decision
made in the open, and the two confidence bounds in §8 are computed over whatever
stratum a claim is actually made on.

## 6. What does not enter the corpus

| Excluded | How it is enforced |
| --- | --- |
| A match played by a bot, a script, or a headless client | `Corpus::store` refuses any seat that recorded **zero device events**, and the schema has no `provenance` value but `human` and `empty` — a part claiming otherwise does not parse. §11's view anchors are counted apart from the device events for exactly this reason: a headless client receives thirty frames a second, and counting those among the samples would hand this refusal to the attacker it exists to catch |
| One person filling several seats | `Corpus::store` refuses a manifest naming one pseudonym twice |
| A match nobody consented to | refused since M5; now also refused when the consent record is from another version of `docs/CONSENT.md`, or has no version at all |
| A session recorded through OS pointer acceleration | §4d |
| Any of it, in git | `.gitignore` plus a `ci` check on *tracked* files |

**How narrow the synthetic-play defence is, stated because a table invites a
reader to conclude more.** A scripted or headless client touches no device and
records no sample, so it is caught. **A bot that moves a real mouse records exactly
as many samples as a person and is not reachable from any file in this corpus** —
which is `docs/SCOPE.md`'s stated ceiling for behavioural detection arriving early
rather than a hole this schema could close. What keeps it closed is that the
operator is in the room while the match is played, and that is a fact about a
person rather than a property of a format — which is why §5a makes it a field
rather than a habit, and why a session that had no operator in the room says so.

Both halves of that narrowness are executed rather than asserted, in
`cheat-client/tests/botting.rs`: a bot plays a whole match, the server accepts
every frame it sends, its replay verifies, and the silent-seat check catches the
headless version and is blind to the mouse-moving one.

### Partially filled seats, and short matches

A match with fewer than nine humans is a legitimate `State` — the rules handle it,
M4's own criterion is three humans and six empty seats — and it stays in the
corpus. What it is not is the same *kind* of data:

- Its **per-input telemetry is as good as any**. An inter-arrival time does not
  know how many seats were occupied.
- Its **situation is different**. A match with three absent champions has
  different fights in it, so anything reading the situation a player was in — map
  position, engagement rate, target selection — must not pool it with a full
  match.

So: kept, counted separately by `replay census`, and never mixed into a
distribution a detector thresholds on without the document saying which. The same
goes for short matches: a five-minute match is five minutes of inputs, and a
detector reading per-match aggregates weights it as such rather than as a match.

## 7. The train/holdout split

Frozen, in `replay::split`, in a commit that lands before the first detector:

- **A pure function of the match identifier**, not a file. `split_of(match_id)`
  hashes a frozen salt with the identifier and holds out one match in four.
- **Nothing is stored.** A list of held-out matches would be the derived index M5
  removed, and a withdrawal that destroyed a match and left it named in a split
  file would leave behind a line about somebody's participation after they asked
  for it to be destroyed.
- **Stable under withdrawal.** A rule like "the first four fifths by date"
  reassigns every match the moment one is destroyed, so a participant exercising
  their right would silently move matches out of a holdout a threshold had already
  been chosen against. A hash does not move, and
  `a_withdrawal_cannot_move_a_match_from_one_half_to_the_other` is the assertion.

Changing `SALT` or `HOLDOUT_IN` reshuffles the corpus, which is the same act as
choosing a split after looking at the data. They may only change alongside a
decision to **discard every result computed under the old ones**.

**It is not stratified, and the limitation is real.** Holding out by *person*
would be the split a "this detector generalises to a new player" claim needs, and
nine people cannot afford it: a fifth of nine is two, and two people are not a
population. This holds out by *match*, so the claim it supports is about new
matches from known players and not about new players. Every detector document has
to say that.

## 8. What the corpus can support

`docs/RISKS.md` R8 is the rule and `docs/MILESTONES.md` M6 the arithmetic. Zero
false positives observed over `N` independent trials supports an upper bound of
about `3/N` at 95% confidence.

**What counts as `N` is the part people get wrong, and there are two answers.** A
detector scoring a *player-match* has `9 × matches` scored units and they are not
independent: nine of them share a match, and a few dozen share a person. So:

| For anything driven by… | `N` is | At 9 people and 40 matches | At 9 people and 20 matches |
| --- | --- | --- | --- |
| a person's style | the number of **distinct people** | `3/9 ≈ 33%` | `3/9 ≈ 33%` |
| a match's circumstances | the number of **matches** | `3/40 ≈ 7.5%` | `3/20 ≈ 15%` |

**And a claim that reads a distance or a speed carries a third stratum.** §4e:
the seats it was computed over are the ones whose calibration state is
`sufficient`, and a page that pooled the rest has pooled device counts that mean
different things. `replay census` prints the four counts beside the two bounds.

**Both bounds appear together, everywhere a claim is made.** In every detector
document at M8, in every published statistic, in `replay census`'s own output. A
reader shown only the friendlier one has been handled, and the friendlier one is
whichever the author is quoting.

**No number in this repository may be written as "0% false positives"**, at any
corpus size this project can reach. `replay census` prints the sentence that says
so, beside the numbers, on every run, and `anticheat report` prints it again
beside its own.

**And since M8 the two bounds are computed by one value.** `anticheat::Bounds`
holds the two counts and its rendering emits both lines and the sentence above,
so quoting the friendlier one means deleting a line of output rather than
choosing not to write one. `docs/detectors/` carries a page per detector, each
with both bounds on it and each currently reporting "nothing at all (no
observations)" for both, because there is no corpus.

**One thing this section governs that M8 discovered it also constrains.** Two of
`docs/MILESTONES.md` M8's five candidate signals are not buildable at all, and
§3 above is why: the kilohertz device stream stays outside the artefact
resimulation is a function of, so it reaches a detector as the four summary
numbers in §4b rather than as a distribution — and the aim reaches the wire only
when a player clicks, so there is no trajectory in a replay for a curvature
statistic to run over. That is a consequence of a frozen format rather than a gap
in it, and `docs/detectors/README.md` states both verdicts, including why neither
reopens the `evdev` question `docs/RISKS.md` R14 left open: the binding
resolution is this schema's millisecond and the game's 33 ms tick, not the
client's 16 µs.

## 9. The destruction procedure

Written here, executed end to end by `replay/tests/destruction.rs` on a recording
built and discarded inside the test — which is what `docs/MILESTONES.md` M6 asks
for. The test drives the **binary**, because a procedure checked by calling the
functions it is a procedure for is a procedure agreeing with itself.

**On enrolment**, once per participant, after the consent text is signed on paper:

```console
$ replay enrol <corpus> <pseudonym> <identity> <consented-on> <retention-until> no|yes
```

The last argument is the separate publication opt-in. The consent record is
stamped with the version of `docs/CONSENT.md` this build holds; the operator does
not type it.

**After a session**, once per match, with the clients' parts collected into one
directory:

```console
$ replay store <corpus> <match.replay> <parts-dir> <recorded-on> <supervision> \
      [<match.telemetry>]
$ replay census <corpus>
```

The companion is an argument rather than a file `store` goes looking for, and
both directions are refusals: a replay committing to a companion cannot be filed
without it, and a companion cannot be filed beside a replay that named none.

Sealing the companion happens **before** the replay is sealed and is
`moba-server`'s job, because the replay's manifest carries the companion's digest
and a digest has to exist before something commits to it. Operationally that
means the clients' `*.telemetry-part` files have to reach the machine holding the
signing key while the server is still running; it waits for one part per seat
that played, with a deadline, and writes no companion at all if they do not all
arrive. **A companion covering some of the seats is never written**, because its
coverage would then be a function of who managed to copy a file.

`store` refuses rather than warns; `replay/src/corpus.rs` carries the table of
what and why. `census` writes nothing.

**On a withdrawal request**, acknowledged within 7 days and carried out within 30:

```console
$ replay withdraw <corpus> <pseudonym> <YYYY-MM-DD>
$ replay audit <corpus> <pseudonym>          # separately, and it must exit 0
```

`withdraw` destroys every match the pseudonym appears in — in full, including the
other participants' contributions — then the mapping, then the consent record,
then writes the tombstone and audits itself. The order matters: dying halfway
leaves a corpus with fewer matches and a live consent record, which the next run
repairs, where the other order would leave telemetry with nothing pointing at it.

The audit is run **again, separately**, because a command that checks itself can
be wrong twice in the same direction. It exits non-zero with a list of paths if
anything is left, and it reads every byte of every file under the root rather than
the places a pseudonym is supposed to be.

**Then delete the signed consent text**, which is paper or a scan and lives with
the corpus rather than in it. No command reaches it, and no command should: it is
the one artefact in this regime a person has to destroy deliberately.

## 10. Publication

Two decisions, taken here and not revisited per match (`docs/MILESTONES.md` M6):

- **Derived statistics are published.** Distributions, counts, thresholds, the
  numbers in this project's documents. They identify nobody and they are kept
  without a time limit, which `docs/CONSENT.md` states plainly rather than letting
  a participant infer that everything disappears.
- **The raw corpus is published only for participants who ticked the separate
  box**, and a match is publishable only if **every** participant in it did — a
  match is one interleaved log and there is no way to publish one seat of it. The
  practical consequence, stated in advance: one refusal in a match of nine
  withholds that match, so the publishable subset will in practice be small or
  empty, and no plan here depends on it existing.

## 11. The telemetry companion — the device stream, field by field

`matches/<id>/match.telemetry`, one sealed file per match, one stream per seat.
`replay::telemetry` is the code and `docs/CONSENT.md` §2b is what a participant
reads about it.

**It starts at the menu.** The client's capture path runs from the moment the
window opens, so a stream's first records are the lobby crossing §4e measures and
`docs/CONSENT.md` §2c describes; the match's records follow, and the boundary
between them is where the [`Viewed`](#11c-per-record-in-the-stream) anchors
begin. Nothing about the format changes: the lobby produces the same three
records the match does, through the same code, and a seat that crossed a lobby
and then played is one stream.

**Why it exists.** `docs/detectors/README.md` recorded two of
`docs/MILESTONES.md` M8's five candidate signals as *not buildable*, and the
reason was not calibration: the inter-arrival distribution and aim-correction
curvature are statistics over a quantity that **was not in the corpus at any
resolution**. `client::input::InputTrace` held every device event at 125 Hz to
1 kHz while a session ran, and §3 and §4b between them kept four summary numbers
and dropped the rest. That was a recording-policy decision — a defensible one
while the format's subject was resimulation — and it is reversed here, at the
last moment at which reversing it destroys nothing, because the corpus is empty.

### 11a. Where it sits, and the commitment that binds it

**It is not in the replay.** M5's invariant does not move: a resimulation is a
function of the seed and the input log alone, and nothing no rule reads can
influence it. The device stream is a second file.

**The replay's manifest carries this file's digest** — `Commitment::Sealed`, over
the companion's whole bytes including its own manifest — and that is what does
the work in both directions:

| Without the commitment | With it |
| --- | --- |
| A companion can be swapped for another. An attacker holding a key the registry accepts seals a second, smoother one for the same match, internally perfect | Refused: the replay named thirty-two other bytes first (`TelemetryError::Substituted`) |
| "Where is the telemetry" has no answer a file can give | The replay says, or says there is none |
| A replay is only as verifiable as the largest file beside it | A replay verifies **without** the companion, and says which state it is in |

**Absence is a signed state, not a missing file.** `Commitment::Absent` is what a
match that recorded no device stream carries, and it is legitimate: a development
run, a session whose parts never arrived, a match nobody was recording. `verify`
reports it in those words rather than failing, and — the half that gives it teeth
— because the absence is *inside the signature*, attaching a companion to such a
replay afterwards is a refusal (`TelemetryError::NotCommitted`) rather than an
upgrade.

**There is one format and it is sealed**, for the reason M5 gives about the
replay: a reader that accepts a sealed and an unsealed companion accepts the
weaker. The one thing that is not sealed is a client's `*.telemetry-part`, and it
cannot be — `client` may not link `replay`, which owns the signing key, so a
client structurally cannot sign. A part is a transport between two processes, it
names one seat rather than a match, it is **not a corpus artefact**, and §1's
"no other file" check is what reports one left in a match directory.

### 11b. Per seat, in the signed manifest

| Field | What it is |
| --- | --- |
| `clock` | `dequeue` or `device` — what `at_ns` actually is (`client::input::CLOCK`) |
| `platform` | `linux`, `windows`, `macos`, `other`. What a device count and a timestamp *are* differs between them |
| `world_units_per_count_e6` | The build's sensitivity. It scales the **aim** and not the record, so it is here to make the aim reconstructible from the stream rather than to be divided out of it |
| `samples` | Device events: motions and control transitions |
| `motions` | Motions among them |
| `views` | View anchors, which are **not** device events |
| `dropped` | Device events the client's buffer refused. Nothing else in the corpus carries it, and a stream that lost its tail silently would be a distribution with a hole nobody can see |

The first six duplicate §4b, deliberately: §4b is what survives when there is no
companion, so neither file is derived from the other and both can drift.
`Corpus::store` refuses them disagreeing, seat by seat.

### 11c. Per record, in the stream

Every record is **25 bytes** whatever it holds, so a file's length is a function
of how many events it carries rather than of which ones.

| Record | Carries |
| --- | --- |
| `Moved` | `dx`, `dy` — the platform's `f64` pair **by its bits**: the device's own units, unscaled, unrounded, unquantised. Downward-positive, which is the platforms' convention and is kept rather than corrected |
| `Pressed` | One of the five controls the game uses, and whether it went down or up. Presses that produced no order are recorded, because a `Targeted` with nobody in range is a thing a player did |
| `Viewed` | `tick` and `seq`: a server view for that tick reached the client, and the client answered with that intention. Thirty a second |

Every record carries `at_ns` on that seat's own monotonic clock.

**`Viewed` is the only record that is not the hand, and it is why a reaction is
measurable at all.** A device stream without it is a hand in a vacuum: an
inter-arrival distribution and a curvature statistic can be computed from motions
alone, but a reaction is the interval between being *shown* something and
answering it, and this is the only clock with both ends on it. `tick` is the
replay's clock and `seq` is the log's per-player counter, so every sample in this
stream can be placed against the match without either side carrying a wall clock.

**It is not counted among the device events**, in the manifest and in
`client::input::TraceStats` alike. §6 refuses a seat that recorded zero device
events, which is the corpus's one mechanical defence against a headless client —
and a headless client *receives views*. Counting anchors among the samples would
hand that defence to the exact attacker it exists to catch.

**And a traced seat with zero anchors is refused**, which is the mirror of that
rule and exists for a different reason. The place the anchor is *attached* —
`client::gfx::Session::advance` — is the one part of the capture path no test can
reach, because the loop needs a display server and CI has none, which is the same
admission `docs/RISKS.md` R16 makes about the tick-budget bracket. A seat that
played a match received frames, so a stream with no anchor in it is a client whose
wiring is broken rather than a session, and `Corpus::store` says so at the door.
An operator finds out when they file the match rather than when a detector reads a
corpus that cannot answer the question it was recorded for.

### 11d. What is deliberately not in it

Named, because what is missing here is missing from the whole corpus.

- **No pseudonym**, and this is the field whose absence costs the most. The
  signed manifest is the one naming of a person; a second naming here would be
  the derived index M5 removed, in a new place. The price is that a search for a
  name cannot find a companion left behind, which is why §1's "no other file"
  check and `Corpus::accountable`'s coherence clause exist and why
  `replay/tests/withdrawal.rs` breaks the withdrawal to prove they work.
- **No wall clock, and no cross-seat time reference.** Each seat's `at_ns` is its
  own client's monotonic clock with its own epoch; two seats' streams are **not**
  comparable in time, and the only common reference is the tick, through `Viewed`,
  which is the server's. A wall-clock anchor would be a number a client wrote, and
  `docs/SCOPE.md`'s adversary model puts that in the attacker's hands by
  definition. Anything that needs two hands aligned to the millisecond is not
  computable from this corpus and will not become so.
- **No aim, and no world coordinate.** The aim is an integral of these deltas
  under `world_units_per_count_e6` and the map clamp, both of which are here or in
  `rules_hash`. A stored aim would be a field that can disagree with its own
  inputs.
- **Nothing from the renderer.** No window size, no pixel, no drawn position, no
  scale factor. `docs/RISKS.md` R14 is the entry and `client::draw` has no inverse
  projection at all, so there is no screen-space quantity for this file to have
  derived from.
- **No key outside the five the game uses**, no text, no pointer position on the
  desktop, no device model or serial, nothing from outside the match.
- **No summary, no score, no derived statistic.** Those are §4b's, and §4b is
  cross-checked against this file rather than computed from it.

### 11e. The size budget, and what a saving would destroy

The number this format costs, before anybody discovers it on a disk. Twenty-five
bytes a record, thirty view anchors a second, and roughly ten control transitions
a second for a busy player:

| Mouse polling rate | Per seat | Nine seats | A 20-minute match | 20 matches | 40 matches |
| --- | --- | --- | --- | --- | --- |
| 125 Hz | 4.1 kB/s | 37 kB/s | **42.5 MiB** | 0.83 GiB | 1.66 GiB |
| 500 Hz | 13.5 kB/s | 122 kB/s | **139 MiB** | 2.72 GiB | 5.43 GiB |
| 1000 Hz | 26 kB/s | 234 kB/s | **268 MiB** | 5.23 GiB | 10.5 GiB |

Against the replay of the same match — nine seats, one intention per tick, 34
bytes an input — which is **10.5 MiB**. So the companion is **4× the replay at
125 Hz and 25× at 1 kHz**, and it is the artefact that determines what a corpus
costs to keep.

**The verdict: it fits, and no saving is taken.** `docs/SCOPE.md` puts scale out
of scope and this corpus is twenty to forty matches on one machine; the worst
case is about five gigabytes at the reduced match count and about ten at the
full one, which is a disk rather than a problem. What follows is what each
available saving would have cost, because a reader is entitled to know that the
question was asked.

| Saving | What it buys | What it destroys |
| --- | --- | --- |
| **Quantise `dx`/`dy` to whole device counts** — two `i16` instead of two `f64` | 25 bytes → 11, a 56% cut and the only large one | **Refused, and by name.** Unaccelerated backends do not report whole counts: Wayland's relative motion is fixed-point in 1/256 of a count and X11's raw valuators are FP16.16. Rounding them puts a grid back in the record — `docs/RISKS.md` R14 exactly, one order finer — and the detector it destroys is the curvature detector this file exists to make possible, whose whole subject is the shape of a trajectory at the finest resolution the device produced |
| **Delta-encode `at_ns`** as a gap rather than an absolute | ~16% | Nothing about the data, and the reader's totality: records stop being fixed-width, so the record count no longer bounds the buffer before an allocation and a decoder gains a case to get wrong. Not worth 16% |
| **Compress the stream** | Plausibly 40–60%, losslessly | Nothing about the data. Not taken on two grounds: `docs/ENGINEERING.md`'s bar for a dependency in the crate that owns the signing key, and the one-format rule — a compressed and an uncompressed companion would be two files nobody can tell apart at a glance, which is M5's lesson |
| **Drop the view anchors** | 18% at 125 Hz, 3% at 1 kHz | Every reaction statistic, entirely. See 11c |
| **Sample at a fixed rate instead of per event** | Whatever rate you choose | A resampling of a stream the client already holds in full: it aliases anything faster than its interval and can only lose information. `client::input` refused it once already |

### 11f. What a 1 kHz seat costs, which is the one honest worry

At 1000 Hz the gap between two device events is **1 ms**, and `docs/RISKS.md` R14
measured the delay this client adds between an event existing and being stamped
at 16 µs of standard deviation and 0.26 ms at worst over 1200 samples in
`release`. Sixteen microseconds against a millisecond is 1.6% and harmless; a
worst pass of the capture loop at 5 ms against a 1 ms gap is **not**, because
five device reports queued during one pass are stamped microseconds apart as the
queue drains, which puts a burst-and-stall structure into the record that belongs
to the client rather than to the hand.

That is not a defect and it is not fixed here. What it is, is a **covariate the
corpus already records**: `device_polling_hz` is declared per seat (§4a) and
`median_gap_ns` is measured beside it (§4b), and `replay census` prints the
declared rates with the sentence saying what pooling them costs. The rule:

> A detector reading an inter-arrival distribution stratifies by declared polling
> rate, or its page says it did not.

`docs/RISKS.md` R14's reopening criterion is where the harder version of this
lives: at 1 kHz the quantity a detector reads *is* at the scale of the residual,
which is the condition under which a per-platform input stack would start buying
something.
