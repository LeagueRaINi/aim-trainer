//! The results breakdown of the bot drills (Head taps, Strafing bot, Counter-strafe): what the
//! run's shots were made of, with the biggest leak named first.

use bevy::prelude::*;

use crate::drill::movement::{self, DEAD_ZONE, RUN_SPEED};
use crate::drill::rifle::{Leak, MoveStats, RECOVERY_SECS, TapStats, burst_error};
use crate::ui::theme::*;
use crate::ui::widgets::*;

/// The biggest leak in plain words: its tone, headline and what to do about it.
fn takeaway(stats: &TapStats) -> (ThemeToken, &'static str, String) {
    let shots = stats.shots;
    let first = format!("{} of {} first shots hit the head", stats.first_heads, stats.first_shots);
    match stats.leak() {
        Some(Leak::Moving) => (
            TEXT_WARN,
            "You shoot before you have stopped",
            format!(
                "{} of {shots} shots were fired while you were still moving, and a shot on the move goes wide. Come to a stop first, then click.",
                stats.movement.map_or(0, |movement| movement.moving)
            ),
        ),
        Some(Leak::Unsettled) => (
            TEXT_WARN,
            "You shoot again too soon",
            format!(
                "{} of {shots} shots were fired before the rifle had settled, so they scattered. Leave about {RECOVERY_SECS:.1} s between taps: the bar under the crosshair is gone when the rifle is ready.",
                stats.unsettled
            ),
        ),
        Some(Leak::FirstShots) => (
            TEXT_WARN,
            "Your first shot misses the head",
            format!(
                "{first}. It is the one shot that goes exactly where you aim: take the moment to put the crosshair on the head before you click."
            ),
        ),
        Some(Leak::Clean) | None => {
            (TEXT_GOOD, "Clean taps", format!("{first}, and few shots were rushed. Now do it faster."))
        },
    }
}

fn percent_of(share: Option<f32>) -> String {
    share.map_or("-".to_string(), |share| format!("{:.0}%", share * 100.0))
}

/// The numbers every bot drill has.
fn tap_row(stats: &TapStats) -> impl Scene {
    let headshots = percent_of(stats.headshot_share());
    let per_kill = stats.shots_per_kill().map_or("-".to_string(), |shots| format!("{shots:.1}"));
    let first = percent_of(stats.first_shot_share());
    let first_hint = format!("{} of {} first shots", stats.first_heads, stats.first_shots);
    let unsettled = format!("{} of {}", stats.unsettled, stats.shots);
    bsn! {
        @row()
        Node { align_items: AlignItems::Stretch }
        Children [
            @stat_hint("Headshots", headshots, "of the shots that hit")
            --
            @stat_hint("Shots per kill", per_kill, "1.0: all head taps")
            --
            @stat_hint("First shot on head", first, first_hint)
            --
            @stat_hint("Shot too soon", unsettled, "rifle not settled yet")
        ]
    }
}

/// The movement numbers of a drill the player strafes in.
fn move_row(stats: &TapStats, movement: &MoveStats) -> impl Scene {
    let moving = format!("{} of {}", movement.moving, stats.shots);
    let moving_hint = format!("faster than {:.0}% of a run", DEAD_ZONE * 100.0);
    let speed = movement.speed_at_shot(stats.shots).map_or("-".to_string(), |speed| format!("{speed:.1} m/s"));
    let speed_hint = format!("average; a run is {RUN_SPEED} m/s");
    let delay = movement.stop_to_shot().map_or("-".to_string(), |secs| format!("{:.0} ms", secs * 1000.0));
    bsn! {
        @row()
        Node { align_items: AlignItems::Stretch }
        Children [
            @stat_hint("Shot while moving", moving, moving_hint)
            --
            @stat_hint("Speed at the shot", speed, speed_hint)
            --
            @stat_hint("Stop to shot", delay, "average, stopped to first shot")
        ]
    }
}

/// The takeaway, then the numbers: head shots, shots per kill, first shots, shots fired too soon
/// and, for a drill with movement, how still the player was at the shot.
pub fn tap_breakdown(stats: &TapStats) -> impl Scene {
    let (tone, headline, text) = takeaway(stats);
    let moved: Vec<_> = stats.movement.iter().map(|movement| move_row(stats, movement)).collect();
    let mut note = format!(
        "The rifle approximates a Valorant Vandal; it is not the game's recoil, which Riot doesn't publish. The first shot from rest goes exactly where the crosshair points. A shot within {RECOVERY_SECS} s of the one before is not settled and can land off it: up to {:.1}° right after a shot, more in a burst. A first shot is the first one fired at each bot. Headshots count hits only; shots per kill count every shot.",
        burst_error()
    );
    if stats.movement.is_some() {
        note += &format!(
            " Moving: faster than {:.0}% of the {RUN_SPEED} m/s run speed, where Valorant stops treating you as standing; such a shot can land up to {:.0}° off. Stop to shot: from the moment you were slow enough to the first shot after it. The stopping times are modelled on Riot's own measurements, the rest is approximate.",
            DEAD_ZONE * 100.0,
            movement::error(RUN_SPEED)
        );
    }
    bsn! {
        @card("How you tapped", bsn_list! {
            @callout(tone, headline, text)
            --
            @tap_row(stats)
            --
            {moved}
            --
            @details("How to read this", note)
        })
    }
}
