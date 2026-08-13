//! The playable client: a terminal, a keyboard, a mouse, and the loop between
//! them.
//!
//! Deliberately dull. Everything that can be wrong about what is drawn lives in
//! [`crate::ui`], which is a pure function with tests; everything that can be
//! wrong about what is *predicted* lives in [`crate::predict`], which is a pure
//! state machine with tests. What is left here is raw mode, an event pump and a
//! `print!`, and it is the one part of the client that cannot be tested without
//! a person in front of it.
//!
//! # Why a terminal
//!
//! `docs/MILESTONES.md` M4 asks for rendering, input capture and enough UI to
//! play, and calls itself the largest and least interesting milestone. A
//! windowed renderer would mean a graphics stack, a font stack, a game framework
//! and a display server in CI, for a game `docs/SCOPE.md` describes as a
//! fixture: nine discs, six towers and some projectiles on a two-hundred-unit
//! map. A terminal draws that, runs on all three platforms in the matrix, and
//! costs one dependency.
//!
//! It costs something real and `docs/RISKS.md` R14 is where the cost is written
//! down rather than here: aim is quantised to a character cell, which a
//! curvature detector at M8 cannot use.
//!
//! # One intention per tick, and `Idle` is not silence
//!
//! The loop sends exactly one intention for every frame it receives, which is
//! the protocol's shape and the traffic under which [`crate::predict`] is exact.
//! What it sends when the player is doing nothing is the *standing* intention
//! repeated, not `Action::Idle`: idling is a rule that stops the champion, so a
//! client that sent it every tick would cancel its own walk on the tick after it
//! started. One-shot abilities are sent once and leave the standing order alone,
//! exactly as `sim::step` treats them.
//!
//! # The terminal is put back the way it was found
//!
//! Raw mode, the alternate screen and mouse reporting are global state on a
//! device the process does not own. [`Restore`] puts all three back on drop, so
//! a panic in the loop leaves a usable shell rather than a terminal that has
//! stopped echoing.

use std::io::Write;
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton,
    MouseEvent, MouseEventKind,
};
use crossterm::{ExecutableCommand, cursor, terminal};
use sim::view::PlayerView;
use sim::{Action, FxVec2, RULES, Seat};

use crate::net::Wire;
use crate::predict::Prediction;
use crate::ui::{Camera, Scene, Status};
use crate::{ClientError, Headless};

/// Puts the terminal back on drop.
struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        let mut out = std::io::stdout();
        let _ = out.execute(DisableMouseCapture);
        let _ = out.execute(terminal::LeaveAlternateScreen);
        let _ = out.execute(cursor::Show);
        let _ = terminal::disable_raw_mode();
    }
}

/// What the player asked for since the last frame.
#[derive(Clone, Copy, Debug, Default)]
struct Pending {
    /// A new standing order, if the player gave one.
    standing: Option<Action>,
    /// A one-shot ability, if the player used one. Takes precedence for the
    /// tick it was asked on, and leaves the standing order alone.
    once: Option<Action>,
    /// The pointer, in cells.
    cursor: Option<(u16, u16)>,
    /// The player wants out.
    leave: bool,
    /// What to print in the status line.
    label: Option<&'static str>,
}

/// The client's own clock, sent with every input and trusted by nobody.
///
/// `docs/SCOPE.md`'s adversary model puts this field in the attacker's hands by
/// definition, and `docs/ARCHITECTURE.md` records that its divergence from the
/// server's arrival timestamp is exploit class 4's signal. An honest client
/// reads its own clock and says so; that is the whole of what makes the
/// divergence mean anything for the dishonest one.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// Connects, plays until the match ends or the player leaves, and restores the
/// terminal.
///
/// # Errors
///
/// A string, because everything this can fail at is something to print once and
/// exit on: a refused connection, a server on another build, or a terminal too
/// small to draw a map in.
pub async fn play(address: SocketAddr, certificate: &[u8]) -> Result<(), String> {
    let mut wire = Wire::connect(address, certificate)
        .await
        .map_err(|error| error.to_string())?;
    let mut headless = Headless::new();

    wire.send(&headless.join())
        .await
        .map_err(|error| error.to_string())?;
    let accepted = wire
        .recv_session()
        .await
        .map_err(|error| error.to_string())?;
    headless.receive(&accepted).map_err(describe)?;
    let seat = headless.seat().ok_or("the server assigned no seat")?;
    wire.send(&headless.ready())
        .await
        .map_err(|error| error.to_string())?;

    // The input pump. `crossterm::event::read` blocks, and it is the only thing
    // in this client that does, so it gets a thread and a channel rather than a
    // place in the select below.
    let (keys, mut inbox) = tokio::sync::mpsc::channel::<Event>(64);
    std::thread::spawn(move || {
        while let Ok(event) = crossterm::event::read() {
            if keys.blocking_send(event).is_err() {
                return;
            }
        }
    });

    terminal::enable_raw_mode().map_err(|error| error.to_string())?;
    let _restore = Restore;
    let mut out = std::io::stdout();
    out.execute(terminal::EnterAlternateScreen)
        .map_err(|error| error.to_string())?;
    out.execute(EnableMouseCapture)
        .map_err(|error| error.to_string())?;
    out.execute(cursor::Hide)
        .map_err(|error| error.to_string())?;

    let mut prediction: Option<Prediction> = None;
    let mut standing = Action::Idle;
    let mut cursor = (0u16, 0u16);
    let mut label: Option<&'static str> = None;

    loop {
        // One frame in. The server ticks on its own clock whatever this client
        // does, so a slow terminal misses ticks rather than stalling the match.
        let Ok(frame) = wire.recv_state().await else {
            break;
        };
        headless.receive(&frame).map_err(describe)?;
        let Some(view) = headless.view() else {
            continue;
        };
        let view = view.clone();

        let seen = prediction.get_or_insert_with(|| Prediction::anchored(&view));
        seen.observe(&view, headless.applied_through());

        let (cols, rows) = terminal::size().map_err(|error| error.to_string())?;
        let camera = Camera::fit(cols, rows).ok_or(
            "this terminal is too small to draw the map in; 20 columns and 14 rows is \
             the floor",
        )?;

        // Everything the player did since the last frame, collapsed into one
        // intention. A player who clicks three times in one tick has asked for
        // the last of them, which is what a game with a standing order means.
        let mut pending = Pending::default();
        while let Ok(event) = inbox.try_recv() {
            interpret(event, camera, &view, seat, seen.position(), &mut pending);
        }
        if let Some(at) = pending.cursor {
            cursor = at;
        }
        if let Some(order) = pending.standing {
            standing = order;
        }
        if let Some(what) = pending.label {
            label = Some(what);
        }
        if pending.leave {
            let _ = wire
                .send(&protocol::ClientFrame::encode(
                    &protocol::ClientMessage::Surrender,
                ))
                .await;
            break;
        }

        // The intention is decided and recorded *before* the frame is drawn,
        // and the order is the whole of what prediction buys. Drawing first
        // would put the authoritative position on the screen — the position the
        // server reported for a tick it has already left — and the click the
        // player just made would not appear until the round trip came back,
        // which is the latency this client exists to hide.
        let action = pending.once.unwrap_or(standing);
        let seq = headless.next_seq();
        seen.sent(seq, action);

        let scene = Scene {
            view: &view,
            seat,
            own: seen.position(),
            cursor,
            status: Status {
                outstanding: seen.outstanding(),
                corrections: seen.corrections(),
                losses: wire.losses(),
                stale: headless.stale(),
                last_order: label,
            },
        };
        draw(&mut out, &crate::ui::compose(&scene, camera)).map_err(|error| error.to_string())?;

        if wire.send(&headless.intend(action, now_ms())).await.is_err() {
            break;
        }
    }

    Ok(())
}

fn describe(error: ClientError) -> String {
    error.to_string()
}

/// Folds one terminal event into what the player is asking for.
fn interpret(
    event: Event,
    camera: Camera,
    view: &PlayerView,
    seat: Seat,
    own: FxVec2,
    pending: &mut Pending,
) {
    match event {
        Event::Key(KeyEvent {
            code, modifiers, ..
        }) => match code {
            KeyCode::Esc => pending.leave = true,
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                pending.leave = true;
            }
            KeyCode::Char('s') => {
                pending.standing = Some(Action::Idle);
                pending.label = Some("stop");
            }
            KeyCode::Char('q') => {
                // Aimed from the champion toward the pointer, which is what a
                // skillshot is. A direction shorter than the rules will
                // normalise is discarded by `sim` without spending the
                // cooldown, so a click on your own feet costs nothing.
                let target = camera.world(
                    pending.cursor.unwrap_or((0, 0)).0,
                    pending.cursor.unwrap_or((0, 0)).1,
                );
                pending.once = Some(Action::Skillshot(target.sub(own)));
                pending.label = Some("skillshot");
            }
            KeyCode::Char('w') => {
                let reach = RULES.targeted_range;
                if let Some(id) = crate::ui::nearest_enemy(view, seat, own, reach) {
                    pending.once = Some(Action::Targeted(id));
                    pending.label = Some("targeted");
                } else {
                    pending.label = Some("nobody in range");
                }
            }
            _ => {}
        },
        Event::Mouse(MouseEvent {
            kind, column, row, ..
        }) => {
            let at = (
                column.min(camera.cols.saturating_sub(1)),
                row.min(camera.rows.saturating_sub(1)),
            );
            pending.cursor = Some(at);
            let point = camera.world(at.0, at.1);
            match kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    pending.standing = Some(Action::Move(point));
                    pending.label = Some("move");
                }
                MouseEventKind::Down(MouseButton::Right) => {
                    // Within a couple of cells of the click, so that a player
                    // does not have to hit a champion exactly. An enemy the fog
                    // has taken away is not in the view and therefore cannot be
                    // clicked, which is the same restriction the rules put on
                    // the order.
                    let reach = sim::Fx::from_int(6);
                    if let Some(id) = crate::ui::nearest_enemy(view, seat, point, reach) {
                        pending.standing = Some(Action::Attack(id));
                        pending.label = Some("attack");
                    } else {
                        pending.standing = Some(Action::Move(point));
                        pending.label = Some("move");
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

/// Repaints the screen.
fn draw(out: &mut std::io::Stdout, lines: &[String]) -> std::io::Result<()> {
    out.execute(cursor::MoveTo(0, 0))?;
    for (index, line) in lines.iter().enumerate() {
        // Cleared per line rather than by clearing the whole screen first: a
        // full clear at 30 Hz is a flicker, and the map is dense enough that
        // leftover characters from a longer previous line would read as
        // entities.
        out.execute(cursor::MoveTo(0, u16::try_from(index).unwrap_or(u16::MAX)))?;
        out.execute(terminal::Clear(terminal::ClearType::CurrentLine))?;
        write!(out, "{line}")?;
    }
    out.flush()
}
