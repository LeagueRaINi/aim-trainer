//! The Calm aim breakdown on the results screen: the weakest part and what to practise for it,
//! the four parts with the numbers behind their scores, the change since the last test, and,
//! folded away, how every measure turned into points.

use bevy::feathers::constants::fonts;
use bevy::feathers::tokens;
use bevy::prelude::*;

use crate::calm_log::CalmEntry;
use crate::metrics::calm::{CalmResult, MIN_TARGETS, MIN_WAIT_SECS, Measure, Part, Unit};
use crate::ui::theme::*;
use crate::ui::widgets::*;

/// A part at or above this score has nothing worth pointing at.
const CALM_SCORE: f32 = 90.0;
/// The measures shown on a part's tile: the heaviest ones.
const TILE_MEASURES: usize = 3;

/// A measure's value with its unit.
fn shown(unit: Unit, value: f32) -> String {
    if value.is_nan() {
        return "-".to_string();
    }
    match unit {
        Unit::Degrees => format!("{value:.2}°"),
        Unit::DegreesPerSecond => format!("{value:.2}°/s"),
        Unit::Share => format!("{:.0}%", value * 100.0),
        Unit::Seconds => format!("{:.0} ms", value * 1000.0),
        Unit::Count => format!("{value:.2}"),
    }
}

/// Why a part couldn't be scored, in the player's terms.
fn unjudged(result: &CalmResult, part: Part) -> String {
    match part {
        Part::Smoothness => "The crosshair was never near the dot.".to_string(),
        Part::Stillness => format!(
            "Only {:.1} s of waiting in the ring; it takes {MIN_WAIT_SECS:.0} s to judge.",
            result.stillness.wait_secs
        ),
        Part::Flick => format!("Hit {} of the {MIN_TARGETS} targets it takes to judge.", result.flicks.kills),
        Part::Settle => {
            format!("Stayed on {} of the {MIN_TARGETS} targets it takes to judge.", result.settling.targets)
        },
    }
}

/// The takeaway: the weakest part, the measure that cost it the most, and what to practise.
fn takeaway(result: &CalmResult) -> (ThemeToken, String, String) {
    let part = result.weakest();
    let score = result.score(part);
    if !result.judged(part) {
        let text = format!("{} Play it as its banner says: {}", unjudged(result, part), part.task());
        return (TEXT_WARN, format!("{} could not be judged", part.title()), text);
    }
    match result.costliest(part) {
        Some(measure) if score < CALM_SCORE => (
            TEXT_WARN,
            format!("Weakest part: {} ({score})", part.title()),
            format!(
                "{} at {} cost it {:.0} of its 100 points. {}",
                measure.name,
                shown(measure.unit, measure.value),
                measure.lost(),
                measure.advice
            ),
        ),
        _ => (
            TEXT_GOOD,
            "Calm in all four parts".to_string(),
            format!("Your lowest part, {}, still scored {score}. Nothing here needs work.", part.title()),
        ),
    }
}

/// A part's score against the previous test's: its text and color.
fn change(now: f32, before: Option<f32>) -> (String, ThemeToken) {
    match before.map(|before| now - before) {
        Some(change) if change != 0.0 => (format!("{change:+}"), delta_color(change)),
        Some(_) => ("same".to_string(), tokens::TEXT_DIM),
        None => (String::new(), tokens::TEXT_DIM),
    }
}

/// One part: its score, the change since the last test, and its main numbers.
fn part_tile(result: &CalmResult, previous: Option<&CalmResult>, part: Part) -> impl Scene {
    let score = result.score(part);
    let (change, change_color) = change(score, previous.map(|previous| previous.score(part)));
    let lines: Vec<String> = if result.judged(part) {
        result
            .measures(part)
            .iter()
            .take(TILE_MEASURES)
            .map(|measure| format!("{} {}", measure.name, shown(measure.unit, measure.value)))
            .collect()
    } else {
        vec![unjudged(result, part)]
    };
    let lines: Vec<_> = lines.into_iter().map(small).collect();
    let (title, score) = (part.title(), format!("{score}"));
    bsn! {
        @tile()
        Node { flex_grow: 1.0, flex_basis: px(0) }
        Children [
            @overline(title)
            --
            @row()
            Node { align_items: AlignItems::Baseline }
            Children [
                @styled(score, fonts::BOLD, FONT_STAT, TEXT_STRONG)
                --
                @styled(change, fonts::BOLD, FONT_SMALL, change_color)
            ]
            --
            {lines}
        ]
    }
}

/// How a measure's value became points, e.g. "Jitter 0.12° = 69 points (50% of the part)".
fn arithmetic(measure: &Measure) -> String {
    format!(
        "{} {} = {:.0} points ({:.0}% of the part; 100 at {}, 0 at {}): {}.",
        measure.name,
        shown(measure.unit, measure.value),
        measure.points(),
        measure.weight * 100.0,
        shown(measure.unit, measure.band.calm),
        shown(measure.unit, measure.band.tense),
        measure.meaning
    )
}

/// Every part's measures with their points, weights and limits. `mm_per_degree` is the mouse
/// travel that turned the view by a degree in this run.
fn explanation(result: &CalmResult, mm_per_degree: f32) -> String {
    let mut text = format!(
        "Calm aim scores control, not speed. Each measure earns 100 points at its calm value or better and 0 at its tense value or worse, on a straight line between. A part's score is the weighted mean of its measures; the overall score is the plain mean of the four parts. Degrees are degrees of view: at this run's sensitivity 1° was {mm_per_degree:.2} mm of mouse travel, so the Smoothness jitter of {:.2}° was {:.2} mm of hand.",
        result.smoothness.jitter,
        result.smoothness.jitter * mm_per_degree
    );
    for part in Part::ALL {
        text += &format!("\n\n{} ({}): {}", part.title().to_uppercase(), part.measures(), result.score(part));
        if !result.judged(part) {
            text += &format!("\n{}", unjudged(result, part));
            continue;
        }
        for measure in result.measures(part) {
            text += &format!("\n{}", arithmetic(&measure));
        }
        if part == Part::Smoothness {
            text += &format!(
                "\nJitter is measured while the crosshair was within 3 radii of the dot ({:.0}% of the part), and its points count for that share.",
                result.smoothness.with_target * 100.0
            );
        }
    }
    text
}

/// The breakdown of a Calm aim run, compared with the previous result of the same test.
/// `mm_per_degree` is the mouse travel that turned the view by a degree in the run.
pub fn calm_result(entry: &CalmEntry, previous: Option<&CalmEntry>, mm_per_degree: f32) -> impl Scene {
    let result = &entry.result;
    let before = previous.map(|previous| &previous.result);
    let (tone, headline, advice) = takeaway(result);
    let tiles: Vec<_> = Part::ALL.into_iter().map(|part| part_tile(result, before, part)).collect();
    let overall = result.overall();
    let total = format!("{overall}");
    let last = before.map_or("first test".to_string(), |before| format!("{}", before.overall()));
    let (change, change_color) = match change(overall, before.map(CalmResult::overall)) {
        (text, _) if text.is_empty() => ("-".to_string(), tokens::TEXT_DIM),
        change => change,
    };
    let note = explanation(result, mm_per_degree);
    bsn! {
        @card("Calm aim: the four parts", bsn_list! {
            @callout(tone, headline, advice)
            --
            @row()
            Node { align_items: AlignItems::Stretch }
            Children [ {tiles} ]
            --
            @row()
            Node { align_items: AlignItems::Stretch }
            Children [
                @stat("Overall: mean of the four", total)
                --
                @stat("Your last test", last)
                --
                @stat_colored("Change", change, change_color)
            ]
            --
            @details("How every number became points", note)
        })
    }
}
