//! The window, the event loop, and the thread that talks to the server.
//!
//! Deliberately dull, in the same way the terminal client's `term` module was.
//! Everything that can be wrong about *what is drawn* lives in [`crate::draw`],
//! which is a pure function with tests; everything that can be wrong about *what
//! is captured* lives in [`crate::input`] and [`crate::play`], which are pure
//! state machines with tests; everything that can be wrong about *what is
//! predicted* lives in [`crate::predict`]. What is left here is a window, a
//! framebuffer, an event loop and a socket.
//!
//! # Why a window, and why this window
//!
//! `docs/RISKS.md` R14 chose a terminal at M4 and priced the choice at "a
//! curvature detector cannot be written against this corpus". A trace of a real
//! session said the price was higher than that: a terminal reports the pointer
//! only when it crosses into a new character cell, so the sampling *rate* is a
//! function of pointer speed and the timing statistics R14 called untouched are
//! contaminated too. That is not a resolution problem with a resolution fix, so
//! the renderer changed. `docs/ARCHITECTURE.md` carries the library decision and
//! its reopening criterion.
//!
//! `winit` for the window because the input path is what motivated the change
//! and `winit` is the layer that exposes raw device motion on every platform in
//! `docs/ENGINEERING.md`'s matrix. `softbuffer` for the pixels because the scene
//! is nine discs, six towers and some projectiles: a CPU framebuffer draws that,
//! needs no shader, no adapter and no driver, and — the part that decided it —
//! leaves `rasterize` a pure function of a slice, so the renderer keeps its
//! tests in a CI job that has no display at all.
//!
//! # The timestamp is taken here, and it is taken per event
//!
//! [`ApplicationHandler::device_event`] reads the clock on its first line. That
//! callback runs once per event as the platform's queue drains, so the stamp
//! does not inherit the frame cadence — which matters, because
//! `docs/MILESTONES.md` M8's timing detectors have to tell a bot's regularity
//! from a renderer's jitter, and a timestamp read once per frame would have put
//! the renderer's jitter into every sample. It is still not a *device*
//! timestamp; [`crate::input::CLOCK`] says so, per platform, in a type.
//!
//! # The window is put back the way it was found
//!
//! Cursor visibility and pointer grab are state on a device the process does not
//! own. Both are released when the window is dropped, which winit does on exit,
//! including on the way out of a panic.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use protocol::{ClientFrame, ClientMessage, SERVER_FRAME_BYTES};
use sim::Seat;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, DeviceEvents, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

use crate::draw::{Mark, Viewport, compose, rasterize};
use crate::input::{Control, TraceStats};
use crate::play::{Aiming, Play};
use crate::predict::Prediction;
use crate::{ClientError, Headless};

/// What the network thread wakes the event loop for.
#[derive(Clone, Copy, Debug)]
pub struct Wake;

/// The default window size, in pixels. A window, not a claim: the projection
/// letterboxes, so any size draws the same map.
const WINDOW: (u32, u32) = (1280, 800);

/// This client's own clock, sent with every input and trusted by nobody.
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

/// The physical keys this client reads, and nothing else.
///
/// A key the client does not use produces no sample, because a record of keys
/// nobody can interpret is not telemetry.
const fn control_for(key: KeyCode) -> Option<Control> {
    match key {
        KeyCode::KeyQ => Some(Control::Skillshot),
        KeyCode::KeyW => Some(Control::Targeted),
        KeyCode::KeyS => Some(Control::Stop),
        _ => None,
    }
}

/// The window and its framebuffer.
struct Screen {
    window: Arc<Window>,
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
}

impl Screen {
    fn open(event_loop: &ActiveEventLoop, title: &str) -> Result<Self, String> {
        let attributes = Window::default_attributes()
            .with_title(title)
            .with_inner_size(winit::dpi::LogicalSize::new(WINDOW.0, WINDOW.1));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|error| error.to_string())?,
        );
        let context =
            softbuffer::Context::new(Arc::clone(&window)).map_err(|error| error.to_string())?;
        let surface = softbuffer::Surface::new(&context, Arc::clone(&window))
            .map_err(|error| error.to_string())?;

        // The OS pointer is hidden and confined so it cannot wander out of the
        // window; the aim a player sees is drawn by this client from raw
        // deltas, and has nothing to do with where the OS thinks the pointer
        // is. `Confined` is what X11 supports and `Locked` is what Wayland
        // supports, so both are tried and neither is required — a platform that
        // refuses both still delivers raw motion, which is the part that
        // matters.
        window.set_cursor_visible(false);
        if window.set_cursor_grab(CursorGrabMode::Confined).is_err() {
            let _ = window.set_cursor_grab(CursorGrabMode::Locked);
        }
        Ok(Self { window, surface })
    }

    /// Paints one frame.
    fn present(&mut self, marks: &[Mark], viewport: Viewport) -> Result<(), String> {
        let (Some(width), Some(height)) = (
            std::num::NonZeroU32::new(viewport.width),
            std::num::NonZeroU32::new(viewport.height),
        ) else {
            return Ok(());
        };
        self.surface
            .resize(width, height)
            .map_err(|error| error.to_string())?;
        let mut buffer = self
            .surface
            .buffer_mut()
            .map_err(|error| error.to_string())?;
        rasterize(marks, viewport, &mut buffer);
        buffer.present().map_err(|error| error.to_string())
    }
}

/// The playable client.
struct Session {
    screen: Option<Screen>,
    epoch: Instant,
    headless: Headless,
    prediction: Option<Prediction>,
    play: Play,
    seat: Seat,
    inbox: Receiver<[u8; SERVER_FRAME_BYTES]>,
    outbox: tokio::sync::mpsc::Sender<ClientFrame>,
    failure: Option<String>,
}

impl Session {
    fn at_ns(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }

    /// Folds in every frame that has arrived, and answers each with one
    /// intention.
    fn advance(&mut self) {
        loop {
            let bytes = match self.inbox.try_recv() {
                Ok(bytes) => bytes,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.failure
                        .get_or_insert_with(|| "the server hung up".to_owned());
                    return;
                }
            };
            if let Err(error) = self.headless.receive(&bytes) {
                self.failure = Some(describe(error));
                return;
            }
            let Some(view) = self.headless.view().cloned() else {
                continue;
            };

            let prediction = self
                .prediction
                .get_or_insert_with(|| Prediction::anchored(&view));
            prediction.observe(&view, self.headless.applied_through());

            // The intention is decided and recorded *before* the frame is
            // drawn, and the order is the whole of what prediction buys.
            // Drawing first would put the authoritative position on the screen
            // — the position the server reported for a tick it has already
            // left — and the click the player just made would not appear until
            // the round trip came back, which is the latency this client exists
            // to hide.
            let action = self.play.intention();
            let seq = self.headless.next_seq();
            prediction.sent(seq, action);
            let frame = self.headless.intend(action, now_ms());
            if self.outbox.try_send(frame).is_err() {
                self.failure
                    .get_or_insert_with(|| "the connection is gone".to_owned());
                return;
            }
        }
    }

    fn redraw(&mut self) {
        let (Some(view), Some(prediction)) = (self.headless.view(), self.prediction.as_ref())
        else {
            return;
        };
        let scene = crate::draw::Scene {
            view,
            seat: self.seat,
            own: prediction.position(),
            aim: self.play.aim(),
        };
        let marks = compose(&scene);
        let viewport = self.play.viewport();
        if let Some(screen) = self.screen.as_mut()
            && let Err(error) = screen.present(&marks, viewport)
        {
            self.failure = Some(error);
        }
    }

    /// What a control press needs to know, or `None` before the first frame.
    fn aiming(&self) -> Option<(sim::view::PlayerView, sim::FxVec2)> {
        let view = self.headless.view()?.clone();
        let own = self
            .prediction
            .as_ref()
            .map_or(view.own.position, Prediction::position);
        Some((view, own))
    }
}

impl ApplicationHandler<Wake> for Session {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.screen.is_some() {
            return;
        }
        match Screen::open(event_loop, "moba") {
            Ok(screen) => {
                let size = screen.window.inner_size();
                self.play.resized(size.width, size.height);
                self.screen = Some(screen);
                event_loop.listen_device_events(DeviceEvents::WhenFocused);
            }
            Err(error) => {
                self.failure = Some(error);
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _wake: Wake) {
        self.advance();
        if self.failure.is_some() {
            event_loop.exit();
            return;
        }
        if let Some(screen) = self.screen.as_ref() {
            screen.window.request_redraw();
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device: DeviceId,
        event: DeviceEvent,
    ) {
        // The clock is read first, before anything decides what this event
        // means. See this module's header.
        let at_ns = self.at_ns();
        if let DeviceEvent::MouseMotion { delta } = event {
            self.play.moved(at_ns, delta.0, delta.1);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window: WindowId,
        event: WindowEvent,
    ) {
        let at_ns = self.at_ns();
        match event {
            WindowEvent::CloseRequested => self.surrender(event_loop),
            WindowEvent::Resized(size) => {
                self.play.resized(size.width, size.height);
                if let Some(screen) = self.screen.as_ref() {
                    screen.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::MouseInput { state, button, .. } => {
                let control = match button {
                    MouseButton::Left => Control::Move,
                    MouseButton::Right => Control::Attack,
                    _ => return,
                };
                self.press(at_ns, control, state.is_pressed());
            }
            WindowEvent::KeyboardInput { event, .. } => {
                // A key the OS repeated is not a key a hand pressed, so it is
                // filtered rather than recorded. Auto-repeat in a corpus would
                // be the operating system's key-repeat setting showing up in a
                // behavioural distribution as if it were a person.
                if event.repeat {
                    return;
                }
                if matches!(event.physical_key, PhysicalKey::Code(KeyCode::Escape))
                    && event.state == ElementState::Pressed
                {
                    self.surrender(event_loop);
                    return;
                }
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                let Some(control) = control_for(code) else {
                    return;
                };
                self.press(at_ns, control, event.state.is_pressed());
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

impl Session {
    /// The player asked to leave. `Surrender` frees the seat; it does not decide
    /// the match, because whether a team that concedes loses is a rule and rules
    /// live in `sim` where a replay resimulates them.
    fn surrender(&mut self, event_loop: &ActiveEventLoop) {
        self.play.leave();
        let _ = self
            .outbox
            .try_send(ClientFrame::encode(&ClientMessage::Surrender));
        event_loop.exit();
    }

    fn press(&mut self, at_ns: u64, control: Control, down: bool) {
        let Some((view, own)) = self.aiming() else {
            return;
        };
        self.play.pressed(
            at_ns,
            control,
            down,
            &Aiming {
                view: &view,
                seat: self.seat,
                own,
            },
        );
    }
}

fn describe(error: ClientError) -> String {
    error.to_string()
}

/// Connects, opens a window, and plays until the match ends or the player
/// leaves.
///
/// Synchronous, because `winit` owns the thread it runs on. The transport keeps
/// its `tokio` runtime on a thread of its own and the two meet over two
/// channels and an event-loop proxy, which is also what keeps every decision —
/// the session state machine, the prediction, the capture — on one thread with
/// no locking to reason about.
///
/// # Errors
///
/// A string, because everything this can fail at is something to print once and
/// exit on: a refused connection, a server on another build, or a display that
/// will not give out a window.
pub fn play(address: std::net::SocketAddr, certificate: &[u8]) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;

    let event_loop = EventLoop::<Wake>::with_user_event()
        .build()
        .map_err(|error| error.to_string())?;
    let proxy = event_loop.create_proxy();

    let mut headless = Headless::new();
    let certificate = certificate.to_vec();
    let join = headless.join();
    let (mut wire, accepted) = runtime.block_on(async move {
        let mut wire = crate::net::Wire::connect(address, &certificate)
            .await
            .map_err(|error| error.to_string())?;
        wire.send(&join).await.map_err(|error| error.to_string())?;
        let accepted = wire
            .recv_session()
            .await
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((wire, accepted))
    })?;
    headless.receive(&accepted).map_err(describe)?;
    let seat = headless.seat().ok_or("the server assigned no seat")?;
    let ready = headless.ready();

    let (in_tx, in_rx) = std::sync::mpsc::channel();
    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<ClientFrame>(64);
    out_tx.try_send(ready).map_err(|error| error.to_string())?;

    runtime.spawn(async move {
        loop {
            tokio::select! {
                // `read_datagram` is cancel-safe and the assembler's state is
                // only touched after a datagram has been read, so losing this
                // branch to the other loses nothing.
                state = wire.recv_state() => {
                    let Ok(bytes) = state else { return };
                    if in_tx.send(bytes).is_err() {
                        return;
                    }
                    let _ = proxy.send_event(Wake);
                }
                outbound = out_rx.recv() => {
                    let Some(frame) = outbound else { return };
                    if wire.send(&frame).await.is_err() {
                        return;
                    }
                }
            }
        }
    });

    let mut session = Session {
        screen: None,
        epoch: Instant::now(),
        headless,
        prediction: None,
        play: Play::new(),
        seat,
        inbox: in_rx,
        outbox: out_tx,
        failure: None,
    };
    event_loop
        .run_app(&mut session)
        .map_err(|error| error.to_string())?;

    report(session.play.trace().stats());
    match session.failure {
        // The server hanging up is how the end of a match looks from here.
        Some(ref reason) if reason == "the server hung up" => Ok(()),
        Some(reason) => Err(reason),
        None => Ok(()),
    }
}

/// The measurement instrument: a window that captures and reports, with no
/// server and no game.
///
/// `docs/MILESTONES.md` asks for milestones verifiable by running a command, and
/// this is the command behind the claim that the capture path's sampling rate no
/// longer follows the pointer's speed. It opens a window, records every device
/// event for `seconds`, and prints the distribution of inter-arrival times and
/// the finest motion observed.
///
/// # Errors
///
/// A string, if there is no display to open a window on.
pub fn probe(seconds: u64) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|error| error.to_string())?;
    let mut probe = Probe {
        screen: None,
        epoch: Instant::now(),
        play: Play::new(),
        until: seconds,
        failure: None,
    };
    event_loop
        .run_app(&mut probe)
        .map_err(|error| error.to_string())?;
    if let Some(failure) = probe.failure {
        return Err(failure);
    }
    report(probe.play.trace().stats());
    for sample in probe.play.trace().samples() {
        match sample.motion {
            crate::input::Motion::Moved { dx, dy } => {
                println!("{}\tmove\t{dx}\t{dy}", sample.at_ns);
            }
            crate::input::Motion::Pressed { control, down } => {
                println!("{}\tpress\t{control:?}\t{down}", sample.at_ns);
            }
        }
    }
    Ok(())
}

struct Probe {
    screen: Option<Screen>,
    epoch: Instant,
    play: Play,
    until: u64,
    failure: Option<String>,
}

impl ApplicationHandler for Probe {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.screen.is_some() {
            return;
        }
        match Screen::open(event_loop, "moba — input probe") {
            Ok(screen) => {
                let size = screen.window.inner_size();
                self.play.resized(size.width, size.height);
                self.screen = Some(screen);
                event_loop.listen_device_events(DeviceEvents::Always);
            }
            Err(error) => {
                self.failure = Some(error);
                event_loop.exit();
            }
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device: DeviceId,
        event: DeviceEvent,
    ) {
        let at_ns = u64::try_from(self.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX);
        if let DeviceEvent::MouseMotion { delta } = event {
            self.play.moved(at_ns, delta.0, delta.1);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => self.play.resized(size.width, size.height),
            WindowEvent::RedrawRequested => {
                if let Some(screen) = self.screen.as_mut() {
                    let marks = [Mark::Cross {
                        at: self.play.aim(),
                        colour: crate::draw::colour::AIM,
                    }];
                    let _ = screen.present(&marks, self.play.viewport());
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.epoch.elapsed().as_secs() >= self.until {
            event_loop.exit();
            return;
        }
        if let Some(screen) = self.screen.as_ref() {
            screen.window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + std::time::Duration::from_millis(16),
        ));
    }
}

/// Prints what a captured trace measures, to standard error so that the probe's
/// sample dump on standard output stays machine-readable.
fn report(stats: TraceStats) {
    let TraceStats {
        samples,
        moves,
        span_ns,
        gaps_ns,
        finest_count,
        finest_world_units,
        travelled_counts,
    } = stats;
    let ms = |ns: u64| (ns as f64) / 1e6;
    eprintln!(
        "capture: {samples} device events ({moves} motion) over {:.3} s",
        ms(span_ns) / 1e3
    );
    eprintln!(
        "capture: inter-arrival ms  min {:.3}  p05 {:.3}  p50 {:.3}  p95 {:.3}  max {:.3}",
        ms(gaps_ns.min),
        ms(gaps_ns.p05),
        ms(gaps_ns.p50),
        ms(gaps_ns.p95),
        ms(gaps_ns.max)
    );
    eprintln!(
        "capture: clock {:?}; finest motion {} counts = {} world units; travelled {travelled_counts:.1} counts",
        crate::input::CLOCK,
        finest_count.map_or("n/a".to_owned(), |value| format!("{value}")),
        finest_world_units.map_or("n/a".to_owned(), |value| format!("{value}")),
    );
}
