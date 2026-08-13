# CONSENT

The text every participant reads and agrees to **in writing, before the first
recording**, and the rules this project holds itself to afterwards.

`docs/RISKS.md` R3 is the reasoning; this is the instrument. `docs/MILESTONES.md`
M4 makes its existence part of an exit criterion, because M4's own criterion —
three people playing a match — is already a collection of personal information
and there is no later milestone at which writing this down is still in time.

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
> For every match you play in, the recording holds:
>
> | Field | What it is |
> | --- | --- |
> | `seed` | The number the match's world was generated from |
> | `rules_hash` | A fingerprint of the game's constants, so the match can be replayed correctly later |
> | `ticks` | How many thirtieths of a second the match lasted |
> | `final_state_digest` | A fingerprint of how the match ended |
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
> Beside the recordings, the project holds a **consent record** for you — your
> pseudonym, the date you consented, the date your data is destroyed, and
> whether you agreed to publication — and a **pseudonym mapping**, which is the
> one file that connects your pseudonym to you.
>
> **What is not collected:** no audio, no video, no screen capture, no chat, no
> IP address in the corpus, no hardware identifiers, no files from your machine,
> and nothing at all outside the match.
>
> **This is still information about you.** Replacing your name with an opaque
> identifier is a security measure, not a change of category: input timing is
> distinctive, and "the mapping is in another file" would be a thin claim. The
> project treats this as personal information throughout.
>
> **3. How long it is kept, and what triggers deletion.**
>
> Raw telemetry, the recordings containing it, and the pseudonym mapping are
> destroyed **24 months after the recording**, or when you withdraw, whichever
> comes first. The date is written into your consent record when you sign it.
>
> Statistics that identify nobody — distributions, counts, thresholds and the
> numbers in the project's documents — are kept without a time limit. This is
> said plainly rather than left for you to infer that everything disappears.
>
> **4. Withdrawing.**
>
> You may withdraw at any time, without giving a reason and without any
> consequence, by a single message to the contact address at the end of this
> text. You do not need to re-consent to anything in order to withdraw, and you
> will not be asked why.
>
> Your withdrawal is acknowledged within **7 days** and carried out within
> **30**.
>
> **5. What withdrawing actually destroys — please read this one before you
> sign.**
>
> A match is a single interleaved log of nine players' inputs. Removing one
> person's inputs leaves a log that no longer replays, so removing only your part
> is not something this project can offer.
>
> Withdrawing therefore destroys **every match you played in, in full**, together
> with your pseudonym mapping and your consent record. That includes the other
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
> **6. Who has access, and where the data lives.**
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

### The corpus cannot be committed by accident

`.gitignore` refuses `corpus/`, `*.replay`, `*.consent` and `*.identity`, and
`ci` fails a pull request that tracks any of them. `docs/RISKS.md` R3 is about an
irreversibility git makes literal: a recording committed once is in the history
and in every fork, and deleting the file does not delete it.

### A match nobody consented to cannot be stored

`Corpus::store` refuses a recording naming a participant with no consent record,
rather than accepting it and leaving the check for whoever operates the corpus at
M6. Consent is a person-to-person act — `docs/ENGINEERING.md` lists admitting a
participant among the things that stay manual — and this is the one part of it
that a program can hold.

## What is deliberately not automated

Signing the text. It is a conversation with a person, it happens before anything
is recorded, and a form that collects a tick box is not the thing
`docs/MILESTONES.md` M4 asks for. The signed texts are kept with the corpus,
outside the repository, and the consent record in the corpus is the machine's
note that one exists.
