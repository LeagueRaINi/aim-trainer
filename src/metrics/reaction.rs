//! Statistics of a list of reaction times: the median and quartiles (which the odd slow trial
//! barely moves), the mean and standard deviation, the score a median becomes, and how far apart
//! two medians have to be before the difference means anything.

/// What a test's valid reaction times come to, in milliseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Summary {
    /// How many reaction times went in.
    pub count: usize,
    pub median: f32,
    /// First and third quartile: the middle half of the trials lies between them.
    pub q1: f32,
    pub q3: f32,
    /// The fastest one.
    pub best: f32,
    pub mean: f32,
    /// Sample standard deviation (n - 1); 0 for a single value.
    pub sd: f32,
}

impl Summary {
    /// `None` without values.
    pub fn of(values: &[f32]) -> Option<Self> {
        let mut sorted = values.to_vec();
        sorted.sort_by(f32::total_cmp);
        let best = *sorted.first()?;
        let count = sorted.len();
        let mean = sorted.iter().sum::<f32>() / count as f32;
        let squares = sorted.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>();
        let sd = if count > 1 { (squares / (count - 1) as f32).sqrt() } else { 0.0 };
        Some(Self {
            count,
            median: quantile(&sorted, 0.5),
            q1: quantile(&sorted, 0.25),
            q3: quantile(&sorted, 0.75),
            best,
            mean,
            sd,
        })
    }

    /// Interquartile range: the width of the middle half of the trials.
    pub fn iqr(&self) -> f32 {
        self.q3 - self.q1
    }

    /// Standard error of the median: for roughly normal data 1.2533 σ / √n, with σ estimated
    /// from the interquartile range (IQR / 1.349) so slow outliers don't inflate it.
    pub fn median_error(&self) -> f32 {
        1.2533 * (self.iqr() / 1.349) / (self.count as f32).sqrt()
    }

    /// How far (ms) this median and `other`'s can differ by chance alone: two standard errors
    /// of their difference, about a 95% band. A smaller difference says nothing.
    pub fn noise_against(&self, other: &Summary) -> f32 {
        2.0 * self.median_error().hypot(other.median_error())
    }
}

/// The value the fraction `p` of `sorted` (ascending, not empty) lies below, interpolated
/// linearly between the two neighbouring values (position `p (n - 1)`, the method spreadsheets
/// and NumPy use by default). For `p` = 0.5 that is the usual median: the middle value, or the
/// mean of the middle two.
pub fn quantile(sorted: &[f32], p: f32) -> f32 {
    let position = p * (sorted.len() - 1) as f32;
    let below = position.floor() as usize;
    let above = (below + 1).min(sorted.len() - 1);
    sorted[below] + (sorted[above] - sorted[below]) * (position - below as f32)
}

/// The history score of a reaction test: 100 000 / median reaction time in ms, rounded, so
/// faster is higher and 200 ms scores 500 (the scale of the Reaction drill's score at full
/// accuracy). Persisted in the history: never change it.
pub fn score(median_ms: f32) -> f32 {
    if median_ms > 0.0 { (100_000.0 / median_ms).round() } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_quartiles_and_score_of_known_reactions() {
        // Unsorted on purpose; eight values, so the median and both quartiles are interpolated.
        let summary = Summary::of(&[230.0, 180.0, 250.0, 200.0, 190.0, 220.0, 210.0, 240.0]).unwrap();
        assert_eq!(summary.count, 8);
        assert_eq!((summary.median, summary.q1, summary.q3), (215.0, 197.5, 232.5));
        assert_eq!((summary.iqr(), summary.best, summary.mean), (35.0, 180.0, 215.0));
        assert!((summary.sd - 600f32.sqrt()).abs() < 1e-3, "{}", summary.sd);
        assert_eq!(score(summary.median), 465.0);

        // An odd count: the middle value itself; one slow outlier moves the mean, not the median.
        let odd = Summary::of(&[200.0, 190.0, 900.0, 210.0, 205.0]).unwrap();
        assert_eq!((odd.median, odd.q1, odd.q3), (205.0, 200.0, 210.0));
        assert_eq!(odd.mean, 341.0);

        let single = Summary::of(&[250.0]).unwrap();
        assert_eq!((single.median, single.q1, single.q3, single.sd), (250.0, 250.0, 250.0, 0.0));
        assert_eq!(Summary::of(&[]), None);
    }

    #[test]
    fn faster_medians_score_higher() {
        assert_eq!(score(200.0), 500.0);
        assert!(score(180.0) > score(200.0) && score(200.0) > score(260.0));
        assert_eq!(score(0.0), 0.0);
    }

    #[test]
    fn noise_shrinks_with_more_and_tighter_trials() {
        let spread = |step: f32, count: usize| {
            let values: Vec<f32> = (0..count).map(|i| 200.0 + step * i as f32).collect();
            Summary::of(&values).unwrap()
        };
        let (few, many, tight) = (spread(4.0, 10), spread(1.0, 40), spread(0.5, 40));
        // Same range of values: four times the trials halve the error.
        assert!((few.median_error() / many.median_error() - 2.0).abs() < 0.2);
        assert!(tight.median_error() < many.median_error());
        // 30 trials with a 40 ms IQR: the median is good to about ±7 ms, a difference to ±19 ms.
        let typical = Summary { count: 30, median: 220.0, q1: 200.0, q3: 240.0, best: 180.0, mean: 222.0, sd: 30.0 };
        assert!((typical.median_error() - 6.78).abs() < 0.05, "{}", typical.median_error());
        assert!((typical.noise_against(&typical) - 19.2).abs() < 0.1);
    }
}
