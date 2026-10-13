use serde::Serialize;
use thiserror::Error;

use crate::types::Tick;

const BASIS_POINTS_PER_ONE: u128 = 10_000;
const MILLIS_PER_SECOND: u128 = 1_000;
const MICROSECONDS_PER_SECOND: u128 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct QueueChange {
    at: Tick,
    total_waiting: u64,
}

/// Piecewise-constant queue occupancy trace. Recording only actual occupancy
/// changes makes scientific queue summaries independent of debug sampling.
#[derive(Debug, Clone, Default)]
pub struct QueueTimeline {
    changes: Vec<QueueChange>,
}

impl QueueTimeline {
    pub fn new(initial_at: Tick, initial_total_waiting: u64) -> Self {
        Self {
            changes: vec![QueueChange {
                at: initial_at,
                total_waiting: initial_total_waiting,
            }],
        }
    }

    pub fn record(&mut self, at: Tick, total_waiting: u64) -> Result<(), MetricsError> {
        let Some(last) = self.changes.last_mut() else {
            self.changes.push(QueueChange { at, total_waiting });
            return Ok(());
        };
        if at < last.at {
            return Err(MetricsError::NonMonotonicQueueTrace);
        }
        if at == last.at {
            last.total_waiting = total_waiting;
        } else if last.total_waiting != total_waiting {
            self.changes.push(QueueChange { at, total_waiting });
        }
        Ok(())
    }

    pub fn area_between(&self, start: Tick, end: Tick) -> Result<u128, MetricsError> {
        if start >= end {
            return Err(MetricsError::InvalidWindow);
        }

        let mut current = 0_u64;
        let mut cursor = start.as_u64();
        let mut area = 0_u128;
        for change in &self.changes {
            if change.at <= start {
                current = change.total_waiting;
                continue;
            }
            if change.at >= end {
                break;
            }
            let duration = change.at.as_u64() - cursor;
            area = area
                .checked_add(u128::from(current) * u128::from(duration))
                .ok_or(MetricsError::ArithmeticOverflow)?;
            cursor = change.at.as_u64();
            current = change.total_waiting;
        }
        area.checked_add(u128::from(current) * u128::from(end.as_u64() - cursor))
            .ok_or(MetricsError::ArithmeticOverflow)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoverySpec {
    pub baseline_start: Tick,
    pub baseline_end: Tick,
    pub baseline_window_us: u64,
    pub trigger_end: Tick,
    pub run_end: Tick,
    pub window_us: u64,
    pub consecutive_windows: u64,
    pub goodput_fraction_basis_points: u64,
    pub queue_margin_fraction_basis_points: u64,
    pub total_waiting_capacity: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecoveryWindow {
    pub start_us: u64,
    pub end_us: u64,
    pub unique_goodput: u64,
    pub goodput_milli_per_second: u64,
    pub mean_waiting_milli_slots: u64,
    pub goodput_pass: bool,
    pub queue_pass: bool,
    pub recovered_window: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HorizonOutcome {
    pub horizon_us: u64,
    pub fully_observed: bool,
    pub recovered_by_horizon: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecoverySummary {
    /// True only when a non-zero G0 reference can be formed. The stricter
    /// pre-trigger stability eligibility checks belong to the experiment
    /// orchestration layer and are not implied by this field.
    pub baseline_reference_available: bool,
    pub baseline_total_unique_goodput: u64,
    pub baseline_window_count: u64,
    pub baseline_g0_milli_per_second: u64,
    pub baseline_q0_milli_slots: u64,
    pub recovery_at_us: Option<u64>,
    pub operational_non_recovery_within_horizon: Option<bool>,
    pub windows: Vec<RecoveryWindow>,
    pub sensitivity_horizons: Vec<HorizonOutcome>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MetricsError {
    #[error("metric window must have positive duration and valid ordering")]
    InvalidWindow,
    #[error("queue trace times must be monotonic")]
    NonMonotonicQueueTrace,
    #[error("metric arithmetic overflow")]
    ArithmeticOverflow,
    #[error("metric value cannot be represented in the output integer type")]
    OutputOverflow,
}

/// Half-open calendar bins on a fixed lattice anchored at incident onset.
/// Edge bins may be partial; no event at `end` belongs to the preceding bin.
pub fn calendar_windows(
    start: Tick,
    end: Tick,
    anchor: Tick,
    window_us: u64,
) -> Result<Vec<(Tick, Tick)>, MetricsError> {
    if start >= end || window_us == 0 {
        return Err(MetricsError::InvalidWindow);
    }
    let mut result = Vec::new();
    let mut cursor = start;
    while cursor < end {
        let distance = if cursor < anchor {
            let remainder = (anchor.as_u64() - cursor.as_u64()) % window_us;
            if remainder == 0 { window_us } else { remainder }
        } else {
            window_us - (cursor.as_u64() - anchor.as_u64()) % window_us
        };
        let next = cursor
            .checked_add(distance)
            .ok_or(MetricsError::ArithmeticOverflow)?
            .min(end);
        result.push((cursor, next));
        cursor = next;
    }
    Ok(result)
}

pub fn evaluate_recovery(
    spec: RecoverySpec,
    goodput_completion_times: &[Tick],
    queue_timeline: &QueueTimeline,
) -> Result<RecoverySummary, MetricsError> {
    if spec.baseline_start >= spec.baseline_end
        || spec.trigger_end > spec.run_end
        || spec.baseline_window_us == 0
        || spec.window_us == 0
        || spec.consecutive_windows == 0
    {
        return Err(MetricsError::InvalidWindow);
    }

    let baseline_duration = spec.baseline_end.as_u64() - spec.baseline_start.as_u64();
    if !baseline_duration.is_multiple_of(spec.baseline_window_us) {
        return Err(MetricsError::InvalidWindow);
    }
    let baseline_window_count = baseline_duration / spec.baseline_window_us;
    let mut baseline_goodput_counts = Vec::new();
    let mut baseline_queue_areas = Vec::new();
    let mut baseline_cursor = spec.baseline_start;
    while baseline_cursor < spec.baseline_end {
        let end = baseline_cursor
            .checked_add(spec.baseline_window_us)
            .ok_or(MetricsError::ArithmeticOverflow)?;
        baseline_goodput_counts.push(count_between(
            goodput_completion_times,
            baseline_cursor,
            end,
        ));
        baseline_queue_areas.push(queue_timeline.area_between(baseline_cursor, end)?);
        baseline_cursor = end;
    }
    baseline_goodput_counts.sort_unstable();
    baseline_queue_areas.sort_unstable();
    let median_pair_sum = median_pair_sum(&baseline_goodput_counts)?;
    let q0_area = percentile_95(&baseline_queue_areas)?;
    let baseline_total_goodput = baseline_goodput_counts
        .iter()
        .try_fold(0_u64, |sum, value| {
            sum.checked_add(*value)
                .ok_or(MetricsError::ArithmeticOverflow)
        })?;
    let baseline_reference_available = median_pair_sum > 0;

    let mut windows = Vec::new();
    let mut start = spec.trigger_end;
    let mut streak = 0_u64;
    let mut recovery_at = None;
    loop {
        let Some(end) = start.checked_add(spec.window_us) else {
            return Err(MetricsError::ArithmeticOverflow);
        };
        if end > spec.run_end {
            break;
        }

        let goodput = count_between(goodput_completion_times, start, end);
        let queue_area = queue_timeline.area_between(start, end)?;
        let goodput_pass = baseline_reference_available
            && u128::from(goodput) * u128::from(spec.baseline_window_us) * 2 * BASIS_POINTS_PER_ONE
                >= u128::from(median_pair_sum)
                    * u128::from(spec.window_us)
                    * u128::from(spec.goodput_fraction_basis_points);
        let queue_pass = queue_threshold_pass(
            queue_area,
            spec.window_us,
            q0_area,
            spec.baseline_window_us,
            spec.total_waiting_capacity,
            spec.queue_margin_fraction_basis_points,
        )?;
        let recovered_window = goodput_pass && queue_pass;
        streak = if recovered_window { streak + 1 } else { 0 };
        if recovery_at.is_none() && streak >= spec.consecutive_windows {
            recovery_at = Some(end.as_u64());
        }

        windows.push(RecoveryWindow {
            start_us: start.as_u64(),
            end_us: end.as_u64(),
            unique_goodput: goodput,
            goodput_milli_per_second: rate_milli_per_second(goodput, spec.window_us)?,
            mean_waiting_milli_slots: mean_milli(queue_area, spec.window_us)?,
            goodput_pass,
            queue_pass,
            recovered_window,
        });
        start = end;
    }

    let available_horizon = spec.run_end.as_u64() - spec.trigger_end.as_u64();
    let sensitivity_horizons = [300_000_000, 600_000_000, 900_000_000]
        .into_iter()
        .map(|horizon_us| {
            let fully_observed = available_horizon >= horizon_us;
            HorizonOutcome {
                horizon_us,
                fully_observed,
                recovered_by_horizon: (fully_observed && baseline_reference_available).then(|| {
                    recovery_at.is_some_and(|at| {
                        spec.trigger_end
                            .checked_add(horizon_us)
                            .is_some_and(|boundary| at <= boundary.as_u64())
                    })
                }),
            }
        })
        .collect();

    Ok(RecoverySummary {
        baseline_reference_available,
        baseline_total_unique_goodput: baseline_total_goodput,
        baseline_window_count,
        baseline_g0_milli_per_second: rate_milli_per_second_rational(
            median_pair_sum,
            spec.baseline_window_us,
            2,
        )?,
        baseline_q0_milli_slots: mean_milli(q0_area, spec.baseline_window_us)?,
        recovery_at_us: recovery_at,
        operational_non_recovery_within_horizon: baseline_reference_available
            .then_some(recovery_at.is_none()),
        windows,
        sensitivity_horizons,
    })
}

fn median_pair_sum(sorted: &[u64]) -> Result<u64, MetricsError> {
    if sorted.is_empty() {
        return Err(MetricsError::InvalidWindow);
    }
    let upper = sorted.len() / 2;
    let lower = (sorted.len() - 1) / 2;
    sorted[lower]
        .checked_add(sorted[upper])
        .ok_or(MetricsError::ArithmeticOverflow)
}

fn percentile_95(sorted: &[u128]) -> Result<u128, MetricsError> {
    if sorted.is_empty() {
        return Err(MetricsError::InvalidWindow);
    }
    let rank = (95_usize
        .checked_mul(sorted.len())
        .ok_or(MetricsError::ArithmeticOverflow)?
        .div_ceil(100))
    .max(1);
    Ok(sorted[rank - 1])
}

fn count_between(times: &[Tick], start: Tick, end: Tick) -> u64 {
    times
        .iter()
        .filter(|time| **time >= start && **time < end)
        .count() as u64
}

fn queue_threshold_pass(
    window_area: u128,
    window_duration: u64,
    baseline_area: u128,
    baseline_duration: u64,
    total_capacity: u64,
    margin_basis_points: u64,
) -> Result<bool, MetricsError> {
    let left = window_area
        .checked_mul(u128::from(baseline_duration))
        .and_then(|value| value.checked_mul(BASIS_POINTS_PER_ONE))
        .ok_or(MetricsError::ArithmeticOverflow)?;
    let baseline_term = baseline_area
        .checked_mul(u128::from(window_duration))
        .and_then(|value| value.checked_mul(BASIS_POINTS_PER_ONE))
        .ok_or(MetricsError::ArithmeticOverflow)?;
    let margin_term = u128::from(margin_basis_points)
        .checked_mul(u128::from(total_capacity))
        .and_then(|value| value.checked_mul(u128::from(window_duration)))
        .and_then(|value| value.checked_mul(u128::from(baseline_duration)))
        .ok_or(MetricsError::ArithmeticOverflow)?;
    let right = baseline_term
        .checked_add(margin_term)
        .ok_or(MetricsError::ArithmeticOverflow)?;
    Ok(left <= right)
}

fn rate_milli_per_second(count: u64, duration_us: u64) -> Result<u64, MetricsError> {
    rate_milli_per_second_rational(count, duration_us, 1)
}

fn rate_milli_per_second_rational(
    numerator: u64,
    duration_us: u64,
    denominator: u64,
) -> Result<u64, MetricsError> {
    let value = u128::from(numerator)
        .checked_mul(MICROSECONDS_PER_SECOND)
        .and_then(|value| value.checked_mul(MILLIS_PER_SECOND))
        .ok_or(MetricsError::ArithmeticOverflow)?
        / (u128::from(duration_us) * u128::from(denominator));
    u64::try_from(value).map_err(|_| MetricsError::OutputOverflow)
}

fn mean_milli(area_slot_us: u128, duration_us: u64) -> Result<u64, MetricsError> {
    let value = area_slot_us
        .checked_mul(MILLIS_PER_SECOND)
        .ok_or(MetricsError::ArithmeticOverflow)?
        / u128::from(duration_us);
    u64::try_from(value).map_err(|_| MetricsError::OutputOverflow)
}

#[cfg(test)]
mod tests {
    use super::{QueueTimeline, RecoverySpec, calendar_windows, evaluate_recovery};
    use crate::types::Tick;

    #[test]
    fn queue_area_is_exact_for_piecewise_constant_trace() {
        let mut queue = QueueTimeline::new(Tick::ZERO, 0);
        queue.record(Tick::new(10), 2).unwrap();
        queue.record(Tick::new(30), 1).unwrap();
        queue.record(Tick::new(40), 0).unwrap();

        assert_eq!(queue.area_between(Tick::ZERO, Tick::new(50)), Ok(50));
        assert_eq!(queue.area_between(Tick::new(20), Tick::new(35)), Ok(25));
    }

    #[test]
    fn calendar_bins_are_anchored_and_keep_partial_edges() {
        assert_eq!(
            calendar_windows(Tick::ZERO, Tick::new(105), Tick::new(35), 30).unwrap(),
            vec![
                (Tick::ZERO, Tick::new(5)),
                (Tick::new(5), Tick::new(35)),
                (Tick::new(35), Tick::new(65)),
                (Tick::new(65), Tick::new(95)),
                (Tick::new(95), Tick::new(105)),
            ]
        );
        assert!(calendar_windows(Tick::ZERO, Tick::new(1), Tick::ZERO, 0).is_err());
    }

    #[test]
    fn unchanged_debug_observations_do_not_change_the_exact_queue_area() {
        let mut sparse = QueueTimeline::new(Tick::ZERO, 0);
        sparse.record(Tick::new(10), 2).unwrap();
        sparse.record(Tick::new(30), 0).unwrap();

        let mut sampled = QueueTimeline::new(Tick::ZERO, 0);
        for at in 1..10 {
            sampled.record(Tick::new(at), 0).unwrap();
        }
        sampled.record(Tick::new(10), 2).unwrap();
        for at in 11..30 {
            sampled.record(Tick::new(at), 2).unwrap();
        }
        sampled.record(Tick::new(30), 0).unwrap();

        assert_eq!(
            sparse.area_between(Tick::ZERO, Tick::new(40)),
            sampled.area_between(Tick::ZERO, Tick::new(40))
        );
    }

    #[test]
    fn three_good_windows_recover_and_a_bad_window_resets_streak() {
        let queue = QueueTimeline::new(Tick::ZERO, 0);
        let goodput = [
            Tick::new(10),
            Tick::new(110),
            Tick::new(310),
            Tick::new(410),
            Tick::new(510),
        ];
        let summary = evaluate_recovery(
            RecoverySpec {
                baseline_start: Tick::ZERO,
                baseline_end: Tick::new(100),
                baseline_window_us: 100,
                trigger_end: Tick::new(100),
                run_end: Tick::new(600),
                window_us: 100,
                consecutive_windows: 3,
                goodput_fraction_basis_points: 10_000,
                queue_margin_fraction_basis_points: 0,
                total_waiting_capacity: 30,
            },
            &goodput,
            &queue,
        )
        .unwrap();

        assert!(!summary.windows[1].recovered_window);
        assert_eq!(summary.recovery_at_us, Some(600));
    }

    #[test]
    fn completion_on_window_end_belongs_to_the_next_window() {
        let queue = QueueTimeline::new(Tick::ZERO, 0);
        let goodput = [Tick::new(10), Tick::new(200), Tick::new(300)];
        let summary = evaluate_recovery(
            RecoverySpec {
                baseline_start: Tick::ZERO,
                baseline_end: Tick::new(100),
                baseline_window_us: 100,
                trigger_end: Tick::new(100),
                run_end: Tick::new(400),
                window_us: 100,
                consecutive_windows: 1,
                goodput_fraction_basis_points: 10_000,
                queue_margin_fraction_basis_points: 0,
                total_waiting_capacity: 30,
            },
            &goodput,
            &queue,
        )
        .unwrap();

        assert_eq!(summary.windows[0].unique_goodput, 0);
        assert_eq!(summary.windows[1].unique_goodput, 1);
    }

    #[test]
    fn baseline_uses_median_goodput_and_nearest_rank_p95_queue() {
        let mut queue = QueueTimeline::new(Tick::ZERO, 0);
        queue.record(Tick::new(100), 1).unwrap();
        queue.record(Tick::new(200), 2).unwrap();
        queue.record(Tick::new(300), 3).unwrap();
        queue.record(Tick::new(400), 0).unwrap();
        let mut goodput = Vec::new();
        for (window, count) in [1_u64, 9, 3, 5].into_iter().enumerate() {
            for offset in 0..count {
                goodput.push(Tick::new(window as u64 * 100 + offset));
            }
        }

        let summary = evaluate_recovery(
            RecoverySpec {
                baseline_start: Tick::ZERO,
                baseline_end: Tick::new(400),
                baseline_window_us: 100,
                trigger_end: Tick::new(400),
                run_end: Tick::new(700),
                window_us: 100,
                consecutive_windows: 3,
                goodput_fraction_basis_points: 10_000,
                queue_margin_fraction_basis_points: 0,
                total_waiting_capacity: 30,
            },
            &goodput,
            &queue,
        )
        .unwrap();

        assert_eq!(summary.baseline_total_unique_goodput, 18);
        assert_eq!(summary.baseline_window_count, 4);
        assert_eq!(summary.baseline_g0_milli_per_second, 40_000_000);
        assert_eq!(summary.baseline_q0_milli_slots, 3_000);
    }

    #[test]
    fn sensitivity_horizons_distinguish_observed_from_right_censored() {
        let queue = QueueTimeline::new(Tick::ZERO, 0);
        let goodput = [Tick::new(10), Tick::new(300_000_010)];
        let summary = evaluate_recovery(
            RecoverySpec {
                baseline_start: Tick::ZERO,
                baseline_end: Tick::new(300_000_000),
                baseline_window_us: 300_000_000,
                trigger_end: Tick::new(300_000_000),
                run_end: Tick::new(1_200_000_000),
                window_us: 300_000_000,
                consecutive_windows: 1,
                goodput_fraction_basis_points: 10_000,
                queue_margin_fraction_basis_points: 0,
                total_waiting_capacity: 30,
            },
            &goodput,
            &queue,
        )
        .unwrap();

        assert_eq!(summary.recovery_at_us, Some(600_000_000));
        assert!(
            summary
                .sensitivity_horizons
                .iter()
                .all(|outcome| outcome.fully_observed
                    && outcome.recovered_by_horizon == Some(true))
        );
    }

    #[test]
    fn zero_goodput_baseline_is_not_misreported_as_non_recovery() {
        let queue = QueueTimeline::new(Tick::ZERO, 0);
        let summary = evaluate_recovery(
            RecoverySpec {
                baseline_start: Tick::ZERO,
                baseline_end: Tick::new(300_000_000),
                baseline_window_us: 300_000_000,
                trigger_end: Tick::new(300_000_000),
                run_end: Tick::new(1_200_000_000),
                window_us: 300_000_000,
                consecutive_windows: 1,
                goodput_fraction_basis_points: 10_000,
                queue_margin_fraction_basis_points: 0,
                total_waiting_capacity: 30,
            },
            &[],
            &queue,
        )
        .unwrap();

        assert!(!summary.baseline_reference_available);
        assert_eq!(summary.operational_non_recovery_within_horizon, None);
        assert!(
            summary
                .sensitivity_horizons
                .iter()
                .all(|outcome| outcome.recovered_by_horizon.is_none())
        );
    }
}
