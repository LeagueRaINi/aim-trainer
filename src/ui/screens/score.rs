//! Score presentation shared by the post-run screens, and the way back to the menu.

use bevy::feathers::constants::fonts;
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::AppState;
use crate::drill::RunStats;
use crate::finder::classic::Finder;
use crate::finder::smart::SmartFinder;
use crate::history::Comparison;
use crate::routine::Routine;
use crate::ui::theme::*;
use crate::ui::widgets::*;

/// Back to the menu, ending any finder or routine in progress.
pub fn go_to_menu(_: On<Activate>, mut commands: Commands, mut next_state: ResMut<NextState<AppState>>) {
    commands.remove_resource::<Finder>();
    commands.remove_resource::<SmartFinder>();
    commands.remove_resource::<Routine>();
    next_state.set(AppState::Menu);
}

// ---------------------------------------------------------------------------------------------
// Score presentation
// ---------------------------------------------------------------------------------------------

/// How a score compares to earlier practice runs of the same drill, in a word.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Verdict {
    /// Nothing to compare with yet.
    FirstRun,
    NewBest,
    /// Percent above (or below) the average of your recent runs.
    VersusAverage(f32),
    /// The reference is zero (e.g. Reaction runs without a hit), so there is no percentage.
    Unrated,
}

impl Verdict {
    pub fn of(score: f32, previous: &Comparison) -> Self {
        match (previous.best, previous.recent_avg) {
            (None, _) => Verdict::FirstRun,
            (Some(best), _) if score > best => Verdict::NewBest,
            (_, Some(average)) if average > 0.0 => Verdict::VersusAverage((score / average - 1.0) * 100.0),
            _ => Verdict::Unrated,
        }
    }

    /// The badge shown next to the score: its text and color.
    fn badge(self) -> Option<(String, ThemeToken)> {
        match self {
            Verdict::FirstRun => Some(("First run: baseline set".into(), TEXT_ACCENT)),
            Verdict::NewBest => Some(("New personal best".into(), GOLD)),
            Verdict::VersusAverage(percent) if percent > 0.05 => Some(("Above your average".into(), TEXT_GOOD)),
            Verdict::VersusAverage(percent) if percent < -0.05 => Some(("Below your average".into(), TEXT_BAD)),
            Verdict::VersusAverage(_) => Some(("Right on your average".into(), tokens::TEXT_DIM)),
            Verdict::Unrated => None,
        }
    }
}

/// How this score compares to earlier practice runs of the same drill: a badge with the verdict
/// and one row per reference with the percent change.
pub fn comparison(score: f32, previous: &Comparison) -> impl Scene {
    // A zero reference (e.g. a Reaction run without hits) has no meaningful percent change.
    let percent = |reference: f32| (reference > 0.0).then(|| (score / reference - 1.0) * 100.0);
    let headline: Vec<_> = Verdict::of(score, previous)
        .badge()
        .map(|(text, color)| {
            bsn! {
                @row()
                Node { margin: UiRect::bottom(px(SPACE_XS)) }
                Children [ @badge(text, color) ]
            }
        })
        .into_iter()
        .collect();
    let mut rows = Vec::new();
    // With a single earlier run, "best" and "previous run" are the same run: show it once.
    let only_one = previous.recent_count == 1;
    if let Some(best) = previous.best {
        let label = match (only_one, score > best) {
            (true, _) => "Your only previous run",
            (false, true) => "Previous best",
            (false, false) => "Personal best",
        };
        rows.push(comparison_row(label.into(), best, percent(best)));
    }
    if let Some(avg) = previous.recent_avg.filter(|_| !only_one) {
        let runs = format!("Avg of your last {} runs", previous.recent_count);
        rows.push(comparison_row(runs, avg, percent(avg)));
    }
    bsn! {
        @column()
        Node { row_gap: px(SPACE_XS) }
        Children [ {headline} -- {rows} ]
    }
}

fn comparison_row(text: String, reference: f32, percent: Option<f32>) -> impl Scene {
    let change = percent.map_or("-".to_string(), |p| format!("{p:+.1}%"));
    let color = percent.map_or(tokens::TEXT_DIM, delta_color);
    bsn! {
        @row()
        Node { column_gap: px(SPACE_M) }
        Children [
            @label_dim(text)
            --
            @spacer()
            --
            @styled(score_text(reference), fonts::BOLD, FONT_BODY, TEXT_STRONG)
            TextFont { font_features: {tabular()} }
            --
            Node { width: px(68), justify_content: JustifyContent::End }
            Children [ @styled(change, fonts::BOLD, FONT_BODY, color) TextFont { font_features: {tabular()} } ]
        ]
    }
}

/// Big score with the comparison next to it, and the secondary stats as tiles below.
pub fn score_summary(stats: &RunStats, previous: &Comparison) -> impl Scene {
    let score = stats.score();
    let tiles: Vec<_> = stats.key_stats().into_iter().map(|(caption, value)| stat(caption, value)).collect();
    bsn! {
        @column()
        Node { row_gap: px(SPACE_M) }
        Children [
            @row()
            Node { column_gap: px(SPACE_XL), align_items: AlignItems::Center }
            Children [
                @hero("Score", score_text(score))
                Node { min_width: px(170) }
                --
                @vertical_divider()
                --
                @comparison(score, previous)
                Node { flex_grow: 1.0 }
            ]
            --
            @row()
            Node { align_items: AlignItems::Stretch }
            Children [ {tiles} ]
        ]
    }
}

/// Small caps context line above a screen title.
pub fn screen_title(context: impl Into<String>, title: impl Into<String>) -> impl Scene {
    bsn! {
        @column()
        Node { row_gap: px(2) }
        Children [ @overline_colored(context, TEXT_ACCENT) -- @heading(title) ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_names_the_first_run_a_new_best_and_the_distance_to_the_average() {
        let none = Comparison::default();
        assert_eq!(Verdict::of(100.0, &none), Verdict::FirstRun);
        let earlier = Comparison { best: Some(120.0), recent_avg: Some(100.0), recent_count: 3 };
        assert_eq!(Verdict::of(130.0, &earlier), Verdict::NewBest);
        assert!(matches!(Verdict::of(110.0, &earlier), Verdict::VersusAverage(p) if (p - 10.0).abs() < 1e-3));
        assert!(matches!(Verdict::of(90.0, &earlier), Verdict::VersusAverage(p) if p < 0.0));
        // A zero average (Reaction runs without a hit) has no percentage.
        let zero = Comparison { best: Some(0.0), recent_avg: Some(0.0), recent_count: 2 };
        assert_eq!(Verdict::of(0.0, &zero), Verdict::Unrated);
    }
}
