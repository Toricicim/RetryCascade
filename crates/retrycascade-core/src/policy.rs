use thiserror::Error;

use crate::config::{PolicyConfig, TokenBucketConfig};
use crate::rng::KeyedRng;
use crate::types::LogicalRequestId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryContext {
    pub logical_request_id: LogicalRequestId,
    /// The attempt that will be created if this retry is admitted. The initial
    /// attempt is one, so the first retry uses `next_attempt_no == 2`.
    pub next_attempt_no: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    ScheduleAfter { delay_us: u64 },
    DeniedByBudget,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetryPolicyCounters {
    pub retries_scheduled: u64,
    pub retries_denied_by_budget: u64,
    pub budget_refill_events: u64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PolicyError {
    #[error("retry attempt number must be at least two")]
    InvalidRetryAttemptNumber,
    #[error("retry delay arithmetic overflow")]
    DelayOverflow,
    #[error("retry policy counter overflow: {0}")]
    CounterOverflow(&'static str),
}

#[derive(Debug, Clone)]
struct TokenBucket {
    config: TokenBucketConfig,
    units: u64,
}

/// Stateful wrapper around the otherwise pure retry-delay calculation.
/// Only the retry-budget variant mutates admission state.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    config: PolicyConfig,
    rng: KeyedRng,
    bucket: Option<TokenBucket>,
    counters: RetryPolicyCounters,
}

impl RetryPolicy {
    pub fn new(config: PolicyConfig, seed: u64) -> Self {
        let bucket = match &config {
            PolicyConfig::RetryBudget { token_bucket, .. } => Some(TokenBucket {
                config: token_bucket.clone(),
                units: token_bucket.initial_units,
            }),
            _ => None,
        };
        Self {
            config,
            rng: KeyedRng::new(seed),
            bucket,
            counters: RetryPolicyCounters::default(),
        }
    }

    pub fn request_retry(&mut self, context: RetryContext) -> Result<RetryDecision, PolicyError> {
        if let Some(bucket) = &mut self.bucket {
            if bucket.units < bucket.config.retry_cost_units {
                increment(
                    &mut self.counters.retries_denied_by_budget,
                    "retries_denied_by_budget",
                )?;
                return Ok(RetryDecision::DeniedByBudget);
            }
            bucket.units -= bucket.config.retry_cost_units;
        }

        let delay_us = retry_delay_us(&self.config, self.rng, context)?;
        increment(&mut self.counters.retries_scheduled, "retries_scheduled")?;
        Ok(RetryDecision::ScheduleAfter { delay_us })
    }

    /// Only the first unique on-time success calls this method. Late and
    /// duplicate completions must not refill the run-global bucket.
    pub fn refill_after_unique_on_time_success(&mut self) -> Result<(), PolicyError> {
        let Some(bucket) = &mut self.bucket else {
            return Ok(());
        };
        bucket.units = bucket
            .units
            .saturating_add(bucket.config.refill_units_per_unique_on_time_success)
            .min(bucket.config.capacity_units);
        increment(
            &mut self.counters.budget_refill_events,
            "budget_refill_events",
        )
    }

    pub fn budget_units(&self) -> Option<u64> {
        self.bucket.as_ref().map(|bucket| bucket.units)
    }

    pub fn budget_capacity_units(&self) -> Option<u64> {
        self.bucket
            .as_ref()
            .map(|bucket| bucket.config.capacity_units)
    }

    pub const fn counters(&self) -> RetryPolicyCounters {
        self.counters
    }
}

/// Pure policy calculation: the same inputs always produce the same delay.
pub fn retry_delay_us(
    config: &PolicyConfig,
    rng: KeyedRng,
    context: RetryContext,
) -> Result<u64, PolicyError> {
    if context.next_attempt_no < 2 {
        return Err(PolicyError::InvalidRetryAttemptNumber);
    }

    match config {
        PolicyConfig::Immediate { epsilon_us } => Ok(*epsilon_us),
        PolicyConfig::Exponential { base_us, cap_us } => {
            exponential_upper_bound(*base_us, *cap_us, context.next_attempt_no)
        }
        PolicyConfig::FullJitter {
            base_us, cap_us, ..
        }
        | PolicyConfig::RetryBudget {
            base_us, cap_us, ..
        } => {
            let upper = exponential_upper_bound(*base_us, *cap_us, context.next_attempt_no)?;
            let sample = rng.jitter_u64(context.logical_request_id, context.next_attempt_no);
            Ok(if upper == u64::MAX {
                sample
            } else {
                sample % (upper + 1)
            })
        }
    }
}

fn exponential_upper_bound(
    base_us: u64,
    cap_us: u64,
    next_attempt_no: u64,
) -> Result<u64, PolicyError> {
    let doublings = next_attempt_no - 2;
    let mut delay = base_us;
    for _ in 0..doublings {
        if delay >= cap_us {
            return Ok(cap_us);
        }
        delay = delay.checked_mul(2).ok_or(PolicyError::DelayOverflow)?;
    }
    Ok(delay.min(cap_us))
}

fn increment(value: &mut u64, name: &'static str) -> Result<(), PolicyError> {
    *value = value
        .checked_add(1)
        .ok_or(PolicyError::CounterOverflow(name))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{RetryContext, RetryDecision, RetryPolicy, retry_delay_us};
    use crate::config::{PolicyConfig, TokenBucketConfig, TokenDenialResult};
    use crate::rng::KeyedRng;
    use crate::types::LogicalRequestId;

    fn context(next_attempt_no: u64) -> RetryContext {
        RetryContext {
            logical_request_id: LogicalRequestId(7),
            next_attempt_no,
        }
    }

    #[test]
    fn immediate_retry_uses_positive_epsilon() {
        let delay = retry_delay_us(
            &PolicyConfig::Immediate { epsilon_us: 1 },
            KeyedRng::new(1),
            context(2),
        )
        .unwrap();
        assert_eq!(delay, 1);
    }

    #[test]
    fn exponential_schedule_reaches_and_stays_at_cap() {
        let policy = PolicyConfig::Exponential {
            base_us: 25,
            cap_us: 100,
        };
        let rng = KeyedRng::new(1);
        let delays: Vec<_> = (2..=7)
            .map(|attempt_no| retry_delay_us(&policy, rng, context(attempt_no)).unwrap())
            .collect();
        assert_eq!(delays, vec![25, 50, 100, 100, 100, 100]);
    }

    #[test]
    fn full_jitter_and_budget_share_the_same_jitter_draw() {
        let full = PolicyConfig::FullJitter {
            base_us: 100,
            cap_us: 800,
            rng_namespace: "retry_jitter".to_owned(),
        };
        let budget = PolicyConfig::RetryBudget {
            base_us: 100,
            cap_us: 800,
            token_bucket: bucket_config(10, 10),
        };
        let rng = KeyedRng::new(42);

        assert_eq!(
            retry_delay_us(&full, rng, context(3)).unwrap(),
            retry_delay_us(&budget, rng, context(3)).unwrap()
        );
    }

    #[test]
    fn jitter_is_bounded_and_repeatable() {
        let policy = PolicyConfig::FullJitter {
            base_us: 100,
            cap_us: 800,
            rng_namespace: "retry_jitter".to_owned(),
        };
        let first = retry_delay_us(&policy, KeyedRng::new(42), context(4)).unwrap();
        let second = retry_delay_us(&policy, KeyedRng::new(42), context(4)).unwrap();
        assert!(first <= 400);
        assert_eq!(first, second);
    }

    #[test]
    fn budget_denies_without_going_negative_and_refill_is_capped() {
        let mut policy = RetryPolicy::new(
            PolicyConfig::RetryBudget {
                base_us: 10,
                cap_us: 100,
                token_bucket: bucket_config(10, 10),
            },
            9,
        );

        assert!(matches!(
            policy.request_retry(context(2)).unwrap(),
            RetryDecision::ScheduleAfter { .. }
        ));
        assert_eq!(policy.budget_units(), Some(0));
        assert_eq!(
            policy.request_retry(context(3)).unwrap(),
            RetryDecision::DeniedByBudget
        );
        assert_eq!(policy.budget_units(), Some(0));

        for _ in 0..30 {
            policy.refill_after_unique_on_time_success().unwrap();
        }
        assert_eq!(policy.budget_units(), policy.budget_capacity_units());
        assert_eq!(policy.counters().retries_denied_by_budget, 1);
        assert_eq!(policy.counters().budget_refill_events, 30);
    }

    fn bucket_config(capacity_units: u64, initial_units: u64) -> TokenBucketConfig {
        TokenBucketConfig {
            units_per_token: 10,
            retry_cost_units: 10,
            refill_units_per_unique_on_time_success: 1,
            capacity_units,
            initial_units,
            token_denial_result: TokenDenialResult::RetryDenied,
        }
    }
}
