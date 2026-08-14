# CONSENT

consent-version: 2026-08-15

The text every participant reads and agrees to **in writing, before the first
recording**, and the rules this project holds itself to afterwards.

`docs/RISKS.md` R3 is the reasoning; this is the instrument. `docs/MILESTONES.md`
M4 makes its existence part of an exit criterion, because M4's own criterion —
three people playing a match — is already a collection of personal information
and there is no later milestone at which writing this down is still in time.

## The version line above, and what it is for

`consent-version` is the identifier of *this* text, and it is the one thing on
this page a program reads. A signature on paper is a fact about a document on a
day; a consent text that later gains a field — a new covariate, a new retention
rule, a new purpose — has stopped being the document somebody signed, and a
corpus of replays would say nothing whatever about the difference.

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

The date is the day this text takes effect rather than the day it was written,
and the two differ by one here because the version before it was written on the
day it was superseded and a version has to move strictly forward. No session has
been recorded under either, so nothing turns on the difference; it is recorded
because a reader who notices a document dated after its own commit is entitled
to an explanation.

### What changed in this version, and why it needed one

**§2b is new: the corpus now keeps the whole device stream**, at the mouse's own
report rate, and not only the one instruction per tick the previous version
described. That is a materially larger collection — `docs/SCHEMA.md` §11 has the
field list and the arithmetic — and the previous text's own promise was that if
the finer stream were ever needed it would be asked for separately and this text
would say so *before* it was collected. Raising the version is what makes that
promise mechanical rather than remembered: `Corpus::store` refuses a match whose
participants consented under `2026-08-14`, so agreement to the older text cannot
be carried forward by the silence of a file.

**It is not a second opt-in, and that is a decision rather than an omission.**
The publication box exists because it is genuinely refusable: a participant can
refuse it and everything else about their participation is unchanged. A box for
the device stream would not be, in either direction. Refusing it leaves a
recording the declared purpose cannot use, so the choice is between taking part
and not — and offering a tick box for a choice that is really "participate or
do not" is the kind of handling this project criticises elsewhere. So §2b says
in the participant's own words what refusing means, and there is one signature
rather than two.

This document is engineering, not legal advice. Whether Quebec's *Act respecting
the protection of personal information in the private sector* (Law 25) formally
binds a non-commercial hobby project is genuinely unsettled, and the position
here is that it does not matter: the project holds itself to the regime
regardless, because the cost is a page of text and the alternative is a security
portfolio that collects behavioural biometrics from friends with no stated rules.

---

## The consent text

> **What this is.** You are being asked to play a match of an experimental game
> and to let the recording of that match be kept. The project is an anti-cheat
> engineering exercise; the game exists to produce data to test the anti-cheat
> against.
>
> **1. What the data is used for, and nothing else.**
>
> The recordings are used to calibrate and evaluate this project's behavioural
> cheat detectors, and to publish statistics derived from that work. That is the
> only declared purpose and the data will not be used for another one.
>
> Specifically, it is **not** used to verify or confirm anyone's identity, it is
> **not** transferred to any third party, it is **not** reused by another
> project, and it is **not** used to train anything that outlives this
> repository.
>
> Publishing the **raw** recordings — as opposed to statistics computed from
> them — is a separate purpose with its own separate box below. You can refuse
> that and still take part in everything else. Refusing it changes nothing about
> your participation.
>
> **2. What is collected, field by field.**
>
> For every match you play in, the replay file holds:
>
> | Field | What it is |
> | --- | --- |
> | `match_id` | A number that tells this match apart from another one |
> | `seed` | The number the match's world was generated from |
> | `rules_hash` | A fingerprint of the game's constants, so the match can be replayed correctly later |
> | `sim_version`, `sim_commit` | Which build of the game resolved the match, so that a replay that no longer replays can be told from a replay that was edited |
> | `started_at_unix_ms` | When the match began. This is what the destruction date below is counted from |
> | `participants` | **Your pseudonym**, and those of the other people in the match, one per seat. Not your name — see the mapping below |
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
>
> **2b. Every movement of your hand, hundreds of times a second.**
>
> This is the part to read slowly, because it is the largest thing on this page
> and it was not collected before this version of the text.
>
> Above, the replay records the one instruction the game acted on in each
> thirtieth of a second. Your mouse reports far more often than that — between
> **125 and 1000 times a second**, depending on the mouse — and the game on your
> machine reads all of it. **All of it is now kept**, in a second file beside the
> replay, one section per seat:
>
> | Recorded | What it is |
> | --- | --- |
> | Every movement your mouse reports | How far it moved right and how far it moved down, in the mouse's own units, exactly as your computer reported it — not rounded, not smoothed, not converted |
> | The moment of each one | Measured by a stopwatch inside the game, started when the game started. Not the time of day, and not comparable to anybody else's |
> | Every press and release | Of the five controls the game uses — left click, right click, `Q`, `W`, `S` — including presses that did nothing |
> | Every frame that arrived | Which thirtieth of a second it was for, and which instruction you sent back. Thirty a second. This is the only line in the file that is not your hand: it is what lets the project tell *when you were shown something* from *when you answered*, and without it nothing about your reaction time can be measured at all |
>
> **What can be worked out from this.** The shape and speed of the way you move
> a mouse: how you accelerate, where you overshoot and correct, how steady your
> rhythm is, how long you take to react to something appearing on screen. Taken
> together, that is **distinctive** — closer to handwriting than to a
> preference. Somebody holding this file and a second recording of you could
> plausibly tell that the two are the same person. That is precisely why the
> project wants it: telling a person from a program is the whole subject, and
> this is the data that difference lives in.
>
> **What is not in it.** No key you pressed outside those five. No text, ever.
> No screen capture, no image, no sound. Nothing about where the mouse pointer
> is on your desktop, nothing about the size of your window or your monitor,
> nothing from any other program, nothing from before the match or after it. The
> file records the movement, and only during the match.
>
> **If you would rather this were not recorded, the honest answer is not to take
> part.** This is not offered as a separate box you can untick, and pretending
> otherwise would be dishonest: the project's declared purpose is calibrating
> detectors that read exactly this, and a recording without it would not be
> usable for the thing you are being asked to help with. It is one refusal and
> it refuses everything, which is your right and costs you nothing. (The
> publication box at the end is different, and genuinely optional.)
>
> **Why this is a new version of this document.** An earlier version of this text
> said that only one instruction per thirtieth of a second was kept, and promised
> that if the finer stream were ever needed it would be asked for separately and
> this text would say so before it was collected. This is that. Nobody's earlier
> agreement carries over: the project's own tooling refuses to store any match
> whose participants signed a different version of this page, so consent to the
> old text is not consent to this one and cannot be mistaken for it.
>
> **3. What is recorded about the equipment you played on.**
>
> Beside each match the project keeps one more file, describing the **seat**
> rather than the person: there is no name and no pseudonym in it, and what
> connects a seat to you is the replay above. It exists because a mouse set to
> 400 counts per inch and one set to 1600 describe the same hand differently, and
> without knowing which you had, a difference of equipment would be read as a
> difference in how you play.
>
> Three things you are **asked**, because no program can read them:
>
> | Field | What it is |
> | --- | --- |
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
> **Nothing here identifies a device.** No model, no serial number, no
> manufacturer, no operating-system version, no machine name. A number is not a
> fingerprint of a mouse.
>
> The same file also records **how the session was supervised** — whether the
> person running it was in the room with you, on a call with you, or not present
> at all. It is one value for the whole session, written down by the operator, and
> it is about the session rather than about any one of you.
>
> It is there for a reason worth stating plainly, because it is the one place this
> project relies on a person rather than on a program. Nothing in a recorded file
> can tell a person playing from a program moving a real mouse: the two produce
> the same kind of record, and this project says so in its own documents rather
> than implying otherwise. What tells them apart is that somebody was there. So the
> project writes down whether somebody was, instead of remembering it — and when it
> later publishes a number about how people play, it can say which sessions that
> number rests on. If nobody was watching, the file says nobody was watching.
>
> **4. What else the project holds.**
>
> A **consent record** for you — your pseudonym, the date you consented, the
> version of *this text* you signed, the date your data is destroyed, and whether
> you agreed to publication — and a **pseudonym mapping**, which is the one file
> that connects your pseudonym to you.
>
> **What is not collected:** no audio, no video, no screen capture, no chat, no
> IP address in the corpus, no hardware identifiers, no device models or serial
> numbers, no operating-system versions, no files from your machine, no keys
> outside the five the game uses, and nothing at all outside the match.
>
> **This is still information about you.** Replacing your name with an opaque
> identifier is a security measure, not a change of category: input timing is
> distinctive, and "the mapping is in another file" would be a thin claim. The
> project treats this as personal information throughout.
>
> **5. How long it is kept, and what triggers deletion.**
>
> Raw telemetry — including the movement file in 2b, which is the largest part
> of it — the recordings containing it, and the pseudonym mapping are destroyed
> **24 months after the recording**, or when you withdraw, whichever comes first.
> The date is written into your consent record when you sign it.
>
> Statistics that identify nobody — distributions, counts, thresholds and the
> numbers in the project's documents — are kept without a time limit. This is
> said plainly rather than left for you to infer that everything disappears.
>
> **6. Withdrawing.**
>
> You may withdraw at any time, without giving a reason and without any
> consequence, by a single message to the contact address at the end of this
> text. You do not need to re-consent to anything in order to withdraw, and you
> will not be asked why.
>
> Your withdrawal is acknowledged within **7 days** and carried out within
> **30**.
>
> **7. What withdrawing actually destroys — please read this one before you
> sign.**
>
> A match is a single interleaved log of nine players' inputs. Removing one
> person's inputs leaves a log that no longer replays, so removing only your part
> is not something this project can offer.
>
> Withdrawing therefore destroys **every match you played in, in full** — the
> replay, the equipment record, and the movement file in 2b — together with your
> pseudonym mapping and your consent record. That includes the other
> eight players' contributions to those matches. They are not asked and are not
> notified.
>
> Statistics already published are not retracted. They identify nobody, they are
> already in documents and pull requests that are public, and unpublishing them
> is not something anyone can actually do.
>
> What survives is a single line recording that a pseudonym withdrew and on what
> date. It contains nothing else, and because the mapping is destroyed in the
> same operation, it names nobody.
>
> **8. Who has access, and where the data lives.**
>
> One person: the author of the project, Vianney Veremme, who is the only
> operator and the only administrator.
>
> The recordings live on the author's own machine, in a directory outside the
> git repository, and are never committed — deleting a committed file does not
> delete it, so the repository's `.gitignore` refuses these paths and CI fails a
> pull request that tracks one. There is no cloud service, no hosted database,
> no analytics provider and no backup off that machine.
>
> If the raw corpus is ever published, only the recordings of participants who
> ticked the separate box below are included, and that decision is taken once,
> at `docs/MILESTONES.md` M6, and not revisited per match.
>
> **What you are agreeing to** — two separate things, and the second is
> optional:
>
> - [ ] My matches may be recorded and used to calibrate and evaluate this
>   project's cheat detectors, and statistics derived from them may be published.
> - [ ] *(optional, refusable)* The raw recordings of my matches may be published
>   as part of an open data set.
>
> Name: ______________________  Date: ____________  Signature: ______________
>
> Contact for questions and for withdrawal: **the address in `SECURITY.md`**.

---

## How the project keeps its side

The three obligations above that are not simply "do not do X" are mechanised,
because a promise nobody can check is a promise.

### Withdrawal is a command

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

It is idempotent. A participant who is not sure their first message landed and
sends a second one does not get an error.

### The destruction is verifiable, by something that does not trust it

```console
$ replay audit <corpus> <pseudonym>
```

Exit status 0 if nothing remains, non-zero with a list of paths if anything does.
It reads **every byte of every file** under the corpus root and looks for the
pseudonym, rather than checking the places the pseudonym is supposed to be. A
cleverer check would be blind in exactly the place a bug would put it — a
temporary file, a backup, a directory a later milestone added and the check was
never told about.

`replay/tests/withdrawal.rs` exercises it by breaking it: a `withdraw` that
forgets the pseudonym mapping, or forgets one match, is caught by the audit and
named in the failure.

### There is no derived index for a withdrawal to miss

The way a destruction promise fails is not a match directory somebody forgot to
unlink — `withdraw` removes those and the audit checks. It is a *derived*
artefact that outlives what it was derived from: a summary, a cache, a list of
who played what.

Until M5 this corpus had exactly one. `store` took a participant list and wrote
it into a `participants` file beside the recording, because a recording named
seats and not people and there was nowhere else to put it. That file was an index
in every sense that matters: derived from what an operator passed in, able to
drift from the recording it sat next to, and able to be deleted while the
telemetry it pointed at survived.

A sealed replay carries its participants **inside the signature**, so the index
has no reason to exist and it is gone. `Corpus::participants_of` reads the
manifest; there is one place a pseudonym is written and one thing to delete. What
guards against a future one is the audit's crudeness rather than a rule anybody
has to remember: it reads every byte of every file under the root, so an index
added quietly is reported the first time somebody withdraws, and
`replay/tests/withdrawal.rs` plants one to prove it.

### The corpus and the signing key cannot be committed by accident

`.gitignore` refuses `corpus/`, `*.replay`, `*.consent`, `*.identity` and
`*.signing-key`, and `ci` fails a pull request that tracks any of them.
`docs/RISKS.md` R3 is about an irreversibility git makes literal: a recording
committed once is in the history and in every fork, and deleting the file does
not delete it. The signing key is refused for a second reason (`docs/RISKS.md`
R4): whoever holds it can seal a replay this project's own verifier accepts.

The **public** key is deliberately not refused, and that is a decision rather
than an oversight. R4 requires every key, including every retired one, to stay
published — a retired key that stops being published orphans every replay it ever
sealed, which would be a way of destroying evidence by housekeeping.

### A match nobody consented to cannot be stored

`Corpus::store` refuses a replay naming a participant with no consent record,
rather than accepting it and leaving the check for whoever operates the corpus at
M6. Since M5 it reads the names out of the replay's own manifest rather than
being told them, so the check is against what the match actually says it was.
Consent is a person-to-person act — `docs/ENGINEERING.md` lists admitting a
participant among the things that stay manual — and this is the one part of it
that a program can hold.

## What is deliberately not automated

Signing the text. It is a conversation with a person, it happens before anything
is recorded, and a form that collects a tick box is not the thing
`docs/MILESTONES.md` M4 asks for. The signed texts are kept with the corpus,
outside the repository, and the consent record in the corpus is the machine's
note that one exists.
