//! Discord Rich Presence: shows what you're doing in the trainer on your Discord profile.
//!
//! The game derives a [`Status`] from the current screen every frame and hands changes to a
//! worker thread that owns the IPC connection, so a missing or slow Discord client never stalls a
//! frame. The worker reconnects every few seconds while Discord isn't running and sends at most
//! one update every few seconds (Discord allows 5 per 20 s), always the latest one.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use bevy::prelude::*;
use discord_rich_presence::{
    DiscordIpc, DiscordIpcClient,
    activity::{Activity, Assets, Timestamps},
};

use crate::{
    AppState,
    finder::Finder,
    routine::Routine,
    drill::{Run, RunPurpose},
    history::LastRun,
    settings::Settings,
    smart_finder::SmartFinder,
};

/// The "Aim Trainer" Discord application; its name is what profiles show. Application IDs are
/// public identifiers, not secrets.
const APP_ID: &str = "1566052324365828197";
/// Art asset key of the logo (large image).
const LOGO: &str = "logo";
const RECONNECT_EVERY: Duration = Duration::from_secs(10);
const MIN_UPDATE_GAP: Duration = Duration::from_secs(4);
/// A countdown end moving by less than this isn't worth an update.
const END_TOLERANCE_SECS: i64 = 2;

pub struct PresencePlugin;

impl Plugin for PresencePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PresenceLink::spawn())
            .add_systems(Update, (configure.run_if(resource_changed::<Settings>), publish).chain());
    }
}

/// Connection state reported by the worker, for the settings screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Connection {
    /// Turned off in the settings.
    Off,
    /// Trying to reach the Discord client (not running).
    Searching,
    Connected,
}

/// What the profile shows: two lines of text, how long it has been going on, optionally a
/// countdown to when it ends, and a badge for the kind of activity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub details: String,
    pub state: String,
    /// Unix seconds.
    pub started: i64,
    /// Unix seconds.
    pub ends: Option<i64>,
    pub badge: Option<Badge>,
}

/// Small corner image next to the logo. Keys name the art assets uploaded to the Discord
/// application (Rich Presence > Art Assets); the art is in `assets/icons/discord`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Badge {
    Practice,
    Routine,
    Finder,
    Replay,
    Results,
}

impl Badge {
    fn key(self) -> &'static str {
        match self {
            Badge::Practice => "practice",
            Badge::Routine => "routine",
            Badge::Finder => "finder",
            Badge::Replay => "replay",
            Badge::Results => "results",
        }
    }

    /// Hover text.
    fn caption(self) -> &'static str {
        match self {
            Badge::Practice => "Practicing",
            Badge::Routine => "Warming up",
            Badge::Finder => "Finding their sensitivity",
            Badge::Replay => "Watching a replay",
            Badge::Results => "Looking at results",
        }
    }
}

enum Message {
    Enable(bool),
    Show(Status),
}

#[derive(Resource)]
pub struct PresenceLink {
    tx: Sender<Message>,
    connection: Arc<AtomicU8>,
    /// Last on/off state sent to the worker.
    enabled: Option<bool>,
    shown: Option<Status>,
}

impl PresenceLink {
    fn spawn() -> Self {
        let (tx, rx) = mpsc::channel();
        let connection = Arc::new(AtomicU8::new(Connection::Off as u8));
        let reported = connection.clone();
        thread::Builder::new()
            .name("discord-presence".into())
            .spawn(move || worker(rx, &reported))
            .expect("spawning the Discord presence thread");
        Self { tx, connection, enabled: None, shown: None }
    }

    pub fn connection(&self) -> Connection {
        match self.connection.load(Ordering::Relaxed) {
            1 => Connection::Searching,
            2 => Connection::Connected,
            _ => Connection::Off,
        }
    }
}

fn configure(settings: Res<Settings>, mut link: ResMut<PresenceLink>) {
    let on = settings.discord.enabled;
    if link.enabled != Some(on) {
        // A send error means the worker is gone; presence just stays off.
        let _ = link.tx.send(Message::Enable(on));
        link.enabled = Some(on);
    }
}

#[allow(clippy::too_many_arguments)]
fn publish(
    state: Res<State<AppState>>,
    run: Option<Res<Run>>,
    last: Option<Res<LastRun>>,
    routine: Option<Res<Routine>>,
    finder: Option<Res<Finder>>,
    smart: Option<Res<SmartFinder>>,
    mut link: ResMut<PresenceLink>,
) {
    let now = unix_now();
    let described = describe(*state.get(), run.as_deref(), last.as_deref(), routine.as_deref(), finder.is_some() || smart.is_some());
    if let Some(status) = next_status(link.shown.as_ref(), now, described) {
        let _ = link.tx.send(Message::Show(status.clone()));
        link.shown = Some(status);
    }
}

/// The two text lines, for a live run the seconds left, and the activity badge.
type Described = (String, String, Option<f32>, Option<Badge>);

fn describe(state: AppState, run: Option<&Run>, last: Option<&LastRun>, routine: Option<&Routine>, finding: bool) -> Described {
    let step = |routine: &Routine| format!("{} · step {}/{}", routine.name, (routine.next + 1).min(routine.steps.len()), routine.steps.len());
    match state {
        AppState::Menu => ("In the menu".into(), "Picking a drill".into(), None, None),
        AppState::Playing => match run {
            Some(run) => {
                let (details, line, badge) = match run.purpose {
                    RunPurpose::Practice => (run.drill.name.clone(), "Practice".to_string(), Badge::Practice),
                    RunPurpose::Routine => (run.drill.name.clone(), routine.map_or_else(|| "Warmup routine".into(), step), Badge::Routine),
                    RunPurpose::FinderTrial => ("Finding their sensitivity".to_string(), run.title.clone(), Badge::Finder),
                };
                let line = if run.time_left().is_some() { line } else { format!("{line} · getting ready") };
                (details, line, run.time_left(), Some(badge))
            }
            None => ("Playing".into(), String::new(), None, None),
        },
        AppState::Results => match last {
            Some(last) => (last.stats.drill.clone(), format!("Scored {}", last.stats.score()), None, Some(Badge::Results)),
            None => ("Results".into(), String::new(), None, Some(Badge::Results)),
        },
        AppState::Finder if finding => ("Finding their sensitivity".into(), "Between trials".into(), None, Some(Badge::Finder)),
        AppState::Finder => ("Sensitivity finder".into(), String::new(), None, Some(Badge::Finder)),
        AppState::Replay => ("Watching a replay".into(), String::new(), None, Some(Badge::Replay)),
        AppState::Routine => match routine {
            Some(routine) if routine.finished() => (routine.name.clone(), "Routine complete".into(), None, Some(Badge::Routine)),
            Some(routine) => (
                routine.name.clone(),
                format!("Break before step {}/{}", routine.next + 1, routine.steps.len()),
                None,
                Some(Badge::Routine),
            ),
            None => ("Warmup routine".into(), String::new(), None, Some(Badge::Routine)),
        },
    }
}

/// The status to send, if it differs meaningfully from what's shown. The elapsed timer keeps
/// running while only the second line changes (e.g. routine steps of the same routine).
fn next_status(shown: Option<&Status>, now: i64, (details, state, left, badge): Described) -> Option<Status> {
    let ends = left.map(|secs| now + secs.round() as i64);
    if let Some(shown) = shown {
        let same_end = match (shown.ends, ends) {
            (Some(a), Some(b)) => (a - b).abs() < END_TOLERANCE_SECS,
            (a, b) => a == b,
        };
        if shown.details == details && shown.state == state && shown.badge == badge && same_end {
            return None;
        }
    }
    let started = shown.filter(|s| s.details == details).map_or(now, |s| s.started);
    Some(Status { details, state, started, ends, badge })
}

fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn worker(rx: Receiver<Message>, connection: &AtomicU8) {
    let mut enabled = false;
    let mut client: Option<DiscordIpcClient> = None;
    let mut wanted: Option<Status> = None;
    let mut dirty = false;
    let mut next_attempt = Instant::now();
    let mut last_sent: Option<Instant> = None;
    let report = |state: Connection| connection.store(state as u8, Ordering::Relaxed);
    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Message::Enable(on)) => {
                if on != enabled {
                    // Turning it off disconnects, which clears the profile's activity.
                    if let Some(mut old) = client.take() {
                        let _ = old.close();
                    }
                    enabled = on;
                    next_attempt = Instant::now();
                    dirty = true;
                }
            }
            Ok(Message::Show(status)) => {
                wanted = Some(status);
                dirty = true;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                if let Some(mut old) = client.take() {
                    let _ = old.close();
                }
                return;
            }
        }
        if !enabled {
            report(Connection::Off);
            continue;
        }
        let now = Instant::now();
        if client.is_none() {
            report(Connection::Searching);
            if now < next_attempt {
                continue;
            }
            let mut fresh = DiscordIpcClient::new(APP_ID);
            if fresh.connect().is_ok() {
                client = Some(fresh);
                dirty = true;
            } else {
                next_attempt = now + RECONNECT_EVERY;
                continue;
            }
        }
        report(Connection::Connected);
        let throttled = last_sent.is_some_and(|at| now.duration_since(at) < MIN_UPDATE_GAP);
        if !dirty || throttled {
            continue;
        }
        let (Some(open), Some(status)) = (client.as_mut(), wanted.as_ref()) else { continue };
        let mut timestamps = Timestamps::new().start(status.started * 1000);
        if let Some(ends) = status.ends {
            timestamps = timestamps.end(ends * 1000);
        }
        let mut assets = Assets::new().large_image(LOGO).large_text("Aim Trainer");
        if let Some(badge) = status.badge {
            assets = assets.small_image(badge.key()).small_text(badge.caption());
        }
        let mut activity = Activity::new().details(status.details.as_str()).timestamps(timestamps).assets(assets);
        if !status.state.is_empty() {
            activity = activity.state(status.state.as_str());
        }
        last_sent = Some(now);
        if open.set_activity(activity).is_ok() {
            dirty = false;
        } else {
            // Discord was closed or restarted: reconnect and resend.
            client = None;
            next_attempt = now + RECONNECT_EVERY;
            report(Connection::Searching);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_meaningful_changes_are_sent() {
        let gridshot = |line: &str, left| ("Gridshot".to_string(), line.to_string(), left, Some(Badge::Practice));
        let first = next_status(None, 1000, gridshot("Practice", Some(30.0))).unwrap();
        assert_eq!((first.started, first.ends), (1000, Some(1030)));
        // A frame later the countdown end is the same (within rounding): nothing to send.
        assert!(next_status(Some(&first), 1001, gridshot("Practice", Some(29.4))).is_none());
        // Same activity, new second line: keeps the elapsed timer.
        let next = next_status(Some(&first), 1031, gridshot("Scored 120", None)).unwrap();
        assert_eq!((next.started, next.ends), (1000, None));
        // A new badge alone is a change worth sending.
        let results = next_status(Some(&next), 1032, ("Gridshot".into(), "Scored 120".into(), None, Some(Badge::Results))).unwrap();
        assert_eq!(results.started, 1000);
        // A different activity restarts it.
        let menu = next_status(Some(&results), 1040, ("In the menu".into(), "Picking a drill".into(), None, None)).unwrap();
        assert_eq!(menu.started, 1040);
    }
}
