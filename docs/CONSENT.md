# CONSENT

consent-version: 2026-08-17

The text every participant reads and agrees to **in writing, before the first
recording**, and the rules this project holds itself to afterwards.

`docs/RISKS.md` R3 is the reasoning; this is the instrument. `docs/MILESTONES.md`
M4 makes its existence part of an exit criterion, because M4's own criterion —
three people playing a match — is already a collection of personal information
and there is no later milestone at which writing this down is still in time.

## The version line above, and what it is for

`consent-version` is the identifier of *this* text, and it is one of the two
things on this page a program reads. A signature on paper is a fact about a
document on a day; a consent text that later gains a field — a new covariate, a
new retention rule, a new purpose — has stopped being the document somebody
signed, and a corpus of replays would say nothing whatever about the difference.

So: every consent record carries the version its participant signed, every
recording session carries the version it was operated under, and `Corpus::store`
**refuses a match where either is missing or is not the current one**. Missing
and stale fail identically and deliberately — a consent record written before
this field existed does not decode, and is therefore not a consent record — so
that a corpus assembled under an older regime cannot be readmitted by the silence
of its own files.

`replay::consent::CURRENT` is the same date as the line above; a test fails if
the two disagree, and `ci` refuses a pull request that edits this document
without raising it. The paper is still paper. What has changed is that its
**absence is now a mechanical error**.

The date is the day this text takes effect rather than the day it was written.
No session has been recorded under this version or any earlier one, so nothing
turns on the difference; it is recorded because a reader who notices a document
dated after its own commit is entitled to an explanation.

### The second thing a program reads, which is new in this version

A consent record now carries **one line per separable purpose**, granted or
refused, and `replay::consent::Permissions::decode` refuses a record that is
silent about any purpose this build knows. That is the same equivalence the
version line draws, one level down: a purpose nobody was asked about is a purpose
nobody granted, and the corpus says so by refusing rather than by defaulting.

The consequence is worth stating because it is the point: **adding a fifth box to
the form below invalidates every consent record already written.** Everybody is
asked again. There is no path by which an old signature quietly covers a new use.

### What changed in this version, `2026-08-17`

Read this rather than the whole page if you have signed before. Nothing here
collects anything new — not one field, not one millisecond — and that is itself
the summary: **the collection is unchanged and the choice is not.**

**Four things are now separate boxes instead of one.** The previous text offered
one optional box, for publication, and folded everything else into a single
signature. It now offers four, and each of them is refusable on its own without
affecting anything else about your participation:

| The box | Refusing it means |
| --- | --- |
| `publication` | the raw recordings of your matches are never published; statistics derived from them still are |
| `bot-training` | your recordings never train a bot; they are still used to calibrate and evaluate the detectors |
| `retention-after-project` | your recordings are destroyed when the project's work concludes rather than at the retention date |
| `named-attribution` | you appear under your pseudonym and never under your name in anything derived from the corpus |

**`bot-training` is a new purpose and it is refused by default.** The previous
text said the data would not be used "to train anything that outlives this
repository", and that promise is unchanged: the bot in question is this project's
own deferred reinforcement-learning sub-project (`docs/SCOPE.md`), nothing
trained on this corpus leaves it, and a model trained on it is destroyed by the
same withdrawal that destroys the matches it was trained on. What changed is that
the possibility is now *asked about* instead of excluded by silence, and the
answer defaults to no.

**Each of the four is now carried out by the tooling rather than remembered.**
A refusal used to be a boolean in a file that one command read. It is now a gate
each use has to pass: `replay publish` cannot construct the value it writes for a
match somebody refused, a training set cannot contain one, `replay conclude`
destroys what may not be kept, and the identity mapping refuses to hand out a
name. "How each choice is applied" below is the table.

**Withdrawal is now partial as well as total.** You can withdraw one permission
and keep taking part. That was not possible before: the only way to stop your
recordings being published was to have all of them destroyed.

**You are asked to confirm you are 18 or over**, and a match is refused if
anybody in it is not. §5 of the form says why.

**And you are shown your own data before you sign.** §L3 is new: a few dozen
lines of your own mouse movements, captured a minute earlier, with the four
things this project can work out from them computed from *your* numbers. That
part of the disclosure replaces nothing — every paragraph that was here is still
here — it is what makes one of them checkable.

Earlier versions are `2026-08-16`, `2026-08-15` and `2026-08-14`; what each of
them changed is in `replay::consent::CHANGES` and is printed by `replay enrol`
when somebody who signed one of them signs again, so that a re-signature is
against a *difference* rather than against the same page a second time.

This document is engineering, not legal advice. Whether Quebec's *Act respecting
the protection of personal information in the private sector* (Law 25) formally
binds a non-commercial hobby project is genuinely unsettled, and the position
here is that it does not matter: the project holds itself to the regime
regardless, because the cost is a page of text and the alternative is a security
portfolio that collects behavioural biometrics from friends with no stated rules.
"What is sent to a human review" at the end of this document is the list of
points where that position is a judgement rather than a settled reading.

---

## The consent text

The page is in five levels. **Level 0 is complete**: everything below it goes
deeper, and none of it introduces a consequence L0 does not name. If you read
only L0 and the form, you have read everything that could reasonably change your
answer.

---

### L0 — In thirty seconds

> You are being asked to play matches of an experimental game with eight other
> people, and to let the recordings be kept. The project is an anti-cheat
> engineering exercise; the game exists to produce data to test the anti-cheat
> against.
>
> **What is recorded.** Everything you do with the mouse and the five keys the
> game uses, from the moment the menu appears until the match ends — **every
> movement your mouse reports, 125 to 1000 times a second**, not a summary of it.
> Every instruction the game acted on, thirty times a second. What your equipment
> is and how it behaves. Nothing else: no text, no image, no sound, no other
> program, nothing outside the game's window.
>
> **What that means about you.** The shape and speed of the way you move a mouse
> is distinctive — closer to **handwriting** than to a preference. Somebody
> holding this file and a second recording of you could plausibly tell the two
> are the same person. That is precisely why the project wants it. §L3 shows you
> your own, so you can judge this rather than take it from us.
>
> **What it is used for.** Calibrating and evaluating this project's cheat
> detectors, and publishing statistics derived from that. **That is the only
> declared purpose and the data will not be used for another one** without your
> separate agreement — the four boxes below are the four separate agreements
> there are. It is never used to verify or confirm anyone's identity, never given
> to a third party, and never reused by another project.
>
> **Who holds it.** One person, the author, on his own machine, in a directory
> **outside the git repository**. No cloud, no database, no backup elsewhere.
>
> **How long.** 24 months from the recording, or until you withdraw, whichever
> comes first. Statistics that identify nobody are kept without a time limit.
>
> **Withdrawing.** One message, no reason, no consequence, at any time.
> Acknowledged within **7 days**, carried out within **30**.
> It destroys **every match you played in, in full** — including the other
> **eight players'** contributions to those matches, who are not asked and not
> notified — because a match is one interleaved log and there is no way to remove
> one person from it.
> You can also withdraw **one permission** and keep taking part.
>
> **What is not a choice, said plainly rather than dressed as one.** The movement
> stream, the instruction log, the session record and the lobby measurement are
> what the declared purpose is made of. This data is necessary to this
> experiment; refusing it means not taking part. There is no tick box for them
> and there should not be, because a box whose only two outcomes are "take part"
> and "do not" is not a choice, it is a form pretending to offer one.
>
> **What is a choice**, refusable on its own with nothing else changing:
> publication of the raw recordings, use for training a bot, keeping the data
> after this project's work ends, and being named rather than pseudonymous.
>
> **You must be 18 or over** to take part.

---

### L1 — The five categories, and which of them are your choice

> Everything held about you falls in one of five categories. The right-hand
> column is the whole of the granularity: three of the five are a choice, and the
> other two are not, and the reason is the same test in every row — **is refusing
> it something the rest of your participation survives?**
>
> | # | Category | What it is | Your choice? |
> | --- | --- | --- | --- |
> | 1 | **Your hand** | Every movement and every press your mouse and the five keys report, at the device's own rate, from the menu to the end of the match | **No.** It is what the detectors read. A recording without it cannot serve the purpose you are being asked to help with — and it is one file for all nine seats, so one person's refusal would remove it for the other eight |
> | 2 | **Your instructions** | The one order the game acted on in each thirtieth of a second, with the time your computer claimed and the time the server observed | **No.** It is the match. Without it there is no replay and nothing to resimulate |
> | 3 | **Your session and your equipment** | What mouse, what settings, what platform, whether the game kept up with its own clock, who was supervising, and what crossing the menu measured about your mouse | **No.** Without it a difference of hardware is read as a difference in how you play. The menu measurement is a *calculation* over a crossing that is recorded either way, so refusing it would change nothing about what is held — see L2 §3 |
> | 4 | **Where it goes** | Whether the raw recordings are published; whether they train a bot; whether you are named rather than pseudonymous | **Yes — three separate boxes.** Each is a use rather than a collection, and refusing any of them leaves everything else exactly as it was |
> | 5 | **How long it stays** | 24 months or withdrawal, and whether it may be kept that long even after the project's own work ends | **Yes — one box.** Refusing it moves your destruction date earlier and changes nothing else |
>
> **Rows 1 to 3 are stated as necessity rather than offered as a choice, and that
> is a decision this project defends rather than a corner it cut.** A form that
> put a tick box beside "every movement of your hand" would be offering you a
> choice whose refusal produces a recording nobody can use, which is the same
> thing as declining to take part — and dressing that up as a granular option is
> the handling this project criticises elsewhere. So the sentence is the honest
> one: *this data is necessary to this experiment, and refusing it means not
> participating.*
>
> **Rows 4 and 5 are four boxes and not more.** A box for each technical field
> would be granularity in the shape of a form and none of the substance: refusing
> `seq` while accepting `tick` is not a decision anybody has an interest in
> making. The test each box passes is that a reasonable person could want exactly
> that one thing to be different, and that the project can honour it.

---

### L2 — In detail, category by category

#### 1. Your hand — every movement your mouse reports

> This is the part to read slowly, because it is the largest thing on this page.
>
> The replay in §2 records the one instruction the game acted on in each
> thirtieth of a second. Your mouse reports far more often than that — between
> **125 and 1000 times a second**, depending on the mouse — and the game on your
> machine reads all of it. **All of it is kept**, in a second file beside the
> replay, one section per seat, **from the moment the menu appears, not from the
> moment the match starts**:
>
> | Recorded | What it is |
> | --- | --- |
> | Every movement your mouse reports | How far it moved right and how far it moved down, in the mouse's own units, exactly as your computer reported it — not rounded, not smoothed, not converted |
> | The moment of each one | Measured by a stopwatch inside the game, started when the game started. Not the time of day, and not comparable to anybody else's |
> | Every press and release | Of the five controls the game uses — left click, right click, `Q`, `W`, `S` — including presses that did nothing |
> | Every frame that arrived | Which thirtieth of a second it was for, and which instruction you sent back. Thirty a second. This is the only line in the file that is not your hand: it is what lets the project tell *when you were shown something* from *when you answered*, and without it nothing about your reaction time can be measured at all |
>
> **What can be worked out from this.** The shape and speed of the way you move a
> mouse: how you accelerate, where you overshoot and correct, how steady your
> rhythm is, how long you take to react to something appearing on screen. Taken
> together, that is **distinctive** — closer to handwriting than to a preference.
> Somebody holding this file and a second recording of you could plausibly tell
> that the two are the same person. That is precisely why the project wants it:
> telling a person from a program is the whole subject, and this is the data that
> difference lives in. **§L3 is where you see your own rather than take this on
> trust.**
>
> **What is not in it.** No key you pressed outside those five. No text, ever. No
> screen capture, no image, no sound. Nothing about where the mouse pointer is on
> your desktop, nothing about the size of your window or your monitor, nothing
> from any other program, nothing from before the game's window opens or after it
> closes.
>
> **Why the menu is recorded too.** Between joining and playing there is a wait:
> nine people have to be at their keyboards at once, and the last one is always a
> few minutes behind. During that wait you are in a menu — you check your name,
> you confirm which version of this page you agreed to, you pick a champion, and
> there is a practice target you can click at. Your mouse movements during that
> menu are recorded exactly as they are during the match, in the same file, by the
> same code, and this is said here because "recorded" and "you might have assumed
> otherwise" should not both be true. What is done with it is §3 below.
>
> **If you would rather this were not recorded, the honest answer is not to take
> part.** It is not offered as a separate box, and pretending otherwise would be
> dishonest twice over: the declared purpose is calibrating detectors that read
> exactly this, and the file covers all nine seats or none, so one person's
> refusal would remove it for the other eight. It is one refusal and it refuses
> everything, which is your right and costs you nothing.

#### 2. Your instructions — the match itself

> For every match you play in, the replay file holds:
>
> | Field | What it is |
> | --- | --- |
> | `match_id` | A number that tells this match apart from another one |
> | `seed` | The number the match's world was generated from |
> | `rules_hash` | A fingerprint of the game's constants, so the match can be replayed correctly later |
> | `sim_version`, `sim_commit` | Which build of the game resolved the match, so that a replay that no longer replays can be told from a replay that was edited |
> | `started_at_unix_ms` | When the match began. This is what the destruction date below is counted from |
> | `participants` | **Your pseudonym**, and those of the other people in the match, one per seat. Not your name — see §4 |
> | `ticks` | How many thirtieths of a second the match lasted |
> | `outcome` | Which team won, and on which tick, or that the match was still being played |
> | `input_log_digest`, `final_state_digest` | Fingerprints of the inputs and of how the match ended |
> | `server_identity`, and a signature | Which machine recorded the match, and a seal over everything above, so that nobody can alter a replay afterwards and present it as yours |
>
> And for **every input you make** — roughly one per thirtieth of a second while
> you are playing:
>
> | Field | What it is |
> | --- | --- |
> | `tick` | Which thirtieth of a second of the match the input belongs to |
> | `seq` | A counter, so that inputs cannot be reordered or replayed |
> | `player` | Which of the nine seats you were sitting in, as a number 0–8 |
> | `action` | What you asked for: stand still, walk to a point, cast toward a point, cast at a target, or attack a target — with the coordinates or the target you chose |
> | `claimed_at_ms` | The time **your** computer said it was when you made the input |
> | `received_at_ms` | The time **the server** observed the input arrive |
>
> The two timestamps are collected separately and deliberately: the difference
> between them is one of the signals the project is studying.

#### 3. Your session and your equipment

> Beside each match the project keeps one more file, describing the **seat**
> rather than the person: there is no name and no pseudonym in it, and what
> connects a seat to you is the replay above. It exists because a mouse set to 400
> counts per inch and one set to 1600 describe the same hand differently, and
> without knowing which you had, a difference of equipment would be read as a
> difference in how you play.
>
> Four things you are **asked**, because no program can read them:
>
> | Field | What it is |
> | --- | --- |
> | `device_profile_id` | A short label — a colour or a word, chosen by the operator — meaning "the mouse you played on tonight". It is **not** your pseudonym and **not** a name for you: it names a *device*, so that two evenings on the same mouse can be read together and two evenings on different mice are not mixed up. If you change mouse, you get a new label, and you are asked nothing else about it |
> | `device_cpi` | How many counts per inch your mouse is set to |
> | `device_polling_hz` | How many times a second it reports |
> | `pointer_acceleration` | Whether your operating system's pointer acceleration is on. It has to be **off** to take part, and a session that says otherwise is refused rather than recorded |
>
> And what your own copy of the game **measures about itself** while you play:
> which operating system, which clock its timestamps came from, the sensitivity
> the game applied, how many device events it recorded, how regularly they
> arrived, and whether the game kept up with its own clock — the number of times
> it fell behind and by how much. That last pair is about the machine and not
> about you: a session in which the game stuttered records a pause that was the
> computer's, and the project would otherwise read it as yours.
>
> **What the menu measures about your mouse.** The positions of everything in
> that menu are fixed and known to the project, so every click is a movement whose
> start and end the project already knows. Putting many of them together gives:
>
> | Worked out | What it is |
> | --- | --- |
> | How many counts your mouse reports for a given distance on screen | The number that lets a movement be described in the same units for you and for somebody with a different mouse. Without it, a mouse that reports twice as much looks like a hand that moves twice as fast |
> | How often your mouse actually reports | Measured, rather than the number you were asked for above — a mouse's setting and its behaviour are not always the same thing |
> | The smallest movement your mouse can report | A property of your hardware and your operating system, not of you |
>
> It does not measure how good you are at anything, it is not scored, nobody
> passes or fails it, and there is no screen that tells you it is happening —
> because there is nothing for you to do about it. It is a measurement of your
> **equipment**, taken this way rather than by asking you to complete a
> calibration exercise because a calibration exercise is a chore and this is a
> wait you were having anyway.
>
> **And it is not a separate box, for a reason that is worth being exact about.**
> Crossing the menu is how you reach the match — the ready button does nothing
> until you have — so the movements are recorded either way, under category 1.
> What the project does with them afterwards is *arithmetic on data it already
> holds*, and a box refusing that arithmetic would change nothing about what is
> kept about you while deleting a number that makes your movements comparable
> with somebody else's. The control that does exist is simpler and you already
> have it: **spend the wait doing something else.** A session in which you did is
> recorded, kept and used exactly like any other; the only consequence is that the
> parts of the analysis that need the measurement say "not known" for you rather
> than producing a number. You are never refused a match, never delayed, and never
> told to move your mouse.
>
> **Nothing here identifies a device.** No model, no serial number, no
> manufacturer, no operating-system version, no machine name. A number is not a
> fingerprint of a mouse, and neither is a label somebody made up.
>
> The same file also records **how the session was supervised** — whether the
> person running it was in the room with you, on a call with you, or not present
> at all. It is one value for the whole session, written down by the operator, and
> it is about the session rather than about any one of you. It is there because it
> is the one place this project relies on a person rather than on a program:
> nothing in a recorded file can tell a person playing from a program moving a
> real mouse, and what tells them apart is that somebody was there. So the project
> writes down whether somebody was, instead of remembering it. If nobody was
> watching, the file says nobody was watching.

#### 4. What else the project holds

> A **consent record** for you — your pseudonym, the date you consented, the
> version of *this text* you signed, the date your data is destroyed, whether you
> confirmed you are 18 or over, and **one line for each of the four boxes below,
> granted or refused** — and a **pseudonym mapping**, which is the one file that
> connects your pseudonym to you.
>
> **What is not collected:** no audio, no video, no screen capture, no chat, no IP
> address in the corpus, no hardware identifiers, no device models or serial
> numbers, no operating-system versions, no files from your machine, no keys
> outside the five the game uses, no date of birth, and nothing at all outside the
> match.
>
> **This is still information about you.** Replacing your name with an opaque
> identifier is a security measure, not a change of category: input timing is
> distinctive, and "the mapping is in another file" would be a thin claim. The
> project treats this as personal information throughout.

#### 5. Where it goes, and for how long

> **The declared purpose, and nothing else.** The recordings are used to
> calibrate and evaluate this project's behavioural cheat detectors, and to
> publish statistics derived from that work. That is the **only declared purpose**
> and the data will not be used for another one. Specifically, it is **not** used
> to verify or confirm anyone's identity, it is **not** transferred to any third
> party, and it is **not** reused by another project.
>
> Three things are outside that purpose and each is a **separate purpose with its
> own box**, refusable without refusing anything else:
>
> - **`publication`** — publishing the **raw** recordings, as opposed to
>   statistics computed from them. A match is one interleaved log, so a match is
>   published only if **every** participant in it agreed; one refusal in a match of
>   nine withholds that match. In practice the publishable set will be small or
>   empty, and no plan here depends on it existing.
> - **`bot-training`** — using the recordings to *train* something that plays, as
>   opposed to calibrating something that measures. This project has a deferred
>   sub-project that would build a reinforcement-learning bot; a recording you
>   allow here may be part of what it learns from. Nothing trained on this corpus
>   leaves this project, and a model trained on it is destroyed by the same
>   withdrawal that destroys the matches it learned from.
> - **`named-attribution`** — appearing under **your name** rather than your
>   pseudonym in work derived from this corpus: an acknowledgement, a report, a
>   talk. Refusing it is the default.
>
> **How long it is kept, and what triggers deletion.** Raw telemetry — including
> the movement file in §1, which is the largest part of it — the recordings
> containing it, and the pseudonym mapping are destroyed **24 months after the
> recording**, or when you withdraw, whichever comes first. The date is written
> into your consent record when you sign it.
>
> **`retention-after-project`** is the fourth box and it is about that date. This
> project's own work finishes before the 24 months run out. Granting it means the
> recordings may be kept until the retention date anyway; refusing it means they
> are destroyed when the work concludes, which is earlier. Refusing costs you
> nothing and costs the project a corpus it can no longer answer questions
> against.
>
> Statistics that identify nobody — distributions, counts, thresholds and the
> numbers in the project's documents — are kept without a time limit. This is said
> plainly rather than left for you to infer that everything disappears.

#### 6. Withdrawing, in whole or in part

> **You may withdraw at any time**, without giving a reason and without any
> consequence, by a single message to the contact address at the end of this text.
> You do not need to re-consent to anything in order to withdraw, and you will not
> be asked why. Your withdrawal is acknowledged within **7 days** and carried out
> within **30**.
>
> **Withdrawing everything.** A match is a single interleaved log of nine players'
> inputs. Removing one person's inputs leaves a log that no longer replays, so
> removing only your part is not something this project can offer. Withdrawing
> therefore destroys **every match you played in, in full** — the replay, the
> equipment record including the mouse label and the menu measurement, and the
> movement file in §1 — together with your pseudonym mapping and your consent
> record. That includes the other **eight players'** contributions to those
> matches. They are not asked and are not notified.
>
> What survives is a single line recording that a pseudonym withdrew and on what
> date. It contains nothing else, and because the mapping is destroyed in the same
> operation, it names nobody.
>
> **Withdrawing one permission.** You can also take back any one of the four boxes
> and keep taking part. Nothing is destroyed, your matches stay in the corpus, and
> what changes is that the use you withdrew stops reaching them — from the moment
> the record is edited, without anything having to be recomputed. It is checked
> the same way a full withdrawal is: by running the use's own test over your
> matches and requiring it to reach none of them.
>
> **Two things withdrawal does not undo, and they are the same thing twice.**
> Statistics already published are not retracted: they identify nobody, they are
> already in documents and pull requests that are public, and unpublishing them is
> not something anyone can actually do. And **a publication cannot be recalled** —
> if the raw recordings have already been published, withdrawing `publication`
> stops every future publication and none of the one that happened. Every refusal
> in force at the moment `replay publish` runs is honoured mechanically; a refusal
> arriving afterwards is a conversation with a person, and this page would rather
> say so than imply a guarantee the internet does not allow anybody to make.

#### 7. Who has access, and where the data lives

> One person: the author of the project, Vianney Veremme, who is the only operator
> and the only administrator.
>
> The recordings live on the author's own machine, in a directory **outside the
> git repository**, and are never committed — deleting a committed file does not
> delete it, so the repository's `.gitignore` refuses these paths and CI fails a
> pull request that tracks one. There is no cloud service, no hosted database, no
> analytics provider and no backup off that machine.

---

### L3 — Your own data, before you sign

> Everything above says the movement of your hand is distinctive. That is the
> single most consequential sentence on this page, and it is also the one you have
> no way to check: it is a claim about data you have never seen, made by the party
> asking for it.
>
> **So you are shown yours.** Before you sign, you cross the lobby once — the
> menu described in §3, a minute of clicking at a practice target — and the
> operator runs one command on the file your own computer just wrote. What comes
> back is not an example and not a simulation:
>
> ```console
> $ replay disclose seat-0.telemetry-part
>
> This is your own recording, from the lobby you have just crossed. Nothing
> below is an example or a simulation: every line is a record your computer
> made in the last few minutes.
>
> WHAT WAS RECORDED — the first 24 of 3184 movements
>
>       time         dx        dy   (dx, dy are your mouse's own counts)
>      0.000 ms     +3.00     -1.00
>      1.031 ms     +4.00     -1.00
>      2.008 ms     +6.00     -2.00
>      3.052 ms     +7.00     -2.00
>      …
>
>   3184 movement(s), 46 button press or release(s) and 812 frame(s) received,
>   over 27.1 second(s).
>
> WHAT THIS PROJECT CAN WORK OUT FROM IT — computed from your numbers
>
>   Your mouse reported about every 1.01 ms — roughly 990 times a second. …
>   The smallest movement your mouse can express is 1 count(s). …
>   Your hand travelled 41 302 counts to move the cursor 26 118. The
>   difference — 15 184 counts, 37% — is overshoot and correction: …
>   The quickest you answered something appearing on your screen was 214 ms. …
> ```
>
> The four numbers at the bottom are the four things the paragraphs above
> describe, computed from *your* movements rather than asserted in general. The
> last one is the one that lands: a reaction time, read off a file you produced by
> waiting in a menu.
>
> **Nothing on that page is a score.** Nobody passes or fails it, no number on it
> is compared against anybody else's, and no threshold exists anywhere in this
> project to compare it to.
>
> **That crossing is not stored.** The command writes nothing — there is no
> argument for a destination and no path in the program to a file — and the
> session it read is never filed into the corpus, whether or not you sign.
>
> **And the honest awkwardness, which this page will not hide from you.** The
> demonstration shows you data that had to be captured *before* the signature it
> is meant to inform. That is the opposite of the order everything else here
> follows. The reason it is done anyway is that no paragraph achieves what thirty
> seconds of your own movements achieve, and the terms are narrow: it is described
> and agreed to out loud before it happens, used for this and nothing else, and it
> never reaches the corpus. If you would rather skip it, say so — you lose the
> demonstration and nothing else, and everything above is still what you are
> agreeing to.

---

### L4 — What you are agreeing to

> **Age.** This project's consent regime covers adults only. A participant under
> 18 cannot give sufficient consent on their own, and this project has no
> parental-consent procedure, no separate text for one and nobody to review one.
> A match is refused by the tooling if anybody in it is recorded as under 18. Your
> date of birth is **not** collected; the one bit is.
>
> - [ ] I am 18 years of age or over.
>
> **Participation.** This is not refusable in parts, for the reason L1 gives:
>
> - [ ] My matches may be recorded — the movement of my hand at my device's own
>   rate, my instructions, my session record and my equipment — and used to
>   calibrate and evaluate this project's cheat detectors, and statistics derived
>   from them may be published.
>
> **The four separate choices.** Each is refusable on its own. Refusing any of
> them changes nothing about the box above:
>
> - [ ] *(optional)* **`publication`** — the raw recordings of my matches may be
>   published as part of an open data set.
> - [ ] *(optional)* **`bot-training`** — my recordings may be used to train this
>   project's reinforcement-learning bot, which is a different purpose from
>   calibrating its detectors.
> - [ ] *(optional)* **`retention-after-project`** — my recordings may be kept
>   until the retention date even after this project's own work concludes, rather
>   than being destroyed when it does.
> - [ ] *(optional)* **`named-attribution`** — I may be named, rather than
>   appearing under my pseudonym, in work derived from this corpus.
>
> Name: ______________________  Date: ____________  Signature: ______________
>
> An unticked box is a refusal and is recorded as one. There is no box that
> defaults to yes.
>
> **The operator will read this page aloud and answer questions before you
> sign.** That is the accessibility provision this project actually has: nine
> people, in one room, with the person who wrote it. There is no audio version —
> see "What is deliberately not automated" — and if reading is not how you want to
> take this in, ask, and it will be explained.
>
> Contact for questions and for withdrawal: **the address in `SECURITY.md`**.

---

## How the project keeps its side

Every obligation above that is not simply "do not do X" is mechanised, because a
promise nobody can check is a promise.

### How each choice is applied, and the shape is the same in every row

The rule is the one M5 established for the participant list and R8 established
for thresholds: **the check is the only constructor of the value the use needs.**
A refusal is not a thing to remember; it is a value that cannot be built.

| Choice | What applies it | Failure mode it removes |
| --- | --- | --- |
| `publication` | `replay::Publishable` is the only value this workspace can write to a publication directory, and `Publishable::of` is its only constructor. It calls `permit::everyone_in`, which refuses unless **every** participant in the match permits it | Publishing a match somebody refused is not a mistake to avoid — the value does not exist. `replay publish` names every withheld match and why |
| `bot-training` | `replay::TrainingSet` is the only value that yields corpus matches for training, and `TrainingSet::of` excludes every match any participant of which refused. `TrainingSet::refusal` answers, by name, why a given match is out | A model fitted on a session whose participant refused is not something to catch in review. A trainer's signature is `fn(…, &TrainingSet)` and a caller holding a `Corpus` and a list of identifiers cannot reach the data |
| `named-attribution` | `Corpus::attribution` is the only path from a pseudonym to a person in this workspace, and it refuses without the permission | The identity mapping cannot be read by a report generator, a credit list or anything else without the permission being checked. **It does not reach a sentence somebody types** — see the tension below |
| `retention-after-project` | `replay conclude <corpus> <date>` destroys, in full, everything belonging to every participant who refused it, and audits each one | A retention promise that lives in a calendar reminder. A refusal here is a withdrawal scheduled on the day it was signed |

The permissions are read **at the moment of use**, off the disk, every time.
Nothing caches. That is what makes a partial withdrawal mechanical rather than a
second bookkeeping problem: revoking a permission is an edit to one file, and the
next publication or training set is computed against the edited one with nothing
to invalidate.

### The age answer is a refusal at the door

`Corpus::store` refuses a match any participant of which has a consent record
saying they are under 18, with `PermissionDenied` and a message naming the human
decision it is standing in for. `replay enrol` refuses to write such a record in
the first place. Two refusals rather than one, because they answer different
questions: the first stops a record written by hand or under an older regime from
admitting a match, the second stops an operator producing a record that could
never be used.

### Withdrawal is a command, and there are two of them

```console
$ replay withdraw <corpus> <pseudonym> <date>
replay: destroyed 3 match(es): 2026-09-03-a, 2026-09-03-b, 2026-09-11-a
replay: pseudonym mapping destroyed, consent record destroyed
replay: no trace of <pseudonym> outside its withdrawal record
```

It deletes, in this order: every match directory the pseudonym appears in, the
pseudonym mapping, and the consent record. Then it writes the one-line tombstone
and **audits itself**. The order matters: if the process dies halfway, what is
left is a corpus with fewer matches and a live consent record, which the next run
repairs — the other order would leave telemetry behind with nothing pointing at
it, which is data nobody knows they are holding.

```console
$ replay withdraw <corpus> <pseudonym> <date> publication
replay: publication withdrawn for <pseudonym> on <date>
replay: participation unchanged — no match destroyed, nothing else touched.
replay: no use of publication reaches <pseudonym> in this corpus
```

The partial one **destroys nothing**, and that is the whole difference rather
than a smaller version of the same operation. A total withdrawal takes back the
*holding* of data and therefore deletes; a partial one takes back a *use* and
therefore must not. Conflating them would mean a participant who no longer wants
their recordings published loses their participation as the price of saying so,
which is precisely the choice this regime exists to stop making on their behalf.

Both are idempotent. A participant who is not sure their first message landed and
sends a second one does not get an error.

### The destruction is verifiable, by something that does not trust it

```console
$ replay audit <corpus> <pseudonym>              # after a total withdrawal
$ replay audit <corpus> <pseudonym> <purpose>    # after a partial one
```

The first exits 0 if nothing remains, non-zero with a list of paths if anything
does. It reads **every byte of every file** under the corpus root and looks for
the pseudonym, rather than checking the places the pseudonym is supposed to be. A
cleverer check would be blind in exactly the place a bug would put it — a
temporary file, a backup, a directory a later milestone added and the check was
never told about.

The second covers the partial withdrawal and is deliberately **not** a check that
the consent record was edited: reading back the file just written would be the
command agreeing with itself. It runs the *use's own gate* over the matches the
participant is in and lists every one the use would still reach. An empty answer
is the only acceptable outcome, and it is the analogue of the first command's
empty list for a withdrawal that destroys nothing.

`replay/tests/withdrawal.rs` and `replay/tests/permissions.rs` exercise both by
breaking them: a `withdraw` that forgets the pseudonym mapping or one match, a
revocation that edits the record and leaves a match publishable, a training set
built over a refuser, and a match published for somebody who said no.

### There is no derived index for a withdrawal to miss

The way a destruction promise fails is not a match directory somebody forgot to
unlink — `withdraw` removes those and the audit checks. It is a *derived* artefact
that outlives what it was derived from: a summary, a cache, a list of who played
what.

Until M5 this corpus had exactly one. `store` took a participant list and wrote it
into a `participants` file beside the recording, because a recording named seats
and not people and there was nowhere else to put it. That file was an index in
every sense that matters: derived from what an operator passed in, able to drift
from the recording it sat next to, and able to be deleted while the telemetry it
pointed at survived.

A sealed replay carries its participants **inside the signature**, so the index
has no reason to exist and it is gone. `Corpus::participants_of` reads the
manifest; there is one place a pseudonym is written and one thing to delete. What
guards against a future one is the audit's crudeness rather than a rule anybody
has to remember: it reads every byte of every file under the root, so an index
added quietly is reported the first time somebody withdraws, and
`replay/tests/withdrawal.rs` plants one to prove it.

**A trained model is the next artefact of that shape, and the rule is written
before the artefact exists.** `bot-training` is the one permission whose use
produces something durable, and a model is exactly a derived artefact that can
outlive what it derived from. So: a model trained on this corpus carries the
provenance `TrainingSet::provenance` produces — the consent version, the matches
and the **pseudonyms** it learned from — and is stored under the corpus root
beside it. That makes it reachable by the machinery that already works: the audit
reads every byte for a name, so a model whose provenance names a withdrawn
participant is reported the first time they withdraw, exactly as a planted index
is. `replay/tests/permissions.rs` plants one to prove it.

### The corpus and the signing key cannot be committed by accident

`.gitignore` refuses `corpus/`, `*.replay`, `*.consent`, `*.identity` and
`*.signing-key`, and `ci` fails a pull request that tracks any of them.
`docs/RISKS.md` R3 is about an irreversibility git makes literal: a recording
committed once is in the history and in every fork, and deleting the file does not
delete it. The signing key is refused for a second reason (`docs/RISKS.md` R4):
whoever holds it can seal a replay this project's own verifier accepts.

The **public** key is deliberately not refused, and that is a decision rather than
an oversight. R4 requires every key, including every retired one, to stay
published — a retired key that stops being published orphans every replay it ever
sealed, which would be a way of destroying evidence by housekeeping.

### A match nobody consented to cannot be stored

`Corpus::store` refuses a replay naming a participant with no consent record,
rather than accepting it and leaving the check for whoever operates the corpus at
M6. Since M5 it reads the names out of the replay's own manifest rather than being
told them, so the check is against what the match actually says it was. Consent is
a person-to-person act — `docs/ENGINEERING.md` lists admitting a participant among
the things that stay manual — and this is the part of it a program can hold.

## The tensions, named rather than resolved in silence

Five, each with where this document landed and what that cost.

**UX against exhaustiveness.** Progressive disclosure is a real risk: a summary
people actually read can become the summary that is *all* they read, and every
sentence moved down a level is a sentence somebody will not see. The position
taken here is that **L0 is complete** — every consequence that could reasonably
change an answer is in it, and the levels below deepen rather than reveal. The
cost is that L0 is longer than a summary wants to be, and it is longer on purpose:
a shorter one would have had to leave out the device stream's distinctiveness or
what full withdrawal does to eight other people, and either omission would make
the layering a trick.

**Granularity against scientific necessity.** Making the movement stream a box
would be the maximally granular form and it would be dishonest: refusing it
produces a recording nobody can use for the stated purpose, so the box has two
outcomes and they are "take part" and "do not". Stating that as necessity costs
this document the appearance of maximal choice, and a reader who counts boxes will
count four where a more generous-looking form would show ten. The four that exist
are the four a refusal survives.

**Simplicity against the validity of the consent.** Law 25 wants consent given
"for specific purposes" and "requested separately"; those pull towards more boxes,
while comprehension pulls towards fewer. Four is the answer here and it is a
judgement, not a derivation. The line drawn is *purposes and retention terms get a
box, technical fields do not*, on the ground that nobody has an interest in
refusing `seq` while accepting `tick`. Whether a regulator would draw it in the
same place is one of the points below.

**Accessibility against precision.** This page is long and it is written in
plain words, which are two things that fight: "every movement your mouse reports"
is comprehensible and "every relative motion event delivered by the platform's
input stack" is exact. The document chooses comprehension in the body and puts
the exact field names in the tables, so the precise version is present and is not
what a participant has to parse. The cost is that the tables are the part nobody
reads, and the demonstration in L3 is the compensation: it is precise *and*
immediate, because it is their own data.

**Human consent against technical enforcement.** The four boxes are enforced by
gates that cannot be bypassed, and one of them — `named-attribution` — is only
half a gate. The corpus refuses to hand out a name; nothing refuses a person who
remembers one and types it into a report. That is stated to the participant rather
than implied away, and it is the one participant choice in this regime kept by a
promise as well as by a control. The general form is worth writing down: **a
mechanism can refuse a use of the data, and cannot refuse a use of what somebody
already knows.**

## What is sent to a human review

This document is engineering, and the following are places where it takes a
position it is not qualified to settle. They are listed rather than decided with
confidence, which is the only honest treatment available to a solo project.

1. **Whether four boxes is the right granularity under Law 25.** The Act asks for
   consent "for specific purposes", "requested separately", and the line drawn
   here — purposes and retention terms get a box, technical fields do not — is a
   judgement.
2. **Whether the L3 demonstration's ordering is permissible.** It captures data a
   minute before the signature it informs. The terms are narrow and the data never
   reaches the corpus, but "collected before consent" is the shape of the thing
   the Act is about, and a reviewer may think the demonstration should use a
   previous participant's stream with their permission instead.
3. **Whether "participation is not refusable in parts" survives scrutiny.** This
   document argues the necessity is genuine. A reviewer might read the single
   participation box as bundled consent, which is exactly what granularity rules
   exist to prevent.
4. **Whether a match may be recorded at all when one of nine withdraws.** Full
   withdrawal destroys eight other people's contributions without asking them.
   They agreed to that in advance, in writing, on this page — and whether
   agreeing in advance to somebody else's future decision is a consent a
   regulator would recognise is not a question this document can answer.
5. **Whether the 24-month retention is justifiable** against a project whose own
   work concludes earlier. `retention-after-project` exists precisely because the
   answer is arguable; the box makes it the participant's arguable question rather
   than the project's.
6. **Whether refusing `bot-training` is meaningfully enforceable in the long
   run.** The gate is real for training; the destruction of a trained model on
   withdrawal is a rule this document states and the audit reaches only because a
   model is required to carry its provenance. Nothing forces a future model to be
   stored where the audit looks.
7. **Whether the age question is sufficient.** A tick box is a self-declaration.
   The project's answer is that nine supervised participants known personally to
   the operator is a stronger check than any form, which is true here and would
   not be true of a corpus that opened up.

## What is deliberately not automated

**Signing the text.** It is a conversation with a person, it happens before
anything is recorded, and a form that collects a tick box is not the thing
`docs/MILESTONES.md` M4 asks for. The signed texts are kept with the corpus,
outside the repository, and the consent record in the corpus is the machine's
note that one exists.

**An audio version of this document.** There is none, and that is a decision.
Nine participants, supervised, in the same room as the operator: what an audio
version would buy — understanding this without reading it — is already available
from the person who wrote it, and the page says so where a participant will meet
it. What it would cost is a synthesis pipeline, a second thing to version, a
standing risk that the text and the audio disagree about what is collected, and an
accessibility claim this project would then have to keep. The trade is only
obviously right *because* of the supervision: **if this corpus ever opened to
participants recording on their own, an audio version and a proper accessibility
review are the first things it would need**, and this paragraph is the note to
whoever reaches that point.

**Deleting the signed paper.** No command reaches it and none should: it is the
one artefact in this regime a person has to destroy deliberately.
