//! Run replays: compact recordings of view angles, targets and shots, saved per practice run,
//! and a first-person player with pause, seek and slow motion.

use std::{
    array, fs, io,
    path::{Path, PathBuf},
};

use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    feathers::{constants::fonts, theme::ThemeBackgroundColor, tokens},
    prelude::*,
    tasks::IoTaskPool,
};

use crate::{
    AppState,
    camera::{Look, apply_look},
    drill::{DrillKind, TargetAssets, def::Environment, spawn_cover},
    tracer,
    ui::{
        theme::{DIVIDER, TEXT_ACCENT, TEXT_BAD, TEXT_GOOD, TEXT_STRONG},
        widgets::{hud_anchor, hud_bar, styled, vertical_divider},
    },
};

const REPLAY_DIR: &str = "replays";
/// Older replays beyond this many are deleted when a new one is saved.
const KEEP_REPLAYS: usize = 100;
/// Recording rate. Shots are always recorded on the exact frame they happen.
const SAMPLE_INTERVAL: f32 = 1.0 / 240.0;
const MAGIC: &[u8; 4] = b"AIMR";
const VERSION: u8 = 3;
/// v3 stores view angles in microradians...
const ANGLE_UNITS: f64 = 1e6;
/// ...and target centers and radii in tenths of a millimeter (units per meter).
const POSITION_UNITS: f64 = 1e4;
/// Quantized values are clamped to this magnitude so the integer math can't overflow.
const QUANTIZED_LIMIT: f64 = (1u64 << 40) as f64;
/// v3 frames list at most this many targets (as v2 did).
const MAX_TARGETS: usize = u8::MAX as usize;
/// Largest decompressed v3 body accepted, so a corrupt file can't exhaust memory.
const MAX_BODY: usize = 256 << 20;
const SPEEDS: [f32; 5] = [0.1, 0.25, 0.5, 1.0, 2.0];
/// How long (replay time) a shot marker stays visible.
const MARKER_SECS: f32 = 0.6;
/// Shot markers are drawn this far along the aim ray, i.e. on the target plane.
const MARKER_DISTANCE: f32 = 10.0;
/// How much of the recent aim path (replay time) the tracer shows.
const TRACER_SECS: f32 = 0.6;

pub struct ReplayPlugin;

impl Plugin for ReplayPlugin {
    fn build(&self, app: &mut App) {
        app.init_gizmo_group::<TracerGizmos>()
            .add_systems(Startup, (create_marker_assets, configure_tracer, migrate_replays))
            .add_systems(OnEnter(AppState::Replay), spawn_replay_hud)
            .add_systems(OnExit(AppState::Replay), clear_look)
            .add_systems(
                Update,
                (playback_controls, advance_playback, update_replay_hud, draw_tracer)
                    .chain()
                    .before(apply_look)
                    .run_if(in_state(AppState::Replay)),
            );
    }
}

// ---------------------------------------------------------------------------------------------
// Data
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shot {
    #[default]
    None,
    Miss,
    Hit,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReplayFrame {
    /// Seconds since the drill went live.
    pub t: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub shot: Shot,
    /// (center, radius) of every target visible this frame.
    pub targets: Vec<(Vec3, f32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Replay {
    pub kind: DrillKind,
    /// Drill name (built-in or custom).
    pub name: String,
    pub sens: f32,
    pub duration: f32,
    pub frames: Vec<ReplayFrame>,
}

impl Replay {
    fn path(timestamp: u64) -> PathBuf {
        PathBuf::from(REPLAY_DIR).join(format!("{timestamp}.replay"))
    }

    pub fn exists(timestamp: u64) -> bool {
        Self::path(timestamp).is_file()
    }

    pub fn load(timestamp: u64) -> Option<Self> {
        Self::decode(&fs::read(Self::path(timestamp)).ok()?)
    }

    /// Timestamps of every saved replay, oldest first.
    fn saved_stamps() -> Vec<u64> {
        let Ok(dir) = fs::read_dir(REPLAY_DIR) else { return Vec::new() };
        let mut stamps: Vec<u64> = dir
            .filter_map(|entry| entry.ok()?.path().file_stem()?.to_str()?.parse().ok())
            .collect();
        stamps.sort_unstable();
        stamps
    }

    /// The newest saved replays (newest first, at most `limit`) of the given drill kinds. Only
    /// the header byte is checked before decoding, so other drills' replays cost a file read.
    pub fn recent(kinds: &[DrillKind], limit: usize) -> Vec<Self> {
        Self::saved_stamps()
            .into_iter()
            .rev()
            .filter_map(|stamp| fs::read(Self::path(stamp)).ok())
            .filter(|bytes| bytes.get(5).and_then(|&i| DrillKind::ALL.get(i as usize)).is_some_and(|s| kinds.contains(s)))
            .filter_map(|bytes| Self::decode(&bytes))
            .take(limit)
            .collect()
    }

    /// Saves under the run's history timestamp and prunes the oldest replays.
    pub fn save(&self, timestamp: u64) {
        let result = fs::create_dir_all(REPLAY_DIR).and_then(|_| fs::write(Self::path(timestamp), self.encode()));
        if let Err(err) = result {
            return error!("Failed to save replay: {err}");
        }
        let stamps = Self::saved_stamps();
        let excess = stamps.len().saturating_sub(KEEP_REPLAYS);
        // Never the replay just saved, even if the clock went backwards and it looks oldest.
        for old in stamps[..excess].iter().filter(|&&old| old != timestamp) {
            let _ = fs::remove_file(Self::path(*old));
        }
    }

    /// Format v3: the header, then the frames as a zlib-compressed [`encode_body`].
    fn encode(&self) -> Vec<u8> {
        let mut out = self.encode_header(VERSION);
        out.extend(miniz_oxide::deflate::compress_to_vec_zlib(&encode_body(&self.frames), 10));
        out
    }

    /// Magic, version, drill, name, sens and duration: the same in every version but 1 (no name).
    fn encode_header(&self, version: u8) -> Vec<u8> {
        let name = truncate_utf8(&self.name, u8::MAX as usize);
        let mut out = Vec::with_capacity(16 + name.len());
        out.extend_from_slice(MAGIC);
        out.push(version);
        out.push(DrillKind::ALL.iter().position(|s| *s == self.kind).unwrap_or(0) as u8);
        out.push(name.len() as u8);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&self.sens.to_le_bytes());
        out.extend_from_slice(&self.duration.to_le_bytes());
        out
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        let mut reader = Reader(bytes);
        if reader.take(4)? != MAGIC {
            return None;
        }
        let version = reader.u8()?;
        if !(1..=VERSION).contains(&version) {
            return None;
        }
        let kind = *DrillKind::ALL.get(reader.u8()? as usize)?;
        // Version 1 predates custom scenarios and has no name.
        let name = if version >= 2 {
            let len = reader.u8()? as usize;
            String::from_utf8(reader.take(len)?.to_vec()).ok()?
        } else {
            kind.name().to_string()
        };
        let sens = reader.f32()?;
        let duration = reader.f32()?;
        let frames = if version >= 3 {
            let body = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(reader.0, MAX_BODY).ok()?;
            decode_body(&body, bytes.len())?
        } else {
            decode_raw_frames(&mut reader, bytes.len())?
        };
        Some(Self { kind, name, sens, duration, frames })
    }

    /// Whether `other` is this replay up to the v3 quantization (and f32 rounding).
    fn matches(&self, other: &Self) -> bool {
        let close = |a: f32, b: f32, units: f64| {
            (f64::from(a) - f64::from(b)).abs() <= 1.0 / units + f64::from(a.abs() * f32::EPSILON)
        };
        let same_target = |&(a, ar): &(Vec3, f32), &(b, br): &(Vec3, f32)| {
            [(a.x, b.x), (a.y, b.y), (a.z, b.z), (ar, br)].into_iter().all(|(a, b)| close(a, b, POSITION_UNITS))
        };
        let same_frame = |a: &ReplayFrame, b: &ReplayFrame| {
            a.t.to_bits() == b.t.to_bits()
                && a.shot == b.shot
                && close(a.yaw, b.yaw, ANGLE_UNITS)
                && close(a.pitch, b.pitch, ANGLE_UNITS)
                && a.targets.len() == b.targets.len()
                && a.targets.iter().zip(&b.targets).all(|(a, b)| same_target(a, b))
        };
        self.kind == other.kind
            && self.name == other.name
            && self.sens.to_bits() == other.sens.to_bits()
            && self.duration.to_bits() == other.duration.to_bits()
            && self.frames.len() == other.frames.len()
            && self.frames.iter().zip(&other.frames).all(|(a, b)| same_frame(a, b))
    }
}

impl Shot {
    fn from_byte(byte: u8) -> Self {
        match byte {
            1 => Shot::Miss,
            2 => Shot::Hit,
            _ => Shot::None,
        }
    }
}

/// Frames of versions 1 and 2: a u32 count, then per frame `t`, yaw and pitch as f32, the shot
/// byte, a target count byte and each target's center and radius as f32.
fn decode_raw_frames(reader: &mut Reader, input_len: usize) -> Option<Vec<ReplayFrame>> {
    let count = reader.u32()? as usize;
    // Each frame takes at least 14 bytes, so a bogus count can't reserve a huge buffer.
    let mut frames = Vec::with_capacity(count.min(input_len / 14));
    for _ in 0..count {
        let (t, yaw, pitch) = (reader.f32()?, reader.f32()?, reader.f32()?);
        let shot = Shot::from_byte(reader.u8()?);
        let targets = (0..reader.u8()?)
            .map(|_| Some((Vec3::new(reader.f32()?, reader.f32()?, reader.f32()?), reader.f32()?)))
            .collect::<Option<Vec<_>>>()?;
        frames.push(ReplayFrame { t, yaw, pitch, shot, targets });
    }
    Some(frames)
}

/// The v3 frames, before compression. Integers are LEB128 varints, signed ones zigzag-encoded.
/// First the angle step: the view rotation per mouse count in nanoradians (see [`angle_step`];
/// 0 if there is none). Then six streams, each prefixed with its length in bytes, holding per frame:
/// - times: the second difference of `t`'s f32 bit pattern (lossless; frames are irregular);
/// - flags: one byte, the shot (0 none, 1 miss, 2 hit) | 4 if the target set changed;
/// - yaw, pitch: the second difference of the angle in whole mouse counts, i.e. the change in
///   microradians divided by the step (with no step: the change in microradians);
/// - residuals: per axis, the change in microradians minus counts × step (rounding, clamping);
/// - targets, only for frames whose set changed: their count, then per target the index + 1 of
///   the previous frame's target it is predicted from (0: none) and its center and radius in
///   0.1 mm minus the prediction (that target plus its last movement, or zero).
fn encode_body(frames: &[ReplayFrame]) -> Vec<u8> {
    let step = angle_step(frames);
    let [mut times, mut flags, mut yaw, mut pitch, mut residuals, mut targets] = array::from_fn(|_| Vec::new());
    let mut time = TimeCoder::default();
    let (mut yaw_coder, mut pitch_coder) = (AngleCoder::default(), AngleCoder::default());
    let mut previous: Vec<TrackedTarget> = Vec::new();
    for frame in frames {
        put_signed(&mut times, time.encode(frame.t));
        yaw_coder.encode(frame.yaw, step, &mut yaw, &mut residuals);
        pitch_coder.encode(frame.pitch, step, &mut pitch, &mut residuals);
        let current: Vec<[i64; 4]> = frame
            .targets
            .iter()
            .take(MAX_TARGETS)
            .map(|&(center, radius)| [center.x, center.y, center.z, radius].map(|v| quantize(v, POSITION_UNITS)))
            .collect();
        let changed = current.len() != previous.len()
            || current.iter().zip(&previous).any(|(value, target)| *value != target.value);
        flags.push(frame.shot as u8 | u8::from(changed) << 2);
        if changed {
            put_unsigned(&mut targets, current.len() as u64);
            previous = current.into_iter().map(|value| encode_target(&mut targets, value, &previous)).collect();
        } else {
            previous.iter_mut().for_each(|target| target.motion = [0; 4]);
        }
    }
    let streams = [times, flags, yaw, pitch, residuals, targets];
    let mut body = Vec::with_capacity(8 + streams.iter().map(|s| s.len() + 4).sum::<usize>());
    put_unsigned(&mut body, step);
    for stream in streams {
        put_unsigned(&mut body, stream.len() as u64);
        body.extend_from_slice(&stream);
    }
    body
}

/// Inverse of [`encode_body`]; `input_len` (the file size) caps the pre-allocation.
fn decode_body(body: &[u8], input_len: usize) -> Option<Vec<ReplayFrame>> {
    let mut reader = Reader(body);
    let step = reader.unsigned()?;
    if step > u64::from(u32::MAX) {
        return None;
    }
    let mut times_in = reader.stream()?;
    let flags_in = reader.stream()?;
    let mut yaw_in = reader.stream()?;
    let mut pitch_in = reader.stream()?;
    let mut residuals_in = reader.stream()?;
    let mut targets_in = reader.stream()?;
    if !reader.0.is_empty() {
        return None;
    }
    let mut time = TimeCoder::default();
    let (mut yaw_coder, mut pitch_coder) = (AngleCoder::default(), AngleCoder::default());
    let mut previous: Vec<TrackedTarget> = Vec::new();
    let mut frames = Vec::with_capacity(flags_in.0.len().min(input_len));
    for &flag in flags_in.0 {
        let t = time.decode(times_in.signed()?)?;
        let yaw = yaw_coder.decode(step, &mut yaw_in, &mut residuals_in)?;
        let pitch = pitch_coder.decode(step, &mut pitch_in, &mut residuals_in)?;
        if flag & 4 != 0 {
            let count = usize::try_from(targets_in.unsigned()?).ok().filter(|&n| n <= MAX_TARGETS)?;
            previous = (0..count).map(|_| decode_target(&mut targets_in, &previous)).collect::<Option<_>>()?;
        } else {
            previous.iter_mut().for_each(|target| target.motion = [0; 4]);
        }
        let targets = previous
            .iter()
            .map(|target| {
                let [x, y, z, radius] = target.value.map(|v| dequantize(v, POSITION_UNITS));
                (Vec3::new(x, y, z), radius)
            })
            .collect();
        frames.push(ReplayFrame { t, yaw, pitch, shot: Shot::from_byte(flag & 3), targets });
    }
    let streams = [times_in, yaw_in, pitch_in, residuals_in, targets_in];
    streams.iter().all(|stream| stream.0.is_empty()).then_some(frames)
}

/// `value` in whole `units` (per meter or radian), clamped; NaN becomes 0.
fn quantize(value: f32, units: f64) -> i64 {
    (f64::from(value) * units).round().clamp(-QUANTIZED_LIMIT, QUANTIZED_LIMIT) as i64
}

fn dequantize(value: i64, units: f64) -> f32 {
    (value as f64 / units) as f32
}

/// The view rotation per mouse count in nanoradians, or 0 if the angles don't move in steps.
/// Raw mouse input turns the view by whole counts, so angle changes are multiples of it: this
/// finds the smallest change that nearly all others are multiples of and averages over them all.
fn angle_step(frames: &[ReplayFrame]) -> u64 {
    let mut changes: Vec<f64> = frames
        .windows(2)
        .flat_map(|w| [(w[0].yaw, w[1].yaw), (w[0].pitch, w[1].pitch)])
        .map(|(a, b)| (f64::from(b) - f64::from(a)).abs())
        .filter(|change| change.is_finite() && change * ANGLE_UNITS >= 1.0)
        .collect();
    changes.sort_unstable_by(f64::total_cmp);
    let step = changes.iter().take(16).find_map(|&candidate| {
        let (mut fitting, mut total, mut counts) = (0usize, 0.0, 0.0);
        for &change in &changes {
            let n = (change / candidate).round();
            if (change / candidate - n).abs() < 0.05 {
                (fitting, total, counts) = (fitting + 1, total + change, counts + n);
            }
        }
        (fitting * 10 >= changes.len() * 9).then(|| total / counts)
    });
    step.map_or(0, |step| (step * 1e9).round().min(f64::from(u32::MAX)) as u64)
}

/// Codes frame times as the second difference of their f32 bit patterns.
#[derive(Default)]
struct TimeCoder {
    bits: i64,
    change: i64,
}

impl TimeCoder {
    fn encode(&mut self, t: f32) -> i64 {
        let bits = i64::from(t.to_bits());
        let change = bits - self.bits;
        let residual = change - self.change;
        *self = Self { bits, change };
        residual
    }

    fn decode(&mut self, residual: i64) -> Option<f32> {
        let change = self.change.wrapping_add(residual);
        let bits = self.bits.wrapping_add(change);
        *self = Self { bits, change };
        Some(f32::from_bits(u32::try_from(bits).ok()?))
    }
}

/// Codes one view angle: its last value (microradians) and change (mouse counts).
#[derive(Default)]
struct AngleCoder {
    value: i64,
    counts: i64,
}

impl AngleCoder {
    fn encode(&mut self, angle: f32, step: u64, counts_out: &mut Vec<u8>, residuals: &mut Vec<u8>) {
        let value = quantize(angle, ANGLE_UNITS);
        let change = value - self.value;
        let counts = if step == 0 { change } else { div_round(i128::from(change) * 1000, i128::from(step)) };
        put_signed(counts_out, counts - self.counts);
        put_signed(residuals, change - step_change(counts, step));
        *self = Self { value, counts };
    }

    fn decode(&mut self, step: u64, counts_in: &mut Reader, residuals: &mut Reader) -> Option<f32> {
        let counts = self.counts.wrapping_add(counts_in.signed()?);
        let value = self.value.wrapping_add(step_change(counts, step)).wrapping_add(residuals.signed()?);
        *self = Self { value, counts };
        Some(dequantize(value, ANGLE_UNITS))
    }
}

/// The angle change in microradians of `counts` mouse counts of `step` nanoradians.
fn step_change(counts: i64, step: u64) -> i64 {
    if step == 0 { counts } else { div_round(i128::from(counts) * i128::from(step), 1000) }
}

/// `n / d` rounded to the nearest integer (halves away from zero), for `d > 0`.
fn div_round(n: i128, d: i128) -> i64 {
    let half = d / 2;
    (if n >= 0 { (n + half) / d } else { (n - half) / d }) as i64
}

/// A target as the v3 coder last saw it: center and radius (0.1 mm) and their last change.
#[derive(Clone, Copy)]
struct TrackedTarget {
    value: [i64; 4],
    motion: [i64; 4],
}

impl TrackedTarget {
    /// The target at `value`, which was `basis` in the previous frame (`None`: a new target).
    fn new(value: [i64; 4], basis: Option<&Self>) -> Self {
        let motion = basis.map_or([0; 4], |basis| array::from_fn(|i| value[i].wrapping_sub(basis.value[i])));
        Self { value, motion }
    }

    /// Where this target will be if it keeps moving the same way.
    fn predicted(&self) -> [i64; 4] {
        array::from_fn(|i| self.value[i].wrapping_add(self.motion[i]))
    }
}

/// Writes `value` predicted from whichever previous target (or none) predicts it best.
fn encode_target(out: &mut Vec<u8>, value: [i64; 4], previous: &[TrackedTarget]) -> TrackedTarget {
    let prediction = |reference: usize| reference.checked_sub(1).map_or([0; 4], |i| previous[i].predicted());
    let error = |reference: usize| -> u64 { value.iter().zip(prediction(reference)).map(|(v, p)| v.abs_diff(p)).sum() };
    let reference = (0..=previous.len()).min_by_key(|&reference| error(reference)).unwrap_or(0);
    put_unsigned(out, reference as u64);
    for (v, p) in value.iter().zip(prediction(reference)) {
        put_signed(out, v.wrapping_sub(p));
    }
    TrackedTarget::new(value, reference.checked_sub(1).map(|i| &previous[i]))
}

fn decode_target(input: &mut Reader, previous: &[TrackedTarget]) -> Option<TrackedTarget> {
    let basis = match usize::try_from(input.unsigned()?).ok()? {
        0 => None,
        reference => Some(previous.get(reference - 1)?),
    };
    let mut value = basis.map_or([0; 4], TrackedTarget::predicted);
    for v in &mut value {
        *v = v.wrapping_add(input.signed()?);
    }
    Some(TrackedTarget::new(value, basis))
}

fn put_unsigned(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(value as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn put_signed(out: &mut Vec<u8>, value: i64) {
    put_unsigned(out, ((value << 1) ^ (value >> 63)) as u64);
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(head)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn unsigned(&mut self) -> Option<u64> {
        let mut value = 0;
        for shift in (0..64).step_by(7) {
            let byte = self.u8()?;
            value |= u64::from(byte & 0x7f) << shift;
            if byte < 0x80 {
                return Some(value);
            }
        }
        None
    }
    fn signed(&mut self) -> Option<i64> {
        let value = self.unsigned()?;
        Some((value >> 1) as i64 ^ -((value & 1) as i64))
    }
    /// A length-prefixed part of the input.
    fn stream(&mut self) -> Option<Reader<'a>> {
        let len = usize::try_from(self.unsigned()?).ok()?;
        Some(Reader(self.take(len)?))
    }
}

/// The longest prefix of `s` that fits in `max` bytes without splitting a character.
fn truncate_utf8(s: &str, max: usize) -> &str {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

// ---------------------------------------------------------------------------------------------
// Migration
// ---------------------------------------------------------------------------------------------

/// Rewrites replays saved in older formats as v3, in the background.
fn migrate_replays() {
    IoTaskPool::get()
        .spawn(async {
            let migrated = migrate_dir(Path::new(REPLAY_DIR));
            if migrated > 0 {
                info!("Rewrote {migrated} replays in the compact format");
            }
        })
        .detach();
}

/// Rewrites every v1/v2 replay in `dir` as v3 and returns how many were rewritten. A file is
/// only replaced (atomically, via a temporary file) once its new encoding decodes back to the
/// same replay; on any failure it is left as it was.
fn migrate_dir(dir: &Path) -> usize {
    let Ok(entries) = fs::read_dir(dir) else { return 0 };
    let mut migrated = 0;
    // Listed up front, so the files written meanwhile don't show up.
    let paths: Vec<PathBuf> = entries.filter_map(|entry| Some(entry.ok()?.path())).collect();
    for path in paths {
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        if name.ends_with(".replay.tmp") {
            // Left over from an interrupted migration.
            let _ = fs::remove_file(&path);
        } else if name.ends_with(".replay") {
            match migrate_file(&path) {
                Ok(rewritten) => migrated += usize::from(rewritten),
                Err(err) => warn!("Replay {} kept in its old format: {err}", path.display()),
            }
        }
    }
    migrated
}

/// Rewrites one replay as v3 if it's in an older format; returns whether it did.
fn migrate_file(path: &Path) -> io::Result<bool> {
    let old_bytes = fs::read(path)?;
    if !old_bytes.starts_with(MAGIC) || old_bytes.get(4).is_none_or(|&version| version >= VERSION) {
        return Ok(false);
    }
    let old = Replay::decode(&old_bytes).ok_or_else(|| io::Error::other("it could not be read"))?;
    let new_bytes = old.encode();
    if !Replay::decode(&new_bytes).is_some_and(|new| new.matches(&old)) {
        return Err(io::Error::other("the new encoding did not match"));
    }
    let tmp = path.with_extension("replay.tmp");
    let result = fs::write(&tmp, &new_bytes).and_then(|()| fs::rename(&tmp, path));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map(|()| true)
}

// ---------------------------------------------------------------------------------------------
// Recording
// ---------------------------------------------------------------------------------------------

/// Collects frames while a drill is live. Owned by the drill systems.
#[derive(Resource, Default)]
pub struct Recorder {
    frames: Vec<ReplayFrame>,
    pending_shot: Shot,
}

impl Recorder {
    pub fn mark_shot(&mut self, hit: bool) {
        self.pending_shot = if hit { Shot::Hit } else { Shot::Miss };
    }

    /// Records a frame if enough time has passed since the last one, or a shot happened.
    pub fn sample(&mut self, t: f32, (yaw, pitch): (f32, f32), targets: impl Iterator<Item = (Vec3, f32)>) {
        let due = self.frames.last().is_none_or(|last| t - last.t >= SAMPLE_INTERVAL);
        if !due && self.pending_shot == Shot::None {
            return;
        }
        let shot = std::mem::take(&mut self.pending_shot);
        self.frames.push(ReplayFrame { t, yaw, pitch, shot, targets: targets.collect() });
    }

    pub fn finish(&mut self, kind: DrillKind, name: &str, sens: f32, duration: f32) -> Replay {
        Replay { kind, name: name.to_string(), sens, duration, frames: std::mem::take(&mut self.frames) }
    }
}

// ---------------------------------------------------------------------------------------------
// Playback
// ---------------------------------------------------------------------------------------------

#[derive(Resource)]
pub struct Playback {
    replay: Replay,
    time: f32,
    speed_index: usize,
    paused: bool,
    /// Screen to go back to when the replay is closed.
    return_to: AppState,
    /// Replay time up to which shot markers have been spawned.
    shown_until: f32,
    /// Whether the aim tracer is drawn (toggled with T).
    tracer: bool,
}

/// Opens the replay player.
pub fn watch(commands: &mut Commands, next_state: &mut NextState<AppState>, replay: Replay, return_to: AppState) {
    commands.insert_resource(Playback {
        replay,
        time: 0.0,
        speed_index: 3,
        paused: false,
        return_to,
        // So a shot at t = 0 is shown too.
        shown_until: -MARKER_SECS,
        tracer: true,
    });
    next_state.set(AppState::Replay);
}

impl Playback {
    fn end(&self) -> f32 {
        self.replay.frames.last().map_or(0.0, |f| f.t)
    }

    /// Index of the last frame at or before `time`.
    fn frame_at(&self, time: f32) -> usize {
        self.replay.frames.partition_point(|f| f.t <= time).saturating_sub(1)
    }

    /// View angles at `time`, interpolated between recorded frames.
    fn angles_at(&self, time: f32) -> (f32, f32) {
        let frames = &self.replay.frames;
        let i = self.frame_at(time);
        let a = &frames[i];
        let Some(b) = frames.get(i + 1) else { return (a.yaw, a.pitch) };
        let s = ((time - a.t) / (b.t - a.t).max(f32::EPSILON)).clamp(0.0, 1.0);
        (a.yaw + (b.yaw - a.yaw) * s, a.pitch + (b.pitch - a.pitch) * s)
    }

    fn shots_until(&self, time: f32) -> (usize, usize) {
        let frames = &self.replay.frames[..self.replay.frames.partition_point(|f| f.t <= time)];
        let hits = frames.iter().filter(|f| f.shot == Shot::Hit).count();
        let misses = frames.iter().filter(|f| f.shot == Shot::Miss).count();
        (hits, misses)
    }
}

#[derive(Resource)]
struct MarkerAssets {
    mesh: Handle<Mesh>,
    hit: Handle<StandardMaterial>,
    miss: Handle<StandardMaterial>,
}

#[derive(Component)]
struct ReplayTarget;

#[derive(Component)]
struct ShotMarker {
    /// Replay time of the shot.
    t: f32,
}

/// Text slots of the replay HUD, filled by `update_replay_hud`.
#[derive(Component, Clone, Copy, Default)]
enum ReplayField {
    #[default]
    Title,
    Time,
    Speed,
    Hits,
    Misses,
    Fps,
}

#[derive(Component, Clone, Copy, Default)]
struct TimelineFill;

fn create_marker_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let unlit = |color: Color| StandardMaterial { base_color: color, unlit: true, ..default() };
    commands.insert_resource(MarkerAssets {
        mesh: meshes.add(Sphere::new(0.06)),
        hit: materials.add(unlit(Color::srgb(0.2, 1.0, 0.4))),
        miss: materials.add(unlit(Color::srgb(1.0, 0.25, 0.25))),
    });
}

fn clear_look(mut look: ResMut<Look>, mut commands: Commands) {
    look.set(0.0, 0.0);
    commands.remove_resource::<Playback>();
}

fn playback_controls(
    keys: Res<ButtonInput<KeyCode>>,
    mut playback: ResMut<Playback>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        next_state.set(playback.return_to);
        return;
    }
    let end = playback.end();
    if keys.just_pressed(KeyCode::Space) {
        // Restart when pressing play at the very end.
        if playback.paused && playback.time >= end {
            playback.time = 0.0;
        }
        playback.paused = !playback.paused;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        playback.time = 0.0;
        playback.paused = false;
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        playback.time = (playback.time - 5.0).max(0.0);
    }
    if keys.just_pressed(KeyCode::ArrowRight) {
        playback.time = (playback.time + 5.0).min(end);
    }
    // Frame stepping while paused, for checking flicks in detail.
    if playback.paused && keys.just_pressed(KeyCode::Comma) {
        playback.time = (playback.time - 1.0 / 60.0).max(0.0);
    }
    if playback.paused && keys.just_pressed(KeyCode::Period) {
        playback.time = (playback.time + 1.0 / 60.0).min(end);
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        playback.speed_index = (playback.speed_index + 1).min(SPEEDS.len() - 1);
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        playback.speed_index = playback.speed_index.saturating_sub(1);
    }
    if keys.just_pressed(KeyCode::KeyT) {
        playback.tracer = !playback.tracer;
    }
}

/// Draws the crosshair's path over the last `TRACER_SECS`, colored by how straight each
/// movement was (see [`tracer`]), on the target plane and on top of the targets.
fn draw_tracer(playback: Res<Playback>, mut gizmos: Gizmos<TracerGizmos>) {
    if !playback.tracer {
        return;
    }
    let now = playback.time;
    let frames = &playback.replay.frames;
    let first = frames.partition_point(|f| f.t <= now - TRACER_SECS);
    let last = playback.frame_at(now);
    let mut points: Vec<(f32, Vec2)> = frames[first.min(last)..=last]
        .iter()
        .map(|f| (f.t, Vec2::new(f.yaw, f.pitch)))
        .collect();
    // End exactly at the crosshair, between recorded frames.
    let (yaw, pitch) = playback.angles_at(now);
    if points.last().is_some_and(|p| p.0 < now) {
        points.push((now, Vec2::new(yaw, pitch)));
    }
    let in_degrees: Vec<(f32, Vec2)> = points.iter().map(|&(t, a)| (t, Vec2::new(a.x.to_degrees(), a.y.to_degrees()))).collect();
    let on_plane = |a: Vec2| Quat::from_euler(EulerRot::YXZ, a.x, a.y, 0.0) * Vec3::NEG_Z * MARKER_DISTANCE;
    for (pair, bow) in points.windows(2).zip(tracer::segment_bows(&in_degrees)) {
        // Older parts of the trail fade out.
        let fade = 1.0 - ((now - pair[1].0) / TRACER_SECS).clamp(0.0, 1.0);
        let color = tracer::bow_color(bow);
        let color = color.with_alpha(color.alpha() * fade);
        gizmos.line(on_plane(pair[0].1), on_plane(pair[1].1), color);
    }
}

/// Tracer lines: thicker than default gizmos and drawn over the targets.
#[derive(Default, Reflect, GizmoConfigGroup)]
struct TracerGizmos;

fn configure_tracer(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<TracerGizmos>();
    config.line.width = 3.0;
    config.depth_bias = -1.0;
}

#[allow(clippy::too_many_arguments)]
fn advance_playback(
    time: Res<Time>,
    mut playback: ResMut<Playback>,
    mut look: ResMut<Look>,
    target_assets: Res<TargetAssets>,
    marker_assets: Res<MarkerAssets>,
    mut commands: Commands,
    mut targets: Query<(&mut Transform, &mut Visibility), With<ReplayTarget>>,
    markers: Query<(Entity, &ShotMarker)>,
) {
    if playback.replay.frames.is_empty() {
        return;
    }
    let shown_until = playback.shown_until;
    if !playback.paused {
        let end = playback.end();
        playback.time = (playback.time + time.delta_secs() * SPEEDS[playback.speed_index]).min(end);
        if playback.time >= end {
            playback.paused = true;
        }
    }
    let now = playback.time;
    let (yaw, pitch) = playback.angles_at(now);
    look.set(yaw, pitch);

    // Targets: reuse a pool of entities, one per target in the current frame.
    let frame = &playback.replay.frames[playback.frame_at(now)];
    let mut pool = targets.iter_mut();
    for &(center, radius) in &frame.targets {
        let transform = Transform::from_translation(center).with_scale(Vec3::splat(radius));
        match pool.next() {
            Some((mut existing, mut visibility)) => {
                *existing = transform;
                *visibility = Visibility::Inherited;
            }
            None => {
                commands.spawn((
                    DespawnOnExit(AppState::Replay),
                    ReplayTarget,
                    Mesh3d(target_assets.mesh.clone()),
                    MeshMaterial3d(target_assets.idle.clone()),
                    transform,
                ));
            }
        }
    }
    for (_, mut visibility) in pool {
        *visibility = Visibility::Hidden;
    }

    // Shot markers: show shots from the last MARKER_SECS of replay time. Seeks and frame steps
    // happen in `playback_controls` before this runs, so compare against what was shown last.
    let (from, reset) = marker_range(shown_until, now);
    for (entity, marker) in &markers {
        if reset || marker.t > now || now - marker.t > MARKER_SECS {
            commands.entity(entity).despawn();
        }
    }
    let frames = &playback.replay.frames;
    let start = frames.partition_point(|f| f.t <= from);
    let stop = frames.partition_point(|f| f.t <= now);
    for frame in &frames[start..stop] {
        let material = match frame.shot {
            Shot::None => continue,
            Shot::Hit => marker_assets.hit.clone(),
            Shot::Miss => marker_assets.miss.clone(),
        };
        let aim = Quat::from_euler(EulerRot::YXZ, frame.yaw, frame.pitch, 0.0) * Vec3::NEG_Z;
        commands.spawn((
            DespawnOnExit(AppState::Replay),
            ShotMarker { t: frame.t },
            Mesh3d(marker_assets.mesh.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(aim * MARKER_DISTANCE),
        ));
    }
    playback.shown_until = now;
}

/// Shots in `(from, now]` get new markers. Moving forward a little (playing or frame stepping)
/// adds only the shots since the last update; jumping back or far ahead rebuilds the whole
/// window, so `reset` asks for existing markers to be cleared first.
fn marker_range(shown_until: f32, now: f32) -> (f32, bool) {
    if now < shown_until || now - shown_until > MARKER_SECS {
        (now - MARKER_SECS, true)
    } else {
        (shown_until, false)
    }
}

fn spawn_replay_hud(mut commands: Commands, playback: Res<Playback>, assets: Res<TargetAssets>) {
    if playback.replay.kind.def().environment == Environment::Cover {
        spawn_cover(&mut commands, &assets, AppState::Replay);
    }
    commands
        .spawn_scene(bsn! {
            @hud_anchor(px(14))
            Children [
                @hud_bar()
                Children [
                    Node { padding: UiRect::axes(px(8), px(2)), border_radius: BorderRadius::all(px(4)) }
                    ThemeBackgroundColor(tokens::BUTTON_PRIMARY_BG)
                    Children [ @styled("REPLAY", fonts::BOLD, 12.0, TEXT_STRONG) ]
                    --
                    @styled("", fonts::BOLD, 15.0, tokens::TEXT_MAIN)
                    ReplayField::Title
                    --
                    @vertical_divider()
                    --
                    @styled("", fonts::BOLD, 22.0, TEXT_STRONG)
                    ReplayField::Time
                    --
                    @styled("", fonts::BOLD, 15.0, TEXT_ACCENT)
                    ReplayField::Speed
                    --
                    @vertical_divider()
                    --
                    @styled("", fonts::REGULAR, 16.0, TEXT_GOOD)
                    ReplayField::Hits
                    --
                    @styled("", fonts::REGULAR, 16.0, TEXT_BAD)
                    ReplayField::Misses
                    --
                    @vertical_divider()
                    --
                    @styled("", fonts::REGULAR, 12.0, tokens::TEXT_DIM)
                    ReplayField::Fps
                ]
            ]
        })
        .insert(DespawnOnExit(AppState::Replay));
    commands
        .spawn_scene(bsn! {
            Node {
                position_type: PositionType::Absolute,
                bottom: px(18),
                left: percent(10),
                width: percent(80),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(10),
            }
            Children [
                @hud_bar()
                Node { padding: UiRect::axes(px(14), px(5)) }
                Children [
                    @styled(
                        "Space play/pause    Left/Right -/+5 s    , . frame step    Up/Down speed    T tracer    R restart    Esc back",
                        fonts::REGULAR,
                        12.0,
                        tokens::TEXT_MAIN
                    )
                ]
                --
                Node { width: percent(100), height: px(6), border_radius: BorderRadius::all(px(3)) }
                ThemeBackgroundColor(DIVIDER)
                Children [
                    Node { width: percent(0), height: percent(100), border_radius: BorderRadius::all(px(3)) }
                    ThemeBackgroundColor(tokens::BUTTON_PRIMARY_BG)
                    TimelineFill
                ]
            ]
        })
        .insert(DespawnOnExit(AppState::Replay));
}

fn update_replay_hud(
    playback: Res<Playback>,
    diagnostics: Res<DiagnosticsStore>,
    mut fields: Query<(&ReplayField, &mut Text)>,
    mut fill: Single<&mut Node, With<TimelineFill>>,
) {
    let end = playback.end().max(f32::EPSILON);
    let (hits, misses) = playback.shots_until(playback.time);
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|fps| fps.smoothed())
        .unwrap_or(0.0);
    for (field, mut text) in &mut fields {
        let wanted = match field {
            ReplayField::Title => format!("{}  ·  sens {:.3}", playback.replay.name, playback.replay.sens),
            ReplayField::Time => format!("{:.1} / {:.1} s", playback.time, end),
            ReplayField::Speed => {
                format!("{}x{}", SPEEDS[playback.speed_index], if playback.paused { "  PAUSED" } else { "" })
            }
            ReplayField::Hits => format!("{hits} hits"),
            ReplayField::Misses => format!("{misses} misses"),
            ReplayField::Fps => format!("{fps:.0} fps"),
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    fill.width = percent(playback.time / end * 100.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shot_markers_follow_steps_and_seeks() {
        // Start: a shot at t = 0 is inside the first window.
        assert_eq!(marker_range(-MARKER_SECS, 0.0), (-MARKER_SECS, false));
        // A frame step forward adds just the shots since the last update.
        assert_eq!(marker_range(1.0, 1.004), (1.0, false));
        // Seeking far ahead or stepping back rebuilds the whole window.
        assert_eq!(marker_range(1.0, 6.0), (6.0 - MARKER_SECS, true));
        assert_eq!(marker_range(1.0, 0.996), (0.996 - MARKER_SECS, true));
    }

    fn sample_replay() -> Replay {
        let mut recorder = Recorder::default();
        recorder.sample(0.0, (0.1, -0.2), [(Vec3::new(1.0, 2.0, -10.0), 0.45)].into_iter());
        recorder.sample(0.001, (0.2, -0.1), std::iter::empty()); // too soon: skipped
        recorder.mark_shot(true);
        recorder.sample(0.002, (0.3, 0.0), std::iter::empty()); // shot: always kept
        recorder.sample(0.01, (0.4, 0.1), std::iter::empty());
        recorder.finish(DrillKind::Tracking, "My tracking", 0.4, 60.0)
    }

    #[test]
    fn recorder_throttles_but_keeps_shot_frames() {
        let replay = sample_replay();
        let times: Vec<f32> = replay.frames.iter().map(|f| f.t).collect();
        assert_eq!(times, [0.0, 0.002, 0.01]);
        assert_eq!(replay.frames[1].shot, Shot::Hit);
        assert_eq!(replay.frames[2].shot, Shot::None);
    }

    /// A drill-like replay: irregular ~170 fps frames, mouse movement in whole counts, shots
    /// between samples, targets moving and shrinking for a while, killed, respawned and reordered.
    fn synthetic_replay() -> Replay {
        let mut rng = fastrand::Rng::with_seed(7);
        let mut recorder = Recorder::default();
        let (mut t, mut yaw, mut pitch) = (0.0_f32, 0.0_f32, 0.0_f32);
        let mut targets = vec![
            (Vec3::new(1.2097, 1.2187, -9.8515), 0.3),
            (Vec3::new(-1.2187, 0.0, -9.9255), 0.28),
            (Vec3::new(0.0, 0.0, -10.0), 0.33),
        ];
        let spawn = |rng: &mut fastrand::Rng| (Vec3::new(rng.f32() * 2.0 - 1.0, rng.f32() * 2.0 - 1.0, -10.0), 0.35);
        for frame in 0..3000 {
            t += 1.0 / 170.0 + rng.f32() * 0.0005;
            yaw -= rng.i32(-20..=20) as f32 * RAD_PER_COUNT;
            pitch = (pitch - rng.i32(-5..=5) as f32 * RAD_PER_COUNT).clamp(-1.56, 1.56);
            if frame < 1500 {
                targets.iter_mut().for_each(|target| target.1 *= 0.999);
            }
            if (1000..2000).contains(&frame) {
                targets[0].0.x += 0.02 * (t * 3.0).sin();
            }
            if frame % 150 == 149 {
                targets.remove(1);
                targets.insert(0, spawn(&mut rng));
            }
            if frame % 400 == 399 && targets.len() > 2 {
                targets.pop();
            } else if frame % 400 == 399 {
                targets.push(spawn(&mut rng));
            }
            recorder.sample(t, (yaw, pitch), targets.iter().copied());
            if frame % 97 == 0 {
                recorder.mark_shot(frame % 2 == 0);
                recorder.sample(t + 0.0013, (yaw, pitch), targets.iter().copied());
            }
        }
        recorder.finish(DrillKind::Gridshot, "Gridshot · decay", 0.4, 30.0)
    }

    const RAD_PER_COUNT: f32 = 0.4 * 0.07 * std::f32::consts::PI / 180.0;

    /// The version 2 encoding, as older builds saved it.
    fn encode_v2(replay: &Replay) -> Vec<u8> {
        let mut out = replay.encode_header(2);
        out.extend_from_slice(&(replay.frames.len() as u32).to_le_bytes());
        for frame in &replay.frames {
            for value in [frame.t, frame.yaw, frame.pitch] {
                out.extend_from_slice(&value.to_le_bytes());
            }
            out.push(frame.shot as u8);
            out.push(frame.targets.len() as u8);
            for (center, radius) in &frame.targets {
                for value in [center.x, center.y, center.z, *radius] {
                    out.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
        out
    }

    #[test]
    fn encoding_round_trips_within_quantization() {
        let replay = sample_replay();
        assert!(Replay::decode(&replay.encode()).is_some_and(|decoded| decoded.matches(&replay)));

        let replay = synthetic_replay();
        assert!((angle_step(&replay.frames) as f64 - f64::from(RAD_PER_COUNT) * 1e9).abs() < 1.0);
        let bytes = replay.encode();
        let decoded = Replay::decode(&bytes).unwrap();
        assert!(decoded.matches(&replay));
        let angle_error = decoded
            .frames
            .iter()
            .zip(&replay.frames)
            .flat_map(|(a, b)| [a.yaw - b.yaw, a.pitch - b.pitch])
            .fold(0.0_f32, |max, error| max.max(error.abs()));
        assert!(angle_error <= 1e-6, "{angle_error}");
        // Shots keep their count, order and exact times, including those between samples.
        let shots = |r: &Replay| r.frames.iter().filter(|f| f.shot != Shot::None).map(|f| (f.t, f.shot)).collect::<Vec<_>>();
        assert_eq!(shots(&decoded).len(), 31);
        assert_eq!(shots(&decoded), shots(&replay));
        // Target sets change (spawns, kills, reorders, movement) exactly where they did.
        let target_counts = |r: &Replay| r.frames.iter().map(|f| f.targets.len()).collect::<Vec<_>>();
        assert_eq!(target_counts(&decoded), target_counts(&replay));
        assert!(bytes.len() * 8 < encode_v2(&replay).len(), "{} bytes", bytes.len());
    }

    #[test]
    fn decodes_version_2() {
        let replay = synthetic_replay();
        assert_eq!(Replay::decode(&encode_v2(&replay)), Some(replay));
    }

    #[test]
    fn rejects_truncated_or_foreign_files() {
        for bytes in [sample_replay().encode(), synthetic_replay().encode(), encode_v2(&sample_replay())] {
            for len in [5, 20, bytes.len() / 2, bytes.len() - 1] {
                assert!(Replay::decode(&bytes[..len]).is_none(), "{len} of {} bytes", bytes.len());
            }
        }
        // The zlib checksum catches corruption inside the compressed body.
        let mut bytes = synthetic_replay().encode();
        let middle = bytes.len() / 2;
        bytes[middle] ^= 0x10;
        assert!(Replay::decode(&bytes).is_none());
        assert!(Replay::decode(b"nope").is_none());
    }

    #[test]
    fn migrates_old_replays_in_place() {
        let dir = std::env::temp_dir().join(format!("aim_trainer_replay_migration_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let replay = synthetic_replay();
        let (old, current, foreign) = (dir.join("1.replay"), dir.join("2.replay"), dir.join("3.replay"));
        fs::write(&old, encode_v2(&replay)).unwrap();
        fs::write(&current, sample_replay().encode()).unwrap();
        fs::write(&foreign, b"nope").unwrap();
        fs::write(dir.join("4.replay.tmp"), b"interrupted").unwrap();

        assert_eq!(migrate_dir(&dir), 1);
        let migrated = fs::read(&old).unwrap();
        assert_eq!(migrated[4], VERSION);
        assert!(migrated.len() * 8 < encode_v2(&replay).len());
        assert!(Replay::decode(&migrated).unwrap().matches(&replay));
        // Current and foreign files are left alone; the stale temporary file is gone.
        assert_eq!(fs::read(&current).unwrap(), sample_replay().encode());
        assert_eq!(fs::read(&foreign).unwrap(), b"nope");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 3);
        assert_eq!(migrate_dir(&dir), 0);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn interpolates_angles_between_frames() {
        let playback = Playback {
            replay: sample_replay(),
            time: 0.0,
            speed_index: 3,
            paused: true,
            return_to: AppState::Menu,
            shown_until: -MARKER_SECS,
            tracer: true,
        };
        let (yaw, pitch) = playback.angles_at(0.006);
        assert!((yaw - 0.35).abs() < 1e-5 && (pitch - 0.05).abs() < 1e-5);
        assert_eq!(playback.angles_at(1.0), (0.4, 0.1));
        assert_eq!(playback.shots_until(0.005), (1, 0));
    }

    fn fixture(name: &str) -> Vec<u8> {
        fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(name)).unwrap()
    }

    /// Pins the v3 bytes (header, body coding, compression) and the v1/v2 decoders.
    #[test]
    fn decodes_saved_fixtures() {
        let replay = fixture_replay();
        let v3 = fixture("replay_v3.bin");
        assert_eq!(replay.encode(), v3);
        assert!(Replay::decode(&v3).unwrap().matches(&replay));
        assert_eq!(Replay::decode(&fixture("replay_v2.bin")), Some(replay.clone()));
        // v1 has no name: the built-in name stands in (the fixture's name is that already).
        assert_eq!(Replay::decode(&fixture("replay_v1.bin")), Some(replay));
    }

    /// Replays store the kind as its index in `ALL`.
    #[test]
    fn kind_index_is_stable() {
        assert_eq!(
            DrillKind::ALL.map(DrillKind::name),
            ["Gridshot", "Microflick", "Tracking", "Head level", "Peeks", "Reaction", "Peripheral"]
        );
    }

    /// The replay the `fixtures/replay_v*.bin` files were saved from.
    fn fixture_replay() -> Replay {
        let frames = vec![
            ReplayFrame { t: 0.0, yaw: 0.0, pitch: 0.0, shot: Shot::None, targets: vec![(Vec3::new(1.5, 0.25, -10.0), 0.2)] },
            ReplayFrame { t: 0.004, yaw: -0.0125, pitch: 0.003, shot: Shot::Miss, targets: vec![(Vec3::new(1.4, 0.25, -10.0), 0.2)] },
            ReplayFrame { t: 0.0085, yaw: -0.025, pitch: 0.006, shot: Shot::Hit, targets: vec![] },
            ReplayFrame { t: 0.0125, yaw: -0.03, pitch: 0.005, shot: Shot::None, targets: vec![(Vec3::new(-2.0, 0.0, -10.0), 0.2), (Vec3::new(0.5, 1.0, -9.5), 0.15)] },
        ];
        Replay { kind: DrillKind::Peek, name: "Peeks".into(), sens: 0.4, duration: 20.0, frames }
    }
}
