# Architecture

Bevy 0.20 app (Rust 2024). `main.rs` declares `AppState` (Menu, Playing, Results, Finder, Replay, Routine), its sub-state `PlayState` (Running, Paused, Resuming; exists only in `Playing`) and the plugin list. Everything else is a plugin in its own module.

## Modules

| Module | Role |
|---|---|
| `main.rs` | `AppState`, plugin registration. Settings are loaded before the window exists. |
| `persist.rs` | RON persistence for any `Persisted` resource: load (a broken file gets a `.bak` backup), debounced autosave, save on exit (`PersistPlugin<T>`). |
| `settings/` | `mod.rs`: `Settings` and its sub-structs. `fields.rs`: the `SettingField` table (label, limits, get/set), which drives both the UI and `Settings::sanitize`. `sens.rs`: game presets and sensitivity math. |
| `camera.rs` | `PlayerCamera`, `Look`, raw mouse look at `ActiveSens`, FOV, cursor lock. `ViewCone`: what the camera shows (from its projection, so 16:9-locked or Hor+), and `contains`, whether a direction is on screen with a true angular margin to every edge. |
| `display.rs`, `level.rs`, `crosshair.rs`, `sfx.rs`, `icon.rs`, `presence.rs` | Window/video, room (rebuilt when `video.scene` changes), crosshair, hit sounds, window icon, Discord Rich Presence. Target materials follow `targets.color`/`targets.style` in `drill::targets::sync_target_look`, which also switches the camera's bloom for the Glow style. |
| `viewmodel/` | The optional first-person gun (`video.weapon`: pistol or rifle), cosmetic only: hits stay ray tests against targets. `mesh.rs`: `Sculpt`, a small mesh builder (extruded and lofted profiles, lathed parts, chamfers, per-vertex colors) whose tests check that every model is closed, wound outward and within its vertex budget. `pistol.rs`, `rifle.rs`: the guns, modelled in code. `hands.rs`: the gloved hands on them, which are not: they are the CC0 model WRAD ARMS (`tools/hands/arms.blend`), posed on each gun in Blender by `tools/hands/pose.py` and embedded as `assets/models/<gun>_hands.obj` (flat faces in the gun's space, a finish per face). A gun and its hands are baked into one vertex-colored mesh at startup (and a mirrored one for the left side). After changing a gun's shape or how it is held, pose the hands again: `cargo test export_guns -- --ignored` writes the guns to `target/hands`, then `blender -b tools/hands/arms.blend --python tools/hands/pose.py` rewrites the two OBJ files (Blender is needed only for that, not to build). `flash.rs`: the muzzle flash. `mod.rs`: the guns as children of `PlayerCamera`, shown only in `Playing`, the same size on screen at any FOV or aspect, kept below the area targets appear in (a test enforces it), with a spring recoil and flash on each shot the run counts (`Run::fired`, so automatic fire kicks per round) and a slight lag behind a turning view. |
| `drill/` | Drills and the run lifecycle (see below). |
| `metrics/` | Pure aim measurements: `flick_probe` (first-flick landing), `bow` (path straightness), `throughput` (click positions in the task-axis frame, effective throughput TPe), `quest` (QUEST+ over target size: posterior, next radius, θ75 estimate), `normal` (Φ), `reaction` (median, quartiles, mean and SD of reaction times, the standard error of a median, the Reaction test's score), `steadiness` (hand steadiness in degrees of view: jitter while holding still or following, overshoot, corrections, settling; `mouse_mm` converts to mouse travel), `follow` (how a crosshair follows one moving target through its changes of direction: reaction lag, recovery, lead, jitter), `calm` (the Calm aim test's result: the measures of its four parts and the bands and weights that turn them into 0-100 scores). |
| `replay/` | `format.rs`: the binary format (v3), save/load/prune; decoding is size-capped and rejects unplayable replays. `recorder.rs`: `Recorder`. `player.rs`: the replay player and tracer. `mod.rs`: `ReplayPlugin`, `ReplayDir` (the one directory replays are saved in and loaded from) and the handler that saves a finished practice run's replay. |
| `history.rs` | Results CSV, comparisons, trends. Records each `RunFinished` and announces it as `RunRecorded`. Also defines `LastRun`. |
| `adaptive_log.rs` | Adaptive-mode results CSV (θ75 per adaptive run, joined to the history row by timestamp); feeds each task's latest θ75 back as the next run's prior. |
| `reaction_log.rs` | Reaction test results CSV (`aim_trainer_reaction.csv`): every finished test with all its scored trials, joined to the history row by timestamp; the previous and best results the results card compares with. |
| `calm_log.rs` | Calm aim results CSV (`aim_trainer_calm.csv`): what every finished Calm aim run measured, joined to the history row by timestamp; the results screen reads a run's breakdown and its previous result from it. |
| `routine.rs` | Built-in warmups, `Routine`, and the handler for finished routine steps. `Routine::origin` (`RoutineOrigin`: free, the benchmark, or the plan's session of a day) tells the benchmark log and the plan which routine is theirs. |
| `training/` | Structured training on top of the drills; the whole design is the module doc of `mod.rs`. `mod.rs`: `AimKind` (the six kinds of aim and the drills that train each), `TrainingPlugin`, the player's local days (`local_day`, `today`, `monday`, `weekday`; `utc_offset_secs` is the system time zone's offset from the `time` crate, UTC where it can't be read). `benchmark.rs`: `SCENARIOS` with every parameter pinned (`pinned`), the benchmark as a routine, `BenchmarkLog` (`aim_trainer_benchmark.csv`), the index and the index per kind, and `record_benchmark_step`, which logs a benchmark only when every scenario was played in order. `plan.rs`: `generate` (a pure function of `PlanInputs`), `PlanStore` (`aim_trainer_plan.ron`), `session_routine`, `finish_session`. |
| `custom/` | `mod.rs`: custom scenarios and playlists (`CustomContent`). `share.rs`: `AIMPL1:` share codes. |
| `finder/` | Finding a sensitivity. `limits.rs`: the design of the whole finder (module doc), the calibration's result (`Calibration`, `CalibrationStore`) and what follows from it: limits on the cm/360 axis, the usable range, the conservative pick, the aim style read off the wrist sweep. `probe.rs`: the live measurements (`SweepMeter`, `SteadyMeter`, the `Probe` state machine with its own ready gate, countdown and pause, and `drive_probe`, which runs it in `AppState::Finder`). `checkup.rs`: the calibration and the tired check as sequences of probe steps (and one flick run). `tired.rs`: the tired check's log and verdict. `monitor.rs`: one summary per practice run of a flick drill (`MonitorLog`) and the evidence gate (`status`). `compare.rs`: the long comparison of two sensitivities over days. `stats.rs`: the statistics the gates share (summary, Welch, the 1% t table, daily means). `classic.rs`, `smart.rs`, `gp.rs`: the quick finders, a first look. `mod.rs`: `FinderPlugin`, `unix_now`, and the handler for finished finder-trial runs (quick finders, the tired check's flick part, comparison blocks). |
| `analysis.rs` | Weak-spot analysis over flick replays (the Insights tab). |
| `update.rs` | Auto-update (`Updater`, `UpdateStatus`). A background thread checks the latest GitHub release at startup, verifies a newer one (GitHub's SHA-256 digest, then the Ed25519 release signature from `signing.rs`) and stages it as `<exe>.update`; it never touches the running exe. `main` swaps a staged exe in (two renames, rolled back on failure) before the app starts, or after it quits for Restart, then starts the new exe. Staging and swapping hold `aim_trainer.update.lock` next to the exe, so two running copies never race. Skipped for cargo builds. |
| `test_util.rs` | Tests only: `TempDir`, a temp directory deleted when dropped. |
| `ui/` | All UI (see "UI" below). `hud.rs`: in-game HUD. `tap_hud.rs`: the tap rifle's spread bar and the movement-key hint. `cue_hud.rs`: the banner of cue-by-cue drills, rebuilt only when `CueBanner` changes. `calm_hud.rs`: Calm aim's part strip and part banner. `replay_hud.rs`: replay HUD. `menu/`: the menu window and one module per page. `range.rs`: the sensitivity range chart, its reasons and verdict, shared by the finder page and the calibration result. `screens/`: results (with one file per kind of breakdown card: `taps.rs`, `reaction_result.rs`, `calm.rs`, `follow.rs`), the finder's screens (`finder.rs`, `checkup.rs`, `probe.rs`, `compare.rs`), routine screens and the pause menu. `finder_tour.rs`: the tour's staged finder scenes. `theme.rs`, `widgets.rs`, `bindings.rs`, `charts.rs`: shared building blocks. |

Dependency direction: `persist` and `metrics` are leaves. `drill` uses `settings`, `camera`, `sfx`, `metrics` and `replay::Recorder`; `settings` in turn uses `drill` (`DrillKind`, `SECS_RANGE`), as does `replay`, so those pairs import each other. Within `drill`, `kind` imports `def` and every `kinds::*` module (for `DrillKind::def`). `history`, `routine`, `custom`, `finder`, `adaptive_log`, `reaction_log` and `analysis` use `drill` (the logs also use `history`; `finder` also uses `analysis`, `replay::Recorder`, `persist` and `camera::Look`). `ui` uses everything. No non-UI module imports `ui`, and `drill` does not import `history`, the logs, `finder` or `routine`. `training` uses `routine`, `history`, `analysis`, `calm_log`, `custom`, `persist` and `drill`.

## Drills

```
drill/
  mod.rs        DrillPlugin, DrillSet (the frame stages), record_frame
  adaptive.rs   AdaptiveRun: QUEST+ sizing of each new target in adaptive configs
  kind.rs       DrillKind: the persisted id; def() is the only per-kind match
  def.rs        DrillDef: data + spawn hooks; Category, DecayRule, TtkFrom, Environment
  config.rs     DrillConfig (flat, serde-frozen), DrillSpec
  params.rs     ParamId table: custom-editor fields and DrillConfig::sanitize limits
  run.rs        Run, Phase, RunPurpose, ready gate, timer, RunFinished; Run::fired (every shot,
                for the gun model) and Run::rate (a score the drill sets itself, for tests)
  pause.rs      pause_control (Escape / focus loss), PauseMenu (resume, restart, leave),
                ResumeCountdown, KeepRunsOnFocusLoss
  stats.rs      RunStats, ScoreModel (score, key stats, HUD line, history columns)
  targets.rs    TargetAssets, Target, Dormant, Slot, NextSpawn, SpawnCtx,
                generic shoot / decay / spawn_pending, TargetKilled / TargetExpired
  geometry.rs   TARGET_DISTANCE, direction, random picks, ray tests
  health.rs     Health: hit points of a target that takes more than one hit (or a while of
                fire), and the bar over it, drawn parallel to the screen
  held_fire.rs  HeldFire: the fire button as a gun (10 rounds/s while held) in drills that
                track rather than click (ScoreModel::held_fire); pull works it once a frame,
                book counts a frame's rounds and time on one target
  cue.rs        Cues answered with a click: Frame / Cue (reaction timing in run time),
                ANTICIPATION_SECS, ahead() (where a cue appears), CueBanner
  wander.rs     Wander: smooth random drifting inside a box, curving away from its edges
                (Moving gridshot, Switch tracking, Target switching)
  bot.rs        Bot: a head (the Target) on a body mesh built from BODY, the table the hit test
                uses too; Zone, hit(), spawn_bot; bodies face the eye
  range.rs      The bots' scenery: a deck 1.62 m under the eye (Environment::Range) and the
                pillar to peek round (Environment::Pillar); Scenery root, EyeAnchor
  rifle.rs      The tap rifle of range drills: heat and scatter per shot, TapStats / Leak,
                TapRun, the `fire` system
  movement.rs   PlayerMove: A/D strafing of the player camera with Valorant-like acceleration
                and stopping; the eye is reset when the drill ends
  kinds/        one module per drill: DEF, its components and systems, register()
```

### Frame stages

While a drill is running (`PlayState::Running`), `Update` runs these `DrillSet`s in this order (chained; commands are applied between stages):

| Stage | Systems |
|---|---|
| `Start` | `ready_gate` (spawns the initial targets via `DEF.spawn_initial`) |
| `Input` | `targets::shoot` (click kills, if `DEF.click_kills`), `reaction::reaction`, `rifle::fire` (drills on the range: head and body hits, scatter while the rifle is unsettled or the player moves), `held_fire::pull` (the rounds a held fire button fires this frame, in tracking drills) |
| `Expire` | `targets::decay` |
| `Measure` | `microflick::probe_flicks` (reads `TargetKilled`/`TargetExpired`, and `Added<Target>`), `adaptive::adapt_targets` (scores the previous target, sizes new ones) |
| `Spawn` | `targets::spawn_pending` (calls `DEF.spawn_next` once `NextSpawn` is due), `counter_strafe::post_bot` |
| `Move` | `peek::move_peeks`, `tracking::move_strafers`, `moving_gridshot::drift`, `survival::approach` (also ends a Survival run when the lives are gone), `switch_tracking::wander`, `target_switch::wander`, `strafing_bot::strafe`, `movement::strafe` (moves the player camera), `smooth_tracking::glide` |
| `Score` | `tracking::track` and `smooth_tracking::track` (while fire is held: every round a shot, a hit if on the target, and the time on it counted), `switch_tracking::track_and_break` (the same, and the dot being hit drains, breaks and is replaced), `target_switch::fire` (lights the bot under the crosshair, takes a round of health off it for every round fired, kills and replaces it), `calm_aim::play` (the whole test: changes part by `Run::elapsed`, runs the part's targets, clicks and measurements, and rates and ends the run after the last part) |
| `Finish` | `tick_run`: writes `RunFinished` when time is up. The HUD updates after this stage. |
| `Log` | `history::record_finished`: appends to the CSV, writes `RunRecorded { run, previous, timestamp }` |
| `Outcome` | `finder::monitor::record_monitor` (before the replay is saved: it reads the recorder's frames, which saving takes away), `replay::save_finished_replay` (practice and routine runs), `adaptive_log::record_adaptive`, `reaction_log::record_reaction_test`, `calm_log::record_calm`, `finder::finder_trial_finished`, `routine::routine_step_finished`, `training::benchmark::record_benchmark_step` (after it), `ui::screens::results::show_results` |

`Input` through `Score` run after `camera::apply_look`. Systems from different drills that share a stage touch disjoint components. In `PostUpdate`, after `targets::face_eye`: `health`'s bars and `bot::add_bodies` / `bot::place_bodies`.

### Held fire

Tracking, Smooth tracking, Switch tracking and Target switching only count what the player fires at (`ScoreModel::held_fire`): the crosshair resting on a target without the fire button down scores nothing and drains nothing. `on_target` is the time a target was being hit, `shots` and `rounds_hit` are the rounds fired and those that were on a target, and `accuracy()` is their ratio, so it only judges the fire the player chose to give. Tracking's score is still `on_target / duration` (fire off the target costs no score, it shows in the accuracy); runs from before fire was required have no rounds and show `-` for accuracy in History. A new tracking drill reads `Res<HeldFire>` in `Score` and calls `book` (one target) or counts `rounds()` itself.

### The eye

The eye is the player camera's translation: the origin, except while a drill with `PlayerMove` runs (Counter-strafe), and reset on entering and leaving `Playing`. `face_eye`, the ray tests and the recorder use it. Replays store target positions relative to the eye, so the format is unchanged: in a replay the eye stays at the origin, and a drill whose scenery the eye moves past records the scenery's root (`EyeAnchor`) as a target of radius 0, which the replay moves its `Scenery` root to (`range::follow_replay_eye`; `bot::dress_replay_bots` gives recorded heads their bodies). Spawn hooks that use `ctx.view.contains` assume the eye at the origin.

### Pausing

`pause::pause_control` runs before `Start` in every `PlayState`. Escape or the window losing focus sets `PlayState::Paused`; the state changes between frames, so a frame's stages either all run or none do. In `Paused` and `Resuming` no `DrillSet` runs, `camera::mouse_look` is off and `Run::elapsed` stands still, so targets, timers, the replay recorder and the view stay exactly as they were; drill systems must keep to the `DrillSet`s (or `run_if(in_state(PlayState::Running))`) for that to hold. `Paused` frees the cursor and shows `ui::screens::pause` (Escape or Resume, R or Restart, Q or leave, all through `PauseMenu`). Resuming a live run goes through `Resuming` (the HUD counts `ResumeCountdown` down) before `Running`; a run still at its ready card or first countdown resumes at once. Restart re-enters `Playing` with `Run::restarted()`; leaving goes to `RunPurpose::return_state()` and records nothing.

A cue that is on screen when the run pauses would give unlimited time to look before "reacting": `reaction_test::void_shown_trial` and `go_no_go::void_shown_cue` run on `OnEnter(PlayState::Paused)`, hide it and take the trial again after a new wait. Waits are in run time and unaffected.

### Reaction timing

Reactions are timed in `drill/cue.rs`: from the start of the frame after the one that made the cue visible (when it is presented) to the middle of the frame interval the click arrived in, in run time. That is independent of the frame rate and good to half a frame; the display's and the mouse's delay remain in the number and are what `Settings::latency_offset_ms` is for. Clicks under `ANTICIPATION_SECS` (110 ms) are not reactions.

### Probes

The calibration's and the tired check's measurements that are not drills (sweeps, hold, follow) are `finder::probe::Probe`s. A probe is a resource in `AppState::Finder`; `probe::drive_probe` (before `camera::apply_look`, only while the resource exists) feeds it `Time` deltas and raw mouse counts, turns the view, moves its target and owns the cursor lock. It has its own ready gate (click or Space), countdown and pause (Escape or focus loss; Escape resumes, R restarts the step, Q leaves it; `ui::screens::probe` shows the HUD and the pause menu). While paused `Probe::advance` does nothing, and a resumed hold or follow goes on exactly where it stopped after the same countdown as a drill; a resumed sweep keeps the sweeps already counted and starts the one in progress again. Probes write nothing to the history. The tired check's flick part and the comparison's blocks are ordinary runs with `RunPurpose::FinderTrial`, so pausing them is the drills' pausing.

## Recipe: add a drill

Exactly these touch points. The recipe was checked with a throwaway drill that had its own system, and `cargo build`, `cargo clippy --all-targets` (no warnings) and `cargo test` all passed.

1. **New file `src/drill/kinds/<name>.rs`.** It contains:
   - `pub const DEF: DrillDef = DrillDef { … }`. This holds:
     - persisted fields: `name` is the display name and the history CSV id (never change it once released);
     - metadata: `blurb` (a `fn(&DrillConfig)` so custom scenarios describe their own parameters; `|_| "…".into()` for fixed text), `category` (which group of the Play page it is listed in), `base_radius`, `decay`, `score` (an existing `ScoreModel`), `ttk_from`;
     - flags: `click_kills` (false if the drill handles clicks itself), `environment`, `first_flick_probe`, `classic_finder`, `flick_analysis`, `adaptive` (offers adaptive mode; needs one click-killed target at a time that can expire), `endless` (ignores the drill length and runs until the drill calls `Run::end`; the HUD timer counts up);
     - `params`: the custom-editor fields, as existing `ParamId`s;
     - hooks: `tune_defaults` (or `def::no_tuning`), `spawn_initial`, `respawn` (or `def::no_respawn`), `spawn_next` (or `def::no_spawn_next`).
   - The spawn hooks are plain `fn(&mut SpawnCtx…)`. Use `ctx.spawn_target(position, lifetime)` (it returns `EntityCommands` for drill-specific components), `ctx.lifetime()`, `ctx.config`, `ctx.aim`, `ctx.now`, `ctx.next` (for delayed spawns) and `ctx.occupied`/`Slot` (for layouts). Targets must appear fully on screen relative to the live aim: check positions with `ctx.view.contains(ctx.aim, dir, ctx.margin())` (or pick through `geometry::away_from`, which falls back to `geometry::pull_in`), and keep them between `geometry::FLOOR_PITCH` and `CEILING_PITCH` (`geometry::random_pitch`).
   - Only if the drill has per-frame behaviour: its components and systems, plus `pub(super) fn register(app: &mut App)`, which adds each system `.in_set(DrillSet::<Stage>)`. Systems can use `Run`'s crate-visible methods: `live()`, `shot()` (once per shot or round fired), `kill(ttk)`, `expire()`, `expired()`, `track_on_target(dt)`, `land(ratio)`, `rate(score)` (the run's score, for a `ScoreModel` that isn't computed from the counters), and `end()` to finish the run before its time is up (`tick_run` then announces it with the time played). For a target that is visible but not yet something to react to, insert `Dormant`; for one that takes several hits, insert `health::Health` and take it off with `damage`, which says when it is destroyed (its bar draws itself).
2. **`src/drill/kind.rs`:**
   - append the variant at the end of `enum DrillKind`;
   - append it at the end of `DrillKind::ALL` and bump the array length;
   - add one `def()` arm: `DrillKind::<Name> => &kinds::<name>::DEF`.

   Never reorder: replays store the index in `ALL`, and RON files store the variant name. The compiler enforces the `def()` arm. Forgetting the `ALL` entry only hides the drill from lists.
3. **`src/drill/kinds/mod.rs`:**
   - add `pub mod <name>;`;
   - only if step 1 has a `register`, add `<name>::register(app);` in `register`.

Nothing else changes; all of the following comes from `DEF`:
- the Play page (its tile with your best score, and its detail card: blurb, how it is scored, your progress, Play), the custom editor's "new scenario" buttons and fields, the HUD line, results stats and history columns;
- the adaptive Measure / Train buttons in its detail card (if `adaptive`);
- classic-finder eligibility, inclusion in the Insights analysis, decay handling, and the replay scenery.

Optional:
- add the drill to a `routine::ROUTINES` plan, or to `finder::smart::focus_drills`;
- add a new `ScoreModel` or `ParamId` only if the existing ones don't fit; each is a table in `stats.rs`/`params.rs`. A new `ScoreModel` also needs its "what the score counts" line in `ui/menu/play.rs` (`scoring`), and a new `Category` its color (`group_color`, a token in `ui/theme.rs`); the compiler asks for both;
- a test (`Category::Tests`) has a fixed procedure: it is `endless` and ends itself, `DrillConfig::builtin` leaves it at its defaults (no global size or decay, so no name suffix), it sets its score with `Run::rate` and uses a `ScoreModel` that returns `RunStats::rating`. Any drill with results beyond the counters keeps them in a resource of its own during the run, logs them to a CSV of its own keyed by the history timestamp (`adaptive_log`, `reaction_log`, `calm_log`) and shows them as a card of its own under `ui/screens/`, added in `spawn_results`;
- add a new `Environment` variant with its spawn and ray-blocking arms in `targets::{spawn_environment, blocked_at}`;
- a drill with bots sets `environment: Environment::Range`, `click_kills: false` and `score: ScoreModel::Taps`, and spawns with `bot::spawn_bot(ctx, head)` (head at eye height, `head.y == 0`); `rifle::fire` calls its `respawn` hook on a kill, with the killed bot's `Slot` still in `ctx.occupied`.

The save-format tests pin the first seven kinds through `DrillKind::fixture_kinds()`, so appending a kind leaves them green.

## Screenshots and benchmark

F12 saves the window to `screenshots/`. `aim_trainer --tour <dir>` (`ui/capture.rs`) saves every menu page (the Play, History, Custom and finder pages in more than one state), the ready card and a moment of play of the drills, the pause menu, the routine and finder screens, the replay HUD and the results screens of runs it plays to `<dir>`, then quits (well over a hundred screenshots, several minutes): use it to check UI changes. Screens that take a whole session to reach (a finder result, a routine summary, a test's result, days of monitor data) are staged with made-up numbers (`Stage`; the finder's scenes are in `ui/finder_tour.rs` and stay in memory, never in the player's files; benchmarks and plans likewise, with `BenchmarkLog::stage`). Add a step to `Tour::new` when a new screen or drill should be covered. The tour has no mouse: `Step::Fire` holds the fire button while following the nearest target, `Step::ShootAt`, `Step::Aim` and `Step::Look` place the crosshair, `Step::Hold` / `Step::Release` hold keys (Counter-strafe's A and D), `Stage::ReactionCue` and `Stage::GoNoGoCue` force a cue, and `Stage::HistoryOf` and `Stage::CustomScenario` open a drill's history and a custom scenario of it. A `Stage` that opens a menu page uses `set_if_neq`, so a page that is already open isn't rebuilt under the systems filling it.

`aim_trainer --bench` plays a script of the same kind (`Tour::bench`): the Play and Video menu pages and a few drills (Gridshot, Tracking, Switch tracking, with no input), each warmed up and then measured for 5 s, logging one `bench <scenario>: <fps> fps over <n> frames; frame ms p50 …, p99 …, max …` line per scenario to `aim_trainer.log`, then quits. Use it before and after a change that could cost frame time, in the release profile as well as the dev one.

Both open the window without focus (and keep running at full rate unfocused), so they can run while the PC is in use; `--size <width>x<height>` sets the starting window size (windowed mode). Run them from a scratch directory: settings, history and the log are read from and written to the working directory, and the tour's runs are recorded like practice runs.

### Frame time

The scene is tiny, so the frame is CPU-bound on Bevy's fixed per-frame work, not the GPU. Choices that keep it short (each measured with `--bench`):
- every schedule runs on the single-threaded executor (`SingleThreadedSchedules` in `main.rs`): hundreds of tiny systems cost more to hand out to threads than to run;
- no light clustering: the room has only directional and ambient light, so the cameras use `ClusterConfig::None` and clustering stays on the CPU (`level::spawn_lights`). A point or spot light would need clustering back;
- the black-bar camera only runs when there are bars (`display::fit_viewport`);
- the menu only holds the page that is open (`ui::menu::show_tab`): every UI entity costs a little each frame, shown or not.

## UI

One look for every screen, defined in two files that pages build on and never bypass:

- **`ui/theme.rs`**: the palette (blue-black surfaces, one cyan accent; green, red, amber and gold only carry meaning: better, worse, look here, personal best), every color as a theme token, and the scales: spacing (`SPACE_*`), corner radii (`RADIUS_*`) and type (`FONT_*`: display 56, H1 24, H2 17, stat 19, body 14, small 12.5, micro 11 for capitals and chart labels only; plus the in-game sizes). The Feathers dark theme's semantic colors are replaced with the same palette, so the Feathers controls still in use (number inputs, text inputs, scrollbars) match.
- **`ui/widgets.rs`**: surfaces (`window`, `panel`, `card`, `surface`, `tile`, `scroll_column`), text (`heading`, `subheading`, `label`, `label_dim`, `small`, `overline`, `styled`; `keycap`/`key_hint` for shortcuts), numbers (`stat`, `stat_hint`, `hero`, tables from `table_header`/`header_cell` and `table_row`/`cell`), chart helpers in `ui/charts.rs` (`plot`, `score_bars`, `polyline`; `legend_item` for their legends), and the pieces that carry a screen's message: `callout` (the takeaway of a section in plain words, in a tone color), `details` (the full explanation, folded away behind a link), `empty_state` (what to do when there is nothing to show yet), `numbered_step`.
- **Buttons** are `button_box(Btn, height)` (and `button`, `cta`, `compact_button` on top of it): a headless `bevy_ui_widgets::Button` whose colors follow its `Btn` kind and state (`style_buttons`: resting, hover, pressed, disabled; keyboard focus draws Feathers' focus ring). `Primary` is the one main action of a screen, `Secondary` any other, `Ghost`/`Link`/`Danger` minor ones, `Selected` the chosen one of a set (a setting's choice, the open page, a picked list item) and `Tile` the others of a tile list. Change a button's `Btn` to restyle it; captions use `caption` so they take the button's color.

Rules for pages: no colors, font sizes or the default font of their own (tokens and widgets only); one `Primary` per screen; explanations go next to what they explain (`setting_row_hint`, `stat_hint`) or into a `callout`/`details`, never a wall of small print; an empty list says what to do next; shortcuts are shown as `keycap`s where they apply.

The menu (`ui/menu/mod.rs`) is a window with the navigation rail on the left (`NAV`: Train with Play, Weekly plan, Custom and Sens finder; Progress with History, Benchmark and Insights; Settings) and the open page on the right. Only the open page exists: `show_tab` builds it into `PageHost` when it is opened and despawns the previous one, so the menu's frame time doesn't grow with the number of pages. Everything a page must remember between visits therefore lives in resources (`MenuTab`, `PlaySelection`, `HistoryView`, `CustomSelection`), not in its entities.

- **Play** (`play.rs`): a browser of tiles (warmup routines, then drills by `Category`, each with your personal best) and a detail card for the picked one (`PlaySelection`): what it trains, how it is scored, your best / last / average and recent scores, Play, and the adaptive modes. Clicking the picked tile again also starts it. Tiles are three to a row, the name over the best score, so all twenty drills fit a 720p window; more drills, or a new group, only add tiles, and the browser scrolls once they outgrow it while the detail card stays. The run settings live in `RunSettingsHost`, which shows a note instead while a test is picked (tests ignore them). A run is recorded under the name its settings give it (`DrillSpec::builtin`: `Gridshot · size 0.75x`), so the tiles' bests and the detail card read the history under that name, not the drill's bare one, and follow the run settings as they change.
- **Weekly plan** (`plan.rs`): `refresh_plan` makes this week's plan when the page opens or a choice changes and the stored plan is out of date; only then does it analyse the replays for the Insights weak spot. `rebuild_plan` shows today's session (its blocks with a reason each, one `Primary`), the week with what is done, the choices (days a week, focus) and what changed since last week's plan and why.
- **Benchmark** (`benchmark.rs`): `rebuild_benchmark` shows the index with its chart, each kind's trend, when the next one is due, the scenarios and Start. `screens/benchmark.rs` replaces the routine summary when a benchmark routine ends, and the routine break screen has no "Skip step" in a benchmark.
- **History** and **Insights** are filled by systems when built (`rebuild_history`, `rebuild_insights`), **Custom** whenever its selection changes. History lists played drills first, most recent on top, and folds built-in drills without runs under "Not played yet"; `HistoryView` is `None` until a drill is picked, which shows the most recently played one. Systems that queue commands on a page's entities run `.after(menu::show_tab)`: it may be replacing that page in the same frame.
- **Sens finder** (`finder.rs`) is filled by `rebuild_finder` when built and whenever `FinderView` changes (mark it changed after anything that alters what the page shows): 1 the range (`ui/range.rs`), 2 the monitor, 3 the checks, 4 the quick finders, whose settings are the page's second view (`FinderView::Quick`). In `AppState::Finder`, `screens::finder::spawn_finder_screen`, `screens::checkup::spawn_checkup_screen` and `screens::compare::spawn_compare_screen` each show their screen if their resource is present (`Finder`/`SmartFinder`, `Checkup`, `CompareSession`); only one is present at a time.
- **Results** (`screens/results.rs`): the score with its verdict and comparison, then one card per breakdown the run has, each a `callout` with the takeaway, the numbers, and `details`: `precision` (flick drills), `adaptive_result` (adaptive runs), `taps` (bot drills), `reaction_result` (Reaction test), `calm::calm_result` (Calm aim, from the calm log) and `follow::direction_changes` (Tracking and Smooth tracking, worked out from the replay just saved). A new kind of run adds its own card to that list. A score model whose numbers all live on its card returns no key stats.
- **In game**: the HUD bar, ready card and countdown (`hud.rs`) stay in the upper part of the screen, clear of the crosshair; the pause menu (`screens/pause.rs`) shows its three actions with their keys.

## Recipe: add a menu page

1. **New file `src/ui/menu/<page>.rs`** with `pub(super) fn <page>_page(settings: &Settings) -> impl Scene`. Wrap the content in `@page(MenuTab::<Page>, bsn_list! { … })` (it adds the page's title and summary), using `@main_column()`/`@side_column()` and the widgets in `ui::widgets`/`ui::bindings`. Settings rows use `SettingField`/`Choice` (`number_field`, `choice`, and their `_hint` forms for a line of explanation).
2. **`src/ui/menu/mod.rs`:**
   - add `mod <page>;`;
   - add a `MenuTab::<Page>` variant with its `name()` and `header()` arms (title and one-line summary);
   - append it to `MenuTab::ALL` (the screenshot tour follows that list) and put it in a group of `NAV` (a test checks the two agree);
   - add its arm in `show_tab`, which builds the page when it is opened.
3. **Only if the page has systems** (live data, rebuilds): register them in `src/ui/mod.rs` in the `run_if(in_state(AppState::Menu))` tuple. The page is built anew every time it is opened, so they should fill it on `Added<Marker>` (as `rebuild_insights` does) and keep state in a resource.

BSN rules:
- tuple-position arguments must be plain identifiers or literals, so bind locals first;
- components used in `bsn!` need `Component + Clone + Default`;
- a `Vec` of scenes must hold one concrete type: build its items with one function that takes no `impl Trait` arguments.

## Recipe: add a setting

1. Add the field to the settings struct and its `Default`. The struct's `#[serde(default)]` lets files saved before it load.
2. Add either a `SettingField` variant in `settings/fields.rs` or a `Choice` variant in `ui/bindings.rs` for discrete values. A `SettingField` needs its `spec`, `stored` and `store` arms and an entry in `SettingField::ALL` (bump the array length); `get`, `set` and sanitize follow. The compiler enforces the arms but not the `ALL` entry: a field missing from it is silently never sanitized, and `sanitize_uses_field_limits` loops over `ALL` too.
3. Add one row in the right `ui/menu/<page>.rs`.
4. Leave `fixtures/settings.ron` alone: `loads_v1_settings_fixture` checks that the v1.0.0 file still loads, with the new field at its default.

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
- the adaptive results CSV header and columns (`adaptive_log::HEADER`) and `QuestMode` names;
- the reaction results CSV: the file name `aim_trainer_reaction.csv` and its header `unix_time,drill,median_ms,false_starts,frame_ms,offset_ms,trials_ms` (`trials_ms`: the scored trials in order, `;`-separated, one decimal, `lapse` for a lapse; all times raw; the game recomputes from `trials_ms`);
- the calm results CSV: the file name `aim_trainer_calm.csv`, its header and columns (`calm_log::HEADER`); and what a Calm aim score means: the bands and weights in `metrics/calm.rs`, the steadiness definitions they score (`metrics/steadiness.rs`) and the test's procedure in `kinds/calm_aim.rs` (`PLAN`, `INTRO_SECS`, the part constants);
- the score formulas, once released: every `ScoreModel` arm, `metrics::reaction::score` and `drill::kinds::go_no_go::score`;
- the release asset names `aim_trainer.exe.zst` and `aim_trainer.exe.zst.sig`, the zstd packing, the signature format (the Ed25519 signature of the `.zst` bytes as 128 lowercase hex digits and a newline), `signing::PUBLIC_KEY`, and `v<major>.<minor>.<patch>` release tags (installed copies look for exactly these; a new key means installed copies refuse every later release).
- the finder's files: `aim_trainer_calibration.ron` (`finder::limits::CalibrationStore`; `Calibration` field names), `aim_trainer_tired.csv` (`unix_time,state,cm_per_360,hold_mm,follow_mm,click_mm,clicks,accuracy`; `state` is `fresh` or `tired`), `aim_trainer_monitor.csv` (`unix_time,drill,kind,sens,cm_per_360,flicks,landing,bias,click_deg,corrections,misses,score`; an empty field is a value the run did not measure), `aim_trainer_comparison.ron` (`finder::compare::CompareStore`; `LongComparison` and `Block` field names, `Side` variant names), and the history names `Tired check` and `Long comparison`;
- the benchmark log: the file name `aim_trainer_benchmark.csv` and its header `benchmark,scenario,run,score,throughput,on_target,cm_per_360` (an empty field: not measured); and what a benchmark means: the scenario history names `<drill> · benchmark`, the scenario set and its pinned values (`training::benchmark::SCENARIOS`, `pinned`: a new `DrillConfig` field must be pinned there too) and the index (`benchmark::index`, `RATIO_LIMITS`, the trimming). Changing any of them makes old benchmarks incomparable;
- the plan file `aim_trainer_plan.ron`: the field names of `PlanStore`, `WeekPlan`, `PlanDay`, `Block`, `Basis`, `WeakSpot`, `Weakest`, `CalmFocus` and the variant names of `Focus`, `AimKind`, `DayKind`, `BlockDrill`, `Why`.

`fixtures/` holds files saved by v1.0.0. `settings.ron` and `custom.ron` are frozen: their tests check they still load into the expected values, with anything added since at its default (separate round-trip tests cover the current writer). `history_rows.csv`, `replay_v3.bin` and `share_code.txt` are also compared byte for byte with what the current code writes. Never regenerate a fixture to make a test pass: a failure means files people already have would no longer load.

## Releases

Releases are built and signed on the maintainer's machine: installed copies only stage a download whose `aim_trainer.exe.zst.sig` verifies with the public key built into them (`signing::PUBLIC_KEY`). The private key is the maintainer's: a file holding the 32-byte Ed25519 seed as 64 hex digits, kept outside the repository and passed to the release tool (`--key <file>`, or `AIM_TRAINER_RELEASE_KEY`).

1. Bump `version` in `Cargo.toml` (and `Cargo.lock`, via any build), commit and push.
2. Tag that commit and push the tag: `git tag v1.2.0 && git push origin v1.2.0`. Only `v<major>.<minor>.<patch>` tags count: installed copies ignore pre-releases.
3. From the repository, with the GitHub CLI (`gh`) logged in, run `cargo run --bin release --features release-tool -- --key <key file> v1.2.0` (`src/bin/release.rs`, not part of the game; the feature builds zstd's encoder for it only). It:
   - refuses a tag that isn't pushed (or differs from GitHub's), one already published, or one whose `Cargo.toml` version differs (the exe reports that version; a mismatch would make installs update on every start);
   - checks the tag out into a fresh worktree, so local edits never ship, and there runs `cargo fmt --check`, clippy (`--all-features`), the tests and `cargo build --release --locked`, with the toolchain from `rust-toolchain.toml` and a target directory shared between releases (`target/release-tool`);
   - packs the exe with zstd level 19, unpacks it with `ruzstd` like the updater and compares, signs the `.zst` with the key file and checks the signature against `signing::PUBLIC_KEY`;
   - uploads `aim_trainer.exe` (for manual downloads), `aim_trainer.exe.zst` (for the updater) and `aim_trainer.exe.zst.sig` to a **draft** release, then publishes it, so `releases/latest` never shows it unsigned.

GitHub records each asset's SHA-256 digest on upload; `update.rs` checks it first, then the signature. A release without a valid signature is never staged.
