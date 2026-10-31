//! Flick weak spots: which flick directions and distances cost you the most, measured from your
//! recent replays, and the Microflick settings that train them.
//!
//! **Flicks.** In replays of flick drills ([`FLICK_KINDS`]), every target that appears during the
//! run and is then killed is one flick, and so is one that expires in a one-target-at-a-time
//! drill. A flick starts when its target appears, or at the previous kill if that was later
//! (Gridshot dots wait while you shoot the others), and ends at the kill or expiry. Targets that
//! were already up when the run went live are skipped (the aim before that isn't recorded), as
//! are flicks under [`MIN_DISTANCE`] degrees.
//!
//! **Per flick**, with angles in (yaw, pitch) degrees:
//! - direction: on-screen direction from the aim to the target, in 8 sectors of 45 degrees;
//! - distance: in [`BANDS`];
//! - landing: how far the first submovement travels along the target direction, as a fraction of
//!   the distance ([`FlickProbe`]; < 1 undershoot, > 1 overshoot), measured where it ends (or at
//!   the kill, if that comes first); off-axis: its sideways error, as a fraction of the distance;
//! - bow: straightness of the flick's main movement ([`tracer::segment_bows`]): its largest
//!   distance from the straight line between its ends, relative to that line's length;
//! - corrections: further submovements after the first one, up to the kill;
//! - time to kill (TTK), missed shots, and whether the target expired.
//!
//! **Expected TTK.** Kill time grows with distance and shrinks with target size, so each kill is
//! compared with what *you* need for it: per drill kind, a least-squares Fitts fit
//! `ttk = a + b * log2(distance / width + 1)` over your kills (TTKs winsorized to the 5th-95th
//! percentile first). Directions and distance bands are then fair to compare.
//!
//! **Weakness score** of a group of flicks (a direction, a distance band, or both). For each
//! metric, the gap `d` between the group and all of your flicks, in units of your own spread of
//! that metric (1.4826 x median absolute deviation, or the standard deviation for counts and
//! rates), clamped to +-3, positive = worse:
//!
//! `score = n / (n + 10) * (0.5 * d_time + 0.5 * mean(d_landing, d_off_axis, d_bow, d_corrections, d_errors))`
//!
//! with medians of TTK / expected TTK, |landing - 1|, |off-axis| and bow, the mean corrections per
//! kill, and errors = (missed shots + expiries) per attempt. Only groups of at least
//! [`MIN_SAMPLES`] flicks are scored; `n / (n + 10)` shrinks small groups toward 0 so a few
//! unlucky flicks can't top the list. Groups scoring over [`MIN_SCORE`] become findings.

use bevy::math::{EulerRot, Quat, Vec2, Vec3};

use crate::{
    replay::{Replay, ReplayFrame, Shot},
    drill::{
        DrillConfig, DrillKind,
        flick_probe::{END_SPEED, FlickProbe, MIN_DISTANCE, START_SPEED, angles, screen_angle, wrap},
    },
    tracer,
};

/// Drills whose replays are made of flicks.
pub const FLICK_KINDS: [DrillKind; 4] = [DrillKind::Microflick, DrillKind::Gridshot, DrillKind::HeadLevel, DrillKind::Peripheral];
/// How many of the newest flick-drill replays are analysed.
pub const RECENT_REPLAYS: usize = 30;
/// Flicks a group needs before it is scored.
pub const MIN_SAMPLES: usize = 8;
/// Flicks needed before anything is shown at all.
pub const MIN_FLICKS: usize = 40;
/// Length of a generated training scenario, seconds.
pub const TRAINING_SECS: f32 = 60.0;
/// Pseudo-count of the small-group shrinkage, see the module docs.
const SHRINK: f32 = 10.0;
/// Lowest weakness score reported as a finding.
const MIN_SCORE: f32 = 0.15;
const MAX_FINDINGS: usize = 3;
/// Metric gaps (in spreads) from which a finding mentions them.
const NOTABLE: f32 = 0.4;
/// Gaps are clamped to this many spreads, so one wild metric can't decide alone.
const MAX_GAP: f32 = 3.0;
/// Targets this close (metres) in consecutive frames are the same target.
const SAME_TARGET: f32 = 1e-3;
/// Half-width of a trained direction: exactly one sector.
const TRAINING_SPREAD: f32 = 22.5;
/// Distance range trained for a direction-only finding, degrees.
const TRAINING_DISTANCE: (f32, f32) = (8.0, 30.0);
/// The longest trained flick, for the open-ended last band.
const TRAINING_MAX: f32 = 60.0;

/// The 8 on-screen flick directions, counter-clockwise from right (sector `i` is centered on
/// `45 * i` degrees).
pub const SECTORS: [&str; 8] = ["Right", "Up-right", "Up", "Up-left", "Left", "Down-left", "Down", "Down-right"];
/// Flick distance bands, degrees.
pub const BANDS: [(f32, f32); 5] = [(MIN_DISTANCE, 8.0), (8.0, 15.0), (15.0, 25.0), (25.0, 40.0), (40.0, f32::INFINITY)];

pub fn band_label(band: usize) -> String {
    match BANDS[band] {
        (lo, hi) if hi.is_finite() => format!("{lo:.0}-{hi:.0}°"),
        (lo, _) => format!("{lo:.0}°+"),
    }
}

// ---------------------------------------------------------------------------------------------
// Flick extraction
// ---------------------------------------------------------------------------------------------

/// One measured flick, see the module docs.
#[derive(Clone, Debug, PartialEq)]
pub struct Flick {
    pub kind: DrillKind,
    /// On-screen direction from the aim to the target, degrees counter-clockwise from right.
    pub angle: f32,
    /// Degrees from the aim to the target when the flick started.
    pub distance: f32,
    /// Angular diameter of the target when it appeared, degrees.
    pub width: f32,
    /// First-submovement landing ratio (1 = on target).
    pub landing: Option<f32>,
    /// Signed sideways error of the first submovement, as a fraction of the distance; positive
    /// is counter-clockwise on screen.
    pub off_axis: Option<f32>,
    /// Bow of the main movement.
    pub bow: Option<f32>,
    /// Submovements after the first one, up to the kill.
    pub corrections: u32,
    /// Seconds from the start to the kill; `None` if the target expired.
    pub ttk: Option<f32>,
    /// Missed shots during the flick.
    pub misses: u32,
}

impl Flick {
    pub fn sector(&self) -> usize {
        (self.angle / 45.0).round() as usize % SECTORS.len()
    }

    pub fn band(&self) -> usize {
        BANDS.iter().rposition(|band| self.distance >= band.0).unwrap_or(0)
    }

    /// Fitts' index of difficulty, bits.
    fn difficulty(&self) -> f32 {
        (self.distance / self.width.max(0.1) + 1.0).log2()
    }
}

struct Alive {
    center: Vec3,
    radius: f32,
    /// Frame it appeared in.
    born: usize,
}

fn aim_angles(frame: &ReplayFrame) -> Vec2 {
    Vec2::new(frame.yaw.to_degrees(), frame.pitch.to_degrees())
}

fn same_target(a: Vec3, b: Vec3) -> bool {
    a.distance_squared(b) < SAME_TARGET * SAME_TARGET
}

/// Every flick in a replay of a flick drill; nothing for other drills.
pub fn extract_flicks(replay: &Replay) -> Vec<Flick> {
    let kind = replay.kind;
    let frames = &replay.frames;
    let (true, Some(first)) = (FLICK_KINDS.contains(&kind), frames.first()) else { return Vec::new() };
    let mut alive: Vec<Alive> = first.targets.iter().map(|&(center, radius)| Alive { center, radius, born: 0 }).collect();
    let mut last_kill = 0;
    // A hit whose target only disappears in a later frame (shouldn't happen: the kill is applied
    // before the frame is recorded) is attributed then.
    let mut pending_hit = false;
    let mut flicks = Vec::new();
    for (i, frame) in frames.iter().enumerate().skip(1) {
        let (gone, kept): (Vec<Alive>, Vec<Alive>) =
            alive.into_iter().partition(|t| !frame.targets.iter().any(|&(c, _)| same_target(c, t.center)));
        alive = kept;
        for &(center, radius) in &frame.targets {
            if !alive.iter().any(|t| same_target(t.center, center)) {
                alive.push(Alive { center, radius, born: i });
            }
        }
        // A kill removes the target in the frame of the shot: the one closest to the crosshair.
        let aim = Quat::from_euler(EulerRot::YXZ, frame.yaw, frame.pitch, 0.0) * Vec3::NEG_Z;
        pending_hit |= frame.shot == Shot::Hit;
        let killed = if pending_hit {
            (0..gone.len()).min_by(|&a, &b| aim.angle_between(gone[a].center).total_cmp(&aim.angle_between(gone[b].center)))
        } else {
            None
        };
        pending_hit &= killed.is_none();
        for (j, target) in gone.iter().enumerate() {
            let hit = killed == Some(j);
            // Gridshot dots wait for you while others are up; one expiring isn't a flick you made.
            if !hit && kind == DrillKind::Gridshot {
                continue;
            }
            let start = if hit { target.born.max(last_kill) } else { target.born };
            flicks.extend(measure(kind, &frames[start..=i], start, target, hit));
        }
        if killed.is_some() {
            last_kill = i;
        }
    }
    flicks
}

/// Measures the flick over `frames` (its first to its last frame); `start` is the index of the
/// first one in the replay.
fn measure(kind: DrillKind, frames: &[ReplayFrame], start: usize, target: &Alive, killed: bool) -> Option<Flick> {
    if start == 0 || frames.len() < 2 {
        return None;
    }
    let points: Vec<(f32, Vec2)> = frames.iter().map(|f| (f.t, aim_angles(f))).collect();
    let from = points[0].1;
    let to = angles(target.center.normalize_or_zero());
    let mut probe = FlickProbe::new(from, to)?;
    let offset = wrap(to - from);

    let mut first = None;
    let mut corrections = 0;
    let mut resting = false;
    for pair in points.windows(2) {
        let (t, aim) = pair[1];
        let landed = probe.update(aim, t - pair[0].0);
        if first.is_none() {
            if let Some(landing) = landed {
                first = Some((landing, probe.off_axis(aim)));
                resting = true;
            }
        } else if resting && probe.speed() > START_SPEED {
            corrections += 1;
            resting = false;
        } else if !resting && probe.speed() < END_SPEED {
            resting = true;
        }
    }
    let (end_t, end_aim) = points[points.len() - 1];
    if killed && first.is_none() {
        first = Some((probe.landing(end_aim), probe.off_axis(end_aim)));
    }
    let width = 2.0 * (target.radius / target.center.length().max(1e-3)).atan().to_degrees();
    Some(Flick {
        kind,
        angle: screen_angle(offset),
        distance: offset.length(),
        width,
        landing: first.map(|f| f.0),
        off_axis: first.map(|f| f.1),
        bow: main_bow(&points),
        corrections,
        ttk: killed.then_some(end_t - points[0].0),
        misses: frames[1..].iter().filter(|f| f.shot == Shot::Miss).count() as u32,
    })
}

/// Bow of the longest movement on the path.
fn main_bow(points: &[(f32, Vec2)]) -> Option<f32> {
    let bows = tracer::segment_bows(points);
    let mut best: Option<(f32, f32)> = None;
    let mut i = 0;
    while i < bows.len() {
        let Some(bow) = bows[i] else {
            i += 1;
            continue;
        };
        // Movements are separated by resting segments, so a run of judged segments is one.
        let start = i;
        while i < bows.len() && bows[i].is_some() {
            i += 1;
        }
        let length = points[start].1.distance(points[i].1);
        if best.is_none_or(|(longest, _)| length > longest) {
            best = Some((length, bow));
        }
    }
    best.map(|(_, bow)| bow)
}

// ---------------------------------------------------------------------------------------------
// Aggregation
// ---------------------------------------------------------------------------------------------

/// Your kill time as a function of difficulty, per drill kind.
struct KillModel(Vec<(DrillKind, f32, f32)>);

impl KillModel {
    fn fit(flicks: &[Flick]) -> Self {
        let mut models = Vec::new();
        for kind in FLICK_KINDS {
            let kills: Vec<(f32, f32)> =
                flicks.iter().filter(|f| f.kind == kind).filter_map(|f| Some((f.difficulty(), f.ttk?))).collect();
            if kills.is_empty() {
                continue;
            }
            let mut ttks: Vec<f32> = kills.iter().map(|k| k.1).collect();
            let (low, high) = (percentile(&mut ttks, 0.05), percentile(&mut ttks, 0.95));
            let points: Vec<(f32, f32)> = kills.iter().map(|&(id, ttk)| (id, ttk.clamp(low, high))).collect();
            let n = points.len() as f32;
            let mean_id = points.iter().map(|p| p.0).sum::<f32>() / n;
            let mean_ttk = points.iter().map(|p| p.1).sum::<f32>() / n;
            let var = points.iter().map(|p| (p.0 - mean_id).powi(2)).sum::<f32>() / n;
            let cov = points.iter().map(|p| (p.0 - mean_id) * (p.1 - mean_ttk)).sum::<f32>() / n;
            let (a, b) = if points.len() >= MIN_SAMPLES && var > 1e-3 {
                let b = (cov / var).max(0.0);
                (mean_ttk - b * mean_id, b)
            } else {
                (median(&mut ttks).unwrap_or(mean_ttk), 0.0)
            };
            models.push((kind, a, b));
        }
        Self(models)
    }

    fn expected(&self, flick: &Flick) -> Option<f32> {
        let &(_, a, b) = self.0.iter().find(|m| m.0 == flick.kind)?;
        Some((a + b * flick.difficulty()).max(0.05))
    }
}

/// One flick's values as used by the statistics.
struct Sample {
    sector: usize,
    band: usize,
    /// The target was to the right of the crosshair on screen.
    right: bool,
    /// More sideways than up/down.
    horizontal: bool,
    landing: Option<f32>,
    off_axis: Option<f32>,
    bow: Option<f32>,
    /// Kills only.
    corrections: Option<f32>,
    ttk: Option<f32>,
    /// TTK over expected TTK.
    ttk_ratio: Option<f32>,
    /// TTK minus expected TTK, seconds.
    ttk_excess: Option<f32>,
    /// Missed shots plus an expiry.
    errors: f32,
    /// Shots plus an expiry: every chance to hit.
    attempts: f32,
}

impl Sample {
    fn new(flick: &Flick, model: &KillModel) -> Self {
        let expected = model.expected(flick);
        let (sin, cos) = flick.angle.to_radians().sin_cos();
        let expired = flick.ttk.is_none() as u32;
        Self {
            sector: flick.sector(),
            band: flick.band(),
            right: cos > 0.0,
            horizontal: cos.abs() >= sin.abs(),
            landing: flick.landing,
            off_axis: flick.off_axis.map(f32::abs),
            bow: flick.bow,
            corrections: flick.ttk.map(|_| flick.corrections as f32),
            ttk: flick.ttk,
            ttk_ratio: flick.ttk.zip(expected).map(|(t, e)| t / e),
            ttk_excess: flick.ttk.zip(expected).map(|(t, e)| t - e),
            errors: (flick.misses + expired) as f32,
            attempts: (flick.misses + 1) as f32,
        }
    }
}

/// The metrics behind the weakness score, see the module docs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Metric {
    Time,
    Landing,
    OffAxis,
    Bow,
    Corrections,
    Errors,
}

const METRICS: [Metric; 6] = [Metric::Time, Metric::Landing, Metric::OffAxis, Metric::Bow, Metric::Corrections, Metric::Errors];

impl Metric {
    fn index(self) -> usize {
        self as usize
    }

    /// The per-flick value, for the median-based metrics.
    fn value(self, sample: &Sample) -> Option<f32> {
        match self {
            Metric::Time => sample.ttk_ratio,
            Metric::Landing => sample.landing.map(|l| (l - 1.0).abs()),
            Metric::OffAxis => sample.off_axis,
            Metric::Bow => sample.bow,
            Metric::Corrections => sample.corrections,
            Metric::Errors => None,
        }
    }

    /// The group's value: a median, the mean corrections, or the pooled error rate. `None`
    /// with fewer than half of [`MIN_SAMPLES`] values.
    fn center(self, samples: &[&Sample]) -> Option<f32> {
        let enough = MIN_SAMPLES / 2;
        match self {
            Metric::Errors => {
                let attempts: f32 = samples.iter().map(|s| s.attempts).sum();
                (samples.len() >= enough).then(|| samples.iter().map(|s| s.errors).sum::<f32>() / attempts)
            }
            Metric::Corrections => {
                let values: Vec<f32> = samples.iter().filter_map(|s| self.value(s)).collect();
                (values.len() >= enough).then(|| mean(&values))
            }
            _ => {
                let mut values: Vec<f32> = samples.iter().filter_map(|s| self.value(s)).collect();
                if values.len() < enough { None } else { median(&mut values) }
            }
        }
    }

    /// Your spread of the metric over all flicks, floored so a near-constant metric can't make
    /// tiny differences look huge.
    fn spread(self, samples: &[&Sample], center: f32) -> f32 {
        match self {
            Metric::Errors => (center * (1.0 - center)).max(0.0).sqrt().max(0.1),
            Metric::Corrections => {
                let values: Vec<f32> = samples.iter().filter_map(|s| self.value(s)).collect();
                let var = values.iter().map(|v| (v - center).powi(2)).sum::<f32>() / values.len().max(1) as f32;
                var.sqrt().max(0.3)
            }
            _ => {
                let mut deviations: Vec<f32> = samples.iter().filter_map(|s| Some((self.value(s)? - center).abs())).collect();
                let floor = match self {
                    Metric::Time => 0.05,
                    Metric::Landing => 0.02,
                    _ => 0.01,
                };
                (1.4826 * median(&mut deviations).unwrap_or(0.0)).max(floor)
            }
        }
    }
}

/// Every metric's overall value and spread, the reference groups are compared with.
struct Reference {
    center: [Option<f32>; 6],
    spread: [f32; 6],
}

impl Reference {
    fn new(samples: &[&Sample]) -> Self {
        let center = METRICS.map(|m| m.center(samples));
        let spread = METRICS.map(|m| center[m.index()].map_or(1.0, |c| m.spread(samples, c)));
        Self { center, spread }
    }
}

/// Statistics of a group of flicks.
#[derive(Clone, Debug, Default)]
pub struct CellStats {
    pub n: usize,
    /// Median first-flick landing ratio.
    pub landing: Option<f32>,
    /// Median |off-axis| error, fraction of the distance.
    pub off_axis: Option<f32>,
    /// Median bow.
    pub bow: Option<f32>,
    /// Mean corrections per kill.
    pub corrections: Option<f32>,
    /// Median time to kill, seconds.
    pub ttk: Option<f32>,
    /// Median TTK minus expected TTK, seconds.
    pub ttk_excess: Option<f32>,
    /// Missed shots plus expiries per attempt.
    pub error_rate: Option<f32>,
    /// Weakness score (module docs); `None` below [`MIN_SAMPLES`] flicks.
    pub score: Option<f32>,
    /// Per-metric gaps to the reference, in spreads (positive = worse), by [`Metric::index`].
    gaps: [Option<f32>; 6],
}

impl CellStats {
    fn new(samples: &[&Sample], reference: &Reference) -> Self {
        let mut landings: Vec<f32> = samples.iter().filter_map(|s| s.landing).collect();
        let mut ttks: Vec<f32> = samples.iter().filter_map(|s| s.ttk).collect();
        let mut excess: Vec<f32> = samples.iter().filter_map(|s| s.ttk_excess).collect();
        let centers = METRICS.map(|m| m.center(samples));
        let gaps = METRICS.map(|m| {
            let i = m.index();
            Some(((centers[i]? - reference.center[i]?) / reference.spread[i]).clamp(-MAX_GAP, MAX_GAP))
        });
        let n = samples.len();
        let score = gaps[Metric::Time.index()].filter(|_| n >= MIN_SAMPLES).map(|time| {
            let errors: Vec<f32> = gaps[1..].iter().flatten().copied().collect();
            let errors = if errors.is_empty() { 0.0 } else { mean(&errors) };
            n as f32 / (n as f32 + SHRINK) * (0.5 * time + 0.5 * errors)
        });
        Self {
            n,
            landing: median(&mut landings),
            off_axis: centers[Metric::OffAxis.index()],
            bow: centers[Metric::Bow.index()],
            corrections: centers[Metric::Corrections.index()],
            ttk: median(&mut ttks),
            ttk_excess: median(&mut excess),
            error_rate: centers[Metric::Errors.index()],
            score,
            gaps,
        }
    }

    /// Median TTK versus expected, relative to your overall median, milliseconds.
    pub fn ttk_vs(&self, overall: &CellStats) -> Option<f32> {
        Some((self.ttk_excess? - overall.ttk_excess?) * 1000.0)
    }
}

/// A group of flicks: a direction, a distance band, or both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    pub sector: Option<usize>,
    pub band: Option<usize>,
}

impl Cell {
    fn contains(&self, sample: &Sample) -> bool {
        self.sector.is_none_or(|s| s == sample.sector) && self.band.is_none_or(|b| b == sample.band)
    }

    /// Whether every flick of `self` is also in `other`.
    fn within(&self, other: &Cell) -> bool {
        other.sector.is_none_or(|s| self.sector == Some(s)) && other.band.is_none_or(|b| self.band == Some(b))
    }

    /// E.g. "Down-left flicks, 15-25°".
    pub fn title(&self) -> String {
        match (self.sector, self.band) {
            (Some(s), Some(b)) => format!("{} flicks, {}", SECTORS[s], band_label(b)),
            (Some(s), None) => format!("{} flicks", SECTORS[s]),
            (None, Some(b)) => format!("{} flicks", band_label(b)),
            (None, None) => "All flicks".to_string(),
        }
    }

    /// Name of the custom scenario training this cell, e.g. "Weak spot: down-left 15-25°".
    pub fn scenario_name(&self) -> String {
        let parts: Vec<String> = [self.sector.map(|s| SECTORS[s].to_lowercase()), self.band.map(band_label)]
            .into_iter()
            .flatten()
            .collect();
        format!("Weak spot: {}", parts.join(" "))
    }

    /// Microflick settings that only spawn this cell's flicks (plus the reset flicks needed to
    /// keep going in one direction, see `constrained_flick_position`), with room for them.
    pub fn training_config(&self, base: DrillConfig) -> DrillConfig {
        let (near, far) = match self.band {
            Some(b) => (BANDS[b].0.max(4.0), BANDS[b].1.min(TRAINING_MAX)),
            None => TRAINING_DISTANCE,
        };
        // The area spans past the longest flick both ways, so several fit in a row.
        let room = 0.6 * far + 5.0;
        let mut config = DrillConfig {
            kind: DrillKind::Microflick,
            min_flick: near,
            max_flick: far,
            flick_direction: self.sector.map(|s| s as f32 * 45.0),
            flick_spread: TRAINING_SPREAD,
            yaw_range: room.max(base.yaw_range).clamp(30.0, 80.0),
            pitch_range: room.max(base.pitch_range).clamp(12.0, 40.0),
            ..base
        };
        config.sanitize();
        config
    }
}

/// A weak spot worth training.
#[derive(Clone, Debug)]
pub struct Finding {
    pub cell: Cell,
    pub n: usize,
    /// E.g. "Down-left flicks, 15-25°".
    pub title: String,
    /// What goes wrong there, versus your overall numbers.
    pub detail: String,
}

/// Everything the Insights page shows.
#[derive(Clone, Debug, Default)]
pub struct Analysis {
    pub replays: usize,
    pub flicks: usize,
    pub overall: CellStats,
    /// By sector, see [`SECTORS`].
    pub sectors: Vec<CellStats>,
    /// By distance band, see [`BANDS`].
    pub bands: Vec<CellStats>,
    /// Weakest groups first, at most 3, none nested in another.
    pub findings: Vec<Finding>,
    /// Overall habits, one sentence each.
    pub tendencies: Vec<String>,
}

impl Analysis {
    pub fn enough_data(&self) -> bool {
        self.flicks >= MIN_FLICKS
    }
}

pub fn analyze(replays: &[Replay]) -> Analysis {
    let flicks: Vec<Flick> = replays.iter().flat_map(extract_flicks).collect();
    analyze_flicks(&flicks, replays.len())
}

pub fn analyze_flicks(flicks: &[Flick], replays: usize) -> Analysis {
    let model = KillModel::fit(flicks);
    let samples: Vec<Sample> = flicks.iter().map(|f| Sample::new(f, &model)).collect();
    let all: Vec<&Sample> = samples.iter().collect();
    let reference = Reference::new(&all);
    let group = |cell: Cell| {
        let members: Vec<&Sample> = samples.iter().filter(|s| cell.contains(s)).collect();
        CellStats::new(&members, &reference)
    };
    let overall = group(Cell::default());
    let sectors: Vec<CellStats> = (0..SECTORS.len()).map(|s| group(Cell { sector: Some(s), band: None })).collect();
    let bands: Vec<CellStats> = (0..BANDS.len()).map(|b| group(Cell { sector: None, band: Some(b) })).collect();

    let mut candidates: Vec<(f32, Cell, CellStats)> = Vec::new();
    for sector in (0..SECTORS.len()).map(Some).chain([None]) {
        for band in (0..BANDS.len()).map(Some).chain([None]) {
            let cell = Cell { sector, band };
            if cell != Cell::default() {
                let stats = group(cell);
                if let Some(score) = stats.score.filter(|&s| s >= MIN_SCORE) {
                    candidates.push((score, cell, stats));
                }
            }
        }
    }
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut findings: Vec<Finding> = Vec::new();
    for (_, cell, stats) in candidates {
        if findings.len() == MAX_FINDINGS {
            break;
        }
        if findings.iter().any(|f| cell.within(&f.cell) || f.cell.within(&cell)) {
            continue;
        }
        findings.push(Finding { cell, n: stats.n, title: cell.title(), detail: describe(&stats, &overall) });
    }

    let tendencies = if flicks.len() >= MIN_FLICKS { tendencies(&samples, &reference, &overall) } else { Vec::new() };
    Analysis { replays, flicks: flicks.len(), overall, sectors, bands, findings, tendencies }
}

/// E.g. "12% under", "8% over", "on target".
fn landing_text(landing: f32) -> String {
    let percent = ((landing - 1.0) * 100.0).round();
    match percent {
        p if p < 0.0 => format!("{:.0}% under", -p),
        p if p > 0.0 => format!("{p:.0}% over"),
        _ => "on target".to_string(),
    }
}

fn percent(value: f32) -> String {
    format!("{:.0}%", value * 100.0)
}

/// What stands out in a weak group, versus your overall numbers.
fn describe(stats: &CellStats, overall: &CellStats) -> String {
    let mut notable: Vec<(f32, Metric)> = METRICS[1..]
        .iter()
        .filter_map(|&m| Some((stats.gaps[m.index()].filter(|&g| g >= NOTABLE)?, m)))
        .collect();
    notable.sort_by(|a, b| b.0.total_cmp(&a.0));
    let parts: Vec<String> = notable
        .iter()
        .take(2)
        .filter_map(|&(_, metric)| {
            Some(match metric {
                Metric::Landing => {
                    let (landing, usual) = (stats.landing?, overall.landing?);
                    let verb = if landing < 1.0 { "undershoot" } else { "overshoot" };
                    format!("you {verb} by {:.0}% (overall {})", (landing - 1.0).abs() * 100.0, landing_text(usual))
                }
                Metric::OffAxis => {
                    format!("your first flick ends {} off-line (overall {})", percent(stats.off_axis?), percent(overall.off_axis?))
                }
                Metric::Bow => format!("your path curves more (bow {}, overall {})", percent(stats.bow?), percent(overall.bow?)),
                Metric::Corrections => {
                    format!("you need {:.1} corrections (overall {:.1})", stats.corrections?, overall.corrections?)
                }
                Metric::Errors => {
                    format!("you miss {} of attempts (overall {})", percent(stats.error_rate?), percent(overall.error_rate?))
                }
                Metric::Time => return None,
            })
        })
        .collect();
    let time = stats.ttk_vs(overall).map(|ms| format!("TTK {ms:+.0} ms vs your average for the distance"));
    let mut text = match (parts.is_empty(), time) {
        (false, Some(time)) => format!("{}; {time}.", parts.join(" and ")),
        (false, None) => format!("{}.", parts.join(" and ")),
        (true, Some(time)) => format!("Slower than your usual: {time}."),
        (true, None) => "Weaker than your average.".to_string(),
    };
    if let Some(first) = text.get(..1) {
        text = first.to_uppercase() + &text[1..];
    }
    text
}

/// Overall habits: landing, axis and side biases, straightness.
fn tendencies(samples: &[Sample], reference: &Reference, overall: &CellStats) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(landing) = overall.landing {
        lines.push(match landing {
            l if l < 0.97 => format!("Your first flick covers {} of the distance: you tend to undershoot, then correct.", percent(l)),
            l if l > 1.03 => format!("Your first flick covers {} of the distance: you tend to overshoot, then come back.", percent(l)),
            l => format!("Your first flick lands on target on average ({}).", percent(l)),
        });
    }
    let capitalized = |s: String| s[..1].to_uppercase() + &s[1..];
    lines.extend(compare(samples, reference, "vertical", "horizontal", |s| !s.horizontal).map(capitalized));
    lines.extend(compare(samples, reference, "left", "right", |s| !s.right).map(capitalized));
    if let (Some(bow), Some(off)) = (overall.bow, overall.off_axis) {
        let shape = match bow {
            b if b < 0.08 => "straight",
            b if b < 0.15 => "slightly curved",
            _ => "curved (the horizontal and vertical parts of the motion aren't in sync)",
        };
        lines.push(format!("Your flick paths are {shape}: median bow {}, first flicks end {} off-line.", percent(bow), percent(off)));
    }
    if let Some(corrections) = overall.corrections {
        lines.push(format!("You make {corrections:.1} corrective movements per kill on average."));
    }
    lines
}

/// Compares the flicks on `a_side` (named `name_a`) with the rest: kill time versus expected,
/// and first-flick landing. `None` without enough flicks on both sides.
fn compare(samples: &[Sample], reference: &Reference, name_a: &str, name_b: &str, a_side: impl Fn(&Sample) -> bool) -> Option<String> {
    let (a, b): (Vec<&Sample>, Vec<&Sample>) = samples.iter().partition(|&s| a_side(s));
    if a.len() < MIN_SAMPLES || b.len() < MIN_SAMPLES {
        return None;
    }
    let (a, b) = (CellStats::new(&a, reference), CellStats::new(&b, reference));
    let mut facts = Vec::new();
    if let Some(ms) = a.ttk_excess.zip(b.ttk_excess).map(|(x, y)| (x - y) * 1000.0).filter(|ms| ms.abs() >= 15.0) {
        let (slower, faster) = if ms > 0.0 { (name_a, name_b) } else { (name_b, name_a) };
        facts.push(format!("{slower} flicks take {:.0} ms longer than {faster} ones of the same difficulty", ms.abs()));
    }
    if let Some((x, y)) = a.landing.zip(b.landing).filter(|(x, y)| (x - y).abs() >= 0.04) {
        facts.push(format!("{name_a} first flicks land {} vs {} for {name_b}", landing_text(x), landing_text(y)));
    }
    Some(if facts.is_empty() { format!("{name_a} and {name_b} flicks are even.") } else { format!("{}.", facts.join("; ")) })
}

fn mean(values: &[f32]) -> f32 {
    values.iter().sum::<f32>() / values.len().max(1) as f32
}

fn median(values: &mut [f32]) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable_by(f32::total_cmp);
    let mid = values.len() / 2;
    Some(if values.len().is_multiple_of(2) { (values[mid - 1] + values[mid]) * 0.5 } else { values[mid] })
}

/// The `q` quantile (nearest rank) of non-empty `values`.
fn percentile(values: &mut [f32], q: f32) -> f32 {
    values.sort_unstable_by(f32::total_cmp);
    values[((values.len() - 1) as f32 * q).round() as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 240.0;

    fn min_jerk(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        t * t * t * (10.0 - 15.0 * t + 6.0 * t * t)
    }

    /// World position of a target at (yaw, pitch) degrees, 10 m out.
    fn at(angles: Vec2) -> Vec3 {
        Quat::from_euler(EulerRot::YXZ, angles.x.to_radians(), angles.y.to_radians(), 0.0) * Vec3::NEG_Z * 10.0
    }

    /// Builds a replay frame by frame.
    struct Script {
        frames: Vec<ReplayFrame>,
        aim: Vec2,
        targets: Vec<(Vec3, f32)>,
    }

    impl Script {
        fn new(targets: &[Vec2]) -> Self {
            let mut script = Self { frames: Vec::new(), aim: Vec2::ZERO, targets: targets.iter().map(|&a| (at(a), 0.2)).collect() };
            script.frame(Shot::None);
            script
        }

        fn frame(&mut self, shot: Shot) {
            let t = self.frames.len() as f32 * DT;
            let (yaw, pitch) = (self.aim.x.to_radians(), self.aim.y.to_radians());
            self.frames.push(ReplayFrame { t, yaw, pitch, shot, targets: self.targets.clone() });
        }

        /// Minimum-jerk movement by `delta` over `secs`, then a rest of `rest` seconds.
        fn moving(&mut self, delta: Vec2, secs: f32, rest: f32) {
            let from = self.aim;
            let steps = (secs / DT) as usize;
            for i in 1..=steps {
                self.aim = from + delta * min_jerk(i as f32 / steps as f32);
                self.frame(Shot::None);
            }
            for _ in 0..(rest / DT) as usize {
                self.frame(Shot::None);
            }
        }

        /// Kills the target nearest the crosshair and spawns `next` in the same frame.
        fn kill(&mut self, next: Option<Vec2>) {
            let aim = at(self.aim);
            let nearest = (0..self.targets.len()).min_by(|&a, &b| self.targets[a].0.distance(aim).total_cmp(&self.targets[b].0.distance(aim))).unwrap();
            self.targets.remove(nearest);
            self.targets.extend(next.map(|a| (at(a), 0.2)));
            self.frame(Shot::Hit);
        }

        fn replay(self, kind: DrillKind) -> Replay {
            Replay { kind, name: kind.name().into(), sens: 0.3, duration: 60.0, frames: self.frames }
        }
    }

    #[test]
    fn flicks_are_found_with_direction_distance_and_outcome() {
        // The first target was up before the run went live, so it's skipped.
        let mut script = Script::new(&[Vec2::new(-10.0, 0.0)]);
        script.moving(Vec2::new(-10.0, 0.0), 0.2, 0.1);
        // Next: 20 degrees down-left on screen (yaw grows to the left).
        let down_left = Vec2::new(-10.0, 0.0) + Vec2::new(1.0, -1.0).normalize() * 20.0;
        script.kill(Some(down_left));
        let start = script.frames.len() - 1;
        script.frame(Shot::Miss);
        script.moving(down_left - script.aim, 0.25, 0.1);
        script.kill(Some(Vec2::new(-10.0, 0.0)));
        let ttk = (script.frames.len() - 1 - start) as f32 * DT;
        // That one expires untouched.
        for _ in 0..60 {
            script.frame(Shot::None);
        }
        script.targets.clear();
        script.frame(Shot::None);

        let flicks = extract_flicks(&script.replay(DrillKind::Microflick));
        assert_eq!(flicks.len(), 2, "{flicks:?}");
        let kill = &flicks[0];
        assert_eq!(SECTORS[kill.sector()], "Down-left");
        assert!((kill.distance - 20.0).abs() < 0.05, "{}", kill.distance);
        assert_eq!(band_label(kill.band()), "15-25°");
        assert!((kill.ttk.unwrap() - ttk).abs() < 1e-4);
        assert_eq!(kill.misses, 1);
        assert!((kill.landing.unwrap() - 1.0).abs() < 0.02, "{:?}", kill.landing);
        assert!(kill.bow.unwrap() < 0.02);
        assert_eq!(kill.corrections, 0);

        let expired = &flicks[1];
        assert_eq!(expired.ttk, None);
        // From the down-left target to (5, 8): right and up on screen.
        assert_eq!(SECTORS[expired.sector()], "Up-right");
        // Other drills aren't analysed.
        assert!(extract_flicks(&Script::new(&[]).replay(DrillKind::Tracking)).is_empty());
    }

    #[test]
    fn undershoot_off_axis_and_corrections_are_measured() {
        let first = Vec2::new(-4.0, 0.0);
        let mut script = Script::new(&[first]);
        script.moving(first, 0.15, 0.1);
        // Target 24 degrees to the right; the first flick covers 85% and drifts 10% upward.
        let target = first + Vec2::new(-24.0, 0.0);
        script.kill(Some(target));
        script.moving(Vec2::new(-24.0 * 0.85, 24.0 * 0.1), 0.22, 0.12);
        script.moving(Vec2::new(-24.0 * 0.15, -24.0 * 0.1), 0.15, 0.1);
        script.kill(None);

        let flicks = extract_flicks(&script.replay(DrillKind::Microflick));
        assert_eq!(flicks.len(), 1);
        let flick = &flicks[0];
        assert_eq!(SECTORS[flick.sector()], "Right");
        assert!((flick.landing.unwrap() - 0.85).abs() < 0.02, "{:?}", flick.landing);
        // Up on screen is counter-clockwise of a rightward flick.
        assert!((flick.off_axis.unwrap() - 0.1).abs() < 0.02, "{:?}", flick.off_axis);
        assert_eq!(flick.corrections, 1);
    }

    #[test]
    fn gridshot_flicks_start_at_the_previous_kill() {
        let (a, b, c) = (Vec2::new(-7.0, 0.0), Vec2::new(7.0, 7.0), Vec2::new(0.0, -7.0));
        let mut script = Script::new(&[a, b]);
        script.moving(a, 0.15, 0.05);
        script.kill(Some(c));
        let start = script.frames.len() - 1;
        // b has been up since the start; its flick begins at the kill of a.
        script.moving(b - a, 0.2, 0.05);
        script.kill(Some(a));
        let ttk = (script.frames.len() - 1 - start) as f32 * DT;
        let flicks = extract_flicks(&script.replay(DrillKind::Gridshot));
        assert_eq!(flicks.len(), 1);
        assert!((flicks[0].ttk.unwrap() - ttk).abs() < 1e-4);
        assert!((flicks[0].distance - (b - a).length()).abs() < 0.05);
        assert_eq!(SECTORS[flicks[0].sector()], "Up-left");
    }

    /// Synthetic flicks: `n` per sector over all bands, with Fitts-like kill times plus noise;
    /// `weak` sectors are 35% slower, undershoot and need corrections.
    fn synthetic(per_sector: &[usize], weak: usize) -> Vec<Flick> {
        let mut rng = fastrand::Rng::with_seed(7);
        let mut flicks = Vec::new();
        for (sector, &n) in per_sector.iter().enumerate() {
            for i in 0..n {
                let distance = [5.0, 11.0, 20.0, 30.0, 45.0][i % 5] + rng.f32() * 2.0;
                let width = 2.3;
                let difficulty = (distance / width + 1.0f32).log2();
                let slow = if sector == weak { 1.35 } else { 1.0 };
                let ttk = (0.2 + 0.08 * difficulty) * slow * (0.9 + 0.2 * rng.f32());
                let weak_spot = sector == weak;
                flicks.push(Flick {
                    kind: DrillKind::Microflick,
                    angle: sector as f32 * 45.0 + rng.f32() * 30.0 - 15.0,
                    distance,
                    width,
                    landing: Some(if weak_spot { 0.85 } else { 0.97 } + rng.f32() * 0.06 - 0.03),
                    off_axis: Some(rng.f32() * 0.1 - 0.05),
                    bow: Some(0.05 + rng.f32() * 0.05),
                    corrections: if weak_spot { 2 } else { rng.u32(0..2) },
                    ttk: Some(ttk),
                    misses: (rng.f32() < 0.1) as u32,
                });
            }
        }
        flicks
    }

    #[test]
    fn the_weakest_sector_ranks_first() {
        let analysis = analyze_flicks(&synthetic(&[30; 8], 5), 10);
        assert!(analysis.enough_data());
        let top = &analysis.findings[0];
        assert_eq!(top.cell.sector, Some(5), "{:?}", analysis.findings);
        assert!(top.detail.contains("undershoot"), "{}", top.detail);
        let scores: Vec<f32> = analysis.sectors.iter().map(|s| s.score.unwrap()).collect();
        let weakest = (0..8).max_by(|&a, &b| scores[a].total_cmp(&scores[b])).unwrap();
        assert_eq!(weakest, 5, "{scores:?}");
        assert!(analysis.sectors[5].ttk_vs(&analysis.overall).unwrap() > 40.0);
        // Findings never repeat a group inside another one.
        for (i, a) in analysis.findings.iter().enumerate() {
            for b in &analysis.findings[i + 1..] {
                assert!(!a.cell.within(&b.cell) && !b.cell.within(&a.cell));
            }
        }
        assert!(!analysis.tendencies.is_empty());
    }

    #[test]
    fn groups_need_enough_flicks() {
        // The weak sector has only 5 flicks: too few to score or report.
        let mut counts = [30; 8];
        counts[5] = MIN_SAMPLES - 3;
        let analysis = analyze_flicks(&synthetic(&counts, 5), 10);
        assert_eq!(analysis.sectors[5].score, None);
        assert_eq!(analysis.sectors[5].n, MIN_SAMPLES - 3);
        assert!(analysis.findings.iter().all(|f| f.cell.sector != Some(5)), "{:?}", analysis.findings);
        // And too few flicks overall show nothing.
        let few = analyze_flicks(&synthetic(&[4; 8], 5), 1);
        assert!(!few.enough_data());
        assert!(few.tendencies.is_empty());
    }

    #[test]
    fn training_targets_the_cell() {
        let base = DrillConfig::defaults(DrillKind::Microflick);
        let cell = Cell { sector: Some(5), band: Some(2) };
        assert_eq!(cell.scenario_name(), "Weak spot: down-left 15-25°");
        assert_eq!(cell.title(), "Down-left flicks, 15-25°");
        let config = cell.training_config(base.clone());
        assert_eq!(config.kind, DrillKind::Microflick);
        assert_eq!(config.flick_direction, Some(225.0));
        assert_eq!((config.min_flick, config.max_flick), (15.0, 25.0));
        assert!(config.flick_constrained());

        let far = Cell { sector: None, band: Some(4) }.training_config(base);
        assert_eq!((far.min_flick, far.max_flick, far.flick_direction), (40.0, TRAINING_MAX, None));
        assert!(far.yaw_range >= far.min_flick);
        assert_eq!(Cell { sector: Some(2), band: None }.scenario_name(), "Weak spot: up");
    }
}
