//! [`DrillDef`]: everything that varies between drills as data, so generic code (scoring, HUD,
//! menus, the custom editor, history, analysis, replays) asks the definition instead of
//! matching on the kind.

use std::borrow::Cow;

use super::DrillConfig;
use super::params::ParamId;
use super::stats::ScoreModel;
use super::targets::SpawnCtx;

/// One built-in drill. Each kind module in [`super::kinds`] defines its `DEF`.
pub struct DrillDef {
    /// Persisted: the built-in drill's history name, the CSV `kind` column and the lookup for
    /// old rows without one. Never change it.
    pub name: &'static str,
    /// One line shown on the play page, the ready card and the custom editor, describing the
    /// given config (custom scenarios show their own numbers).
    pub blurb: fn(&DrillConfig) -> Cow<'static, str>,
    /// Which play-page card it is listed under.
    pub category: Category,
    /// Target radius in meters at size 1.0.
    pub base_radius: f32,
    pub decay: DecayRule,
    /// How runs are scored and which stats are shown.
    pub score: ScoreModel,
    /// What a kill's time is measured from.
    pub ttk_from: TtkFrom,
    /// Whether a click kills the target under the crosshair (the generic shooting). Drills that
    /// handle clicks themselves (or ignore them) set this to false.
    pub click_kills: bool,
    /// Static scenery the drill is played in; replays rebuild it too.
    pub environment: Environment,
    /// Whether the first flick towards each new target is measured (for the smart finder).
    pub first_flick_probe: bool,
    /// Offered as a drill for the classic sensitivity finder.
    pub classic_finder: bool,
    /// Its replays are made of flicks and feed the weak-spot analysis.
    pub flick_analysis: bool,
    /// Offers an adaptive mode (QUEST+ target sizes, see [`DrillConfig::adaptive`]). Needs one
    /// click-killed target at a time that can expire.
    pub adaptive: bool,
    /// Runs until the drill ends it (`Run::end`) rather than for the drill length, so scores
    /// don't depend on the length setting; the HUD timer counts up.
    pub endless: bool,
    /// The custom editor's fields for this kind, after the length, size and (if it decays)
    /// lifetime.
    pub params: &'static [ParamId],
    /// Adjusts [`DrillConfig::defaults`] for this kind.
    pub tune_defaults: fn(&mut DrillConfig),
    /// Spawns the first targets when the countdown starts (or schedules them via `ctx.next`).
    pub spawn_initial: fn(&mut SpawnCtx<'_, '_, '_>),
    /// Replaces a target that was killed by a click or expired; gets the cleared target's slot.
    pub respawn: fn(&mut SpawnCtx<'_, '_, '_>, Option<usize>),
    /// Spawns the next target once the time set in `ctx.next` has come (one-at-a-time drills).
    pub spawn_next: fn(&mut SpawnCtx<'_, '_, '_>),
}

/// The play-page cards, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Aim,
    Tactical,
    Eyes,
}

impl Category {
    pub const ALL: [Category; 3] = [Category::Aim, Category::Tactical, Category::Eyes];

    pub fn title(self) -> &'static str {
        match self {
            Category::Aim => "Aim",
            Category::Tactical => "Valorant / CS2",
            Category::Eyes => "Eye training",
        }
    }
}

/// Whether targets expire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecayRule {
    /// Targets never expire on a timer.
    None,
    /// Uses the global target decay setting when played as a built-in drill.
    Global,
    /// Always decays with its own default lifetime, which is its normal mode (not a variant).
    Own,
}

/// What a kill's time to kill is measured from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TtkFrom {
    /// The target appearing.
    Spawn,
    /// The previous kill: targets wait while you shoot the others (Gridshot).
    PreviousKill,
}

/// Static scenery a drill needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Environment {
    Open,
    /// A cover box targets peek out from.
    Cover,
}

/// `tune_defaults` for drills that use the common defaults as they are.
pub fn no_tuning(_: &mut DrillConfig) {}

/// `respawn` for drills that keep their targets (or replace them in their own systems).
pub fn no_respawn(_: &mut SpawnCtx<'_, '_, '_>, _: Option<usize>) {}

/// `spawn_next` for drills that don't schedule delayed spawns.
pub fn no_spawn_next(_: &mut SpawnCtx<'_, '_, '_>) {}
