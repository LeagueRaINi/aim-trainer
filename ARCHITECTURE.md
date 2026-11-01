# Architecture

Bevy 0.20 app (Rust 2024). `main.rs` declares `AppState` (Menu, Playing, Results, Finder, Replay, Routine) and the plugin list. Everything else is a plugin in its own module.

## Modules

| Module | Role |
|---|---|
| `main.rs` | `AppState`, plugin registration. Settings are loaded before the window exists. |
| `persist.rs` | RON persistence for any `Persisted` resource: load (a broken file gets a `.bak` backup), debounced autosave, save on exit (`PersistPlugin<T>`). |
| `settings/` | `mod.rs`: `Settings` and its sub-structs. `fields.rs`: the `SettingField` table (label, limits, get/set), which drives both the UI and `Settings::sanitize`. `sens.rs`: game presets and sensitivity math. |
| `camera.rs` | `PlayerCamera`, `Look`, raw mouse look at `ActiveSens`, FOV, cursor lock. `ViewCone`: what the camera shows (from its projection, so 16:9-locked or Hor+), and `contains`, whether a direction is on screen with a true angular margin to every edge. |
| `display.rs`, `level.rs`, `crosshair.rs`, `sfx.rs`, `icon.rs`, `presence.rs` | Window/video, room (rebuilt when `video.scene` changes), crosshair, hit sounds, window icon, Discord Rich Presence. Target materials follow `targets.color`/`targets.style` in `drill::targets::sync_target_look`, which also switches the camera's bloom for the Glow style. |
| `viewmodel.rs` | The optional gun model (`video.weapon`: pistol or rifle): low-poly guns built from primitives and baked into one vertex-colored mesh each at startup, children of `PlayerCamera`, the chosen one shown only in `Playing`, kept the same size on screen at any FOV/aspect, with a spring recoil on clicks of a live `Run`. Cosmetic only: hits stay ray tests against targets. |
| `drill/` | Drills and the run lifecycle (see below). |
| `metrics/` | Pure aim measurements: `flick_probe` (first-flick landing), `bow` (path straightness), `throughput` (click positions in the task-axis frame, effective throughput TPe), `quest` (QUEST+ over target size: posterior, next radius, θ75 estimate), `normal` (Φ). |
| `replay/` | `format.rs`: the binary format (v3), save/load/prune; decoding is size-capped and rejects unplayable replays. `recorder.rs`: `Recorder`. `player.rs`: the replay player and tracer. `mod.rs`: `ReplayPlugin`, `ReplayDir` (the one directory replays are saved in and loaded from) and the handler that saves a finished practice run's replay. |
| `history.rs` | Results CSV, comparisons, trends. Records each `RunFinished` and announces it as `RunRecorded`. Also defines `LastRun`. |
| `adaptive_log.rs` | Adaptive-mode results CSV (θ75 per adaptive run, joined to the history row by timestamp); feeds each task's latest θ75 back as the next run's prior. |
| `routine.rs` | Built-in warmups, `Routine`, and the handler for finished routine steps. |
| `custom/` | `mod.rs`: custom scenarios and playlists (`CustomContent`). `share.rs`: `AIMPL1:` share codes. |
| `finder/` | `classic.rs`: A/B bisection. `smart.rs`: the Bayesian-optimisation finder (opening design, expected-improvement trial picks, best range, stopping rule, recommendation). `gp.rs`: its Gaussian process (squared-exponential kernel + trial drift + noise, hyperparameter mixture, joint posterior draws). `mod.rs`: `FinderPlugin` and the handler for finished finder trials (including smart-finder auto-chaining). |
| `analysis.rs` | Weak-spot analysis over flick replays (the Insights tab). |
| `update.rs` | Auto-update (`Updater`, `UpdateStatus`). A background thread checks the latest GitHub release at startup, verifies a newer one (GitHub's SHA-256 digest, then the Ed25519 release signature from `signing.rs`) and stages it as `<exe>.update`; it never touches the running exe. `main` swaps a staged exe in (two renames, rolled back on failure) before the app starts, or after it quits for Restart, then starts the new exe. Staging and swapping hold `aim_trainer.update.lock` next to the exe, so two running copies never race. Skipped for cargo builds. |
| `test_util.rs` | Tests only: `TempDir`, a temp directory deleted when dropped. |
| `ui/` | All UI. `hud.rs`: in-game HUD. `replay_hud.rs`: replay HUD. `menu/`: one module per tab. `screens/`: results, finder, routine screens. `widgets.rs`, `bindings.rs`, `charts.rs`, `theme.rs`: shared building blocks. |

Dependency direction: `persist` and `metrics` are leaves. `drill` uses `settings`, `camera`, `sfx`, `metrics` and `replay::Recorder`; `settings` in turn uses `drill` (`DrillKind`, `SECS_RANGE`), as does `replay`, so those pairs import each other. Within `drill`, `kind` imports `def` and every `kinds::*` module (for `DrillKind::def`). `history`, `routine`, `custom`, `finder`, `adaptive_log` and `analysis` use `drill` (`adaptive_log` also uses `history`). `ui` uses everything. No non-UI module imports `ui`, and `drill` does not import `history`, `adaptive_log`, `finder` or `routine`.

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
  wander.rs     Wander: smooth random drifting inside a box that bounces off its edges
                (Moving gridshot, Switch tracking)
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
| `Move` | `peek::move_peeks`, `tracking::move_strafers`, `moving_gridshot::drift`, `survival::approach` (also ends the run when the lives are gone), `switch_tracking::wander` |
| `Score` | `tracking::track`, `switch_tracking::track_and_break` (drains the dot under the crosshair, kills and replaces it) |
| `Finish` | `tick_run`: writes `RunFinished` when time is up. The HUD updates after this stage. |
| `Log` | `history::record_finished`: appends to the CSV, writes `RunRecorded { run, previous, timestamp }` |
| `Outcome` | `replay::save_finished_replay` (practice and routine runs), `adaptive_log::record_adaptive`, `finder::finder_trial_finished`, `routine::routine_step_finished`, `ui::screens::results::show_results` |

`Input` through `Score` run after `camera::apply_look`. Systems from different drills that share a stage touch disjoint components.

## Recipe: add a drill

Exactly these touch points. The recipe was checked with a throwaway drill that had its own system, and `cargo build`, `cargo clippy --all-targets` (no warnings) and `cargo test` all passed.

1. **New file `src/drill/kinds/<name>.rs`.** It contains:
   - `pub const DEF: DrillDef = DrillDef { … }`. This holds:
     - persisted fields: `name` is the display name and the history CSV id (never change it once released);
     - metadata: `blurb` (a `fn(&DrillConfig)` so custom scenarios describe their own parameters; `|_| "…".into()` for fixed text), `category` (which play-page card it goes on), `base_radius`, `decay`, `score` (an existing `ScoreModel`), `ttk_from`;
     - flags: `click_kills` (false if the drill handles clicks itself), `environment`, `first_flick_probe`, `classic_finder`, `flick_analysis`, `adaptive` (offers adaptive mode; needs one click-killed target at a time that can expire), `endless` (ignores the drill length and runs until the drill calls `Run::end`; the HUD timer counts up);
     - `params`: the custom-editor fields, as existing `ParamId`s;
     - hooks: `tune_defaults` (or `def::no_tuning`), `spawn_initial`, `respawn` (or `def::no_respawn`), `spawn_next` (or `def::no_spawn_next`).
   - The spawn hooks are plain `fn(&mut SpawnCtx…)`. Use `ctx.spawn_target(position, lifetime)` (it returns `EntityCommands` for drill-specific components), `ctx.lifetime()`, `ctx.config`, `ctx.aim`, `ctx.now`, `ctx.next` (for delayed spawns) and `ctx.occupied`/`Slot` (for layouts). Targets must appear fully on screen relative to the live aim: check positions with `ctx.view.contains(ctx.aim, dir, ctx.margin())` (or pick through `geometry::away_from`, which falls back to `geometry::pull_in`), and keep them between `geometry::FLOOR_PITCH` and `CEILING_PITCH` (`geometry::random_pitch`).
   - Only if the drill has per-frame behaviour: its components and systems, plus `pub(super) fn register(app: &mut App)`, which adds each system `.in_set(DrillSet::<Stage>)`. Systems can use `Run`'s crate-visible methods: `live()`, `shot()`, `kill(ttk)`, `expire()`, `expired()`, `track_on_target(dt)`, `land(ratio)`, and `end()` to finish the run before its time is up (`tick_run` then announces it with the time played). For a target that is visible but not yet something to react to, insert `Dormant`.
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

## Screenshots and benchmark

F12 saves the window to `screenshots/`. `aim_trainer --tour <dir>` (`ui/capture.rs`) saves every menu page, the ready card and a moment of play of several drills, and a results screen to `<dir>`, then quits: use it to check UI changes. Add a step to `Tour::new` when a new screen or drill should be covered.

`aim_trainer --bench` plays a script of the same kind (`Tour::bench`): the Play and Video menu pages and a few drills (Gridshot, Tracking, Switch tracking, with no input), each warmed up and then measured for 5 s, logging one `bench <scenario>: <fps> fps over <n> frames; frame ms p50 …, p99 …, max …` line per scenario to `aim_trainer.log`, then quits. Use it before and after a change that could cost frame time, in the release profile as well as the dev one.

Both open the window without focus (and keep running at full rate unfocused), so they can run while the PC is in use; `--size <width>x<height>` sets the starting window size (windowed mode). Run them from a scratch directory: settings, history and the log are read from and written to the working directory, and the tour's runs are recorded like practice runs.

### Frame time

The scene is tiny, so the frame is CPU-bound on Bevy's fixed per-frame work, not the GPU. Choices that keep it short (each measured with `--bench`):
- every schedule runs on the single-threaded executor (`SingleThreadedSchedules` in `main.rs`): hundreds of tiny systems cost more to hand out to threads than to run;
- no light clustering: the room has only directional and ambient light, so the cameras use `ClusterConfig::None` and clustering stays on the CPU (`level::spawn_lights`). A point or spot light would need clustering back;
- the black-bar camera only runs when there are bars (`display::fit_viewport`).

## Recipe: add a menu page

1. **New file `src/ui/menu/<page>.rs`** with `pub(super) fn <page>_page(settings: &Settings) -> impl Scene`. Wrap the content in `@page(MenuTab::<Page>)`, using `@main_column()`/`@side_column()` and the widgets in `ui::widgets`/`ui::bindings`. Settings rows use `SettingField`/`Choice`.
2. **`src/ui/menu/mod.rs`:**
   - add `mod <page>;` and add `<page>::<page>_page` to the `use self::{…}` list;
   - add a `MenuTab::<Page>` variant and its `name()` arm;
   - append it to `MenuTab::ALL` (the tab strip and the screenshot tour follow that list);
   - add `@<page>_page(&settings)` in `spawn_menu`, as a sibling of the other pages.
3. **Only if the page has systems** (live data, rebuilds): register them in `src/ui/mod.rs` in the `run_if(in_state(AppState::Menu))` tuple.

BSN rules:
- tuple-position arguments must be plain identifiers or literals, so bind locals first;
- components used in `bsn!` need `Component + Clone + Default`.

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
- the release asset names `aim_trainer.exe.zst` and `aim_trainer.exe.zst.sig`, the zstd packing, the signature format (the Ed25519 signature of the `.zst` bytes as 128 lowercase hex digits and a newline), `signing::PUBLIC_KEY`, and `v<major>.<minor>.<patch>` release tags (installed copies look for exactly these; a new key means installed copies refuse every later release).

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
