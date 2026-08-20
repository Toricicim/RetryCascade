use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::types::Tick;
use crate::validation::ValidationError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageName {
    Source,
    Transform,
    Sink,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageConfig {
    pub name: StageName,
    pub nominal_rate_work_units_per_second: u64,
    pub queue_capacity_waiting: u64,
    pub workers: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArrivalDistribution {
    Poisson,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArrivalRateDefinition {
    RhoTimesNominalSinkRate,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArrivalProcessConfig {
    pub distribution: ArrivalDistribution,
    pub rate_definition: ArrivalRateDefinition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceDistribution {
    Lognormal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceParameterization {
    MeanAndCoefficientOfVariation,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceDemandConfig {
    pub distribution: ServiceDistribution,
    pub parameterization: ServiceParameterization,
    pub mean_work_units_milli: u64,
    pub coefficient_of_variation_milli: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadKind {
    ReadLikeIdempotent,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestSemanticsConfig {
    pub workload_kind: WorkloadKind,
    pub cancellation_after_timeout: bool,
    pub cancellation_after_other_attempt_success: bool,
    pub late_timed_out_completion_produces_goodput: bool,
    pub unique_goodput_per_logical_request: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeoutOrigin {
    SourceAdmission,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryCause {
    AttemptTimeout,
    QueueRejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetryContractConfig {
    pub per_attempt_timeout_us: u64,
    pub timeout_origin: TimeoutOrigin,
    pub timeout_includes_all_queue_waiting: bool,
    pub overall_logical_deadline_us: u64,
    pub max_attempts_including_initial: u64,
    pub retryable_causes: [RetryCause; 2],
    pub maximum_one_retry_schedule_per_attempt_cause: bool,
    pub stop_creating_retries_after_logical_success: bool,
    pub stop_creating_retries_after_overall_deadline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenDenialResult {
    RetryDenied,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenBucketConfig {
    pub units_per_token: u64,
    pub retry_cost_units: u64,
    pub refill_units_per_unique_on_time_success: u64,
    pub capacity_units: u64,
    pub initial_units: u64,
    pub token_denial_result: TokenDenialResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PolicyConfig {
    Immediate {
        epsilon_us: u64,
    },
    Exponential {
        base_us: u64,
        cap_us: u64,
    },
    FullJitter {
        base_us: u64,
        cap_us: u64,
        rng_namespace: String,
    },
    RetryBudget {
        base_us: u64,
        cap_us: u64,
        token_bucket: TokenBucketConfig,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationConfig {
    pub arrivals_begin_us: Tick,
    pub warmup_end_us: Tick,
    pub baseline_start_us: Tick,
    pub baseline_end_us: Tick,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoverySensitivityConfig {
    pub goodput_fractions_basis_points: Vec<u64>,
    pub queue_margin_fractions_basis_points: Vec<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NonRecoveryLabel {
    OperationalNonRecoveryWithinHorizon,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryConfig {
    pub window_us: u64,
    pub consecutive_non_overlapping_windows: u64,
    pub goodput_fraction_of_g0_basis_points: u64,
    pub queue_margin_fraction_of_total_waiting_capacity_basis_points: u64,
    pub sensitivity: RecoverySensitivityConfig,
    pub non_recovery_label: NonRecoveryLabel,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RngNamespaces {
    pub arrival: String,
    pub service: String,
    pub jitter: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RngConfig {
    pub namespaces: RngNamespaces,
    pub arrival_key: Vec<String>,
    pub service_key: Vec<String>,
    pub jitter_key: Vec<String>,
    pub policy_must_not_change_arrival_trace: bool,
    pub policy_must_not_change_shared_service_samples: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedRunConfig {
    pub schema_version: String,
    pub analysis_eligible: bool,
    pub protocol_version: String,
    pub model_version: String,
    pub rng_version: String,
    pub protocol_sha256: String,
    pub run_config_sha256: String,
    pub seed: u64,
    pub rho_basis_points: u64,
    pub arrival_process: ArrivalProcessConfig,
    pub service_demand: ServiceDemandConfig,
    pub request_semantics: RequestSemanticsConfig,
    pub policy: PolicyConfig,
    pub stages: [StageConfig; 3],
    pub retry_contract: RetryContractConfig,
    pub trigger_enabled: bool,
    pub trigger_start_us: Tick,
    pub trigger_duration_us: u64,
    pub trigger_end_us: Tick,
    pub trigger_capacity_multiplier_basis_points: u64,
    pub post_trigger_duration_us: u64,
    pub run_end_us: Tick,
    pub observation: ObservationConfig,
    pub recovery: RecoveryConfig,
    pub rng: RngConfig,
    pub maximum_events: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigProvenance {
    pub schema_version: String,
    pub analysis_eligible: bool,
    pub protocol_version: String,
    pub model_version: String,
    pub rng_version: String,
    pub protocol_sha256: String,
    pub run_config_sha256: String,
    pub seed: u64,
}

impl ResolvedRunConfig {
    /// Simülasyon çıktısına daha sonra eklenecek kaynak kimliklerini kopyalar.
    /// Rust bu hash'leri hesaplamaz; Python otoritesinden geldiği gibi taşır.
    pub fn provenance(&self) -> ConfigProvenance {
        ConfigProvenance {
            schema_version: self.schema_version.clone(),
            analysis_eligible: self.analysis_eligible,
            protocol_version: self.protocol_version.clone(),
            model_version: self.model_version.clone(),
            rng_version: self.rng_version.clone(),
            protocol_sha256: self.protocol_sha256.clone(),
            run_config_sha256: self.run_config_sha256.clone(),
            seed: self.seed,
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("resolved run config could not be read from {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("resolved run config is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Validation(#[from] ValidationError),
}

pub fn parse_resolved_run_config(json: &str) -> Result<ResolvedRunConfig, ConfigError> {
    let config: ResolvedRunConfig = serde_json::from_str(json)?;
    config.validate()?;
    Ok(config)
}

pub fn load_resolved_run_config(path: impl AsRef<Path>) -> Result<ResolvedRunConfig, ConfigError> {
    let path = path.as_ref();
    let json = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    parse_resolved_run_config(&json)
}
