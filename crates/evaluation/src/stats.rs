//! Simple statistics for reports.

/// Wilson score interval for a binomial proportion (95% by default).
pub fn wilson_interval(successes: u64, n: u64, z: f64) -> (f64, f64) {
    if n == 0 {
        return (0.0, 0.0);
    }
    let n = n as f64;
    let p = successes as f64 / n;
    let z2 = z * z;
    let denom = 1.0 + z2 / n;
    let centre = (p + z2 / (2.0 * n)) / denom;
    let margin = z * ((p * (1.0 - p) / n) + z2 / (4.0 * n * n)).sqrt() / denom;
    ((centre - margin).max(0.0), (centre + margin).min(1.0))
}

pub fn mean(values: impl Iterator<Item = f64>) -> f64 {
    let (sum, n) = values.fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
    if n == 0 {
        0.0
    } else {
        sum / n as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wilson_bounds() {
        let (lo, hi) = wilson_interval(8, 10, 1.96);
        assert!(lo > 0.4 && lo < 0.6);
        assert!(hi > 0.9 && hi < 1.0);
        assert_eq!(wilson_interval(0, 0, 1.96), (0.0, 0.0));
    }
}
