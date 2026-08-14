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
  withdrawals/<pseudonym>.withdrawn     a tombstone, and nothing else
```

Outside the repository, always. `.gitignore` refuses every one of these shapes
and `ci` fails a pull request that tracks one, because `docs/RISKS.md` R3 is about
an irreversibility git makes literal.

**There is no other file.** No index, no cache, no summary, no participant list,
no split file. `docs/CONSENT.md` records why — a derived artefact is what outlives
the thing it was derived from — and `Corpus::audit` is the guard: it reads every
byte of every file under the root, so an artefact added quietly is reported the
first time somebody withdraws. `replay/tests/withdrawal.rs` plants one to prove
it.

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
and will not be: anything at a higher rate. `client::input::InputTrace` holds a
kilohertz stream of raw device deltas while a session runs, and it stays outside
the artefact resimulation is a function of (`replay/src/manifest.rs`). What
reaches the corpus from it is the summary in §4.

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
| `device_cpi` | counts per inch the mouse is configured at | a mouse reports counts. Nothing in the stream says what physical distance produced them |
| `device_polling_hz` | the device's report rate | the client observes an *arrival* rate, which is the report rate plus the platform. §4b records the observation beside the declaration |
| `pointer_acceleration` | whether the OS's acceleration was left on | the client sees deltas after the platform has applied it, and cannot invert a curve it does not know |

All three are **declarations**. They are as good as the participant's answer and
no better, and no analysis may treat them as measurements. They are collected
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
   `device_cpi`**, which means they rest on a number nobody checked. A detector
   that thresholds on a speed must say so, and must state that a participant who
   misreported their CPI by a factor of two is a participant that detector will
   score as an outlier for a reason that has nothing to do with how they played.
3. **Shape-shaped statistics — curvature, smoothness, the ratio of one distance to
   another — are scale-invariant and therefore comparable without the
   declaration.** This is the strongest position available, and it is the one a
   curvature detector at M8 should be built from.

The residual nobody can close: acceleration being *off* is itself a declaration.
The corpus cannot tell an accelerated session from an unaccelerated one, because
the only difference is in a transformation applied upstream of the first byte this
project sees.

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
| A match played by a bot, a script, or a headless client | `Corpus::store` refuses any seat that recorded **zero device events**, and the schema has no `provenance` value but `human` and `empty` — a part claiming otherwise does not parse |
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
$ replay store <corpus> <match.replay> <parts-dir> <recorded-on>
$ replay census <corpus>
```

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
