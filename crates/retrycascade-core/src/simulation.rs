use std::collections::BTreeSet;
use std::f64::consts::TAU;

use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::config::{ConfigProvenance, PolicyConfig, ResolvedRunConfig, StageName};
use crate::event::{Event, EventKind};
use crate::lifecycle::{
    AttemptProgress, LifecycleError, LifecycleLedger, LifecycleSpec, LifecycleSummary,
    TimeoutHandling,
};
use crate::metrics::{
    MetricsError, QueueTimeline, RecoverySpec, RecoverySummary, evaluate_recovery,
};
use crate::pipeline::{
    Pipeline, PipelineError, SinkTriggerSpec, StageCompletionHandling, StageRuntimeSpec,
};
use crate::policy::{PolicyError, RetryContext, RetryDecision, RetryPolicy};
use crate::rng::KeyedRng;
use crate::scheduler::{Scheduler, SchedulerError};
use crate::types::{AttemptId, LogicalRequestId, StageId, Tick};

const SUMMARY_SCHEMA_VERSION: &str = "retrycascade-run-summary-v1.0.0";
const STAGE_COUNT: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StageRunSummary {
    pub name: &'static str,
    pub admissions: u64,
    pub completions: u64,
    pub queue_rejections: u64,
    pub max_in_service: u64,
    pub max_waiting: u64,
    pub mean_waiting_milli_slots_full_run: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunCounts {
    pub logical_arrivals: u64,
    pub unique_goodput: u64,
    pub logical_deadline_abandoned: u64,
    pub logical_attempts_exhausted: u64,
    pub logical_outstanding_at_horizon: u64,
    pub physical_attempts: u64,
    pub attempt_timeouts: u64,
    pub late_completions: u64,
    pub duplicate_completions: u64,
    pub queue_rejections: u64,
    pub attempt_outstanding_at_horizon: u64,
    pub retries_scheduled: u64,
    pub retries_denied_by_budget: u64,
    pub budget_refill_events: u64,
    pub retry_ready_noops: u64,
    pub duplicate_retry_schedules_suppressed: u64,
    pub events_processed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RetryAmplificationSummary {
    pub window_start_us: u64,
    pub window_end_us: u64,
    pub numerator_physical_attempts: u64,
    pub denominator_logical_arrivals: u64,
    pub ratio_milli: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunSummary {
    pub summary_schema_version: &'static str,
    pub provenance: ConfigProvenance,
    pub policy: &'static str,
    pub run_end_us: u64,
    pub trigger_end_us: u64,
    pub counts: RunCounts,
    pub retry_amplification: RetryAmplificationSummary,
    pub budget_final_units: Option<u64>,
    pub budget_capacity_units: Option<u64>,
    pub arrival_trace_sha256: String,
    pub potential_service_trace_sha256: String,
    pub lifecycle_ledger_closed: bool,
    pub stages: [StageRunSummary; STAGE_COUNT],
    pub recovery: RecoverySummary,
    pub summary_sha256: String,
}

#[derive(Debug, Error)]
pub enum SimulationError {
    #[error("stage configuration cannot be represented on this platform")]
    StageConfigOverflow,
    #[error("max attempts cannot be represented by the lifecycle model")]
    AttemptNumberOverflow,
    #[error("attempt identifier arithmetic overflow")]
    AttemptIdOverflow,
    #[error("arrival or service distribution produced a non-finite value")]
    NonFiniteDistribution,
    #[error("simulation exceeded maximum_events={limit}")]
    EventLimitExceeded { limit: u64 },
    #[error("simulation counter overflow: {0}")]
    CounterOverflow(&'static str),
    #[error("summary serialization failed: {0}")]
    SummarySerialization(#[from] serde_json::Error),
    #[error(transparent)]
    Pipeline(#[from] PipelineError),
    #[error(transparent)]
    Lifecycle(#[from] LifecycleError),
    #[error(transparent)]
    Scheduler(#[from] SchedulerError),
    #[error(transparent)]
    Policy(#[from] PolicyError),
    #[error(transparent)]
    Metrics(#[from] MetricsError),
}

#[derive(Debug, Default)]
struct EngineCounters {
    retry_ready_noops: u64,
    duplicate_retry_schedules_suppressed: u64,
    events_processed: u64,
}

struct Engine<'a> {
    config: &'a ResolvedRunConfig,
    scheduler: Scheduler,
    pipeline: Pipeline,
    ledger: LifecycleLedger,
    policy: RetryPolicy,
    rng: KeyedRng,
    retry_scheduled_for_attempt: BTreeSet<AttemptId>,
    goodput_completion_times: Vec<Tick>,
    queue_timeline: QueueTimeline,
    counters: EngineCounters,
}

pub fn run_simulation(config: &ResolvedRunConfig) -> Result<RunSummary, SimulationError> {
    let (arrival_trace_sha256, potential_service_trace_sha256, arrival_events) =
        build_common_traces(config)?;
    let mut engine = Engine::new(config)?;
    engine
        .pipeline
        .schedule_trigger_boundaries(&mut engine.scheduler)?;
    for event in arrival_events {
        engine.scheduler.schedule_at(event.at, event.kind)?;
    }

    while let Some(event) = engine.scheduler.pop_next() {
        if event.at > config.run_end_us {
            break;
        }
        engine.counters.events_processed = engine
            .counters
            .events_processed
            .checked_add(1)
            .ok_or(SimulationError::CounterOverflow("events_processed"))?;
        if engine.counters.events_processed > config.maximum_events {
            return Err(SimulationError::EventLimitExceeded {
                limit: config.maximum_events,
            });
        }
        engine.handle_event(event)?;
        engine
            .queue_timeline
            .record(event.at, engine.pipeline.total_waiting()?)?;
    }

    let conservation = engine.ledger.close_horizon(config.run_end_us)?;
    let pipeline_summary = engine.pipeline.summary_at(config.run_end_us)?;
    let lifecycle = engine.ledger.summary();
    let policy_counters = engine.policy.counters();
    let total_waiting_capacity = config.stages.iter().try_fold(0_u64, |sum, stage| {
        sum.checked_add(stage.queue_capacity_waiting)
            .ok_or(SimulationError::CounterOverflow("total_waiting_capacity"))
    })?;
    let recovery = evaluate_recovery(
        RecoverySpec {
            baseline_start: config.observation.baseline_start_us,
            baseline_end: config.observation.baseline_end_us,
            baseline_window_us: config.baseline.window_us,
            trigger_end: config.trigger_end_us,
            run_end: config.run_end_us,
            window_us: config.recovery.window_us,
            consecutive_windows: config.recovery.consecutive_non_overlapping_windows,
            goodput_fraction_basis_points: config.recovery.goodput_fraction_of_g0_basis_points,
            queue_margin_fraction_basis_points: config
                .recovery
                .queue_margin_fraction_of_total_waiting_capacity_basis_points,
            total_waiting_capacity,
        },
        &engine.goodput_completion_times,
        &engine.queue_timeline,
    )?;

    let run_duration = config.run_end_us.as_u64();
    let stages = [
        stage_summary(config, &pipeline_summary, 0, run_duration)?,
        stage_summary(config, &pipeline_summary, 1, run_duration)?,
        stage_summary(config, &pipeline_summary, 2, run_duration)?,
    ];
    let retry_amplification = RetryAmplificationSummary {
        window_start_us: config.observation.arrivals_begin_us.as_u64(),
        window_end_us: config.run_end_us.as_u64(),
        numerator_physical_attempts: lifecycle.physical_attempts,
        denominator_logical_arrivals: lifecycle.logical_arrivals,
        ratio_milli: ratio_milli(lifecycle.physical_attempts, lifecycle.logical_arrivals)?,
    };
    let mut summary = RunSummary {
        summary_schema_version: SUMMARY_SCHEMA_VERSION,
        provenance: config.provenance(),
        policy: policy_name(&config.policy),
        run_end_us: config.run_end_us.as_u64(),
        trigger_end_us: config.trigger_end_us.as_u64(),
        counts: counts(lifecycle, policy_counters, &engine.counters),
        retry_amplification,
        budget_final_units: engine.policy.budget_units(),
        budget_capacity_units: engine.policy.budget_capacity_units(),
        arrival_trace_sha256,
        potential_service_trace_sha256,
        lifecycle_ledger_closed: conservation.all_terminal(),
        stages,
        recovery,
        summary_sha256: String::new(),
    };
    summary.summary_sha256 = summary_checksum(&summary)?;
    Ok(summary)
}

impl<'a> Engine<'a> {
    fn new(config: &'a ResolvedRunConfig) -> Result<Self, SimulationError> {
        let specs = [
            stage_runtime_spec(config, 0)?,
            stage_runtime_spec(config, 1)?,
            stage_runtime_spec(config, 2)?,
        ];
        let trigger = config.trigger_enabled.then_some(SinkTriggerSpec {
            start: config.trigger_start_us,
            end: config.trigger_end_us,
            capacity_multiplier_basis_points: config.trigger_capacity_multiplier_basis_points,
        });
        Ok(Self {
            config,
            scheduler: Scheduler::new(),
            pipeline: Pipeline::new_with_sink_trigger(specs, trigger)?,
            ledger: LifecycleLedger::new(LifecycleSpec {
                per_attempt_timeout_us: config.retry_contract.per_attempt_timeout_us,
                overall_logical_deadline_us: config.retry_contract.overall_logical_deadline_us,
            })?,
            policy: RetryPolicy::new(config.policy.clone(), config.seed),
            rng: KeyedRng::new(config.seed),
            retry_scheduled_for_attempt: BTreeSet::new(),
            goodput_completion_times: Vec::new(),
            queue_timeline: QueueTimeline::new(Tick::ZERO, 0),
            counters: EngineCounters::default(),
        })
    }

    fn handle_event(&mut self, event: Event) -> Result<(), SimulationError> {
        match event.kind {
            EventKind::StageCompletion {
                attempt_id,
                stage_id,
                service_epoch,
            } => match self.pipeline.handle_stage_completion(
                &mut self.scheduler,
                event.at,
                stage_id,
                attempt_id,
                service_epoch,
            )? {
                StageCompletionHandling::Stale { .. } => {}
                StageCompletionHandling::Applied(completed) => {
                    let progress = self.ledger.observe_pipeline_progress(event.at, completed)?;
                    match progress {
                        AttemptProgress::OnTimeSuccess => {
                            self.goodput_completion_times.push(event.at);
                            self.policy.refill_after_unique_on_time_success()?;
                        }
                        AttemptProgress::QueueRejected { .. } => {
                            self.schedule_retry(event.at, attempt_id)?;
                        }
                        AttemptProgress::Advanced
                        | AttemptProgress::LateCompletion
                        | AttemptProgress::DuplicateCompletion => {}
                    }
                }
            },
            EventKind::AttemptTimeout { attempt_id } => {
                if self.ledger.handle_attempt_timeout(event.at, attempt_id)?
                    == TimeoutHandling::TimedOut
                {
                    self.schedule_retry(event.at, attempt_id)?;
                }
            }
            EventKind::LogicalDeadline { logical_request_id } => {
                self.ledger
                    .handle_logical_deadline(event.at, logical_request_id)?;
            }
            EventKind::TriggerBoundary { transition } => {
                self.pipeline
                    .handle_trigger_boundary(&mut self.scheduler, event.at, transition)?;
            }
            EventKind::RetryReady {
                logical_request_id,
                parent_attempt_id,
                cause: _,
            } => {
                let parent = *self
                    .ledger
                    .attempt(parent_attempt_id)
                    .ok_or(LifecycleError::UnknownAttempt(parent_attempt_id))?;
                let next_attempt_no = parent
                    .attempt_no
                    .checked_add(1)
                    .ok_or(SimulationError::AttemptNumberOverflow)?;
                let logical = self
                    .ledger
                    .logical_request(logical_request_id)
                    .ok_or(LifecycleError::UnknownLogicalRequest(logical_request_id))?;
                if logical.terminal.is_some()
                    || event.at >= logical.deadline_at
                    || u64::from(next_attempt_no)
                        > self.config.retry_contract.max_attempts_including_initial
                {
                    increment(&mut self.counters.retry_ready_noops, "retry_ready_noops")?;
                } else {
                    self.admit_attempt(event.at, logical_request_id, next_attempt_no)?;
                }
            }
            EventKind::LogicalArrival { logical_request_id } => {
                self.ledger.register_logical_request(
                    &mut self.scheduler,
                    event.at,
                    logical_request_id,
                )?;
                self.admit_attempt(event.at, logical_request_id, 1)?;
            }
            EventKind::QueueObservation => {}
        }
        Ok(())
    }

    fn admit_attempt(
        &mut self,
        now: Tick,
        logical_request_id: LogicalRequestId,
        attempt_no: u32,
    ) -> Result<(), SimulationError> {
        let attempt_id = attempt_id(
            logical_request_id,
            u64::from(attempt_no),
            self.config.retry_contract.max_attempts_including_initial,
        )?;
        let demands = [
            service_demand_milli(
                self.rng,
                logical_request_id,
                u64::from(attempt_no),
                StageId(0),
                self.config.service_demand.mean_work_units_milli,
                self.config.service_demand.coefficient_of_variation_milli,
            )?,
            service_demand_milli(
                self.rng,
                logical_request_id,
                u64::from(attempt_no),
                StageId(1),
                self.config.service_demand.mean_work_units_milli,
                self.config.service_demand.coefficient_of_variation_milli,
            )?,
            service_demand_milli(
                self.rng,
                logical_request_id,
                u64::from(attempt_no),
                StageId(2),
                self.config.service_demand.mean_work_units_milli,
                self.config.service_demand.coefficient_of_variation_milli,
            )?,
        ];
        let admission = self.ledger.admit_attempt_with_stage_demands(
            &mut self.pipeline,
            &mut self.scheduler,
            now,
            logical_request_id,
            attempt_id,
            attempt_no,
            demands,
        )?;
        if admission
            .pipeline_admission
            .queue_rejected_stage()
            .is_some()
        {
            self.schedule_retry(now, attempt_id)?;
        }
        Ok(())
    }

    fn schedule_retry(
        &mut self,
        now: Tick,
        parent_attempt_id: AttemptId,
    ) -> Result<(), SimulationError> {
        if !self.retry_scheduled_for_attempt.insert(parent_attempt_id) {
            increment(
                &mut self.counters.duplicate_retry_schedules_suppressed,
                "duplicate_retry_schedules_suppressed",
            )?;
            return Ok(());
        }

        let parent = *self
            .ledger
            .attempt(parent_attempt_id)
            .ok_or(LifecycleError::UnknownAttempt(parent_attempt_id))?;
        let max_attempts = self.config.retry_contract.max_attempts_including_initial;
        if u64::from(parent.attempt_no) >= max_attempts {
            self.ledger
                .mark_attempts_exhausted(now, parent.logical_request_id)?;
            return Ok(());
        }
        let logical = self
            .ledger
            .logical_request(parent.logical_request_id)
            .ok_or(LifecycleError::UnknownLogicalRequest(
                parent.logical_request_id,
            ))?;
        if logical.terminal.is_some() || now >= logical.deadline_at {
            return Ok(());
        }
        let next_attempt_no = u64::from(parent.attempt_no) + 1;
        match self.policy.request_retry(RetryContext {
            logical_request_id: parent.logical_request_id,
            next_attempt_no,
        })? {
            RetryDecision::DeniedByBudget => {}
            RetryDecision::ScheduleAfter { delay_us } => {
                self.scheduler.schedule_after(
                    now,
                    delay_us,
                    EventKind::RetryReady {
                        logical_request_id: parent.logical_request_id,
                        parent_attempt_id,
                        cause: retry_cause(parent),
                    },
                )?;
            }
        }
        Ok(())
    }
}

fn build_common_traces(
    config: &ResolvedRunConfig,
) -> Result<(String, String, Vec<Event>), SimulationError> {
    let rng = KeyedRng::new(config.seed);
    let sink_rate = config.stages[2].nominal_rate_work_units_per_second as f64;
    let mean_work_units = config.service_demand.mean_work_units_milli as f64 / 1_000.0;
    let arrival_rate = sink_rate * config.rho_basis_points as f64 / 10_000.0 / mean_work_units;
    if !arrival_rate.is_finite() || arrival_rate <= 0.0 {
        return Err(SimulationError::NonFiniteDistribution);
    }

    let mut arrival_hasher = Sha256::new();
    let mut service_hasher = Sha256::new();
    let mut events = Vec::new();
    let mut at = config.observation.arrivals_begin_us;
    let mut index = 0_u64;
    loop {
        let uniform = open_unit_interval(rng.arrival_u64(index));
        let gap = (-uniform.ln() / arrival_rate * 1_000_000.0).ceil();
        if !gap.is_finite() || gap < 0.0 || gap > u64::MAX as f64 {
            return Err(SimulationError::NonFiniteDistribution);
        }
        at = at
            .checked_add((gap as u64).max(1))
            .ok_or(SimulationError::CounterOverflow("arrival_time"))?;
        if at > config.run_end_us {
            break;
        }
        let logical_id = LogicalRequestId(
            index
                .checked_add(1)
                .ok_or(SimulationError::CounterOverflow("logical_request_id"))?,
        );
        arrival_hasher.update(at.as_u64().to_le_bytes());
        events.push(Event::new(
            at,
            EventKind::LogicalArrival {
                logical_request_id: logical_id,
            },
            0,
        ));

        for attempt_no in 1..=config.retry_contract.max_attempts_including_initial {
            for stage_index in 0..STAGE_COUNT {
                let demand = service_demand_milli(
                    rng,
                    logical_id,
                    attempt_no,
                    StageId(stage_index as u8),
                    config.service_demand.mean_work_units_milli,
                    config.service_demand.coefficient_of_variation_milli,
                )?;
                service_hasher.update(logical_id.0.to_le_bytes());
                service_hasher.update(attempt_no.to_le_bytes());
                service_hasher.update([stage_index as u8]);
                service_hasher.update(demand.to_le_bytes());
            }
        }
        index = index
            .checked_add(1)
            .ok_or(SimulationError::CounterOverflow("arrival_index"))?;
    }
    Ok((
        format!("{:x}", arrival_hasher.finalize()),
        format!("{:x}", service_hasher.finalize()),
        events,
    ))
}

fn stage_runtime_spec(
    config: &ResolvedRunConfig,
    index: usize,
) -> Result<StageRuntimeSpec, SimulationError> {
    Ok(StageRuntimeSpec {
        workers: usize::try_from(config.stages[index].workers)
            .map_err(|_| SimulationError::StageConfigOverflow)?,
        queue_capacity_waiting: usize::try_from(config.stages[index].queue_capacity_waiting)
            .map_err(|_| SimulationError::StageConfigOverflow)?,
        service_rate_work_units_per_second: config.stages[index].nominal_rate_work_units_per_second,
    })
}

fn service_demand_milli(
    rng: KeyedRng,
    logical_id: LogicalRequestId,
    attempt_no: u64,
    stage_id: StageId,
    mean_milli: u64,
    coefficient_of_variation_milli: u64,
) -> Result<u64, SimulationError> {
    let u1 = open_unit_interval(rng.service_u64(logical_id, attempt_no, stage_id));
    let u2 = open_unit_interval(rng.service_aux_u64(logical_id, attempt_no, stage_id));
    let cv = coefficient_of_variation_milli as f64 / 1_000.0;
    let sigma_squared = (1.0 + cv * cv).ln();
    let sigma = sigma_squared.sqrt();
    let mu = (mean_milli as f64).ln() - sigma_squared / 2.0;
    let normal = (-2.0 * u1.ln()).sqrt() * (TAU * u2).cos();
    let demand = (mu + sigma * normal).exp().round();
    if !demand.is_finite() || demand > u64::MAX as f64 {
        return Err(SimulationError::NonFiniteDistribution);
    }
    Ok((demand as u64).max(1))
}

fn open_unit_interval(raw: u64) -> f64 {
    let mantissa = raw >> 11;
    (mantissa as f64 + 1.0) / ((1_u64 << 53) as f64 + 1.0)
}

fn attempt_id(
    logical_id: LogicalRequestId,
    attempt_no: u64,
    max_attempts: u64,
) -> Result<AttemptId, SimulationError> {
    let base = logical_id
        .0
        .checked_sub(1)
        .and_then(|value| value.checked_mul(max_attempts))
        .ok_or(SimulationError::AttemptIdOverflow)?;
    base.checked_add(attempt_no)
        .map(AttemptId)
        .ok_or(SimulationError::AttemptIdOverflow)
}

fn retry_cause(attempt: crate::lifecycle::Attempt) -> crate::config::RetryCause {
    if attempt.timed_out_at.is_some() {
        crate::config::RetryCause::AttemptTimeout
    } else {
        crate::config::RetryCause::QueueRejected
    }
}

fn counts(
    lifecycle: LifecycleSummary,
    policy: crate::policy::RetryPolicyCounters,
    engine: &EngineCounters,
) -> RunCounts {
    RunCounts {
        logical_arrivals: lifecycle.logical_arrivals,
        unique_goodput: lifecycle.logical_successes,
        logical_deadline_abandoned: lifecycle.logical_deadline_abandoned,
        logical_attempts_exhausted: lifecycle.logical_attempts_exhausted,
        logical_outstanding_at_horizon: lifecycle.logical_outstanding_at_horizon,
        physical_attempts: lifecycle.physical_attempts,
        attempt_timeouts: lifecycle.attempt_timeouts,
        late_completions: lifecycle.attempt_late_completions,
        duplicate_completions: lifecycle.attempt_duplicate_completions,
        queue_rejections: lifecycle.attempt_queue_rejections,
        attempt_outstanding_at_horizon: lifecycle.attempt_outstanding_at_horizon,
        retries_scheduled: policy.retries_scheduled,
        retries_denied_by_budget: policy.retries_denied_by_budget,
        budget_refill_events: policy.budget_refill_events,
        retry_ready_noops: engine.retry_ready_noops,
        duplicate_retry_schedules_suppressed: engine.duplicate_retry_schedules_suppressed,
        events_processed: engine.events_processed,
    }
}

fn stage_summary(
    config: &ResolvedRunConfig,
    pipeline_summary: &crate::pipeline::PipelineSummary,
    index: usize,
    run_duration: u64,
) -> Result<StageRunSummary, SimulationError> {
    let stage = pipeline_summary.stages[index];
    let mean_waiting = u128::from(stage.waiting_area_slot_us)
        .checked_mul(1_000)
        .ok_or(SimulationError::CounterOverflow("mean_waiting"))?
        / u128::from(run_duration);
    Ok(StageRunSummary {
        name: stage_name(config.stages[index].name),
        admissions: stage.admissions,
        completions: stage.completions,
        queue_rejections: stage.queue_rejections,
        max_in_service: stage.max_in_service,
        max_waiting: stage.max_waiting,
        mean_waiting_milli_slots_full_run: u64::try_from(mean_waiting)
            .map_err(|_| SimulationError::CounterOverflow("mean_waiting"))?,
    })
}

fn ratio_milli(numerator: u64, denominator: u64) -> Result<u64, SimulationError> {
    if denominator == 0 {
        Ok(0)
    } else {
        let value = u128::from(numerator) * 1_000 / u128::from(denominator);
        u64::try_from(value).map_err(|_| SimulationError::CounterOverflow("retry_amplification"))
    }
}

fn stage_name(name: StageName) -> &'static str {
    match name {
        StageName::Source => "source",
        StageName::Transform => "transform",
        StageName::Sink => "sink",
    }
}

fn policy_name(policy: &PolicyConfig) -> &'static str {
    match policy {
        PolicyConfig::Immediate { .. } => "immediate",
        PolicyConfig::Exponential { .. } => "exponential",
        PolicyConfig::FullJitter { .. } => "full_jitter",
        PolicyConfig::RetryBudget { .. } => "retry_budget",
    }
}

fn summary_checksum(summary: &RunSummary) -> Result<String, serde_json::Error> {
    let bytes = serde_json::to_vec(summary)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn increment(value: &mut u64, name: &'static str) -> Result<(), SimulationError> {
    *value = value
        .checked_add(1)
        .ok_or(SimulationError::CounterOverflow(name))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Engine, run_simulation};
    use crate::config::{PolicyConfig, parse_resolved_run_config};
    use crate::lifecycle::TimeoutHandling;
    use crate::types::{AttemptId, LogicalRequestId, Tick};

    const SMOKE: &str = include_str!("../../../tests/golden/resolved_smoke_config.json");

    #[test]
    fn smoke_run_is_deterministic_and_closes_ledgers() {
        let config = parse_resolved_run_config(SMOKE).unwrap();
        let first = run_simulation(&config).unwrap();
        let second = run_simulation(&config).unwrap();

        assert_eq!(first, second);
        assert!(first.lifecycle_ledger_closed);
        assert_eq!(first.summary_sha256.len(), 64);
        assert!(first.counts.logical_arrivals > 0);
        assert!(first.counts.physical_attempts >= first.counts.logical_arrivals);
    }

    #[test]
    fn four_policies_share_arrival_and_potential_service_traces() {
        let mut base = parse_resolved_run_config(SMOKE).unwrap();
        // Force the small fixture through the retry path instead of merely
        // checking four retry-free executions.
        base.retry_contract.per_attempt_timeout_us = 1;
        let policies = [
            PolicyConfig::Immediate { epsilon_us: 1 },
            PolicyConfig::Exponential {
                base_us: 25_000,
                cap_us: 200_000,
            },
            PolicyConfig::FullJitter {
                base_us: 25_000,
                cap_us: 200_000,
                rng_namespace: "retry_jitter".to_owned(),
            },
            PolicyConfig::RetryBudget {
                base_us: 25_000,
                cap_us: 200_000,
                token_bucket: crate::config::TokenBucketConfig {
                    units_per_token: 10,
                    retry_cost_units: 10,
                    refill_units_per_unique_on_time_success: 1,
                    capacity_units: 100,
                    initial_units: 100,
                    token_denial_result: crate::config::TokenDenialResult::RetryDenied,
                },
            },
        ];
        let summaries: Vec<_> = policies
            .into_iter()
            .map(|policy| {
                let mut config = base.clone();
                config.policy = policy;
                run_simulation(&config).unwrap()
            })
            .collect();

        for summary in &summaries[1..] {
            assert_eq!(
                summary.arrival_trace_sha256,
                summaries[0].arrival_trace_sha256
            );
            assert_eq!(
                summary.potential_service_trace_sha256,
                summaries[0].potential_service_trace_sha256
            );
            assert!(summary.lifecycle_ledger_closed);
        }
        assert!(
            summaries
                .iter()
                .all(|summary| summary.counts.retries_scheduled > 0)
        );
        assert!(summaries[3].counts.retries_denied_by_budget > 0);
    }

    #[test]
    fn event_cap_fails_closed_instead_of_livelocking() {
        let mut config = parse_resolved_run_config(SMOKE).unwrap();
        config.maximum_events = 1;
        assert!(run_simulation(&config).is_err());
    }

    #[test]
    fn max_attempts_includes_the_initial_attempt() {
        let mut config = parse_resolved_run_config(SMOKE).unwrap();
        config.retry_contract.max_attempts_including_initial = 1;
        config.retry_contract.per_attempt_timeout_us = 1;
        let summary = run_simulation(&config).unwrap();

        assert_eq!(
            summary.counts.physical_attempts,
            summary.counts.logical_arrivals
        );
        assert_eq!(summary.counts.retries_scheduled, 0);
        assert!(summary.counts.logical_attempts_exhausted > 0);
    }

    #[test]
    fn retry_that_becomes_ready_after_deadline_is_a_noop() {
        let mut config = parse_resolved_run_config(SMOKE).unwrap();
        config.retry_contract.per_attempt_timeout_us = 1;
        config.retry_contract.overall_logical_deadline_us = 10;
        config.policy = PolicyConfig::Exponential {
            base_us: 20,
            cap_us: 20,
        };
        let summary = run_simulation(&config).unwrap();

        assert!(summary.counts.retries_scheduled > 0);
        assert!(summary.counts.retry_ready_noops > 0);
        assert_eq!(
            summary.counts.physical_attempts,
            summary.counts.logical_arrivals
        );
    }

    #[test]
    fn one_parent_attempt_can_schedule_at_most_one_retry() {
        let config = parse_resolved_run_config(SMOKE).unwrap();
        let mut engine = Engine::new(&config).unwrap();
        let logical_id = LogicalRequestId(1);
        let attempt_id = AttemptId(1);
        engine
            .ledger
            .register_logical_request(&mut engine.scheduler, Tick::ZERO, logical_id)
            .unwrap();
        engine.admit_attempt(Tick::ZERO, logical_id, 1).unwrap();
        assert_eq!(
            engine
                .ledger
                .handle_attempt_timeout(Tick::new(500_000), attempt_id)
                .unwrap(),
            TimeoutHandling::TimedOut
        );

        engine
            .schedule_retry(Tick::new(500_000), attempt_id)
            .unwrap();
        let scheduled_after_first = engine.scheduler.len();
        engine
            .schedule_retry(Tick::new(500_000), attempt_id)
            .unwrap();

        assert_eq!(engine.scheduler.len(), scheduled_after_first);
        assert_eq!(engine.policy.counters().retries_scheduled, 1);
        assert_eq!(engine.counters.duplicate_retry_schedules_suppressed, 1);
    }
}
