//! The player's usable sensitivity range as shown on the finder page and at the end of a
//! calibration: a cm/360 axis with every limit drawn and named, the reasons in plain words,
//! and the verdict on the current sensitivity. See [`crate::finder::limits`].

use bevy::feathers::constants::fonts;
use bevy::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor};
use bevy::feathers::tokens;
use bevy::prelude::*;

use super::charts::*;
use super::theme::*;
use super::widgets::*;
use crate::finder::limits::{
    Calibration, HEAD_DISTANCE_M, HEAD_RADIUS_M, Limits, NOISE_SDS, PLATEAU, PROS, SlowCap, Style, Verdict,
    WRIST_COVER_DEG, sens_for, wrist_cover,
};
use crate::finder::tired::TiredVerdict;
use crate::history::format_date;
use crate::settings::Settings;

/// The axis: cm/360 on a log scale, fast (few centimetres) on the left.
const AXIS: (f32, f32) = (10.0, 120.0);
const TICKS: [f32; 8] = [10.0, 15.0, 20.0, 30.0, 40.0, 60.0, 80.0, 120.0];
const PLOT_H: f32 = 40.0;
const ROW_H: f32 = 16.0;
/// Height of a reference band under the plot.
const BAND_H: f32 = 5.0;
/// Width of a flag centred on a point of the axis, and of a tick's number.
const FLAG_W: f32 = 170.0;
const TICK_W: f32 = 30.0;
/// Room a limit's name needs beside its line.
const NAME_W: f32 = 92.0;

/// Everything the range display is built from.
pub struct RangeView<'a> {
    pub settings: &'a Settings,
    pub calibration: Option<&'a Calibration>,
    /// What the tired checks at the current sensitivity say.
    pub tired: TiredVerdict,
}

impl RangeView<'_> {
    pub fn current_cm(&self) -> f32 {
        self.settings.cm_per_360(self.settings.sens)
    }

    /// The limits, with the fast one moved by a measured tired loss.
    pub fn limits(&self) -> Option<Limits> {
        self.calibration.map(|calibration| Limits::new(calibration, self.tired.loss()))
    }

    pub fn verdict(&self) -> Option<Verdict> {
        self.limits().map(|limits| limits.verdict(self.current_cm()))
    }

    /// The sensitivity of the conservative pick, when the current one is outside the range.
    pub fn pick_sens(&self) -> Option<f32> {
        match self.verdict()? {
            Verdict::TooFast { pick, .. } | Verdict::TooSlow { pick, .. } => {
                Some(round_sens(sens_for(self.settings, pick)))
            },
            Verdict::Keep | Verdict::Empty => None,
        }
    }
}

/// Sensitivities are set to three decimals, as the games show them.
pub fn round_sens(sens: f32) -> f32 {
    (sens * 1000.0).round() / 1000.0
}

fn x(cm: f32, width: f32) -> f32 {
    ((cm.max(1e-3) / AXIS.0).ln() / (AXIS.1 / AXIS.0).ln() * width).clamp(0.0, width)
}

/// A row above or below the plot for labels placed along the axis.
fn label_row(commands: &mut Commands, chart: Entity, width: f32) -> Entity {
    commands.spawn((ChildOf(chart), Node { width: px(width), height: px(ROW_H), flex_shrink: 0.0, ..default() })).id()
}

/// A label `label_w` wide centred on `at` in a [`label_row`], kept inside the chart's width.
#[allow(clippy::too_many_arguments)]
fn label_at(
    commands: &mut Commands,
    row: Entity,
    width: f32,
    label_w: f32,
    at: f32,
    value: String,
    font: &'static str,
    color: ThemeToken,
) {
    let left = (at - label_w * 0.5).clamp(0.0, width - label_w);
    let justify = if left <= 0.0 {
        JustifyContent::FlexStart
    } else if left >= width - label_w {
        JustifyContent::FlexEnd
    } else {
        JustifyContent::Center
    };
    let slot =
        commands.spawn((ChildOf(row), Node { justify_content: justify, ..absolute(left, 0.0, label_w, ROW_H) })).id();
    spawn_in(commands, slot, styled(value, font, FONT_SMALL, color));
}

/// A limit's name in the row above the plot, beside where its line is: on the side `left` if
/// the edge of the axis leaves room for it there, else on the other.
fn limit_name(commands: &mut Commands, row: Entity, width: f32, at: f32, name: &'static str, left: bool) {
    let room = if left { at } else { width - at };
    let left_of_line = left == (room >= NAME_W);
    let (from, justify) =
        if left_of_line { (at - 4.0 - NAME_W, JustifyContent::FlexEnd) } else { (at + 4.0, JustifyContent::FlexStart) };
    let slot =
        commands.spawn((ChildOf(row), Node { justify_content: justify, ..absolute(from, 0.0, NAME_W, ROW_H) })).id();
    spawn_in(commands, slot, styled(name, fonts::BOLD, FONT_SMALL, TEXT_WARN));
}

/// The cm/360 axis: the usable range tinted between its named limits, the current sensitivity
/// and the pick as labelled markers, and the two reference bands under it.
pub fn range_chart(commands: &mut Commands, parent: Entity, width: f32, view: &RangeView) {
    let chart = commands
        .spawn((ChildOf(parent), Node { flex_direction: FlexDirection::Column, row_gap: px(2), ..default() }))
        .id();
    let current = view.current_cm();
    let limits = view.limits();
    let pick = match view.verdict() {
        Some(Verdict::TooFast { pick, .. } | Verdict::TooSlow { pick, .. }) => Some(pick),
        _ => None,
    };

    if let Some(pick) = pick {
        let flags = label_row(commands, chart, width);
        label_at(commands, flags, width, FLAG_W, x(pick, width), format!("pick {pick:.0}"), fonts::BOLD, TEXT_GOOD);
    }
    let flags = label_row(commands, chart, width);
    let you = format!("you: {current:.1} cm/360");
    label_at(commands, flags, width, FLAG_W, x(current, width), you, fonts::BOLD, TEXT_STRONG);
    if let Some(limits) = limits {
        // Each limit's name on its outer side; when they cross, the sides swap with them.
        let names = label_row(commands, chart, width);
        let ordered = limits.fast <= limits.slow;
        limit_name(commands, names, width, x(limits.fast, width), "hand noise", ordered);
        limit_name(commands, names, width, x(limits.slow, width), "pad", !ordered);
    }

    let plot = plot(commands, chart, width, PLOT_H);
    for tick in TICKS {
        commands.spawn((ChildOf(plot), absolute(x(tick, width), 0.0, 1.0, PLOT_H), ThemeBackgroundColor(CHART_GRID)));
    }
    if let Some(limits) = limits {
        if let Some((fast, slow)) = limits.range() {
            let (left, right) = (x(fast, width), x(slow, width));
            commands.spawn((
                ChildOf(plot),
                absolute(left, 0.0, (right - left).max(1.0), PLOT_H),
                ThemeBackgroundColor(CHART_RANGE),
            ));
        }
        // Where the fast limit was before the tired check moved it.
        if limits.fresh_fast < limits.fast {
            let at = x(limits.fresh_fast, width);
            commands.spawn((ChildOf(plot), absolute(at, 0.0, 1.0, PLOT_H), ThemeBackgroundColor(BORDER_STRONG)));
        }
        for limit in [limits.fast, limits.slow] {
            let line = absolute(x(limit, width) - 1.0, 0.0, 2.0, PLOT_H);
            commands.spawn((ChildOf(plot), line, ThemeBackgroundColor(TEXT_WARN)));
        }
        if limits.wrist < AXIS.1 {
            // A cap on the pick, not a limit: a short tick at the bottom.
            let at = x(limits.wrist, width);
            commands.spawn((
                ChildOf(plot),
                Node { border: UiRect::left(px(2)), ..absolute(at - 1.0, PLOT_H * 0.5, 2.0, PLOT_H * 0.5) },
                ThemeBorderColor(BORDER_STRONG),
            ));
        }
    }
    if let Some(pick) = pick {
        commands.spawn((
            ChildOf(plot),
            absolute(x(pick, width) - 1.0, 0.0, 2.0, PLOT_H),
            ThemeBackgroundColor(TEXT_GOOD),
        ));
    }
    commands.spawn((
        ChildOf(plot),
        absolute(x(current, width) - 1.5, 0.0, 3.0, PLOT_H),
        ThemeBackgroundColor(TEXT_STRONG),
    ));

    // Reference bands: what a study and the pros' settings say, for everyone.
    for (band, color) in [(PLATEAU, CHART_BAND), (PROS, CHART_BAR)] {
        let strip = commands.spawn((ChildOf(chart), Node { width: px(width), height: px(BAND_H), ..default() })).id();
        let (left, right) = (x(band.0, width), x(band.1, width));
        commands.spawn((
            ChildOf(strip),
            Node { border_radius: BorderRadius::all(px(2)), ..absolute(left, 0.0, right - left, BAND_H) },
            ThemeBackgroundColor(color),
        ));
    }
    let axis = label_row(commands, chart, width);
    for tick in TICKS {
        label_at(commands, axis, width, TICK_W, x(tick, width), format!("{tick:.0}"), fonts::REGULAR, tokens::TEXT_DIM);
    }
    let legend = spawn_in(commands, chart, bsn! { @wrap_row() Node { column_gap: px(0), row_gap: px(2) } });
    if limits.is_some_and(|limits| limits.range().is_some()) {
        spawn_in(commands, legend, legend_item(CHART_RANGE_EDGE, "your range"));
    }
    spawn_in(commands, legend, legend_item(CHART_BAND, "20-80 cm/360: near-equal aim in a study"));
    spawn_in(commands, legend, legend_item(CHART_BAR, "47-71: middle half of Valorant pros"));
}

/// A named fact with the sentence that explains it, marked in `tone`.
fn reason(tone: ThemeToken, title: String, text: String) -> impl Scene {
    bsn! {
        @row()
        Node { column_gap: px(SPACE_M), align_items: AlignItems::Stretch }
        Children [
            @marker(tone)
            --
            @column()
            Node { flex_grow: 1.0, flex_basis: px(0), row_gap: px(1) }
            Children [ @label_strong(title) -- @small(text) ]
        ]
    }
}

/// The three measurements and what each means for the range, in plain words.
pub fn reasons(commands: &mut Commands, parent: Entity, view: &RangeView) {
    let (Some(calibration), Some(limits)) = (view.calibration, view.limits()) else { return };
    let noise = if limits.fresh_fast < limits.fast {
        format!(
            "Your hand wobbles {:.2} mm following a slow target ({:.2} mm holding still), and your control error grows {:.0}% when tired. Faster than {:.0} cm/360 ({:.0} when fresh) that is bigger than a head at {HEAD_DISTANCE_M:.0} m.",
            calibration.follow_mm,
            calibration.hold_mm,
            (limits.fast / limits.fresh_fast - 1.0) * 100.0,
            limits.fast,
            limits.fresh_fast,
        )
    } else {
        format!(
            "Your hand wobbles {:.2} mm following a slow target ({:.2} mm holding still). Faster than {:.0} cm/360 that wobble is bigger than a head at {HEAD_DISTANCE_M:.0} m.",
            calibration.follow_mm, calibration.hold_mm, limits.fast,
        )
    };
    let fast = format!("Hand noise: not faster than {:.0} cm/360", limits.fast);
    spawn_in(commands, parent, reason(TEXT_WARN, fast, noise));

    let pad = format!(
        "Your comfortable sweep without lifting is {:.1} cm. Slower than {:.0} cm/360 a 180° turn no longer fits in one sweep.",
        calibration.sweep_cm, limits.slow,
    );
    spawn_in(commands, parent, reason(TEXT_WARN, format!("Pad: not slower than {:.0} cm/360", limits.slow), pad));

    let current = view.current_cm();
    let cover = wrist_cover(calibration.wrist_cm, current);
    let hfov = view.settings.hfov;
    let style = match Style::of(calibration.wrist_cm, current, hfov) {
        Style::Wrist => "the whole view, so you can aim from the wrist alone",
        Style::Hybrid => "most of the view, wide flicks need the forearm",
        Style::Arm => "a small part of the view, so flicks come from the arm and the wrist finishes them",
    };
    let wrist = format!(
        "With the forearm planted you sweep {:.1} cm, which turns {cover:.0}° of the {hfov:.0}° you see at your sensitivity: {style}.",
        calibration.wrist_cm,
    );
    spawn_in(commands, parent, reason(BORDER_STRONG, "Wrist and fingers".to_string(), wrist));
}

/// The takeaway about the current sensitivity: tone, title and one or two sentences.
pub fn verdict_text(view: &RangeView) -> Option<(ThemeToken, String, String)> {
    let limits = view.limits()?;
    let (sens, current) = (view.settings.sens, view.current_cm());
    let cap = |cap: SlowCap| match cap {
        SlowCap::Pad => "with room left on your pad".to_string(),
        SlowCap::Wrist => format!("where your wrist still covers {WRIST_COVER_DEG:.0}°"),
        SlowCap::Plateau => "inside what the study measured".to_string(),
    };
    Some(match limits.verdict(current) {
        Verdict::Keep => (
            TEXT_GOOD,
            format!("Keep {sens:.3}: it is inside your range"),
            format!(
                "{current:.1} cm/360 fits your pad and your hand. Inside the range scores cannot single out one best value, so there is nothing to gain from changing."
            ),
        ),
        Verdict::TooFast { pick, cap: by } => (
            TEXT_WARN,
            format!("Too fast for your hand: try {:.3}", round_sens(sens_for(view.settings, pick))),
            format!(
                "At {current:.1} cm/360 your hand's wobble is bigger than a head at {HEAD_DISTANCE_M:.0} m. {pick:.0} cm/360 is on the slower side of your range, {}. The long comparison can check it before you commit.",
                cap(by)
            ),
        ),
        Verdict::TooSlow { pick, cap: by } => (
            TEXT_WARN,
            format!("Too slow for your pad: try {:.3}", round_sens(sens_for(view.settings, pick))),
            format!(
                "At {current:.1} cm/360 a 180° turn does not fit in one sweep. {pick:.0} cm/360 is on the slower side of your range, {}. The long comparison can check it before you commit.",
                cap(by)
            ),
        ),
        Verdict::Empty => (
            TEXT_BAD,
            "No sensitivity fits both limits".to_string(),
            format!(
                "Your hand needs {:.0} cm/360 or slower, your pad allows {:.0} at most. Only a longer sweep (a larger pad, more room on it) or a steadier hand opens a range; no pick is made.",
                limits.fast, limits.slow
            ),
        ),
    })
}

/// When the calibration was done and with which DPI, with a warning if the DPI setting has
/// changed since.
pub fn calibrated_note(calibration: &Calibration, settings: &Settings) -> String {
    let base = format!("Calibrated {} at {:.0} DPI", format_date(calibration.unix_time), calibration.dpi);
    if (calibration.dpi - settings.dpi).abs() > 0.5 {
        format!("{base}. Your DPI setting is {:.0} now: redo it if the old value was wrong.", settings.dpi)
    } else {
        base
    }
}

/// What the numbers rest on, for the folded explanation.
pub fn assumptions() -> String {
    format!(
        "Centimetres come from mouse counts and your DPI setting. A head is taken as {:.0} cm across at {HEAD_DISTANCE_M:.0} m (the trainer's Head level target at a common Valorant duel distance), and the wobble that must fit inside it as {NOISE_SDS:.0} standard deviations, measured at the sensitivity you calibrated at and assumed to be the same in millimetres at others. The pad limit asks for a 180° turn in one sweep. The pick sits two thirds of the way to the slow end of the range, no slower than where your wrist covers {WRIST_COVER_DEG:.0}° or the study's {:.0} cm/360. The study (Boudaoud, Spjut, Kim, NVIDIA 2022) found aim near-equal from {:.0} to {:.0} cm/360; the pros' band is community statistics.",
        HEAD_RADIUS_M * 200.0,
        PLATEAU.1,
        PLATEAU.0,
        PLATEAU.1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_axis_is_logarithmic_and_clamped() {
        assert_eq!(x(10.0, 480.0), 0.0);
        assert_eq!(x(120.0, 480.0), 480.0);
        // 34.6 is the geometric middle of 10 and 120.
        assert!((x((10.0f32 * 120.0).sqrt(), 480.0) - 240.0).abs() < 1e-2);
        assert_eq!(x(3.0, 480.0), 0.0);
        assert_eq!(x(400.0, 480.0), 480.0);
        assert_eq!(x(0.0, 480.0), 0.0);
    }
}
