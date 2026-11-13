//! Screens of the calibration and the tired check ([`Checkup`]): a card before every step saying
//! what to do, and the result with what it means, saved only when the player accepts it.

use bevy::feathers::theme::ThemeBackgroundColor;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;

use super::score::screen_title;
use crate::AppState;
use crate::drill::begin_run;
use crate::finder::checkup::{Checkup, CheckupKind, FLICK_SECS, MIN_CLICKS, Step};
use crate::finder::compare::CompareSession;
use crate::finder::limits::{CalibrationStore, HEAD_DISTANCE_M};
use crate::finder::probe::{FOLLOW_SECS, HOLD_SECS, LEAD_SECS, Probe, ProbeTask, Reach, SWEEPS};
use crate::finder::tired::{Freshness, MIN_CHECKS, TiredLog, TiredVerdict, verdict};
use crate::finder::unix_now;
use crate::settings::Settings;
use crate::ui::charts::spawn_in;
use crate::ui::range::{RangeView, range_chart, reasons, verdict_text};
use crate::ui::theme::*;
use crate::ui::widgets::*;

/// Width of a step's card and of a result.
const STEP_W: f32 = 620.0;
const RESULT_W: f32 = 700.0;

/// A step's name, as its card's title and in the probe's HUD.
pub(super) fn step_title(step: Step) -> &'static str {
    match step {
        Step::Probe(ProbeTask::Sweep(Reach::Arm)) => "Sweep across your pad",
        Step::Probe(ProbeTask::Sweep(Reach::Wrist)) => "Sweep from the wrist",
        Step::Probe(ProbeTask::Hold) => "Hold on the dot",
        Step::Probe(ProbeTask::Follow) => "Follow the dot",
        Step::Flicks => "Precise flicks",
    }
}

/// What to do in a step.
pub(super) fn step_text(step: Step) -> String {
    match step {
        Step::Probe(ProbeTask::Sweep(Reach::Arm)) => format!(
            "Put the mouse where a wide swipe would start. Sweep left and right as far as is comfortable without lifting: {SWEEPS} full sweeps. How far counts, not how fast."
        ),
        Step::Probe(ProbeTask::Sweep(Reach::Wrist)) => format!(
            "Plant your forearm as you do for a precise shot. Sweep left and right with wrist and fingers only, as far as is comfortable: {SWEEPS} full sweeps."
        ),
        Step::Probe(ProbeTask::Hold) => format!(
            "Keep the crosshair on the dot, as still as you can, for {:.0} s. It is the size of a head at {HEAD_DISTANCE_M:.0} m. The first {LEAD_SECS:.0} s don't count.",
            LEAD_SECS + HOLD_SECS
        ),
        Step::Probe(ProbeTask::Follow) => format!(
            "The dot drifts slowly from side to side. Keep the crosshair on it for {:.0} s. What counts is how much your hand wobbles up and down.",
            LEAD_SECS + FOLLOW_SECS
        ),
        Step::Flicks => format!(
            "{FLICK_SECS:.0} s of small targets. Take your time: only how close to the centre your first click lands counts, not how many you hit."
        ),
    }
}

/// "Calibration" or "Tired check (fresh)".
fn checkup_name(kind: CheckupKind) -> &'static str {
    match kind {
        CheckupKind::Calibration => "Calibration",
        CheckupKind::Tired(Freshness::Fresh) => "Tired check: fresh",
        CheckupKind::Tired(Freshness::Tired) => "Tired check: tired",
    }
}

/// The overlay and window of a checkup screen; content is added to the returned entity.
fn panel_in(commands: &mut Commands, width: f32) -> Entity {
    let root = commands.spawn_scene(overlay()).insert(DespawnOnExit(AppState::Finder)).id();
    spawn_in(commands, root, bsn! { @panel() Node { width: px(width), max_height: percent(96) } })
}

/// One bar segment per step: done ones filled, the current one white.
fn step_progress(done: usize, steps: usize) -> impl Scene {
    let pips: Vec<_> = (0..steps)
        .map(|index| {
            let color = match index.cmp(&done) {
                std::cmp::Ordering::Less => ACCENT,
                std::cmp::Ordering::Equal => TEXT_STRONG,
                std::cmp::Ordering::Greater => BORDER_STRONG,
            };
            bsn! {
                Node { flex_grow: 1.0, height: px(4), border_radius: BorderRadius::all(px(2)) }
                ThemeBackgroundColor(color)
            }
        })
        .collect();
    bsn! {
        @row()
        Node { column_gap: px(SPACE_XS) }
        Children [ {pips} ]
    }
}

/// What has been measured so far, as tiles.
fn measured(commands: &mut Commands, parent: Entity, checkup: &Checkup, settings: &Settings) {
    let mut tiles = Vec::new();
    if let Some(cm) = checkup.sweep_cm {
        tiles.push(stat_hint("Sweep", format!("{cm:.1} cm"), "pad and arm"));
    }
    if let Some(cm) = checkup.wrist_cm {
        tiles.push(stat_hint("Wrist sweep", format!("{cm:.1} cm"), "wrist and fingers"));
    }
    if let Some(mm) = checkup.hold_mm {
        tiles.push(stat_hint("Holding still", format!("{mm:.2} mm"), "hand wobble"));
    }
    if let Some(mm) = checkup.follow_mm {
        tiles.push(stat_hint("Following", format!("{mm:.2} mm"), "hand wobble"));
    }
    if let Some(entry) = checkup.tired_entry(settings, 0) {
        tiles.push(stat_hint("Click error", format!("{:.2} mm", entry.click_mm), "of hand travel"));
    }
    if !tiles.is_empty() {
        spawn_in(commands, parent, bsn! { @row() Node { align_items: AlignItems::Stretch } Children [ {tiles} ] });
    }
}

/// The checkup's screen for where it stands: the next step's card, or the result. The probe's
/// HUD takes over while a measurement is live.
#[allow(clippy::too_many_arguments)]
pub fn spawn_checkup_screen(
    mut commands: Commands,
    checkup: Option<Res<Checkup>>,
    probe: Option<Res<Probe>>,
    settings: Res<Settings>,
    calibration: Res<CalibrationStore>,
    tired: Res<TiredLog>,
) {
    let (Some(checkup), None) = (checkup, probe) else { return };
    let commands = &mut commands;
    let name = checkup_name(checkup.kind);
    let steps = checkup.steps().len();
    let Some(step) = checkup.step() else {
        match checkup.kind {
            CheckupKind::Calibration => calibration_result(commands, &checkup, &settings, &tired),
            CheckupKind::Tired(_) => tired_result(commands, &checkup, &settings, &calibration, &tired),
        }
        return;
    };
    let panel = panel_in(commands, STEP_W);
    let context = format!("{name}  ·  Step {} of {steps}", checkup.done() + 1);
    spawn_in(commands, panel, screen_title(context, step_title(step)));
    spawn_in(commands, panel, step_progress(checkup.done(), steps));
    spawn_in(commands, panel, label(step_text(step)));
    if checkup.flicks_too_few {
        let text = format!(
            "The last attempt had fewer than {MIN_CLICKS} measured flicks, too few for a click error. Play it again."
        );
        spawn_in(commands, panel, callout(TEXT_WARN, "Too few clicks", text));
    }
    if checkup.done() == 0 && checkup.kind == CheckupKind::Calibration {
        let text = format!(
            "Mouse counts become centimetres through your DPI setting ({:.0}): check it on the Sensitivity page if you are not sure. Nothing is saved until you accept the result.",
            settings.dpi
        );
        spawn_in(commands, panel, small(text));
    }
    measured(commands, panel, &checkup, &settings);
    let actions = spawn_in(commands, panel, actions(bsn_list! {}));
    let start = spawn_in(commands, actions, bsn! { @button("Start", Btn::Primary) on(start_step) });
    let cancel = spawn_in(commands, actions, bsn! { @button("Cancel", Btn::Secondary) on(leave_finder) });
    if checkup.flicks_too_few {
        // Right after a flick run clicks may still be in flight: the buttons arm after a moment.
        for button in [start, cancel] {
            commands.entity(button).insert((ArmDelay::default(), InteractionDisabled));
        }
    }
    spawn_in(commands, actions, spacer());
    spawn_in(commands, actions, key_hint("Esc", "pauses a step"));
}

fn start_step(
    _: On<Activate>,
    checkup: Res<Checkup>,
    settings: Res<Settings>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    match checkup.step() {
        Some(Step::Probe(task)) => {
            commands.insert_resource(Probe::new(task, &settings));
            // Re-enter: the probe's HUD replaces this card.
            next_state.set(AppState::Finder);
        },
        Some(Step::Flicks) => begin_run(&mut commands, &mut next_state, Checkup::flick_run(&settings)),
        None => {},
    }
}

/// Back to the menu, ending whatever of the finder's procedures is open; nothing unsaved is
/// kept.
pub fn leave_finder(_: On<Activate>, mut commands: Commands, mut next_state: ResMut<NextState<AppState>>) {
    commands.remove_resource::<Checkup>();
    commands.remove_resource::<Probe>();
    commands.remove_resource::<CompareSession>();
    next_state.set(AppState::Menu);
}

/// The finished calibration: the measurements, the range they imply with its reasons, and
/// whether the current sensitivity fits.
fn calibration_result(commands: &mut Commands, checkup: &Checkup, settings: &Settings, tired: &TiredLog) {
    let Some(calibration) = checkup.calibration(settings, unix_now()) else { return };
    let view = RangeView {
        settings,
        calibration: Some(&calibration),
        tired: tired.verdict(settings.cm_per_360(settings.sens)),
    };
    let panel = panel_in(commands, RESULT_W);
    spawn_in(commands, panel, screen_title("Calibration  ·  Result", "Your range"));
    measured(commands, panel, checkup, settings);
    range_chart(commands, panel, RESULT_W - 2.0 * SPACE_XL - 2.0, &view);
    if let Some((tone, title, text)) = verdict_text(&view) {
        spawn_in(commands, panel, callout(tone, title, text));
    }
    let list = spawn_in(commands, panel, bsn! { @column() Node { row_gap: px(SPACE_M) } });
    reasons(commands, list, &view);
    spawn_in(
        commands,
        panel,
        bsn! {
            @actions(bsn_list! {
                @button("Save calibration", Btn::Primary)
                on(save_calibration)
                --
                @button("Discard", Btn::Secondary)
                on(leave_finder)
            })
        },
    );
}

fn save_calibration(
    _: On<Activate>,
    checkup: Res<Checkup>,
    settings: Res<Settings>,
    mut store: ResMut<CalibrationStore>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if let Some(calibration) = checkup.calibration(&settings, unix_now()) {
        store.latest = Some(calibration);
    }
    commands.remove_resource::<Checkup>();
    next_state.set(AppState::Menu);
}

/// The finished tired check: this check's numbers, and what the saved checks say with it.
fn tired_result(
    commands: &mut Commands,
    checkup: &Checkup,
    settings: &Settings,
    calibration: &CalibrationStore,
    log: &TiredLog,
) {
    let Some(entry) = checkup.tired_entry(settings, unix_now()) else { return };
    let panel = panel_in(commands, STEP_W);
    spawn_in(
        commands,
        panel,
        screen_title(format!("{}  ·  Result", checkup_name(checkup.kind)), "Your control just now"),
    );
    let tiles = spawn_in(commands, panel, bsn! { @row() Node { align_items: AlignItems::Stretch } });
    let noise = entry.hold_mm.max(entry.follow_mm);
    spawn_in(commands, tiles, stat_hint("Hand wobble", format!("{noise:.2} mm"), "hold and follow"));
    spawn_in(commands, tiles, stat_hint("Click error", format!("{:.2} mm", entry.click_mm), "from the centre"));
    spawn_in(commands, tiles, stat_hint("Control error", format!("{:.2} mm", entry.control_error()), "both combined"));
    spawn_in(commands, tiles, stat_hint("Accuracy", format!("{:.0}%", entry.accuracy * 100.0), "hits per shot"));

    // What the checks say once this one is saved with them.
    let with_this = verdict(log.at(entry.cm_per_360).chain(std::iter::once(&entry)));
    let (tone, title, text) = tired_text(with_this, calibration.latest.is_some());
    spawn_in(commands, panel, callout(tone, title, text));
    let note = "Millimetres of hand travel, so checks at slightly different sensitivities compare. Control error is the geometric mean of hand wobble and click error. A drop counts only when tired checks are clearly worse than fresh ones, beyond the scatter between checks.";
    spawn_in(commands, panel, details("How it is compared", note));
    spawn_in(
        commands,
        panel,
        bsn! {
            @actions(bsn_list! {
                @button("Save check", Btn::Primary)
                ArmDelay
                InteractionDisabled
                on(save_tired_check)
                --
                @button("Discard", Btn::Secondary)
                ArmDelay
                InteractionDisabled
                on(leave_finder)
            })
        },
    );
}

/// The tired checks' verdict as a takeaway.
fn tired_text(verdict: TiredVerdict, calibrated: bool) -> (ThemeToken, String, String) {
    match verdict {
        TiredVerdict::Collecting { fresh, tired } => (
            TEXT_ACCENT,
            "Not compared yet".to_string(),
            format!(
                "With this one there are {fresh} fresh and {tired} tired checks. They are compared once there are {MIN_CHECKS} of each: run it rested, and again right after a match or a long session."
            ),
        ),
        TiredVerdict::Steady { fresh, tired, change } => (
            TEXT_GOOD,
            "No clear drop when tired".to_string(),
            format!(
                "Tired, your control error is {:+.0}% of fresh, which is inside the scatter of your {fresh} fresh and {tired} tired checks. Your range stays as it is.",
                (change - 1.0) * 100.0
            ),
        ),
        TiredVerdict::Drops { fresh, tired, loss } => (
            TEXT_WARN,
            format!("Control drops {:.0}% when tired", (loss - 1.0) * 100.0),
            format!(
                "{tired} tired checks against {fresh} fresh ones, clear of their scatter. {}",
                if calibrated {
                    "Your range's fast limit moves by the same factor, so the tired hand still fits a head."
                } else {
                    "Calibrate to see what that means for your range."
                }
            ),
        ),
    }
}

fn save_tired_check(
    _: On<Activate>,
    checkup: Res<Checkup>,
    settings: Res<Settings>,
    mut log: ResMut<TiredLog>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if let Some(entry) = checkup.tired_entry(&settings, unix_now()) {
        log.record(&entry);
    }
    commands.remove_resource::<Checkup>();
    next_state.set(AppState::Menu);
}
