//! The small statistics the finder's evidence gates share: means and spreads, t statistics,
//! and one critical value. All of it is deliberately strict: the monitor, the tired check and
//! the long comparison look at their data again after every run or day, and every look is another
//! chance for noise to pass a lenient test.

/// Days are counted in UTC: a "day" is a calendar day of the history timestamps. Two sessions
/// either side of midnight UTC count as two days, which only makes "several separate days"
/// slightly easier to reach, never a pattern easier to find.
pub const SECS_PER_DAY: u64 = 86_400;

/// The UTC day a history timestamp falls on.
pub fn day_of(unix_time: u64) -> u64 {
    unix_time / SECS_PER_DAY
}

/// Count, mean and sample standard deviation of a set of values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Summary {
    pub n: usize,
    pub mean: f32,
    /// Sample standard deviation (n - 1); 0 for a single value.
    pub sd: f32,
}

impl Summary {
    /// `None` without values.
    pub fn of(values: &[f32]) -> Option<Self> {
        if values.is_empty() {
            return None;
        }
        let n = values.len();
        let mean = values.iter().map(|&v| f64::from(v)).sum::<f64>() / n as f64;
        let squares = values.iter().map(|&v| (f64::from(v) - mean).powi(2)).sum::<f64>();
        let sd = if n > 1 { (squares / (n - 1) as f64).sqrt() } else { 0.0 };
        Some(Self { n, mean: mean as f32, sd: sd as f32 })
    }

    /// Standard error of the mean.
    pub fn se(&self) -> f32 {
        self.sd / (self.n as f32).sqrt()
    }

    /// How many standard errors the mean lies above `reference`: the one-sample t statistic.
    /// Infinite (with the sign of the difference) when the values don't vary at all; 0 when
    /// they equal the reference exactly. `None` with fewer than 2 values.
    pub fn t_against(&self, reference: f32) -> Option<f32> {
        if self.n < 2 {
            return None;
        }
        let difference = self.mean - reference;
        let se = self.se();
        Some(if se > 1e-9 {
            difference / se
        } else if difference == 0.0 {
            0.0
        } else {
            f32::INFINITY.copysign(difference)
        })
    }
}

/// The difference between two independent groups (Welch's unequal-variance t test).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Welch {
    /// Mean of the second group minus the mean of the first.
    pub difference: f32,
    /// Standard error of the difference.
    pub se: f32,
    /// Welch-Satterthwaite degrees of freedom.
    pub dof: f32,
}

impl Welch {
    /// `None` unless both groups have at least 2 values.
    pub fn of(first: &[f32], second: &[f32]) -> Option<Self> {
        let (a, b) = (Summary::of(first)?, Summary::of(second)?);
        if a.n < 2 || b.n < 2 {
            return None;
        }
        let (va, vb) = (a.sd * a.sd / a.n as f32, b.sd * b.sd / b.n as f32);
        let se = (va + vb).sqrt();
        let dof = if se > 1e-9 {
            (va + vb).powi(2) / (va * va / (a.n - 1) as f32 + vb * vb / (b.n - 1) as f32).max(1e-12)
        } else {
            (a.n + b.n - 2) as f32
        };
        Some(Self { difference: b.mean - a.mean, se, dof })
    }

    /// The difference in standard errors; infinite when neither group varies.
    pub fn t(&self) -> f32 {
        if self.se > 1e-9 {
            self.difference / self.se
        } else if self.difference == 0.0 {
            0.0
        } else {
            f32::INFINITY.copysign(self.difference)
        }
    }

    /// Whether the difference is clear at the gates' level (see [`t_critical`]).
    pub fn clear(&self) -> bool {
        self.t().abs() >= t_critical(self.dof)
    }
}

/// Student's t critical value for a two-sided 1% test (0.5% per side) with `dof` degrees of
/// freedom (rounded down, which is the strict side). 1% rather than the customary 5% because
/// every gate is evaluated again and again as data comes in.
pub fn t_critical(dof: f32) -> f32 {
    /// Two-sided 1% critical values for 1 to 30 degrees of freedom.
    const TABLE: [f32; 30] = [
        63.657, 9.925, 5.841, 4.604, 4.032, 3.707, 3.499, 3.355, 3.250, 3.169, 3.106, 3.055, 3.012, 2.977, 2.947,
        2.921, 2.898, 2.878, 2.861, 2.845, 2.831, 2.819, 2.807, 2.797, 2.787, 2.779, 2.771, 2.763, 2.756, 2.750,
    ];
    let whole = dof.floor();
    if whole < 1.0 {
        return f32::INFINITY;
    }
    match TABLE.get(whole as usize - 1) {
        Some(&value) => value,
        // Beyond the table the value approaches the normal quantile 2.576 like 1/dof (2.704 at
        // 40, 2.660 at 60, 2.617 at 120: within 0.003 of this).
        None => 2.576 + 5.2 / whole,
    }
}

/// Values grouped by the UTC day of their timestamp, as each day's mean, oldest day first.
pub fn daily_means(values: impl Iterator<Item = (u64, f32)>) -> Vec<(u64, f32)> {
    let mut days: Vec<(u64, f32, u32)> = Vec::new();
    for (unix_time, value) in values {
        let day = day_of(unix_time);
        match days.iter_mut().find(|d| d.0 == day) {
            Some(entry) => {
                entry.1 += value;
                entry.2 += 1;
            },
            None => days.push((day, value, 1)),
        }
    }
    days.sort_by_key(|d| d.0);
    days.into_iter().map(|(day, sum, n)| (day, sum / n as f32)).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A standard normal draw (Box-Muller), for the gates' synthetic players.
    pub(crate) fn normal(rng: &mut fastrand::Rng) -> f32 {
        let (u, v) = (1.0 - rng.f32(), rng.f32());
        (-2.0 * u.ln()).sqrt() * (std::f32::consts::TAU * v).cos()
    }

    #[test]
    fn summary_matches_hand_computed_values() {
        let s = Summary::of(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]).unwrap();
        assert_eq!(s.n, 8);
        assert!((s.mean - 5.0).abs() < 1e-6);
        // Sum of squares 32, over n - 1 = 7.
        assert!((s.sd - (32.0f32 / 7.0).sqrt()).abs() < 1e-5);
        assert!((s.t_against(4.0).unwrap() - 1.0 / s.se()).abs() < 1e-4);
        assert_eq!(Summary::of(&[]), None);
        assert_eq!(Summary::of(&[3.0]).unwrap().t_against(0.0), None);
        // No spread: any difference is infinitely clear, none is no evidence.
        let flat = Summary::of(&[1.0, 1.0, 1.0]).unwrap();
        assert_eq!(flat.t_against(0.5), Some(f32::INFINITY));
        assert_eq!(flat.t_against(1.0), Some(0.0));
    }

    #[test]
    fn welch_matches_a_textbook_example() {
        // Two groups with different spreads and sizes.
        let a = [27.5, 21.0, 19.0, 23.6, 17.0, 17.9, 16.9, 20.1, 21.9, 22.6, 23.1, 19.6, 19.0, 21.7, 21.4];
        let b = [27.1, 22.0, 20.8, 23.4, 23.4, 23.5, 25.8, 22.0, 24.8, 20.2, 21.9, 22.1, 22.9, 20.5, 24.4];
        let w = Welch::of(&a, &b).unwrap();
        let (sa, sb) = (Summary::of(&a).unwrap(), Summary::of(&b).unwrap());
        assert!((w.difference - (sb.mean - sa.mean)).abs() < 1e-5);
        assert!((w.se - (sa.sd.powi(2) / 15.0 + sb.sd.powi(2) / 15.0).sqrt()).abs() < 1e-5);
        // Between the smaller group's n - 1 and the pooled n1 + n2 - 2.
        assert!(w.dof > 14.0 && w.dof <= 28.0, "{}", w.dof);
        assert!(w.t() > 2.0 && !w.clear(), "t {} at {} dof is not clear at 1%", w.t(), w.dof);
        assert_eq!(Welch::of(&[1.0], &b), None);
    }

    #[test]
    fn critical_values_follow_the_t_table() {
        assert_eq!(t_critical(4.0), 4.604);
        // Fractional degrees of freedom round down: the strict side.
        assert_eq!(t_critical(4.9), 4.604);
        assert_eq!(t_critical(30.0), 2.750);
        assert!((t_critical(40.0) - 2.704).abs() < 0.004);
        assert!((t_critical(120.0) - 2.617).abs() < 0.004);
        assert_eq!(t_critical(0.5), f32::INFINITY);
        // Never below the normal quantile, and falling with more data.
        assert!(t_critical(1e6) >= 2.576 && t_critical(31.0) < t_critical(30.0));
    }

    /// The critical value keeps what it promises: a no-difference comparison passes about 1% of
    /// the time, also with few values.
    #[test]
    fn the_test_level_holds_for_small_samples() {
        let mut rng = fastrand::Rng::with_seed(7);
        let mut passed = 0;
        const TRIALS: usize = 20_000;
        for _ in 0..TRIALS {
            let values: Vec<f32> = (0..5).map(|_| normal(&mut rng)).collect();
            let t = Summary::of(&values).unwrap().t_against(0.0).unwrap();
            passed += usize::from(t.abs() >= t_critical(4.0));
        }
        let rate = passed as f32 / TRIALS as f32;
        assert!((0.006..0.014).contains(&rate), "{rate}");
    }

    #[test]
    fn daily_means_group_by_utc_day() {
        let day = SECS_PER_DAY;
        let means = daily_means([(3 * day + 10, 1.0), (2 * day + 5, 4.0), (3 * day + 500, 3.0)].into_iter());
        assert_eq!(means, vec![(2, 4.0), (3, 2.0)]);
    }
}
