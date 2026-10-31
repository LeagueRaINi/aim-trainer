//! Widgets bound to [`Settings`]: choice buttons for discrete settings and number inputs for
//! the numeric [`SettingField`]s, plus the systems that apply and highlight them.

use bevy::feathers::controls::{
    ButtonVariant, FeathersButton, FeathersNumberInput, HardLimit, NumberInputPrecision, NumberInputStep,
    NumberInputValue, SoftLimit,
};
use bevy::feathers::display::caption;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, ValueChange};

use super::theme::*;
use super::widgets::*;
use crate::drill::DrillKind;
use crate::settings::fields::FieldSpec;
use crate::settings::{
    AspectMode, CROSSHAIR_COLORS, DisplayMode, FinderFocus, FinderMethod, HitSound, PRESETS, SettingField, Settings,
    TARGET_COLORS,
};
use crate::sfx::HitSounds;

/// Width of every number input in a setting row.
pub const FIELD_W: f32 = 120.0;

// ---------------------------------------------------------------------------------------------
// Choice buttons: a button that sets one discrete setting and highlights while it is active.
// ---------------------------------------------------------------------------------------------

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub enum Choice {
    Preset(usize),
    FinderDrill(DrillKind),
    Display(DisplayMode),
    Aspect(AspectMode),
    Vsync(bool),
    Msaa(u32),
    HitSound(HitSound),
    CrosshairColor(usize),
    TargetColor(usize),
    FinderMethod(FinderMethod),
    FinderFocus(FinderFocus),
    CenterDot(bool),
    InnerLines(bool),
    Outlines(bool),
    Discord(bool),
}

impl Default for Choice {
    fn default() -> Self {
        Choice::Preset(0)
    }
}

impl Choice {
    fn apply(self, settings: &mut Settings) {
        match self {
            Choice::Preset(index) => settings.apply_preset(&PRESETS[index]),
            Choice::FinderDrill(kind) => settings.finder_drill = kind,
            Choice::Display(mode) => settings.video.display = mode,
            Choice::Aspect(mode) => settings.video.aspect = mode,
            Choice::Vsync(on) => settings.video.vsync = on,
            Choice::Msaa(samples) => settings.video.msaa = samples,
            Choice::HitSound(sound) => settings.audio.hit_sound = sound,
            Choice::CrosshairColor(index) => settings.crosshair.color = CROSSHAIR_COLORS[index].1,
            Choice::TargetColor(index) => settings.targets.color = TARGET_COLORS[index].1,
            Choice::FinderMethod(method) => settings.finder_method = method,
            Choice::FinderFocus(focus) => settings.finder_focus = focus,
            Choice::CenterDot(on) => settings.crosshair.center_dot = on,
            Choice::InnerLines(on) => settings.crosshair.inner_lines = on,
            Choice::Outlines(on) => settings.crosshair.outlines = on,
            Choice::Discord(on) => settings.discord.enabled = on,
        }
    }

    fn is_selected(self, settings: &Settings) -> bool {
        match self {
            Choice::Preset(index) => settings.matching_preset() == Some(&PRESETS[index]),
            Choice::FinderDrill(kind) => settings.finder_drill == kind,
            Choice::Display(mode) => settings.video.display == mode,
            Choice::Aspect(mode) => settings.video.aspect == mode,
            Choice::Vsync(on) => settings.video.vsync == on,
            Choice::Msaa(samples) => settings.video.msaa == samples,
            Choice::HitSound(sound) => settings.audio.hit_sound == sound,
            Choice::CrosshairColor(index) => settings.crosshair.color == CROSSHAIR_COLORS[index].1,
            Choice::TargetColor(index) => settings.targets.color == TARGET_COLORS[index].1,
            Choice::FinderMethod(method) => settings.finder_method == method,
            Choice::FinderFocus(focus) => settings.finder_focus == focus,
            Choice::CenterDot(on) => settings.crosshair.center_dot == on,
            Choice::InnerLines(on) => settings.crosshair.inner_lines == on,
            Choice::Outlines(on) => settings.crosshair.outlines == on,
            Choice::Discord(on) => settings.discord.enabled == on,
        }
    }
}

pub fn choice(text: &'static str, choice: Choice) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { @caption(text) },
        }
        Node { min_width: px(64), height: px(28), padding: UiRect::horizontal(px(SPACE_M)) }
        choice
        on(pick_choice)
    }
}

/// A choice button with a color swatch in front of its caption.
pub fn swatch_choice(text: &'static str, rgb: [u8; 3], choice: Choice) -> impl Scene {
    let swatch = BackgroundColor(Color::srgb_u8(rgb[0], rgb[1], rgb[2]));
    bsn! {
        @FeathersButton {
            @caption: bsn_list! {
                Node { width: px(10), height: px(10), border_radius: BorderRadius::all(px(3)) }
                swatch
                --
                @caption(text)
            },
        }
        Node { min_width: px(64), height: px(28), padding: UiRect::horizontal(px(SPACE_M)), column_gap: px(6) }
        choice
        on(pick_choice)
    }
}

/// Swatch choices for the target colors.
pub fn target_color_choices() -> impl Scene {
    bsn! {
        @wrap_row()
        Children [
            @swatch_choice(TARGET_COLORS[0].0, TARGET_COLORS[0].1, Choice::TargetColor(0))
            --
            @swatch_choice(TARGET_COLORS[1].0, TARGET_COLORS[1].1, Choice::TargetColor(1))
            --
            @swatch_choice(TARGET_COLORS[2].0, TARGET_COLORS[2].1, Choice::TargetColor(2))
            --
            @swatch_choice(TARGET_COLORS[3].0, TARGET_COLORS[3].1, Choice::TargetColor(3))
            --
            @swatch_choice(TARGET_COLORS[4].0, TARGET_COLORS[4].1, Choice::TargetColor(4))
            --
            @swatch_choice(TARGET_COLORS[5].0, TARGET_COLORS[5].1, Choice::TargetColor(5))
        ]
    }
}

/// Swatch choices for Valorant's crosshair colors.
pub fn crosshair_color_choices() -> impl Scene {
    bsn! {
        @wrap_row()
        Children [
            @swatch_choice(CROSSHAIR_COLORS[0].0, CROSSHAIR_COLORS[0].1, Choice::CrosshairColor(0))
            --
            @swatch_choice(CROSSHAIR_COLORS[1].0, CROSSHAIR_COLORS[1].1, Choice::CrosshairColor(1))
            --
            @swatch_choice(CROSSHAIR_COLORS[2].0, CROSSHAIR_COLORS[2].1, Choice::CrosshairColor(2))
            --
            @swatch_choice(CROSSHAIR_COLORS[3].0, CROSSHAIR_COLORS[3].1, Choice::CrosshairColor(3))
            --
            @swatch_choice(CROSSHAIR_COLORS[4].0, CROSSHAIR_COLORS[4].1, Choice::CrosshairColor(4))
            --
            @swatch_choice(CROSSHAIR_COLORS[5].0, CROSSHAIR_COLORS[5].1, Choice::CrosshairColor(5))
            --
            @swatch_choice(CROSSHAIR_COLORS[6].0, CROSSHAIR_COLORS[6].1, Choice::CrosshairColor(6))
            --
            @swatch_choice(CROSSHAIR_COLORS[7].0, CROSSHAIR_COLORS[7].1, Choice::CrosshairColor(7))
        ]
    }
}

fn pick_choice(
    activate: On<Activate>,
    choices: Query<&Choice>,
    mut settings: ResMut<Settings>,
    sounds: Res<HitSounds>,
    mut commands: Commands,
) {
    let Ok(&choice) = choices.get(activate.event_target()) else { return };
    choice.apply(&mut settings);
    if let Choice::HitSound(_) = choice {
        sounds.play(&mut commands, &settings.audio);
    }
}

pub fn highlight_choices(settings: Res<Settings>, mut buttons: Query<(&Choice, &mut ButtonVariant)>) {
    for (&choice, mut variant) in &mut buttons {
        variant.set_if_neq(ButtonVariant::selected(choice.is_selected(&settings)));
    }
}

// ---------------------------------------------------------------------------------------------
// Number fields
// ---------------------------------------------------------------------------------------------

/// A labelled number input row.
pub fn number_field(field: SettingField, settings: &Settings) -> impl Scene {
    let text = field.spec().label;
    setting_row(text, bsn_list! { @number_input(field, settings, FIELD_W) })
}

/// A bare number input of fixed width.
pub fn number_input(field: SettingField, settings: &Settings, width: f32) -> impl Scene {
    let FieldSpec { precision, step, soft, hard, .. } = field.spec();
    let value = field.get(settings);
    let soft = SoftLimit::f32(soft.0..=soft.1);
    let hard = HardLimit::f32(hard.0..=hard.1);
    bsn! {
        Node { display: Display::Flex, width: px(width), flex_shrink: 0.0 }
        Children [
            @FeathersNumberInput
            NumberInputValue::F32(value)
            NumberInputPrecision(precision)
            NumberInputStep(step)
            soft
            hard
            Node { flex_grow: 1.0 }
            field
            on(edit_field)
        ]
    }
}

fn edit_field(
    change: On<ValueChange<f32>>,
    fields: Query<&SettingField>,
    mut settings: ResMut<Settings>,
    sounds: Res<HitSounds>,
    mut commands: Commands,
) {
    let input = change.event_target();
    let Ok(&field) = fields.get(input) else { return };
    // Leaving a field always reports its value; only a real change should mark the settings
    // for saving or replay the volume preview.
    let before = field.get(&settings);
    field.set(settings.bypass_change_detection(), change.value);
    let after = field.get(&settings);
    if after != before {
        settings.set_changed();
    }
    // The widget is controlled: echo the (clamped/rounded) value back so it displays it.
    commands.entity(input).insert(NumberInputValue::F32(after));
    if field == SettingField::Volume && change.is_final && after != before {
        sounds.play(&mut commands, &settings.audio);
    }
}

/// Pushes setting changes made elsewhere (presets, finder results) into the number inputs.
pub fn sync_fields(
    settings: Res<Settings>,
    fields: Query<(Entity, &SettingField, &NumberInputValue)>,
    mut commands: Commands,
) {
    for (entity, field, value) in &fields {
        let wanted = field.get(&settings);
        if !matches!(value, NumberInputValue::F32(shown) if *shown == wanted) {
            commands.entity(entity).insert(NumberInputValue::F32(wanted));
        }
    }
}

/// A setting row with "On" and "Off" choices.
pub fn on_off_row(text: &'static str, on: Choice, off: Choice) -> impl Scene {
    setting_row(text, bsn_list! { @choice("On", on) -- @choice("Off", off) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choosing_a_preset_selects_it_and_keeps_cm_per_360() {
        let mut settings = Settings::default();
        let before = settings.cm_per_360(settings.sens);
        Choice::Preset(1).apply(&mut settings);
        assert!(Choice::Preset(1).is_selected(&settings));
        assert!(!Choice::Preset(0).is_selected(&settings));
        assert!(Choice::Aspect(AspectMode::Native).is_selected(&settings));
        assert!((settings.cm_per_360(settings.sens) - before).abs() < 1e-3);
    }
}
