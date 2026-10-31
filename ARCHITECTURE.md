# Architecture

Bevy 0.20 app (Rust 2024). `main.rs` declares `AppState` (Menu, Playing, Results, Finder, Replay, Routine) and the plugin list. Everything else is a plugin in its own module.

## Modules

| Module | Role |
|---|---|
| `main.rs` | `AppState`, plugin registration. Settings are loaded before the window exists. |
| `persist.rs` | RON persistence for any `Persisted` resource: load (a broken file gets a `.bak` backup), debounced autosave, save on exit (`PersistPlugin<T>`). |
| `settings/` | `mod.rs`: `Settings` and its sub-structs. `fields.rs`: the `SettingField` table (label, limits, get/set), which drives both the UI and `Settings::sanitize`. `sens.rs`: game presets and sensitivity math. |
| `camera.rs` | `PlayerCamera`, `Look`, raw mouse look at `ActiveSens`, FOV, cursor lock. |
| `display.rs`, `level.rs`, `crosshair.rs`, `sfx.rs`, `icon.rs`, `presence.rs` | Window/video, room, crosshair, hit sounds, window icon, Discord Rich Presence. |
| `drill/` | Drills and the run lifecycle (see below). |
| `metrics/` | Pure aim measurements: `flick_probe` (first-flick landing), `bow` (path straightness), `throughput` (click positions in the task-axis frame, effective throughput TPe), `quest` (QUEST+ over target size: posterior, next radius, θ75 estimate), `normal` (Φ). |
| `replay/` | `format.rs`: the binary format (v3 encoder, v1–v3 decoders), save/load/prune, migration. `recorder.rs`: `Recorder`. `player.rs`: the replay player and tracer. `mod.rs`: `ReplayPlugin` and the handler that saves a finished practice run's replay. |
| `history.rs` | Results CSV, comparisons, trends. Records each `RunFinished` and announces it as `RunRecorded`. Also defines `LastRun`. |
| `adaptive_log.rs` | Adaptive-mode results CSV (θ75 per adaptive run, joined to the history row by timestamp); feeds each task's latest θ75 back as the next run's prior. |
| `routine.rs` | Built-in warmups, `Routine`, and the handler for finished routine steps. |
| `custom/` | `mod.rs`: custom scenarios and playlists (`CustomContent`). `share.rs`: `AIMPL1:` share codes. |
| `finder/` | `classic.rs`: A/B bisection. `smart.rs`: the Bayesian-optimisation finder (opening design, expected-improvement trial picks, best range, stopping rule, recommendation). `gp.rs`: its Gaussian process (squared-exponential kernel + trial drift + noise, hyperparameter mixture, joint posterior draws). `mod.rs`: `FinderPlugin` and the handler for finished finder trials (including smart-finder auto-chaining). |
| `analysis.rs` | Weak-spot analysis over flick replays (the Insights tab). |
| `ui/` | All UI. `hud.rs`: in-game HUD. `replay_hud.rs`: replay HUD. `menu/`: one module per tab. `screens/`: results, finder, routine screens. `widgets.rs`, `bindings.rs`, `charts.rs`, `theme.rs`: shared building blocks. |

Dependency direction: `persist`, `metrics` and `drill::kind` are leaves. `drill` uses `settings`, `camera`, `sfx`, `metrics` and `replay::Recorder`. `history`, `routine`, `custom`, `finder`, `replay`, `adaptive_log` and `analysis` use `drill` (`adaptive_log` also uses `history`). `ui` uses everything. No non-UI module imports `ui`, and `drill` does not import `history`, `adaptive_log`, `finder` or `routine`.

## Drills

```
drill/
  mod.rs        DrillPlugin, DrillSet (the frame stages), record_frame
  adaptive.rs   AdaptiveRun: QUEST+ sizing of each new target in adaptive configs
  kind.rs       DrillKind: the persisted id; def() is the only per-kind match
  def.rs        DrillDef: data + spawn hooks; Category, DecayRule, TtkFrom, Environment
  config.rs     DrillConfig (flat, serde-frozen), DrillSpec
  params.rs     ParamId table: custom-editor fields and DrillConfig::sanitize limits
  run.rs        Run, Phase, RunPurpose, ready gate, timer, abort, RunFinished
  stats.rs      RunStats, ScoreModel (score, key stats, HUD line, history columns)
  targets.rs    TargetAssets, Target, Dormant, Slot, NextSpawn, SpawnCtx,
                generic shoot / decay / spawn_pending, TargetKilled / TargetExpired
  geometry.rs   TARGET_DISTANCE, direction, random picks, ray tests
  kinds/        one module per drill: DEF, its components and systems, register()
```

### Frame stages

In `Playing`, `Update` runs these `DrillSet`s in this order (chained; commands are applied between stages):

| Stage | Systems |
|---|---|
| `Start` | `abort_run`, then `ready_gate` (spawns the initial targets via `DEF.spawn_initial`) |
| `Input` | `targets::shoot` (click kills, if `DEF.click_kills`), `reaction::reaction` |
| `Expire` | `targets::decay` |
| `Measure` | `microflick::probe_flicks` (reads `TargetKilled`/`TargetExpired`, and `Added<Target>`), `adaptive::adapt_targets` (scores the previous target, sizes new ones) |
| `Spawn` | `targets::spawn_pending` (calls `DEF.spawn_next` once `NextSpawn` is due) |
| `Move` | `peek::move_peeks`, `tracking::move_strafers` |
| `Score` | `tracking::track` |
| `Record` | `record_frame` (replay sample; skips `Dormant` targets) |
| `Finish` | `tick_run`: writes `RunFinished` when time is up. The HUD updates after this stage. |
| `Log` | `history::record_finished`: appends to the CSV, writes `RunRecorded { run, previous, timestamp }` |
| `Outcome` | `replay::save_finished_replay` (practice and routine runs), `adaptive_log::record_adaptive`, `finder::finder_trial_finished`, `routine::routine_step_finished`, `ui::screens::results::show_results` |

`Input` through `Score` run after `camera::apply_look`. Systems from different drills that share a stage touch disjoint components.

## Recipe: add a drill

Exactly these touch points. The recipe was checked with a throwaway drill that had its own system, and `cargo build`, `cargo clippy --all-targets` (no warnings) and `cargo test` all passed.

1. **New file `src/drill/kinds/<name>.rs`.** It contains:
   - `pub const DEF: DrillDef = DrillDef { … }`. This holds:
     - persisted fields: `name` is the display name and the history CSV id (never change it once released);
     - metadata: `blurb`, `category` (which play-page card it goes on), `base_radius`, `decay`, `score` (an existing `ScoreModel`), `ttk_from`;
     - flags: `click_kills` (false if the drill handles clicks itself), `environment`, `first_flick_probe`, `classic_finder`, `flick_analysis`, `adaptive` (offers adaptive mode; needs one click-killed target at a time that can expire);
     - `params`: the custom-editor fields, as existing `ParamId`s;
     - hooks: `tune_defaults` (or `def::no_tuning`), `spawn_initial`, `respawn` (or `def::no_respawn`), `spawn_next` (or `def::no_spawn_next`).
   - The spawn hooks are plain `fn(&mut SpawnCtx…)`. Use `ctx.spawn_target(position, lifetime)` (it returns `EntityCommands` for drill-specific components), `ctx.lifetime()`, `ctx.config`, `ctx.aim`, `ctx.now`, `ctx.next` (for delayed spawns) and `ctx.occupied`/`Slot` (for layouts).
   - Only if the drill has per-frame behaviour: its components and systems, plus `pub(super) fn register(app: &mut App)`, which adds each system `.in_set(DrillSet::<Stage>)`. Systems can use `Run`'s crate-visible methods: `live()`, `shot()`, `kill(ttk)`, `expire()`, `track_on_target(dt)`, `land(ratio)`. For a target that is visible but not yet something to react to, insert `Dormant`.
2. **`src/drill/kind.rs`:**
   - append the variant at the end of `enum DrillKind`;
   - append it at the end of `DrillKind::ALL` and bump the array length;
   - add one `def()` arm: `DrillKind::<Name> => &kinds::<name>::DEF`.

   Never reorder: replays store the index in `ALL`, and RON files store the variant name. The compiler enforces the `def()` arm. Forgetting the `ALL` entry only hides the drill from lists.
3. **`src/drill/kinds/mod.rs`:**
   - add `pub mod <name>;`;
   - only if step 1 has a `register`, add `<name>::register(app);` in `register`.

Nothing else changes; all of the following comes from `DEF`:
- the play page, the custom editor's "new scenario" buttons and fields, the HUD line, results stats and history columns;
- the Adaptive card on the play page (if `adaptive`);
- classic-finder eligibility, inclusion in the Insights analysis, decay handling, and the replay scenery.

Optional:
- add the drill to a `routine::ROUTINES` plan, or to `finder::smart::focus_drills`;
- add a new `ScoreModel` or `ParamId` only if the existing ones don't fit; each is a table in `stats.rs`/`params.rs`;
- add a new `Environment` variant with its spawn and ray-blocking arms in `targets::{spawn_environment, blocked_at}`.

The save-format tests pin the first seven kinds through `DrillKind::fixture_kinds()`, so appending a kind leaves them green.

## Recipe: add a menu page

1. **New file `src/ui/menu/<page>.rs`** with `pub(super) fn <page>_page(settings: &Settings) -> impl Scene`. Wrap the content in `@page(MenuTab::<Page>)`, using `@main_column()`/`@side_column()` and the widgets in `ui::widgets`/`ui::bindings`. Settings rows use `SettingField`/`Choice`.
2. **`src/ui/menu/mod.rs`:**
   - add `mod <page>;` and add `<page>::<page>_page` to the `use self::{…}` list;
   - add a `MenuTab::<Page>` variant and its `name()` arm;
   - add `@tab_button(MenuTab::<Page>)` in `tab_bar`;
   - add `@<page>_page(&settings)` in `spawn_menu`, as a sibling of the other pages.
3. **Only if the page has systems** (live data, rebuilds): register them in `src/ui/mod.rs` in the `run_if(in_state(AppState::Menu))` tuple.

BSN rules:
- tuple-position arguments must be plain identifiers or literals, so bind locals first;
- components used in `bsn!` need `Component + Clone + Default`.

## Recipe: add a setting

1. Add the field to the settings struct and its `Default`.
2. Add either a `SettingField` variant in `settings/fields.rs` (spec, get and set; the limits and sanitize follow automatically) or a `Choice` variant in `ui/bindings.rs` for discrete values.
3. Add one row in the right `ui/menu/<page>.rs`.

## Persisted formats (never change)

- `DrillKind` variant names and order;
- `DrillDef::name` strings;
- `RunPurpose::csv_name` strings;
- the CSV header and columns;
- the replay `MAGIC`/`VERSION`/layout;
- the `Settings`/`DrillConfig`/`CustomContent` field names (new `DrillConfig` fields need a serde default; `adaptive` is also skipped when unset so existing files and share codes stay byte-identical);
- the `DrillRef`/`SharedDrill` variant names;
- `SHARE_PREFIX`;
- file names;
- the adaptive results CSV header and columns (`adaptive_log::HEADER`) and `QuestMode` names.

The tests in `fixtures/` pin these.
