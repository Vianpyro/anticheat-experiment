//! Exploit class 1: the maphack.
//!
//! `docs/MILESTONES.md` M7: *maphack against a build with culling disabled, to
//! prove the exploit is real and the culling is what stops it.* This file is that
//! sentence made executable. One world, two projections: the attacker is run
//! against a server that culls nothing and places every living enemy, and then
//! against the projection this project ships, unchanged, and it places only what
//! the fog already showed.
//!
//! The weakened projection is `vision::omniscient` rather than a Cargo feature,
//! for the reason `cheat-client/src/lib.rs` records: a feature on `sim` is a
//! switch any crate in the graph can throw for the server binary too, and
//! `docs/ARCHITECTURE.md` refuses that shape for the serialization impl for the
//! same reason.

#![deny(unsafe_code)]

use cheat_client::maphack::Maphack;
use protocol::{EntityView, ServerFrame, ServerMessage};
use sim::view::PlayerView;
use sim::{Action, Fx, FxVec2, RULES, Seat, base_position};

#[path = "harness/authority.rs"]
mod authority;
#[path = "harness/entitlement.rs"]
mod entitlement;
#[path = "harness/vision.rs"]
mod vision;

use authority::started_match;
use vision::legitimately_visible_enemies;

/// The seat the attacker sits in.
const ATTACKER: Seat = Seat::Blue0;

/// Drives a match to a state in which the attacker can see some enemies and not
/// others, which is the antecedent both halves of the exploit need
/// (`docs/RISKS.md` R15). Returns the world at that tick.
///
/// Blue0 walks the length of its lane to Red's base while the other eight seats
/// hold position. Long before it arrives it is inside the Red cluster's vision
/// and the three Reds are in its; the three Greens, a full lane away at the third
/// vertex, never are. So the state it stops on has three enemies the fog reveals
/// and three the fog hides — not zero of either, which a match that never left
/// its base would give and which would make the exploit's success and the
/// defence's success indistinguishable from doing nothing.
fn a_state_with_some_enemies_hidden() -> sim::State {
    let mut game = started_match(0x0F1E_2D3C_4B5A_6978, 9);

    // Blue0 to Red's base. A standing order, so it is sent once.
    let target = base_position(Seat::Red0.team(), &RULES);
    let frame = protocol::ClientFrame::encode(&protocol::ClientMessage::Input {
        seq: 0,
        claimed_at_ms: 0,
        action: Action::Move(target),
    });
    game.deliver(ATTACKER, frame.as_bytes().as_slice(), 0)
        .expect("the move was accepted");

    for _ in 0..900 {
        let _ = game.tick();
    }
    game.world().clone()
}

/// The exploit, both halves, over one world.
#[test]
fn a_maphack_reads_a_leaking_view_and_is_blind_against_a_culling_one() {
    let state = a_state_with_some_enemies_hidden();

    // R15: the antecedent, before either half is judged. If nothing is hidden the
    // defence has nothing to do, and if nothing is visible the exploit's failure
    // against the real projection is failure against an empty room.
    let truly_alive = vision::true_enemy_positions(&state, ATTACKER);
    let legitimately_visible = legitimately_visible_enemies(&state, ATTACKER);
    println!(
        "{ATTACKER:?}: {} enemy champions alive, {} of them the fog reveals",
        truly_alive.len(),
        legitimately_visible.len()
    );
    assert!(
        truly_alive.len() > legitimately_visible.len(),
        "no enemy was hidden from {ATTACKER:?}, so culling has nothing to prove here \
         (docs/RISKS.md R15)"
    );
    assert!(
        !legitimately_visible.is_empty(),
        "no enemy was visible to {ATTACKER:?}, so the culling projection's own \
         output is empty and 'the maphack sees only what the fog shows' is a claim \
         about nothing (docs/RISKS.md R15)"
    );

    // --- The exploit succeeds against the leaking projection ---
    let leaked: PlayerView = vision::omniscient(&state, ATTACKER);
    let mut against_leak = Maphack::new(ATTACKER);
    against_leak.fold(&leaked);
    let located = against_leak.locates();

    // It placed every living enemy, and every placement is where the champion
    // actually is. "Learns X" is a claim about the truth, not about a belief.
    assert_eq!(
        located.len(),
        truly_alive.len(),
        "the maphack did not place every living enemy against a leaking server"
    );
    for (id, position) in &located {
        let seat = Seat::from_index(u8::try_from(id.0).unwrap()).expect("a champion handle");
        let truth = truly_alive
            .iter()
            .find(|(who, _)| *who == seat)
            .map(|(_, at)| *at)
            .unwrap_or_else(|| panic!("{seat:?} was placed but is not a living enemy"));
        assert_eq!(*position, truth, "{seat:?} was placed at the wrong point");
    }

    // And the surplus over what the fog shows is the whole of the cheat: enemies
    // named that a fair client would never have been told about.
    let surplus = located
        .iter()
        .filter(|(id, _)| {
            let seat = Seat::from_index(u8::try_from(id.0).unwrap()).unwrap();
            !legitimately_visible.contains(&seat)
        })
        .count();
    println!("{ATTACKER:?}: the leaking server handed over {surplus} hidden enemies");
    assert!(
        surplus > 0,
        "the leaking projection disclosed nothing the fog would not have"
    );

    // --- The same exploit fails against the projection this project ships ---
    let culled: PlayerView = vision::culled(&state, ATTACKER);
    let mut against_culling = Maphack::new(ATTACKER);
    against_culling.fold(&culled);
    let located = against_culling.locates();

    let placed: Vec<Seat> = located
        .iter()
        .map(|(id, _)| Seat::from_index(u8::try_from(id.0).unwrap()).unwrap())
        .collect();
    // It placed exactly the enemies the fog already showed, and not one more. The
    // information is not in the bytes, so the attacker — unchanged — cannot find
    // it.
    let mut expected = legitimately_visible.clone();
    expected.sort_by_key(|seat| seat.index());
    let mut got = placed.clone();
    got.sort_by_key(|seat| seat.index());
    assert_eq!(
        got, expected,
        "against the real projection the maphack placed enemies the fog withheld: \
         the culling is not what it claims to be"
    );
    let hidden_surplus = placed
        .iter()
        .filter(|seat| !legitimately_visible.contains(seat))
        .count();
    assert_eq!(
        hidden_surplus, 0,
        "the maphack located {hidden_surplus} enemy champions the fog was hiding, \
         against the shipping projection"
    );

    println!(
        "{ATTACKER:?}: against culling, the maphack placed {} enemies — exactly the \
         {} the fog showed, and none of the {} it hid",
        placed.len(),
        legitimately_visible.len(),
        truly_alive.len() - legitimately_visible.len()
    );
}

/// The same, through real server frames rather than a handed-over view.
///
/// The exploit above folds a `PlayerView` directly, which is the cleanest way to
/// isolate the projection. This one confirms the attacker's *framing* path — the
/// one it would use against a live server — reaches the same conclusion: it
/// decodes `ServerFrame` bytes off the wire and still places only the visible
/// enemies. A maphack that worked on a struct but not on the bytes would be an
/// exploit that does not run against the thing it claims to attack.
#[test]
fn the_maphack_reads_real_frames_and_still_sees_only_the_unculled() {
    let state = a_state_with_some_enemies_hidden();
    let legitimately_visible = legitimately_visible_enemies(&state, ATTACKER);

    // The frame the server would actually send this seat: the culled view,
    // encoded and padded exactly as `Match::tick` encodes it.
    let culled = vision::culled(&state, ATTACKER);
    let frame = ServerFrame::encode(&ServerMessage::View {
        view: culled,
        applied_through: Some(0),
    });

    let mut attacker = Maphack::new(ATTACKER);
    attacker
        .observe(frame.as_bytes().as_slice())
        .expect("the frame decoded");
    let (views, refused) = attacker.counts();
    assert_eq!(
        (views, refused),
        (1, 0),
        "the attacker folded exactly one view"
    );

    let placed: Vec<Seat> = attacker
        .locates()
        .iter()
        .map(|(id, _)| Seat::from_index(u8::try_from(id.0).unwrap()).unwrap())
        .collect();
    let mut expected = legitimately_visible;
    expected.sort_by_key(|seat| seat.index());
    let mut got = placed;
    got.sort_by_key(|seat| seat.index());
    assert_eq!(
        got, expected,
        "reading real frames, the maphack placed something other than the visible enemies"
    );
}

/// The projectile back-track, which is an exploit that **lands against the
/// shipping build** — and is here because a milestone that only keeps the attacks
/// that fail is a milestone that has been curated.
///
/// A view names a projectile's position and velocity and no owner, deliberately
/// (`docs/ARCHITECTURE.md` removed the owner for exactly this reason). But
/// velocity is constant for a projectile's life, so the ray behind it passes
/// through the point it was cast from — and a caster the fog was hiding is on that
/// ray. `docs/SCOPE.md` puts this class of inference in the same register as the
/// behavioural ceiling: named, not defended, because closing it means removing
/// projectiles from views or capping the entity list, and the latter trades a
/// length channel for a content channel.
#[test]
fn a_projectile_betrays_the_ray_its_caster_stood_on() {
    let mut game = started_match(0x00C0_FFEE_0D15_EA5E, 9);
    let attacker_seat = ATTACKER;

    // A skillshot lives 45 ticks and covers 0.6 units a tick, so it travels 27
    // units — nowhere near far enough to cross a 173-unit lane. So the caster is
    // walked to a point just *outside* Blue0's 12-unit vision, from which a
    // projectile aimed at Blue0 crosses into that vision within a handful of
    // ticks while the caster itself stays hidden the whole time.
    let blue0 = game.world().champion(attacker_seat).position;
    let caster = Seat::Red0;
    // Eighteen units below Blue0: outside the vision radius, and a straight-up
    // shot from there passes through Blue0's spawn.
    let hide = blue0.add(FxVec2::new(Fx::ZERO, Fx::from_int(-18)));
    let approach = protocol::ClientFrame::encode(&protocol::ClientMessage::Input {
        seq: 0,
        claimed_at_ms: 0,
        action: Action::Move(hide),
    });
    game.deliver(caster, approach.as_bytes().as_slice(), 0)
        .expect("the approach was accepted");

    // Long enough for the caster to walk the lane to its hiding point. The lane
    // is about 158 units from Red's base to here at 0.2 a tick, so ~790 ticks;
    // 850 leaves margin, and the assertion below refuses a run in which it did
    // not arrive.
    for _ in 0..850 {
        let _ = game.tick();
    }
    let cast_from = game.world().champion(caster).position;
    assert!(
        !legitimately_visible_enemies(game.world(), attacker_seat).contains(&caster),
        "the caster is already visible before it fires, so there is no hidden \
         position to recover (docs/RISKS.md R15)"
    );

    // Now fire, straight at Blue0. The standing move order is already satisfied,
    // so the caster holds `cast_from` while the projectile flies.
    let toward_blue = blue0.sub(cast_from);
    let cast = protocol::ClientFrame::encode(&protocol::ClientMessage::Input {
        seq: 1,
        claimed_at_ms: 0,
        action: Action::Skillshot(toward_blue),
    });
    game.deliver(caster, cast.as_bytes().as_slice(), 0)
        .expect("the cast was accepted");

    let mut seen_projectile = None;
    let mut caster_ever_visible = false;
    for _ in 0..RULES.skillshot_lifetime_ticks {
        let _ = game.tick();
        let view = vision::culled(game.world(), attacker_seat);
        // Did the caster itself ever become visible? The exploit is only
        // interesting while it did not.
        if view.visible.iter().any(|entity| {
            matches!(entity, EntityView::Champion { id, .. } if *id == vision::champion_handle(caster))
        }) {
            caster_ever_visible = true;
        }
        if let Some(EntityView::Projectile { id, .. }) = view
            .visible
            .iter()
            .find(|entity| matches!(entity, EntityView::Projectile { .. }))
        {
            seen_projectile = Some(*id);
            // Fold this exact view into the attacker and stop: the earliest
            // sighting is the one nearest the cast.
            let mut attacker = Maphack::new(attacker_seat);
            attacker.fold(&view);
            let origins = attacker.candidate_origins(*id, RULES.skillshot_lifetime_ticks);
            // The true cast point is the caster's own position when it fired,
            // which the projectile's constant velocity runs exactly back to.
            let hit = origins.iter().any(|candidate| near(*candidate, cast_from));
            println!(
                "{attacker_seat:?}: saw a projectile it did not cast; {} candidate \
                 origins, caster's true position {}among them",
                origins.len(),
                if hit { "" } else { "NOT " }
            );
            assert!(
                hit,
                "the ray behind the projectile did not pass through the point it \
                 was cast from — the back-track does not work and this exploit is \
                 miscalibrated (docs/RISKS.md R15)"
            );
            break;
        }
    }

    assert!(
        seen_projectile.is_some(),
        "the attacker never saw the projectile, so nothing was back-tracked \
         (docs/RISKS.md R15)"
    );
    assert!(
        !caster_ever_visible,
        "the caster came into view, so the inference recovered a position the fog \
         was not hiding after all"
    );
}

/// Two fixed-point points within one raw unit on each axis.
///
/// Running a constant velocity backwards accumulates the truncation `sim`'s
/// fixed-point multiplication does toward zero, so the recovered origin can miss
/// the true one by a raw unit a component without the exploit being wrong.
fn near(a: FxVec2, b: FxVec2) -> bool {
    let close = |x: Fx, y: Fx| (x.to_raw() - y.to_raw()).abs() <= 1;
    close(a.x, b.x) && close(a.y, b.y)
}
