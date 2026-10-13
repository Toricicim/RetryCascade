use std::collections::HashSet;

use thiserror::Error;

use crate::config::{PolicyConfig, ResolvedRunConfig, RetryCause, StageName};

pub const SUPPORTED_SCHEMA_VERSION: &str = "1.1.0";
pub const SUPPORTED_MODEL_VERSION: &str = "retrycascade-des-v1.1.0";
pub const SUPPORTED_RNG_VERSION: &str = "keyed-chacha20-v2";
pub const MAX_JSON_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

const EXPECTED_ARRIVAL_KEY: [&str; 2] = ["main_seed", "logical_arrival_index"];
const EXPECTED_SERVICE_KEY: [&str; 4] = ["main_seed", "logical_request_id", "attempt_no", "stage"];
const EXPECTED_JITTER_KEY: [&str; 3] = ["main_seed", "logical_request_id", "attempt_no"];

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("unsupported {field}: expected {expected}, got {actual}")]
    UnsupportedVersion {
        field: &'static str,
        expected: &'static str,
        actual: String,
    },
    #[error("{field} must not be empty")]
    EmptyString { field: &'static str },
    #[error("{field} must be a 64-character lowercase hexadecimal SHA-256 value")]
    InvalidSha256 { field: &'static str },
    #[error("{field} must be between {min} and {max}, got {actual}")]
    OutOfRange {
        field: &'static str,
        min: u64,
        max: u64,
        actual: u64,
    },
    #[error("stage {index} must be {expected:?}, got {actual:?}")]
    InvalidStageOrder {
        index: usize,
        expected: StageName,
        actual: StageName,
    },
    #[error(
        "sink must be a bottleneck: sink rate {sink_rate} exceeds another stage rate {other_rate}"
    )]
    SinkIsNotBottleneck { sink_rate: u64, other_rate: u64 },
    #[error("invalid fixed V1 contract at {field}: expected {expected}, got {actual}")]
    InvalidFixedContract {
        field: &'static str,
        expected: &'static str,
        actual: String,
    },
    #[error("invalid ordering: {earlier} must be before {later}")]
    InvalidOrdering {
        earlier: &'static str,
        later: &'static str,
    },
    #[error("time arithmetic overflow while calculating {field}")]
    TimeOverflow { field: &'static str },
    #[error("inconsistent derived time {field}: expected {expected}, got {actual}")]
    InconsistentDerivedTime {
        field: &'static str,
        expected: u64,
        actual: u64,
    },
    #[error("trigger_enabled={enabled} is inconsistent with trigger_duration_us={duration_us}")]
    InvalidTriggerDuration { enabled: bool, duration_us: u64 },
    #[error("{field} must not contain empty or duplicate values")]
    InvalidCollection { field: &'static str },
}

impl ResolvedRunConfig {
    pub fn validate(&self) -> Result<(), ValidationError> {
        let v2 = self.model_version == "retrycascade-des-v2.0.0";
        validate_version(
            "schema_version",
            &self.schema_version,
            if v2 {
                "2.0.0"
            } else {
                SUPPORTED_SCHEMA_VERSION
            },
        )?;
        validate_version(
            "model_version",
            &self.model_version,
            if v2 {
                "retrycascade-des-v2.0.0"
            } else {
                SUPPORTED_MODEL_VERSION
            },
        )?;
        validate_version("rng_version", &self.rng_version, SUPPORTED_RNG_VERSION)?;
        nonempty("protocol_version", &self.protocol_version)?;
        sha256("protocol_sha256", &self.protocol_sha256)?;
        sha256("run_config_sha256", &self.run_config_sha256)?;

        range("seed", self.seed, 0, MAX_JSON_SAFE_INTEGER)?;
        range("rho_basis_points", self.rho_basis_points, 1, 9_999)?;
        positive("maximum_events", self.maximum_events)?;

        self.validate_stages()?;
        positive(
            "service_demand.mean_work_units_milli",
            self.service_demand.mean_work_units_milli,
        )?;
        positive(
            "service_demand.coefficient_of_variation_milli",
            self.service_demand.coefficient_of_variation_milli,
        )?;
        self.validate_request_semantics()?;
        self.validate_retry_contract()?;
        self.validate_trigger_and_observation()?;
        self.validate_policy()?;
        self.validate_baseline()?;
        self.validate_recovery()?;
        self.validate_rng()?;
        if v2 {
            range(
                "retry_contract.max_attempts_including_initial",
                self.retry_contract.max_attempts_including_initial,
                1,
                3,
            )?;
            let ext = self
                .extensions
                .as_ref()
                .ok_or(ValidationError::InvalidFixedContract {
                    field: "extensions",
                    expected: "V2 extensions present",
                    actual: "missing".into(),
                })?;
            range(
                "extensions.pre_admission_failure_basis_points",
                ext.pre_admission_failure_basis_points,
                0,
                10_000,
            )?;
            positive(
                "extensions.failure_notification_delay_us",
                ext.failure_notification_delay_us,
            )?;
            if let Some(observation) = &ext.observation {
                range(
                    "extensions.observation.window_us",
                    observation.window_us,
                    30_000_000,
                    30_000_000,
                )?;
                range(
                    "extensions.observation.reference_trigger_duration_us",
                    observation.reference_trigger_duration_us,
                    0,
                    self.post_trigger_duration_us,
                )?;
            }
        } else if self.extensions.is_some() {
            return Err(ValidationError::InvalidFixedContract {
                field: "extensions",
                expected: "absent for V1",
                actual: "present".into(),
            });
        }
        Ok(())
    }

    fn validate_stages(&self) -> Result<(), ValidationError> {
        let expected = [StageName::Source, StageName::Transform, StageName::Sink];
        for (index, (stage, expected_name)) in self.stages.iter().zip(expected).enumerate() {
            if stage.name != expected_name {
                return Err(ValidationError::InvalidStageOrder {
                    index,
                    expected: expected_name,
                    actual: stage.name,
                });
            }
            positive("stages[].workers", stage.workers)?;
            positive(
                "stages[].queue_capacity_waiting",
                stage.queue_capacity_waiting,
            )?;
            positive(
                "stages[].nominal_rate_work_units_per_second",
                stage.nominal_rate_work_units_per_second,
            )?;
        }

        let sink_rate = self.stages[2].nominal_rate_work_units_per_second;
        for stage in &self.stages[..2] {
            if sink_rate > stage.nominal_rate_work_units_per_second {
                return Err(ValidationError::SinkIsNotBottleneck {
                    sink_rate,
                    other_rate: stage.nominal_rate_work_units_per_second,
                });
            }
        }
        Ok(())
    }

    fn validate_request_semantics(&self) -> Result<(), ValidationError> {
        fixed_bool(
            "request_semantics.cancellation_after_timeout",
            self.request_semantics.cancellation_after_timeout,
            false,
        )?;
        fixed_bool(
            "request_semantics.cancellation_after_other_attempt_success",
            self.request_semantics
                .cancellation_after_other_attempt_success,
            false,
        )?;
        fixed_bool(
            "request_semantics.late_timed_out_completion_produces_goodput",
            self.request_semantics
                .late_timed_out_completion_produces_goodput,
            false,
        )?;
        fixed_bool(
            "request_semantics.unique_goodput_per_logical_request",
            self.request_semantics.unique_goodput_per_logical_request,
            true,
        )
    }

    fn validate_retry_contract(&self) -> Result<(), ValidationError> {
        let retry = &self.retry_contract;
        positive(
            "retry_contract.per_attempt_timeout_us",
            retry.per_attempt_timeout_us,
        )?;
        positive(
            "retry_contract.overall_logical_deadline_us",
            retry.overall_logical_deadline_us,
        )?;
        if retry.per_attempt_timeout_us >= retry.overall_logical_deadline_us {
            return Err(ValidationError::InvalidOrdering {
                earlier: "per_attempt_timeout_us",
                later: "overall_logical_deadline_us",
            });
        }
        positive(
            "retry_contract.max_attempts_including_initial",
            retry.max_attempts_including_initial,
        )?;
        let mut expected_causes = vec![RetryCause::AttemptTimeout, RetryCause::QueueRejected];
        if self.model_version == "retrycascade-des-v2.0.0" {
            expected_causes.push(RetryCause::PreAdmissionFailure);
        }
        if retry.retryable_causes != expected_causes {
            return Err(ValidationError::InvalidFixedContract {
                field: "retry_contract.retryable_causes",
                expected: "[attempt_timeout, queue_rejected]",
                actual: format!("{:?}", retry.retryable_causes),
            });
        }
        fixed_bool(
            "retry_contract.timeout_includes_all_queue_waiting",
            retry.timeout_includes_all_queue_waiting,
            true,
        )?;
        fixed_bool(
            "retry_contract.maximum_one_retry_schedule_per_attempt_cause",
            retry.maximum_one_retry_schedule_per_attempt_cause,
            true,
        )?;
        fixed_bool(
            "retry_contract.stop_creating_retries_after_logical_success",
            retry.stop_creating_retries_after_logical_success,
            true,
        )?;
        fixed_bool(
            "retry_contract.stop_creating_retries_after_overall_deadline",
            retry.stop_creating_retries_after_overall_deadline,
            true,
        )
    }

    fn validate_trigger_and_observation(&self) -> Result<(), ValidationError> {
        if (self.trigger_enabled && self.trigger_duration_us == 0)
            || (!self.trigger_enabled && self.trigger_duration_us != 0)
        {
            return Err(ValidationError::InvalidTriggerDuration {
                enabled: self.trigger_enabled,
                duration_us: self.trigger_duration_us,
            });
        }
        range(
            "trigger_capacity_multiplier_basis_points",
            self.trigger_capacity_multiplier_basis_points,
            1,
            10_000,
        )?;
        positive("post_trigger_duration_us", self.post_trigger_duration_us)?;

        let expected_trigger_end = self
            .trigger_start_us
            .checked_add(self.trigger_duration_us)
            .ok_or(ValidationError::TimeOverflow {
                field: "trigger_end_us",
            })?;
        derived_time(
            "trigger_end_us",
            expected_trigger_end.as_u64(),
            self.trigger_end_us.as_u64(),
        )?;

        let expected_run_end = self
            .trigger_end_us
            .checked_add(self.post_trigger_duration_us)
            .ok_or(ValidationError::TimeOverflow {
                field: "run_end_us",
            })?;
        derived_time(
            "run_end_us",
            expected_run_end.as_u64(),
            self.run_end_us.as_u64(),
        )?;

        let observation = &self.observation;
        if observation.arrivals_begin_us > observation.warmup_end_us {
            return Err(order("arrivals_begin_us", "warmup_end_us"));
        }
        if observation.warmup_end_us > observation.baseline_start_us {
            return Err(order("warmup_end_us", "baseline_start_us"));
        }
        if observation.baseline_start_us >= observation.baseline_end_us {
            return Err(order("baseline_start_us", "baseline_end_us"));
        }
        if observation.baseline_end_us != self.trigger_start_us {
            return Err(ValidationError::InvalidFixedContract {
                field: "observation.baseline_end_us",
                expected: "equal to trigger_start_us",
                actual: observation.baseline_end_us.as_u64().to_string(),
            });
        }
        Ok(())
    }

    fn validate_policy(&self) -> Result<(), ValidationError> {
        match &self.policy {
            PolicyConfig::Immediate { epsilon_us } => {
                positive("policy.epsilon_us", *epsilon_us)?;
            }
            PolicyConfig::Exponential { base_us, cap_us }
            | PolicyConfig::FullJitter {
                base_us, cap_us, ..
            }
            | PolicyConfig::RetryBudget {
                base_us, cap_us, ..
            } => validate_backoff(*base_us, *cap_us)?,
        }

        if let PolicyConfig::FullJitter { rng_namespace, .. } = &self.policy
            && rng_namespace != &self.rng.namespaces.jitter
        {
            return Err(ValidationError::InvalidFixedContract {
                field: "policy.rng_namespace",
                expected: "equal to rng.namespaces.jitter",
                actual: rng_namespace.clone(),
            });
        }

        if let PolicyConfig::RetryBudget { token_bucket, .. } = &self.policy {
            positive(
                "policy.token_bucket.units_per_token",
                token_bucket.units_per_token,
            )?;
            positive(
                "policy.token_bucket.retry_cost_units",
                token_bucket.retry_cost_units,
            )?;
            positive(
                "policy.token_bucket.refill_units_per_unique_on_time_success",
                token_bucket.refill_units_per_unique_on_time_success,
            )?;
            positive(
                "policy.token_bucket.capacity_units",
                token_bucket.capacity_units,
            )?;
            positive(
                "policy.token_bucket.initial_units",
                token_bucket.initial_units,
            )?;
            if token_bucket.initial_units > token_bucket.capacity_units {
                return Err(ValidationError::OutOfRange {
                    field: "policy.token_bucket.initial_units",
                    min: 1,
                    max: token_bucket.capacity_units,
                    actual: token_bucket.initial_units,
                });
            }
        }
        Ok(())
    }

    fn validate_recovery(&self) -> Result<(), ValidationError> {
        let recovery = &self.recovery;
        positive("recovery.window_us", recovery.window_us)?;
        positive(
            "recovery.consecutive_non_overlapping_windows",
            recovery.consecutive_non_overlapping_windows,
        )?;
        range(
            "recovery.goodput_fraction_of_g0_basis_points",
            recovery.goodput_fraction_of_g0_basis_points,
            1,
            10_000,
        )?;
        range(
            "recovery.queue_margin_fraction_of_total_waiting_capacity_basis_points",
            recovery.queue_margin_fraction_of_total_waiting_capacity_basis_points,
            0,
            10_000,
        )?;
        validate_basis_points(
            "recovery.sensitivity.goodput_fractions_basis_points",
            &recovery.sensitivity.goodput_fractions_basis_points,
        )?;
        validate_basis_points(
            "recovery.sensitivity.queue_margin_fractions_basis_points",
            &recovery.sensitivity.queue_margin_fractions_basis_points,
        )?;

        let recovery_span = recovery
            .window_us
            .checked_mul(recovery.consecutive_non_overlapping_windows)
            .ok_or(ValidationError::TimeOverflow {
                field: "recovery window span",
            })?;
        if recovery_span > self.post_trigger_duration_us {
            return Err(ValidationError::OutOfRange {
                field: "recovery window span",
                min: 1,
                max: self.post_trigger_duration_us,
                actual: recovery_span,
            });
        }
        Ok(())
    }

    fn validate_baseline(&self) -> Result<(), ValidationError> {
        positive("baseline.window_us", self.baseline.window_us)?;
        let duration =
            self.observation.baseline_end_us.as_u64() - self.observation.baseline_start_us.as_u64();
        if duration < self.baseline.window_us || !duration.is_multiple_of(self.baseline.window_us) {
            return Err(ValidationError::InvalidFixedContract {
                field: "baseline.window_us",
                expected: "one or more whole windows inside the baseline interval",
                actual: self.baseline.window_us.to_string(),
            });
        }
        Ok(())
    }

    fn validate_rng(&self) -> Result<(), ValidationError> {
        nonempty("rng.namespaces.arrival", &self.rng.namespaces.arrival)?;
        nonempty("rng.namespaces.service", &self.rng.namespaces.service)?;
        nonempty("rng.namespaces.jitter", &self.rng.namespaces.jitter)?;
        exact_key(
            "rng.arrival_key",
            &self.rng.arrival_key,
            &EXPECTED_ARRIVAL_KEY,
        )?;
        exact_key(
            "rng.service_key",
            &self.rng.service_key,
            &EXPECTED_SERVICE_KEY,
        )?;
        exact_key("rng.jitter_key", &self.rng.jitter_key, &EXPECTED_JITTER_KEY)?;
        fixed_bool(
            "rng.policy_must_not_change_arrival_trace",
            self.rng.policy_must_not_change_arrival_trace,
            true,
        )?;
        fixed_bool(
            "rng.policy_must_not_change_shared_service_samples",
            self.rng.policy_must_not_change_shared_service_samples,
            true,
        )
    }
}

fn validate_version(
    field: &'static str,
    actual: &str,
    expected: &'static str,
) -> Result<(), ValidationError> {
    if actual == expected {
        Ok(())
    } else {
        Err(ValidationError::UnsupportedVersion {
            field,
            expected,
            actual: actual.to_owned(),
        })
    }
}

fn nonempty(field: &'static str, value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        Err(ValidationError::EmptyString { field })
    } else {
        Ok(())
    }
}

fn sha256(field: &'static str, value: &str) -> Result<(), ValidationError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(ValidationError::InvalidSha256 { field })
    }
}

fn positive(field: &'static str, value: u64) -> Result<(), ValidationError> {
    range(field, value, 1, u64::MAX)
}

fn range(field: &'static str, value: u64, min: u64, max: u64) -> Result<(), ValidationError> {
    if (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(ValidationError::OutOfRange {
            field,
            min,
            max,
            actual: value,
        })
    }
}

fn fixed_bool(field: &'static str, actual: bool, expected: bool) -> Result<(), ValidationError> {
    if actual == expected {
        Ok(())
    } else {
        Err(ValidationError::InvalidFixedContract {
            field,
            expected: if expected { "true" } else { "false" },
            actual: actual.to_string(),
        })
    }
}

fn validate_backoff(base_us: u64, cap_us: u64) -> Result<(), ValidationError> {
    positive("policy.base_us", base_us)?;
    positive("policy.cap_us", cap_us)?;
    if base_us > cap_us {
        return Err(ValidationError::OutOfRange {
            field: "policy.base_us",
            min: 1,
            max: cap_us,
            actual: base_us,
        });
    }
    Ok(())
}

fn validate_basis_points(field: &'static str, values: &[u64]) -> Result<(), ValidationError> {
    let mut unique = HashSet::new();
    if values.is_empty()
        || values
            .iter()
            .any(|value| *value > 10_000 || !unique.insert(*value))
    {
        return Err(ValidationError::InvalidCollection { field });
    }
    Ok(())
}

fn exact_key(
    field: &'static str,
    actual: &[String],
    expected: &[&str],
) -> Result<(), ValidationError> {
    if actual
        .iter()
        .map(String::as_str)
        .eq(expected.iter().copied())
    {
        Ok(())
    } else {
        Err(ValidationError::InvalidFixedContract {
            field,
            expected: "the frozen RNG key template",
            actual: format!("{actual:?}"),
        })
    }
}

fn derived_time(field: &'static str, expected: u64, actual: u64) -> Result<(), ValidationError> {
    if expected == actual {
        Ok(())
    } else {
        Err(ValidationError::InconsistentDerivedTime {
            field,
            expected,
            actual,
        })
    }
}

fn order(earlier: &'static str, later: &'static str) -> ValidationError {
    ValidationError::InvalidOrdering { earlier, later }
}
