//! Pure aim measurements shared by the drills, the replay player and the weak-spot analysis.

pub mod bow;
pub mod calm;
pub mod flick_probe;
pub mod follow;
pub mod normal;
pub mod quest;
pub mod reaction;
pub mod steadiness;
pub mod throughput;

/// Median of `values` (sorted in place); `None` when empty.
pub fn median(values: &mut [f32]) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable_by(f32::total_cmp);
    let mid = values.len() / 2;
    Some(if values.len().is_multiple_of(2) { (values[mid - 1] + values[mid]) * 0.5 } else { values[mid] })
}
