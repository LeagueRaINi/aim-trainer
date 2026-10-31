//! The standard normal distribution, for the model-based estimators (QUEST+, the finder's
//! Gaussian process).

use std::f64::consts::FRAC_1_SQRT_2;

/// Complementary error function, relative error below 1.2e-7 everywhere (Numerical Recipes'
/// `erfcc` Chebyshev fit), so tail probabilities stay accurate too.
fn erfc(x: f64) -> f64 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let poly = -z * z - 1.265_512_23
        + t * (1.000_023_68
            + t * (0.374_091_96
                + t * (0.096_784_18
                    + t * (-0.186_288_06
                        + t * (0.278_868_07
                            + t * (-1.135_203_98 + t * (1.488_515_87 + t * (-0.822_152_23 + t * 0.170_872_77))))))));
    let ans = t * poly.exp();
    if x >= 0.0 { ans } else { 2.0 - ans }
}

/// Standard normal cumulative distribution function.
pub(crate) fn normal_cdf(x: f64) -> f64 {
    0.5 * erfc(-x * FRAC_1_SQRT_2)
}

/// Standard normal probability density function.
pub(crate) fn normal_pdf(x: f64) -> f64 {
    (-0.5 * x * x).exp() / (2.0 * std::f64::consts::PI).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_known_values() {
        for (x, cdf) in [(0.0, 0.5), (1.0, 0.841_344_746), (-1.959_963_985, 0.025), (2.326_347_874, 0.99), (-4.0, 3.167_124_2e-5)] {
            let got = normal_cdf(x);
            assert!(((got - cdf) / cdf).abs() < 1e-6, "Phi({x}) = {got}, want {cdf}");
        }
        assert!((normal_cdf(0.7) + normal_cdf(-0.7) - 1.0).abs() < 1e-12);
    }
}
