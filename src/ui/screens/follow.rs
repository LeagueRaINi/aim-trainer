//! The results card of tracking runs: how the crosshair followed the target through its changes
//! of direction ([`crate::metrics::follow`]), worked out from the replay the run just saved.
//! Nothing of it is stored.

use bevy::math::Vec2;
use bevy::prelude::*;

use crate::drill::DrillKind;
use crate::metrics::flick_probe::angles;
use crate::metrics::follow::{Following, following};
use crate::metrics::steadiness::Tracked;
use crate::replay::Replay;
use crate::ui::theme::*;
use crate::ui::widgets::*;

/// The drills with one moving target to follow, whose results get this card.
const KINDS: [DrillKind; 2] = [DrillKind::Tracking, DrillKind::SmoothTracking];
/// Fewer changes of direction than this say nothing about how they are followed.
const MIN_TURNS: u32 = 3;
/// A crosshair turning within this many seconds of the target turns with it.
const TOGETHER: f32 = 0.03;
/// A mean offset under this share of the target's radius is centred, and under [`SLIGHT`] of it
/// slight.
const CENTRED: f32 = 0.15;
const SLIGHT: f32 = 0.5;

/// Whether runs of `kind` get this card.
pub fn applies(kind: DrillKind) -> bool {
    KINDS.contains(&kind)
}

/// How the tracking run in `replay` followed its target, if it has enough changes of direction
/// to tell. Frames without a target are skipped; the eye is at the origin.
pub fn measure(replay: &Replay) -> Option<Following> {
    let frames: Vec<Tracked> = replay
        .frames
        .iter()
        .filter_map(|frame| {
            let &(centre, radius) = frame.targets.first()?;
            let distance = centre.length().max(1e-3);
            Some(Tracked {
                t: frame.t,
                aim: Vec2::new(frame.yaw.to_degrees(), frame.pitch.to_degrees()),
                target: angles(centre / distance),
                radius: (radius / distance).atan().to_degrees(),
            })
        })
        .collect();
    following(&frames).filter(|result| result.turns >= MIN_TURNS)
}

/// A reaction or recovery time: milliseconds up to a second, seconds beyond.
fn millis(secs: f32) -> String {
    if secs.abs() < 1.0 { format!("{:.0} ms", secs * 1000.0) } else { format!("{secs:.2} s") }
}

/// The takeaway in plain words: when the crosshair turns, and where it sits on the target.
fn takeaway(result: &Following) -> (String, String) {
    let turning = match result.reaction {
        Some(lag) if lag > TOGETHER => format!("You react to turns in about {}", millis(lag)),
        Some(lag) if lag < -TOGETHER => format!("You turn about {} before the target does", millis(-lag)),
        Some(_) => "You turn together with the target".to_string(),
        None => "Your turns couldn't be timed".to_string(),
    };
    let offset = result.lead.abs() / result.radius.max(1e-3);
    let side = if result.lead < 0.0 { "trail the target" } else { "run ahead of the target" };
    let sitting = if offset < CENTRED {
        "stay centred on the target".to_string()
    } else if offset < SLIGHT {
        format!("{side} slightly")
    } else {
        format!("{side} by a good part of its size")
    };
    let (turns, kept, lost) = (result.turns, result.kept, result.lost);
    let mut detail = if kept == turns {
        format!("You stayed on the target through all {turns} changes of direction.")
    } else {
        format!("You stayed on the target through {kept} of {turns} changes of direction.")
    };
    if let Some(recovery) = result.recovery {
        detail += &format!(" After {} you were back on it in about {}.", turns - kept - lost, millis(recovery));
    }
    if lost > 0 {
        detail += &format!(" After {lost} you weren't back on it before the next one.");
    }
    (format!("{turning} and {sitting}"), detail)
}

/// The card: the takeaway, the four numbers, and how they are measured.
pub fn direction_changes(result: &Following) -> impl Scene {
    let (headline, detail) = takeaway(result);
    let reaction = result.reaction.map_or("-".to_string(), millis);
    let reaction_hint = format!("target turns, then you: {} of {} turns", result.reacted, result.turns);
    let recovery = result.recovery.map_or("-".to_string(), millis);
    let recovery_hint = match result.turns - result.kept - result.lost {
        0 if result.lost == 0 => "never thrown off the target".to_string(),
        0 => "never back on target before the next turn".to_string(),
        1 => "back on target, after 1 turn that threw you off".to_string(),
        n => format!("back on target, after {n} turns that threw you off"),
    };
    let lead = match result.lead {
        lead if lead.abs() < 0.005 => "centred".to_string(),
        lead if lead < 0.0 => format!("{:.2}° behind", -lead),
        lead => format!("{lead:.2}° ahead"),
    };
    let lead_hint = format!("on average; the target's radius is {:.2}°", result.radius);
    let jitter = format!("{:.2}°", result.jitter);
    let note = format!(
        "A change of direction is the moment the target's velocity along its main line of motion passes through zero: where it stops going one way and sets off the other (it slows down first, which you can read). Reaction lag: from that moment to your crosshair's own reversal, median over the turns it could be timed at; around 200 ms is reacting to the reversal itself, much less is reading the slowdown, negative is turning early. Recovery: for turns that threw the crosshair off the target, the time from the reversal until it was back on to stay. Lead / trail: mean offset from the target's centre along its motion while it moves (it moved at about {:.0}°/s). Jitter: the crosshair's fast wobble (above about 4 Hz) relative to the target's motion, catch-up moves after turns included. All of it is measured from this run's replay, to within a frame or two.",
        result.pace
    );
    bsn! {
        @card("How you followed its changes of direction", bsn_list! {
            @callout(TEXT_ACCENT, headline, detail)
            --
            @row()
            Node { align_items: AlignItems::Stretch }
            Children [
                @stat_hint("Reaction lag", reaction, reaction_hint)
                --
                @stat_hint("Recovery", recovery, recovery_hint)
                --
                @stat_hint("Lead / trail", lead, lead_hint)
                --
                @stat_hint("Jitter", jitter, "fast wobble while following")
            ]
            --
            @details("How this is measured", note)
        })
    }
}
