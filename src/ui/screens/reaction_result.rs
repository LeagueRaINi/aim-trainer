//! The Reaction test's card on the results screen: the median reaction time and how it compares
//! with earlier tests, the spread, every trial as a dot, and what the number can and cannot
//! tell.
//!
//! With a latency offset set, every time on the card is the measured one minus the offset; the
//! one tile labelled "raw" is not.

use bevy::feathers::constants::fonts;
use bevy::feathers::theme::ThemeBackgroundColor;
use bevy::feathers::tokens;
use bevy::prelude::*;

use crate::drill::cue::ANTICIPATION_SECS;
use crate::drill::kinds::reaction_test::{LAPSE_SECS, MIN_VALID, PRACTICE_TRIALS, SCORED_TRIALS, Trial, WAIT_SECS};
use crate::history::format_date;
use crate::metrics::reaction::Summary;
use crate::reaction_log::ReactionEntry;
use crate::ui::theme::*;
use crate::ui::widgets::*;

/// Height of the trial plot, px, and the room kept above and below the fastest and slowest dot.
const PLOT_H: f32 = 72.0;
const PLOT_PAD: f32 = 9.0;
/// Width of the plot's column of axis labels.
const AXIS_W: f32 = 52.0;
const DOT: f32 = 6.0;

/// What simple reaction time is good for, in one sentence.
const MEANING: &str =
    "Reaction time hardly improves with practice, unlike deciding and anticipating: it shows how ready you are today.";

/// The card for `entry`, compared with the previous test and the best one before it.
pub fn reaction_result(
    entry: &ReactionEntry,
    previous: Option<&ReactionEntry>,
    best: Option<&ReactionEntry>,
) -> impl Scene {
    let (tone, headline, text) = takeaway(entry, previous);
    let breakdown: Vec<_> = entry.summary().map(|summary| breakdown(entry, summary, best)).into_iter().collect();
    bsn! {
        @card("Reaction time", bsn_list! {
            @callout(tone, headline, text)
            --
            {breakdown}
        })
    }
}

/// A time as shown: whole milliseconds.
fn ms(value: f32) -> String {
    format!("{value:.0} ms")
}

/// The result in plain words: the median, and whether it differs from the previous test by more
/// than the two tests' noise.
fn takeaway(entry: &ReactionEntry, previous: Option<&ReactionEntry>) -> (ThemeToken, String, String) {
    let Some(summary) = entry.summary() else {
        return (
            TEXT_WARN,
            "No reaction to time".to_string(),
            "Every scored trial was a false start or a lapse. Wait for the dot, then click as soon as you see it."
                .to_string(),
        );
    };
    let shown = |value: f32| ms((value - entry.offset_ms).max(0.0));
    let median = shown(summary.median);
    if !entry.comparable() {
        let text = format!(
            "A median of so few trials can be far off: take the test again for a number to compare. This one was {median}."
        );
        return (TEXT_WARN, format!("A rough result: only {} valid trials", summary.count), text);
    }
    let Some((before, earlier)) = previous.and_then(|p| Some((p, p.summary()?))) else {
        let text =
            format!("Half of your reactions were faster than {median}: the number your next tests are compared with.");
        return (TEXT_ACCENT, "Your first reaction test".to_string(), text);
    };
    // Positive: faster than last time.
    let gain = earlier.median - summary.median;
    let noise = summary.noise_against(&earlier);
    let direction = if gain >= 0.0 { "faster" } else { "slower" };
    let (tone, headline, verdict) = if gain > noise {
        (TEXT_GOOD, "Faster than last time", "more than")
    } else if gain < -noise {
        (TEXT_WARN, "Slower than last time", "more than")
    } else {
        (TEXT_ACCENT, "About the same as last time", "within")
    };
    let text = format!(
        "{median}: {:.0} ms {direction} than your previous test ({} on {}), {verdict} the {noise:.0} ms two tests can differ by chance.",
        gain.abs(),
        shown(earlier.median),
        format_date(before.timestamp)
    );
    (tone, headline.to_string(), text)
}

/// The numbers, the trial plot and the notes of a test with at least one valid trial.
fn breakdown(entry: &ReactionEntry, summary: Summary, best: Option<&ReactionEntry>) -> impl Scene {
    let offset = entry.offset_ms;
    let corrected = offset > 0.0;
    let shown = |value: f32| ms((value - offset).max(0.0));
    let median_hint = if corrected { format!("minus {offset:.0} ms offset") } else { "as measured".to_string() };
    // Only with an offset is there a second, raw median to show.
    let raw: Vec<_> =
        corrected.then(|| stat_hint("Raw median", ms(summary.median), "as measured")).into_iter().collect();
    let middle = format!("{:.0}–{}", (summary.q1 - offset).max(0.0), shown(summary.q3));
    let spread = format!("{:.0} ms wide (IQR)", summary.iqr());
    let (record, record_hint) = match best.and_then(|b| Some((b, b.summary()?.median))) {
        _ if !entry.comparable() => ("-".to_string(), format!("needs {MIN_VALID} valid trials")),
        Some((b, median)) if median < summary.median => (shown(median), format!("on {}", format_date(b.timestamp))),
        _ => (shown(summary.median), "this test".to_string()),
    };
    let (median, mean, fastest) = (shown(summary.median), shown(summary.mean), shown(summary.best));
    let mean_hint = format!("SD {:.0} ms, {} trials", summary.sd, summary.count);
    let false_starts = entry.false_starts.to_string();
    let lapses = (entry.trials.len() - summary.count).to_string();
    let resolution = format!("±{:.1} ms", entry.frame_ms * 0.5);
    let fps = if entry.frame_ms > 0.0 { 1000.0 / entry.frame_ms } else { 0.0 };
    let resolution_hint = format!("at {fps:.0} fps");

    let (compare_tone, compare_title, compare_text) = if corrected {
        (
            TEXT_ACCENT,
            format!("Corrected by your {offset:.0} ms latency offset"),
            format!(
                "Times here are the measured ones minus that offset: as right as the offset is, and only then comparable with web tests or other setups. {MEANING}"
            ),
        )
    } else {
        (
            TEXT_WARN,
            "Raw times: not comparable with web tests or other setups".to_string(),
            format!(
                "They include your display and mouse delay, 10 to 30 ms on gaming setups: set the latency offset in Settings > Video to remove it. {MEANING}"
            ),
        )
    };
    let note = format!(
        "{SCORED_TRIALS} scored trials after {PRACTICE_TRIALS} practice ones, each after a random wait of {} to {} s. A click before the dot or within {:.0} ms of it is a false start and the trial is taken again; no click within {:.0} ms is a lapse; neither is averaged. Median: half of the valid trials were faster; the middle half lies between the quartiles. A trial is timed from the frame that put the dot on screen to the frame that read the click, so it is good to half a frame: {resolution} at this run's {fps:.0} fps. Two tests' medians differ by chance by up to two standard errors, worked out from each test's spread and number of trials. The score is 100 000 / the measured median in ms, whatever the offset.",
        WAIT_SECS.0,
        WAIT_SECS.1,
        ANTICIPATION_SECS * 1000.0,
        LAPSE_SECS * 1000.0
    );
    bsn! {
        @column()
        Children [
            @row()
            Node { align_items: AlignItems::Stretch }
            Children [
                @stat_hint("Median", median, median_hint)
                --
                {raw}
                --
                @stat_hint("Middle half", middle, spread)
                --
                @stat_hint("Best median", record, record_hint)
            ]
            --
            @trial_plot(entry, summary)
            --
            @wrap_row()
            Children [
                @legend_item(TEXT_STRONG, "a reaction, in trial order (higher is slower)")
                --
                @legend_item(TEXT_WARN, "lapse")
                --
                @legend_item(CHART_BAND, "middle half")
                --
                @legend_item(TEXT_ACCENT, "median")
            ]
            --
            @callout(compare_tone, compare_title, compare_text)
            --
            @row()
            Node { align_items: AlignItems::Stretch }
            Children [
                @stat_hint("Mean", mean, mean_hint)
                --
                @stat_hint("Fastest", fastest, "partly luck")
                --
                @stat_hint("False starts", false_starts, "taken again")
                --
                @stat_hint("Lapses", lapses, "not clicked")
                --
                @stat_hint("Trial timing", resolution, resolution_hint)
            ]
            --
            @details("How this is measured", note)
        ]
    }
}

/// Every scored trial in order, left to right: a dot per reaction (higher is slower), a mark at
/// the top edge per lapse, over the band of the middle half and the median line.
fn trial_plot(entry: &ReactionEntry, summary: Summary) -> impl Scene {
    let offset = entry.offset_ms;
    let slowest = entry.trials.iter().fold(summary.best, |slowest, trial| match trial {
        Trial::Valid(value) => slowest.max(*value),
        Trial::Lapse => slowest,
    });
    // At least 20 ms tall, so a very even test doesn't look scattered.
    let span = (slowest - summary.best).max(20.0);
    let height_of = |value: f32| PLOT_PAD + (slowest - value) / span * (PLOT_H - 2.0 * PLOT_PAD);
    let slots = entry.trials.len().max(SCORED_TRIALS as usize) as f32;
    let line = |top: f32, height: f32| Node {
        position_type: PositionType::Absolute,
        left: px(0),
        right: px(0),
        top: px(top),
        height: px(height),
        ..default()
    };
    let mut marks =
        vec![mark(line(height_of(summary.q3), height_of(summary.q1) - height_of(summary.q3)), CHART_BAND, 0.0)];
    marks.push(mark(line(height_of(summary.median) - 1.0, 2.0), TEXT_ACCENT, 0.0));
    for (index, trial) in entry.trials.iter().enumerate() {
        let (top, color) = match trial {
            Trial::Valid(value) => (height_of(*value) - DOT * 0.5, TEXT_STRONG),
            Trial::Lapse => (2.0, TEXT_WARN),
        };
        let dot = Node {
            position_type: PositionType::Absolute,
            left: percent((index as f32 + 0.5) / slots * 100.0),
            top: px(top),
            width: px(DOT),
            height: px(DOT),
            margin: UiRect::left(px(-DOT * 0.5)),
            ..default()
        };
        marks.push(mark(dot, color, DOT));
    }
    let shown = |value: f32| ms((value - offset).max(0.0));
    let (top, bottom) = (shown(slowest), shown(slowest - span));
    bsn! {
        @row()
        Node { column_gap: px(SPACE_XS), align_items: AlignItems::Stretch }
        Children [
            @column()
            Node {
                width: px(AXIS_W),
                flex_shrink: 0.0,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::End,
                padding: UiRect::vertical(px(3)),
            }
            Children [
                @styled(top, fonts::REGULAR, FONT_MICRO, tokens::TEXT_DIM)
                --
                @styled(bottom, fonts::REGULAR, FONT_MICRO, tokens::TEXT_DIM)
            ]
            --
            Node {
                flex_grow: 1.0,
                flex_basis: px(0),
                height: px(PLOT_H),
                overflow: Overflow::clip(),
                border_radius: BorderRadius::all(px(RADIUS_S)),
            }
            ThemeBackgroundColor(CHART_BG)
            Children [ {marks} ]
        ]
    }
}

/// A mark of the plot: a `color`ed box at `node`, its corners rounded by `radius` px.
fn mark(node: Node, color: ThemeToken, radius: f32) -> impl Scene {
    let node = Node { border_radius: BorderRadius::all(px(radius)), ..node };
    bsn! {
        node
        ThemeBackgroundColor(color)
    }
}
