//! Showing a participant their own device stream, and what it permits.
//!
//! # Why a demonstration is part of the disclosure and not an ornament
//!
//! `docs/CONSENT.md` §2b tells a participant, in words, that the shape and speed
//! of the way they move a mouse is distinctive — "closer to handwriting than to a
//! preference" — and that somebody holding the file and a second recording could
//! plausibly tell the two are the same person. That is the single most
//! consequential sentence on the page and it is also the one a reader has no way
//! to evaluate: it is an assertion about data they have never seen, made by the
//! party asking for it.
//!
//! Thirty seconds of their own crossing of the lobby answers it in a way no
//! paragraph does. The data already exists at that moment of the session, on
//! their own machine, in a file their own client wrote, and this module is what
//! reads it back at them: a few dozen records exactly as they were captured, and
//! then the four things this project can work out from a stream like it, computed
//! from *their* numbers rather than described in general.
//!
//! # It writes nothing, and that is the mechanism rather than a habit
//!
//! There is no destination argument and no path from this module to a file. The
//! demonstration crossing is therefore not a recording the corpus holds: the
//! operator runs `replay disclose` on the part, the participant reads it, and
//! `replay store` is never run on that session. Nothing has to be cleaned up
//! afterwards, because nothing was filed.
//!
//! **The honest tension, stated rather than smoothed over.** The demonstration
//! shows data that had to be captured before the signature it informs, which is
//! an order Law 25's "consent before collection" does not obviously permit.
//! `docs/CONSENT.md` handles it the only way that is not a fiction: the crossing
//! is described and agreed to verbally before it happens, it is used for nothing
//! but this, and it never reaches a corpus. That is a judgement about a narrow,
//! immediate, single-use collection and it is one of the points the document
//! sends to a human review rather than settling with confidence.
//!
//! # What it derives, and the one thing it deliberately does not
//!
//! Four quantities, each of which is a real answer computed from the stream in
//! front of it: how often the device reported, the finest movement it can
//! express, how far the hand travelled against how far the pointer went, and how
//! quickly the participant answered something appearing on screen. The fourth is
//! the one that lands.
//!
//! What it does **not** do is score anybody. There is no threshold here, no
//! comparison against other participants, and no verdict — for the reason
//! `docs/SCOPE.md` gives about detector findings and `docs/SCHEMA.md` §4e gives
//! about calibration: a number shown to a participant as a judgement is a
//! judgement, and nothing in this project is calibrated to make one.

use crate::telemetry::{Event, TelemetryPart};

/// How many records of the participant's own stream are printed verbatim.
///
/// Enough that the rhythm is visible and few enough to be read in the room. A
/// stream at 1 kHz produces this many in twenty milliseconds, which is itself
/// part of what the page says.
pub const EXCERPT: usize = 24;

/// The disclosure page for one seat's stream.
///
/// Returns the text rather than printing it, so that the same page can be
/// asserted over in a test — which is what keeps this from being a formatting
/// function nobody has read the output of.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one page, printed in the order a participant reads it"
)]
pub fn of(part: &TelemetryPart) -> String {
    let facts = part.stream.facts();
    let samples = &part.stream.samples;
    let mut out = String::new();

    out.push_str(
        "This is your own recording, from the lobby you have just crossed. Nothing\n\
         below is an example or a simulation: every line is a record your computer\n\
         made in the last few minutes.\n\n",
    );

    // The excerpt. Timestamps are relative to the first record, because the
    // absolute value is a stopwatch reading with no meaning outside the session
    // and printing it would invite exactly the wrong question.
    let first_ns = samples.first().map_or(0, |sample| sample.at_ns);
    let motions: Vec<_> = samples
        .iter()
        .filter(|sample| matches!(sample.event, Event::Moved { .. }))
        .collect();

    out.push_str(&format!(
        "WHAT WAS RECORDED — the first {} of {} movements\n\n",
        EXCERPT.min(motions.len()),
        motions.len()
    ));
    out.push_str("      time         dx        dy   (dx, dy are your mouse's own counts)\n");
    for sample in motions.iter().take(EXCERPT) {
        if let Event::Moved { dx, dy } = sample.event {
            out.push_str(&format!(
                "  {:>8.3} ms  {:>+8.2}  {:>+8.2}\n",
                (sample.at_ns.saturating_sub(first_ns) as f64) / 1e6,
                dx,
                dy
            ));
        }
    }
    if motions.len() > EXCERPT {
        out.push_str(&format!(
            "  … and {} more, on the same clock.\n",
            motions.len().saturating_sub(EXCERPT)
        ));
    }

    let span_ns = samples
        .last()
        .map_or(0, |sample| sample.at_ns.saturating_sub(first_ns));
    out.push_str(&format!(
        "\n  {} movement(s), {} button press or release(s) and {} frame(s) received,\n  \
         over {:.1} second(s).\n",
        facts.motions,
        facts.samples.saturating_sub(facts.motions),
        facts.views,
        (span_ns as f64) / 1e9
    ));

    out.push_str("\nWHAT THIS PROJECT CAN WORK OUT FROM IT — computed from your numbers\n\n");

    // 1. The report rate.
    let gaps = motion_gaps(&motions);
    match median(&gaps) {
        Some(gap_ns) if gap_ns > 0 => out.push_str(&format!(
            "  Your mouse reported about every {:.2} ms — roughly {:.0} times a\n  \
             second. That is a property of your hardware, and it is why the project\n  \
             records it: two people's movements are not comparable until it is known.\n\n",
            (gap_ns as f64) / 1e6,
            1e9 / (gap_ns as f64)
        )),
        _ => out.push_str(
            "  Not enough movement to read a report rate. That is a state and not a\n  \
             failure: nothing about you is missing from the record because of it.\n\n",
        ),
    }

    // 2. The quantum.
    match finest(&motions) {
        Some(quantum) => out.push_str(&format!(
            "  The smallest movement your mouse can express is {quantum} count(s). A\n  \
             mouse reporting whole counts gives 1; a system reporting in fractions\n  \
             gives less. This is your equipment and your operating system, not you.\n\n"
        )),
        None => out.push_str("  Nothing moved, so there is no resolution to report.\n\n"),
    }

    // 3. Path against displacement — the first thing here that is about the hand
    //    rather than the device, and it is deliberately the gentlest one.
    let (path, net) = travel(&motions);
    if path > 0.0 {
        out.push_str(&format!(
            "  Your hand travelled {path:.0} counts to move the cursor {net:.0}. The\n  \
             difference — {:.0} counts, {:.0}% — is overshoot and correction: where\n  \
             you went past a target and came back. How much of it there is, and what\n  \
             it looks like, is one of the things that makes a hand recognisable.\n\n",
            path - net,
            if path > 0.0 {
                (path - net) / path * 100.0
            } else {
                0.0
            }
        ));
    }

    // 4. The reaction, which is the one that lands.
    match fastest_answer(part) {
        Some(delay_ns) => out.push_str(&format!(
            "  The quickest you answered something appearing on your screen was\n  \
             {:.0} ms. A reaction that fast, and how much it varies from one to the\n  \
             next, is exactly what this project's detectors read — it is the\n  \
             difference between a person and a program, and it is the reason the\n  \
             recording is worth making.\n\n",
            (delay_ns as f64) / 1e6
        )),
        None => out.push_str(
            "  Nothing here to measure a reaction from yet: a reaction is measured\n  \
             from the moment the screen showed you something to the moment you\n  \
             answered, and this crossing has no such pair in it.\n\n",
        ),
    }

    out.push_str(
        "PUT TOGETHER, this is distinctive — closer to handwriting than to a\n\
         preference. Somebody holding this file and a second\n\
         recording of you could plausibly tell the two are the same person. That is\n\
         precisely why the project wants it, and it is why the page you are about to\n\
         sign treats it as information about you rather than as a log.\n\n\
         WHAT IS NOT HERE, and you can check it against the lines above: no key\n\
         outside the five the game uses, no text, no image, no sound, nothing about\n\
         where your pointer is on your desktop, nothing from any other program.\n\n\
         Nothing on this page is a score. Nobody passes or fails it, and no number\n\
         here is compared against anybody else's.\n\n\
         This reading wrote nothing. The crossing it was taken from is not stored,\n\
         and does not become part of the corpus whether or not you sign.\n",
    );
    out
}

/// The gaps between consecutive motions, in nanoseconds.
fn motion_gaps(motions: &[&crate::telemetry::Sample]) -> Vec<u64> {
    motions
        .windows(2)
        .map(|pair| pair[1].at_ns.saturating_sub(pair[0].at_ns))
        .collect()
}

/// The middle value, taking the lower of two on an even count.
fn median(values: &[u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted.get(sorted.len().saturating_sub(1) / 2).copied()
}

/// The finest non-zero delta component in the stream.
fn finest(motions: &[&crate::telemetry::Sample]) -> Option<f64> {
    let mut smallest: Option<f64> = None;
    for sample in motions {
        if let Event::Moved { dx, dy } = sample.event {
            for component in [dx.abs(), dy.abs()] {
                if component > 0.0 && smallest.is_none_or(|held| component < held) {
                    smallest = Some(component);
                }
            }
        }
    }
    smallest
}

/// Path length and net displacement, in device counts.
///
/// The pair rather than either alone: a path length on its own is a number, and
/// the *difference* is the thing a participant can see themselves doing.
fn travel(motions: &[&crate::telemetry::Sample]) -> (f64, f64) {
    let (mut path, mut net_x, mut net_y) = (0.0, 0.0, 0.0);
    for sample in motions {
        if let Event::Moved { dx, dy } = sample.event {
            path += dx.hypot(dy);
            net_x += dx;
            net_y += dy;
        }
    }
    (path, net_x.hypot(net_y))
}

/// The shortest delay between a frame arriving and the next button press.
///
/// The reaction the corpus is actually about, computed the way a detector would:
/// from an [`Event::Viewed`] anchor — the moment the screen showed something —
/// to the next [`Event::Pressed`] that is a press rather than a release. A
/// release is somebody letting go and is not an answer to anything.
///
/// It is a **lower bound on the participant's own fastest reaction and not an
/// estimate of it**: the anchor before a press is not necessarily the frame that
/// prompted it, so the shortest such pair over a session is the friendliest
/// number the stream can produce. Shown anyway, because the point of the page is
/// that the quantity is readable at all, and a page that showed a slower number
/// would be understating what is being disclosed.
fn fastest_answer(part: &TelemetryPart) -> Option<u64> {
    let mut last_view: Option<u64> = None;
    let mut fastest: Option<u64> = None;
    for sample in &part.stream.samples {
        match sample.event {
            Event::Viewed { .. } => last_view = Some(sample.at_ns),
            Event::Pressed { down: true, .. } => {
                if let Some(shown) = last_view {
                    let delay = sample.at_ns.saturating_sub(shown);
                    if fastest.is_none_or(|held| delay < held) {
                        fastest = Some(delay);
                    }
                    last_view = None;
                }
            }
            Event::Pressed { down: false, .. } | Event::Moved { .. } => {}
        }
    }
    fastest
}
