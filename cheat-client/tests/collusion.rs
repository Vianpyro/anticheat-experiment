//! Class 6: cross-team collusion. **No exploit, deliberately — a demonstration.**
//!
//! `docs/SCOPE.md` is emphatic and this file does not try to be cleverer than it:
//! **no technical defence applies, and this project will not pretend one does.**
//! Two of the three teams put their entitled views together on a voice call and
//! obtain a map neither could get from the protocol. Every frame involved is
//! correctly culled, every message is one the server intended to send, and there
//! is no invalid input, no malformed frame, and no impossible action for the
//! server to reject — because nothing invalid happens. The leak is two people
//! talking, outside the system.
//!
//! So there is no attacker here and no frame to send. Writing something that
//! *looked* like an exploit — a fabricated protocol violation, a contrived
//! rejection — would be manufacturing an antecedent to fill a row in a table,
//! which is exactly the `docs/RISKS.md` R15 failure the rest of this crate is
//! built to avoid, wearing the costume of thoroughness. `docs/MILESTONES.md` M7
//! is explicit: *ne fabrique pas d'exploit artificiel pour cocher une case.*
//!
//! What this file does instead is execute the *statement* `docs/SCOPE.md` makes,
//! so that the claim "the union of two entitled views exceeds either, and nothing
//! was violated to obtain it" is a thing that ran rather than a paragraph. It is
//! labelled a demonstration, not an exploit, and it asserts a fact about the game,
//! not a defence.

#![deny(unsafe_code)]

use protocol::EntityView;
use sim::view::view_for;
use sim::{Action, RULES, Seat, base_position};

#[path = "harness/authority.rs"]
mod authority;
#[path = "harness/entitlement.rs"]
mod entitlement;

use authority::started_match;
use entitlement::team_can_see;

/// Two enemy teams share their entitled vision and see more than either could,
/// while every frame remains correctly culled and no rule is broken.
#[test]
fn two_teams_pooling_entitled_vision_see_more_than_either_and_break_nothing() {
    // A state with real fog: Blue0 walks toward Red's base, so at the end Red can
    // see it and Green — a lane away — cannot. Neither Red nor Green alone sees the
    // whole board; together they see strictly more.
    let mut game = started_match(0x0F1E_2D3C_4B5A_6978, 9);
    let target = base_position(Seat::Red0.team(), &RULES);
    let frame = protocol::ClientFrame::encode(&protocol::ClientMessage::Input {
        seq: 0,
        claimed_at_ms: 0,
        action: Action::Move(target),
    });
    game.deliver(Seat::Blue0, frame.as_bytes().as_slice(), 0)
        .expect("the move was accepted");
    for _ in 0..820 {
        let _ = game.tick();
    }
    let state = game.world();

    // The two colluding teams are Red and Green. Each takes the view it is
    // entitled to — through the real, culling projection, with nothing bypassed.
    let red_view = view_for(state, Seat::Red0);
    let green_view = view_for(state, Seat::Green0);

    let handles = |view: &sim::view::PlayerView| -> std::collections::BTreeSet<u16> {
        view.visible
            .iter()
            .map(|entity| match entity {
                EntityView::Champion { id, .. }
                | EntityView::Tower { id, .. }
                | EntityView::Projectile { id, .. } => id.0,
            })
            .collect()
    };
    let red_sees = handles(&red_view);
    let green_sees = handles(&green_view);
    let together: std::collections::BTreeSet<u16> = red_sees.union(&green_sees).copied().collect();

    // R15: the two views actually differ, or "the union exceeds either" is a
    // statement about two identical sets and the collusion is about nothing.
    let red_only = red_sees.difference(&green_sees).count();
    let green_only = green_sees.difference(&red_sees).count();
    println!(
        "collusion: Red sees {} entities, Green {}, together {} (Red-only {red_only}, \
         Green-only {green_only})",
        red_sees.len(),
        green_sees.len(),
        together.len()
    );
    assert!(
        red_only > 0 && green_only > 0,
        "the two teams saw the same entities, so pooling gained nothing and this \
         demonstration is about nothing (docs/RISKS.md R15)"
    );

    // The point: the union is strictly larger than either team's entitlement. Two
    // people on a call now know something neither client was told.
    assert!(
        together.len() > red_sees.len() && together.len() > green_sees.len(),
        "the pooled map was no larger than one team's, so there is nothing to \
         demonstrate"
    );

    // And nothing was violated to get it. Each view is exactly what the culling
    // projection produces — every handle in it is accompanied by a point inside
    // that team's vision — so there is no frame here a server would reject and no
    // rule that was broken. The leak is entirely outside the protocol.
    for (view, team) in [(&red_view, Seat::Red0), (&green_view, Seat::Green0)] {
        for entity in &view.visible {
            let position = match entity {
                EntityView::Champion { position, .. }
                | EntityView::Tower { position, .. }
                | EntityView::Projectile { position, .. } => *position,
            };
            assert!(
                team_can_see(state, team.team(), position),
                "a view contained an entity outside its team's vision, which would \
                 be a culling failure — a class-1 leak, not the class-6 one this \
                 demonstrates"
            );
        }
    }

    println!(
        "collusion: the pooled view is strictly larger than either team's, and every \
         frame that produced it was correctly culled — no technical defence applies \
         (docs/SCOPE.md class 6)"
    );
}
