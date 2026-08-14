//! The attacker.
//!
//! This crate is a first-class deliverable and not a test utility
//! (`docs/SCOPE.md`). A defence in this project is only *delivered* once the
//! matching exploit exists here and fails against it in CI, so what lives in this
//! crate is the other half of every claim the rest of the repository makes.
//!
//! # What it is, and what it deliberately cannot be
//!
//! It speaks this project's protocol and nothing else. There is no memory
//! scanner, no injector, no hooking library, no signature scanner and no
//! dependency on any other game; there is nothing generic in it to point
//! somewhere else. `SECURITY.md` states that as policy and this crate's
//! dependency list is what makes it true: one normal dependency on
//! [`protocol`], plus a signature library, because
//! [`forge`] has to sign.
//!
//! `docs/RISKS.md` R7 is the entry this discharges, and it was decided before
//! the first line of it was pushed: a public repository containing working
//! exploit code cannot be un-published, so the boundary is set by what is here
//! on day one rather than by a later README edit. Every exploit below is
//! expressed as an assertion — *the attacker learns X*, *the server accepts Y* —
//! and the exploit suite in `tests/` is where those assertions are made.
//!
//! # The attacker does not link the victim
//!
//! `docs/ARCHITECTURE.md`: an exploit that reaches into the real client's
//! internals is not an exploit, it is a test double. So nothing under `src/` can
//! name a `State`, call `view_for`, run `step`, or ask `replay` what it would
//! accept. What the attacker knows is what the wire told it, plus the published
//! layout of a replay file, which [`forge`] writes out by hand.
//!
//! The harness in `tests/harness` is on the other side of that line and links
//! `sim`, `server` and `replay` as dev-dependencies — because an exploit that
//! asserts *the attacker did not learn where Red0 was* is an assertion about
//! where Red0 actually was, and only the world holds that. **The judge needs the
//! truth; the attacker must not have it**, and the two are different crates'
//! dependency lists rather than a rule somebody follows.
//!
//! # Every exploit is run twice, and that is the design
//!
//! `docs/MILESTONES.md` M7 asks for a suite that passes on the real build and
//! fails on a deliberately weakened one. It is not two builds here, it is two
//! *servers in one test*: each exploit is a function from what an attacker
//! observes to what an attacker concludes, and it is run against a weakened
//! projection or transport that does not stop it, and then against the one this
//! project ships.
//!
//! **Both halves are assertions and the test is red if either fails.** The
//! second half is the defence. The first half is `docs/RISKS.md` R15 applied to
//! attacks: an exploit that fails against the real defence *without ever having
//! worked* is an exploit whose antecedent is never reached, and it proves nothing
//! about the defence — it looks exactly like a defence that holds, and there is
//! no red to tell them apart. Four times in this project a test has been green
//! because the condition it was about never occurred; an exploit suite is where
//! that failure would be cheapest to commit and most expensive to discover.
//!
//! Why not a Cargo feature, which is what M7's plan named: features are additive
//! and unified, so `no-culling` on `sim` is a switch any crate in the graph can
//! turn on for the server binary too. `docs/ARCHITECTURE.md` refuses exactly that
//! shape for a `Serialize` impl, and refusing it there while adding it for the
//! culling would be putting the switch on the more dangerous of the two.
//! `docs/MILESTONES.md` M7 records the substitution.
//!
//! # The classes, and where each one lives
//!
//! | Class | Exploit | Module |
//! | --- | --- | --- |
//! | 1, maphack | what a view tells an attacker | [`maphack`] |
//! | 1, maphack | what the packet stream tells an attacker who cannot read it | [`traffic`] |
//! | 2, result forgery | a replay of a match nobody played | [`forge`] |
//! | 3, synthetic input | a bot that plays the protocol | [`bot`] |
//! | 4, time manipulation | a client that lies about its clock | [`bot`] |
//! | 5, protocol abuse | frames a client is not allowed to send | [`abuse`] |
//! | 6, cross-team collusion | **no exploit, deliberately** — see below | — |
//!
//! Class 6 has no module and no attack. Two teams that put their entitled views
//! together on a voice call obtain a map neither could obtain from the protocol,
//! and every frame involved is correctly culled, every message is one the server
//! intended to send, and there is no invalid input for anything to reject. There
//! is nothing here for an attacker to *send*, so writing something that looked
//! like one would be manufacturing an antecedent in order to have a row in a
//! table. `tests/collusion.rs` executes the *statement* instead — that the union
//! of two entitled views is strictly larger than either and that nothing was
//! violated to obtain it — and it is labelled as a demonstration rather than as
//! an exploit.

#![forbid(unsafe_code)]
#![deny(missing_docs, missing_debug_implementations)]

pub mod abuse;
pub mod bot;
pub mod forge;
pub mod maphack;
pub mod traffic;
