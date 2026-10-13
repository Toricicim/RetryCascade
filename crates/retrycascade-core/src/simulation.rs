use std::collections::BTreeSet;
use std::f64::consts::TAU;

use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::config::{ConfigProvenance, DemandIdentity, PolicyConfig, ResolvedRunConfig, StageName};
use crate::event::{Event, EventKind};
use crate::lifecycle::{
    AttemptProgress, AttemptTerminal, LifecycleError, LifecycleLedger, LifecycleSpec,
    LifecycleSummary, LogicalTerminal, TimeoutHandling,
};
use crate::metrics::{
    MetricsError, QueueTimeline, RecoverySpec, RecoverySummary, calendar_windows, evaluate_recovery,
};
use crate::pipeline::{
    Pipeline, PipelineError, SinkTriggerSpec, StageCompletionHandling, StageRuntimeSpec,
};
use crate::policy::{PolicyError, RetryContext, RetryDecision, RetryPolicy};
use crate::rng::KeyedRng;
use crate::scheduler::{Scheduler, SchedulerError};
use crate::types::{AttemptId, LogicalRequestId, StageId, Tick};

const SUMMARY_SCHEMA_VERSION: &str = "retrycascade-run-summary-v1.1.0";
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
    #[serde(skip_serializing_if = "is_zero")]
    pub logical_budget_denied: u64,
    pub logical_outstanding_at_horizon: u64,
    pub physical_attempts: u64,
    pub attempt_timeouts: u64,
    pub late_completions: u64,
    pub duplicate_completions: u64,
    pub queue_rejections: u64,
    #[serde(skip_serializing_if = "is_zero")]
    pub pre_admission_failures: u64,
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
    pub diagnostics: RunDiagnostics,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub v2_metrics: Option<V2Metrics>,
    pub summary_sha256: String,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LatencySummary {
    pub count: u64,
    pub sum_us: u128,
    pub p50_us: Option<u64>,
    pub p95_us: Option<u64>,
    pub p99_us: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CohortOutcomeMetrics {
    pub name: &'static str,
    pub arrivals: u64,
    pub observed_outcomes: u64,
    pub right_censored: u64,
    pub successes: u64,
    pub failures: u64,
    pub success_latency: LatencySummary,
    pub failure_latency: LatencySummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct V2Metrics {
    pub potential_failure_trace_sha256: String,
    pub stage_consumed_work_milli: [u128; 3],
    pub stage_busy_worker_us: [u128; 3],
    pub useful_attempt_work_milli: u128,
    pub unsuccessful_terminal_attempt_work_milli: u128,
    pub outstanding_attempt_work_milli: u128,
    pub outcome_cohorts: Vec<CohortOutcomeMetrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_trace: Option<RecoveryTrace>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecoveryTrace {
    pub window_us: u64,
    pub reference_trigger_start_us: u64,
    pub reference_trigger_end_us: u64,
    pub total_waiting_capacity: u64,
    pub interval_convention: &'static str,
    pub windows: Vec<CalendarWindowTrace>,
    pub cohorts: Vec<ReferenceCohort>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReferenceCohort {
    pub start_us: u64,
    pub end_us: u64,
    pub mature_available_at_us: u64,
    pub outcomes: CohortOutcomeMetrics,
    pub deadline_abandoned: u64,
    pub attempts_exhausted: u64,
    pub budget_denied: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CalendarWindowTrace {
    pub start_us: u64,
    pub end_us: u64,
    pub is_full_window: bool,
    pub unique_goodput: u64,
    pub queue_area_slot_us: u128,
    pub stage_consumed_work_milli: [u128; 3],
    pub stage_busy_worker_us: [u128; 3],
    pub stage_available_worker_us: [u128; 3],
    pub stage_capacity_work_milli: [u128; 3],
    pub logical_arrivals: u64,
    pub first_attempts: u64,
    pub retry_attempts: u64,
    pub attempt_timeouts: u64,
    pub pre_admission_failures: u64,
    pub queue_rejections: u64,
    pub late_completions: u64,
    pub duplicate_completions: u64,
    pub budget_denied: u64,
    pub retries_denied_by_budget: u64,
    /// Outcome fields use arrival time, not completion time. They must not be
    /// used as online features before this explicit maturity watermark.
    pub mature_available_at_us: u64,
    pub cohort: CohortOutcomeMetrics,
    pub cohort_deadline_abandoned: u64,
    pub cohort_attempts_exhausted: u64,
    pub cohort_budget_denied: u64,
}

struct TraceState {
    windows: Vec<CalendarWindowTrace>,
    next_snapshot: usize,
    previous_work: [u128; 3],
    previous_busy: [u128; 3],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArrivalCohort {
    pub name: &'static str,
    pub start_us: u64,
    pub end_us: u64,
    pub logical_arrivals: u64,
    pub unique_on_time_successes: u64,
    pub deadline_abandoned: u64,
    pub attempts_exhausted: u64,
    #[serde(skip_serializing_if = "is_zero")]
    pub budget_denied: u64,
    pub outstanding_at_horizon: u64,
    pub successes_by_interval_end: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BaselineWindowDiagnostic {
    pub start_us: u64,
    pub end_us: u64,
    pub logical_arrivals: u64,
    pub unique_goodput: u64,
    pub queue_area_slot_us: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunDiagnostics {
    pub baseline_cohort: ArrivalCohort,
    pub arrival_cohorts: Vec<ArrivalCohort>,
    pub baseline_windows: Vec<BaselineWindowDiagnostic>,
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
    trace: Option<TraceState>,
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
        engine.snapshot_trace_until(event.at.min(config.run_end_us))?;
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
        let before = engine
            .trace
            .as_ref()
            .map(|_| (engine.ledger.summary(), engine.policy.counters()));
        engine.handle_event(event)?;
        if let Some((before_ledger, before_policy)) = before {
            engine.record_trace_event(event, before_ledger, before_policy);
        }
        engine
            .queue_timeline
            .record(event.at, engine.pipeline.total_waiting()?)?;
    }

    engine.snapshot_trace_until(config.run_end_us)?;

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
    let diagnostics = run_diagnostics(config, &engine)?;
    let v2_metrics = if config.extensions.is_some() {
        Some(v2_metrics(config, &engine)?)
    } else {
        None
    };
    let mut summary = RunSummary {
        summary_schema_version: if config
            .extensions
            .as_ref()
            .is_some_and(|ext| ext.observation.is_some())
        {
            "retrycascade-run-summary-v2.1.0"
        } else if config.extensions.is_some() {
            "retrycascade-run-summary-v2.0.0"
        } else {
            SUMMARY_SCHEMA_VERSION
        },
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
        diagnostics,
        v2_metrics,
        summary_sha256: String::new(),
    };
    summary.summary_sha256 = summary_checksum(&summary)?;
    Ok(summary)
}

fn arrival_cohort(
    ledger: &LifecycleLedger,
    name: &'static str,
    start: Tick,
    end: Tick,
) -> ArrivalCohort {
    let mut result = ArrivalCohort {
        name,
        start_us: start.as_u64(),
        end_us: end.as_u64(),
        logical_arrivals: 0,
        unique_on_time_successes: 0,
        deadline_abandoned: 0,
        attempts_exhausted: 0,
        budget_denied: 0,
        outstanding_at_horizon: 0,
        successes_by_interval_end: 0,
    };
    for request in ledger
        .logical_requests()
        .filter(|request| request.arrived_at >= start && request.arrived_at < end)
    {
        result.logical_arrivals += 1;
        match request.terminal {
            Some(LogicalTerminal::Succeeded { at, .. }) => {
                result.unique_on_time_successes += 1;
                if at < end {
                    result.successes_by_interval_end += 1;
                }
            }
            Some(LogicalTerminal::DeadlineAbandoned { .. }) => result.deadline_abandoned += 1,
            Some(LogicalTerminal::AttemptsExhausted { .. }) => result.attempts_exhausted += 1,
            Some(LogicalTerminal::BudgetDenied { .. }) => result.budget_denied += 1,
            Some(LogicalTerminal::OutstandingAtHorizon { .. }) | None => {
                result.outstanding_at_horizon += 1;
            }
        }
    }
    result
}

fn run_diagnostics(
    config: &ResolvedRunConfig,
    engine: &Engine<'_>,
) -> Result<RunDiagnostics, SimulationError> {
    let mut baseline_windows = Vec::new();
    let mut start = config.observation.baseline_start_us;
    while start < config.observation.baseline_end_us {
        let end = start
            .checked_add(config.baseline.window_us)
            .ok_or(SimulationError::CounterOverflow("baseline_window_end"))?;
        let cohort = arrival_cohort(&engine.ledger, "baseline_window", start, end);
        let unique_goodput = engine
            .goodput_completion_times
            .iter()
            .filter(|at| **at >= start && **at < end)
            .count() as u64;
        baseline_windows.push(BaselineWindowDiagnostic {
            start_us: start.as_u64(),
            end_us: end.as_u64(),
            logical_arrivals: cohort.logical_arrivals,
            unique_goodput,
            queue_area_slot_us: engine.queue_timeline.area_between(start, end)?,
        });
        start = end;
    }
    Ok(RunDiagnostics {
        baseline_cohort: arrival_cohort(
            &engine.ledger,
            "baseline",
            config.observation.baseline_start_us,
            config.observation.baseline_end_us,
        ),
        arrival_cohorts: vec![
            arrival_cohort(
                &engine.ledger,
                "pre_trigger",
                config.observation.arrivals_begin_us,
                config.trigger_start_us,
            ),
            arrival_cohort(
                &engine.ledger,
                "during_trigger",
                config.trigger_start_us,
                config.trigger_end_us,
            ),
            arrival_cohort(
                &engine.ledger,
                "post_trigger",
                config.trigger_end_us,
                config.run_end_us,
            ),
        ],
        baseline_windows,
    })
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
        let mut ledger = LifecycleLedger::new(LifecycleSpec {
            per_attempt_timeout_us: config.retry_contract.per_attempt_timeout_us,
            overall_logical_deadline_us: config.retry_contract.overall_logical_deadline_us,
        })?;
        ledger.set_accept_timed_out_completion(
            config
                .extensions
                .as_ref()
                .is_some_and(|ext| ext.accept_timed_out_completion),
        );
        Ok(Self {
            config,
            scheduler: Scheduler::new(),
            pipeline: Pipeline::new_with_sink_trigger(specs, trigger)?,
            ledger,
            policy: RetryPolicy::new(config.policy.clone(), config.seed),
            rng: KeyedRng::new(config.seed),
            retry_scheduled_for_attempt: BTreeSet::new(),
            goodput_completion_times: Vec::new(),
            queue_timeline: QueueTimeline::new(Tick::ZERO, 0),
            counters: EngineCounters::default(),
            trace: trace_state(config)?,
        })
    }

    /// Capture before the first event at a boundary. Work and busy time are
    /// continuous integrals; completion/event counts use [start,end) instead.
    /// This does not add scheduler events or change its tie-breaking order.
    fn snapshot_trace_until(&mut self, now: Tick) -> Result<(), SimulationError> {
        let Some(trace) = self.trace.as_mut() else {
            return Ok(());
        };
        while let Some(window) = trace.windows.get_mut(trace.next_snapshot) {
            if window.end_us > now.as_u64() {
                break;
            }
            let work = self.pipeline.work_at(Tick::new(window.end_us))?;
            let mut cumulative_work = [0_u128; 3];
            for stages in work.values() {
                for (stage, amount) in stages.iter().enumerate() {
                    cumulative_work[stage] = cumulative_work[stage]
                        .checked_add(*amount)
                        .ok_or(SimulationError::CounterOverflow("trace_work"))?;
                }
            }
            let cumulative_busy = self.pipeline.busy_time_at(Tick::new(window.end_us))?;
            for stage in 0..3 {
                window.stage_consumed_work_milli[stage] = cumulative_work[stage]
                    .checked_sub(trace.previous_work[stage])
                    .ok_or(SimulationError::CounterOverflow("trace_work_delta"))?;
                window.stage_busy_worker_us[stage] = cumulative_busy[stage]
                    .checked_sub(trace.previous_busy[stage])
                    .ok_or(SimulationError::CounterOverflow("trace_busy_delta"))?;
            }
            trace.previous_work = cumulative_work;
            trace.previous_busy = cumulative_busy;
            trace.next_snapshot += 1;
        }
        Ok(())
    }

    fn record_trace_event(
        &mut self,
        event: Event,
        before: LifecycleSummary,
        policy_before: crate::policy::RetryPolicyCounters,
    ) {
        let after = self.ledger.summary();
        let policy_after = self.policy.counters();
        let Some(trace) = self.trace.as_mut() else {
            return;
        };
        let index = trace
            .windows
            .partition_point(|window| window.end_us <= event.at.as_u64());
        let Some(window) = trace.windows.get_mut(index) else {
            return;
        };
        if event.at.as_u64() < window.start_us {
            return;
        }
        window.logical_arrivals += after.logical_arrivals - before.logical_arrivals;
        let created = after.physical_attempts - before.physical_attempts;
        if matches!(event.kind, EventKind::LogicalArrival { .. }) {
            window.first_attempts += created;
        } else {
            window.retry_attempts += created;
        }
        window.unique_goodput += after.logical_successes - before.logical_successes;
        window.attempt_timeouts += after.attempt_timeouts - before.attempt_timeouts;
        window.pre_admission_failures +=
            after.attempt_pre_admission_failures - before.attempt_pre_admission_failures;
        window.queue_rejections += after.attempt_queue_rejections - before.attempt_queue_rejections;
        window.late_completions += after.attempt_late_completions - before.attempt_late_completions;
        window.duplicate_completions +=
            after.attempt_duplicate_completions - before.attempt_duplicate_completions;
        window.budget_denied += after.logical_budget_denied - before.logical_budget_denied;
        window.retries_denied_by_budget +=
            policy_after.retries_denied_by_budget - policy_before.retries_denied_by_budget;
    }

    fn handle_event(&mut self, event: Event) -> Result<(), SimulationError> {
        match event.kind {
            EventKind::PreAdmissionFailure { attempt_id } => {
                self.ledger
                    .handle_pre_admission_failure(event.at, attempt_id)?;
                self.schedule_retry(event.at, attempt_id)?;
            }
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
        if let Some(ext) = &self.config.extensions
            && self.rng.pre_admission_fails(
                logical_request_id,
                u64::from(attempt_no),
                ext.pre_admission_failure_basis_points,
            )
        {
            self.ledger.register_pre_admission_failure(
                &mut self.scheduler,
                now,
                logical_request_id,
                attempt_id,
                attempt_no,
                ext.failure_notification_delay_us,
            )?;
            return Ok(());
        }
        let demands = [
            draw_service(
                self.config,
                self.rng,
                logical_request_id,
                u64::from(attempt_no),
                StageId(0),
            )?,
            draw_service(
                self.config,
                self.rng,
                logical_request_id,
                u64::from(attempt_no),
                StageId(1),
            )?,
            draw_service(
                self.config,
                self.rng,
                logical_request_id,
                u64::from(attempt_no),
                StageId(2),
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
            if self
                .config
                .extensions
                .as_ref()
                .is_some_and(|ext| ext.accept_timed_out_completion)
                && self
                    .ledger
                    .has_live_attempt_for_logical(parent.logical_request_id)
            {
                // Exhausting retry permission need not abandon an already
                // running request in the explicit await-in-flight variant.
                return Ok(());
            }
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
            RetryDecision::DeniedByBudget => {
                if self
                    .config
                    .extensions
                    .as_ref()
                    .is_some_and(|ext| ext.budget_denial_finishes_logical_request)
                {
                    self.ledger
                        .mark_budget_denied(now, parent.logical_request_id)?;
                }
            }
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

        let trace_attempts = if config.extensions.is_some() {
            3
        } else {
            config.retry_contract.max_attempts_including_initial
        };
        for attempt_no in 1..=trace_attempts {
            for stage_index in 0..STAGE_COUNT {
                let demand = draw_service(
                    config,
                    rng,
                    logical_id,
                    attempt_no,
                    StageId(stage_index as u8),
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

fn draw_service(
    config: &ResolvedRunConfig,
    rng: KeyedRng,
    id: LogicalRequestId,
    attempt_no: u64,
    stage: StageId,
) -> Result<u64, SimulationError> {
    let key_attempt = if config
        .extensions
        .as_ref()
        .is_some_and(|ext| ext.demand_identity == DemandIdentity::RequestStable)
    {
        1
    } else {
        attempt_no
    };
    service_demand_milli(
        rng,
        id,
        key_attempt,
        stage,
        config.service_demand.mean_work_units_milli,
        config.service_demand.coefficient_of_variation_milli,
    )
}

fn latency_summary(mut times: Vec<u64>) -> LatencySummary {
    times.sort_unstable();
    let percentile = |p: usize| {
        if times.is_empty() {
            None
        } else {
            Some(times[(times.len() * p).div_ceil(100) - 1])
        }
    };
    LatencySummary {
        count: times.len() as u64,
        sum_us: times.iter().map(|v| u128::from(*v)).sum(),
        p50_us: percentile(50),
        p95_us: percentile(95),
        p99_us: percentile(99),
    }
}

fn empty_cohort(name: &'static str) -> CohortOutcomeMetrics {
    CohortOutcomeMetrics {
        name,
        arrivals: 0,
        observed_outcomes: 0,
        right_censored: 0,
        successes: 0,
        failures: 0,
        success_latency: latency_summary(Vec::new()),
        failure_latency: latency_summary(Vec::new()),
    }
}

fn trace_state(config: &ResolvedRunConfig) -> Result<Option<TraceState>, SimulationError> {
    let Some(observation) = config
        .extensions
        .as_ref()
        .and_then(|ext| ext.observation.as_ref())
    else {
        return Ok(None);
    };
    let mut windows = Vec::new();
    let regular_windows = calendar_windows(
        config.observation.arrivals_begin_us,
        config.run_end_us,
        config.trigger_start_us,
        observation.window_us,
    )?;
    let reference_end = config
        .trigger_start_us
        .checked_add(observation.reference_trigger_duration_us)
        .ok_or(SimulationError::CounterOverflow("reference_trigger_end"))?;
    let early_post_end = reference_end
        .checked_add(60_000_000)
        .ok_or(SimulationError::CounterOverflow("early_post_end"))?;
    let mut boundaries = BTreeSet::new();
    for (start, end) in regular_windows {
        boundaries.insert(start);
        boundaries.insert(end);
    }
    for boundary in [reference_end, early_post_end] {
        if boundary > config.observation.arrivals_begin_us && boundary < config.run_end_us {
            boundaries.insert(boundary);
        }
    }
    let boundaries: Vec<_> = boundaries.into_iter().collect();
    for pair in boundaries.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        windows.push(CalendarWindowTrace {
            start_us: start.as_u64(),
            end_us: end.as_u64(),
            is_full_window: end.as_u64() - start.as_u64() == observation.window_us,
            unique_goodput: 0,
            queue_area_slot_us: 0,
            stage_consumed_work_milli: [0; 3],
            stage_busy_worker_us: [0; 3],
            stage_available_worker_us: [0; 3],
            stage_capacity_work_milli: [0; 3],
            logical_arrivals: 0,
            first_attempts: 0,
            retry_attempts: 0,
            attempt_timeouts: 0,
            pre_admission_failures: 0,
            queue_rejections: 0,
            late_completions: 0,
            duplicate_completions: 0,
            budget_denied: 0,
            retries_denied_by_budget: 0,
            mature_available_at_us: end
                .checked_add(config.retry_contract.overall_logical_deadline_us)
                .ok_or(SimulationError::CounterOverflow("cohort_maturity"))?
                .as_u64(),
            cohort: empty_cohort("calendar_arrival_cohort"),
            cohort_deadline_abandoned: 0,
            cohort_attempts_exhausted: 0,
            cohort_budget_denied: 0,
        });
    }
    Ok(Some(TraceState {
        windows,
        next_snapshot: 0,
        previous_work: [0; 3],
        previous_busy: [0; 3],
    }))
}

fn mature_cohort(
    ledger: &LifecycleLedger,
    name: &'static str,
    start: Tick,
    end: Tick,
    observed_until: Tick,
) -> CohortOutcomeMetrics {
    let mut result = empty_cohort(name);
    let mut successes = Vec::new();
    let mut failures = Vec::new();
    for request in ledger
        .logical_requests()
        .filter(|request| request.arrived_at >= start && request.arrived_at < end)
    {
        result.arrivals += 1;
        if request.deadline_at > observed_until {
            result.right_censored += 1;
            continue;
        }
        match request.terminal {
            Some(LogicalTerminal::Succeeded { at, .. }) if at <= observed_until => {
                successes.push(at.as_u64() - request.arrived_at.as_u64())
            }
            Some(LogicalTerminal::DeadlineAbandoned { at })
            | Some(LogicalTerminal::AttemptsExhausted { at })
            | Some(LogicalTerminal::BudgetDenied { at })
                if at <= observed_until =>
            {
                failures.push(at.as_u64() - request.arrived_at.as_u64())
            }
            _ => result.right_censored += 1,
        }
    }
    result.successes = successes.len() as u64;
    result.failures = failures.len() as u64;
    result.observed_outcomes = result.successes + result.failures;
    result.success_latency = latency_summary(successes);
    result.failure_latency = latency_summary(failures);
    result
}

fn recovery_trace(
    config: &ResolvedRunConfig,
    engine: &Engine<'_>,
) -> Result<Option<RecoveryTrace>, SimulationError> {
    let Some(state) = &engine.trace else {
        return Ok(None);
    };
    let observation = config
        .extensions
        .as_ref()
        .and_then(|ext| ext.observation.as_ref())
        .expect("trace requires observation config");
    let mut windows = state.windows.clone();
    for window in &mut windows {
        window.queue_area_slot_us = engine
            .queue_timeline
            .area_between(Tick::new(window.start_us), Tick::new(window.end_us))?;
        window.cohort = mature_cohort(
            &engine.ledger,
            "calendar_arrival_cohort",
            Tick::new(window.start_us),
            Tick::new(window.end_us),
            Tick::new(window.mature_available_at_us).min(config.run_end_us),
        );
        let reasons = cohort_failure_reasons(
            &engine.ledger,
            Tick::new(window.start_us),
            Tick::new(window.end_us),
            Tick::new(window.mature_available_at_us).min(config.run_end_us),
        );
        window.cohort_deadline_abandoned = reasons[0];
        window.cohort_attempts_exhausted = reasons[1];
        window.cohort_budget_denied = reasons[2];
        for stage in 0..3 {
            let duration = window.end_us - window.start_us;
            window.stage_available_worker_us[stage] =
                u128::from(config.stages[stage].workers) * u128::from(duration);
            let overlap = if stage == 2 && config.trigger_enabled {
                window
                    .end_us
                    .min(config.trigger_end_us.as_u64())
                    .saturating_sub(window.start_us.max(config.trigger_start_us.as_u64()))
            } else {
                0
            };
            let multiplier_integral = u128::from(duration - overlap) * 10_000
                + u128::from(overlap) * u128::from(config.trigger_capacity_multiplier_basis_points);
            window.stage_capacity_work_milli[stage] =
                u128::from(config.stages[stage].nominal_rate_work_units_per_second)
                    .checked_mul(u128::from(config.stages[stage].workers))
                    .and_then(|v| v.checked_mul(1_000))
                    .and_then(|v| v.checked_mul(multiplier_integral))
                    .ok_or(SimulationError::CounterOverflow("trace_capacity_work"))?
                    / 10_000
                    / 1_000_000;
        }
    }
    let reference_end = config
        .trigger_start_us
        .checked_add(observation.reference_trigger_duration_us)
        .ok_or(SimulationError::CounterOverflow("reference_trigger_end"))?;
    let first_60_end = reference_end
        .checked_add(60_000_000)
        .ok_or(SimulationError::CounterOverflow("post_first_60_end"))?
        .min(config.run_end_us);
    let mut cohorts = Vec::new();
    for (name, start, end) in [
        (
            "during_reference_trigger",
            config.trigger_start_us,
            reference_end,
        ),
        ("post_reference_first_60s", reference_end, first_60_end),
        ("post_reference_later", first_60_end, config.run_end_us),
    ] {
        let mature_at = end
            .checked_add(config.retry_contract.overall_logical_deadline_us)
            .ok_or(SimulationError::CounterOverflow(
                "reference_cohort_maturity",
            ))?;
        let reasons =
            cohort_failure_reasons(&engine.ledger, start, end, mature_at.min(config.run_end_us));
        cohorts.push(ReferenceCohort {
            start_us: start.as_u64(),
            end_us: end.as_u64(),
            mature_available_at_us: mature_at.as_u64(),
            outcomes: mature_cohort(
                &engine.ledger,
                name,
                start,
                end,
                mature_at.min(config.run_end_us),
            ),
            deadline_abandoned: reasons[0],
            attempts_exhausted: reasons[1],
            budget_denied: reasons[2],
        });
    }
    let total_waiting_capacity = config.stages.iter().try_fold(0_u64, |sum, stage| {
        sum.checked_add(stage.queue_capacity_waiting)
            .ok_or(SimulationError::CounterOverflow("trace_waiting_capacity"))
    })?;
    Ok(Some(RecoveryTrace {
        window_us: observation.window_us,
        reference_trigger_start_us: config.trigger_start_us.as_u64(),
        reference_trigger_end_us: reference_end.as_u64(),
        total_waiting_capacity,
        interval_convention: "[start,end)",
        windows,
        cohorts,
    }))
}

fn cohort_failure_reasons(
    ledger: &LifecycleLedger,
    start: Tick,
    end: Tick,
    observed_until: Tick,
) -> [u64; 3] {
    let mut counts = [0; 3];
    for request in ledger.logical_requests().filter(|request| {
        request.arrived_at >= start
            && request.arrived_at < end
            && request.deadline_at <= observed_until
    }) {
        match request.terminal {
            Some(LogicalTerminal::DeadlineAbandoned { at }) if at <= observed_until => {
                counts[0] += 1
            }
            Some(LogicalTerminal::AttemptsExhausted { at }) if at <= observed_until => {
                counts[1] += 1
            }
            Some(LogicalTerminal::BudgetDenied { at }) if at <= observed_until => counts[2] += 1,
            _ => {}
        }
    }
    counts
}

fn v2_metrics(
    config: &ResolvedRunConfig,
    engine: &Engine<'_>,
) -> Result<V2Metrics, SimulationError> {
    let work = engine.pipeline.work_at(config.run_end_us)?;
    let mut stage_work = [0_u128; 3];
    let mut useful = 0_u128;
    let mut unsuccessful = 0_u128;
    let mut outstanding = 0_u128;
    for (id, stages) in work {
        for (stage, amount) in stages.iter().enumerate() {
            stage_work[stage] = stage_work[stage]
                .checked_add(*amount)
                .ok_or(SimulationError::CounterOverflow("stage_work"))?;
        }
        let total: u128 = stages.iter().sum();
        let attempt = engine
            .ledger
            .attempt(id)
            .ok_or(LifecycleError::UnknownAttempt(id))?;
        let category = match attempt.terminal {
            Some(AttemptTerminal::OnTimeSuccess { .. }) => &mut useful,
            Some(AttemptTerminal::OutstandingAtHorizon { .. }) | None => &mut outstanding,
            _ => &mut unsuccessful,
        };
        *category = category
            .checked_add(total)
            .ok_or(SimulationError::CounterOverflow("classified_work"))?;
    }
    let mut failure_hasher = Sha256::new();
    for request in engine.ledger.logical_requests() {
        for attempt in 1..=3 {
            failure_hasher.update(request.id.0.to_le_bytes());
            failure_hasher.update(engine.rng.failure_u64(request.id, attempt).to_le_bytes());
        }
    }
    let baseline_mature_end = config
        .observation
        .baseline_end_us
        .as_u64()
        .saturating_sub(config.retry_contract.overall_logical_deadline_us);
    let intervals = [
        (
            "all",
            config.observation.arrivals_begin_us,
            config.run_end_us,
        ),
        (
            "during_trigger",
            config.trigger_start_us,
            config.trigger_end_us,
        ),
        ("post_trigger", config.trigger_end_us, config.run_end_us),
        (
            "baseline_mature",
            config.observation.baseline_start_us,
            Tick::new(baseline_mature_end),
        ),
    ];
    let mut cohorts = Vec::new();
    for (name, start, end) in intervals {
        let mut successes = Vec::new();
        let mut failures = Vec::new();
        let mut arrivals = 0;
        let mut censored = 0;
        for request in engine
            .ledger
            .logical_requests()
            .filter(|request| request.arrived_at >= start && request.arrived_at < end)
        {
            arrivals += 1;
            if request.deadline_at > config.run_end_us {
                // Exclude the entire trailing deadline cohort, even successes.
                censored += 1;
                continue;
            }
            match request.terminal {
                Some(LogicalTerminal::Succeeded { at, .. }) => {
                    successes.push(at.as_u64() - request.arrived_at.as_u64())
                }
                Some(LogicalTerminal::DeadlineAbandoned { at })
                | Some(LogicalTerminal::AttemptsExhausted { at })
                | Some(LogicalTerminal::BudgetDenied { at }) => {
                    failures.push(at.as_u64() - request.arrived_at.as_u64())
                }
                _ => censored += 1,
            }
        }
        cohorts.push(CohortOutcomeMetrics {
            name,
            arrivals,
            observed_outcomes: successes.len() as u64 + failures.len() as u64,
            right_censored: censored,
            successes: successes.len() as u64,
            failures: failures.len() as u64,
            success_latency: latency_summary(successes),
            failure_latency: latency_summary(failures),
        });
    }
    Ok(V2Metrics {
        potential_failure_trace_sha256: format!("{:x}", failure_hasher.finalize()),
        stage_consumed_work_milli: stage_work,
        stage_busy_worker_us: engine.pipeline.busy_time_at(config.run_end_us)?,
        useful_attempt_work_milli: useful,
        unsuccessful_terminal_attempt_work_milli: unsuccessful,
        outstanding_attempt_work_milli: outstanding,
        outcome_cohorts: cohorts,
        recovery_trace: recovery_trace(config, engine)?,
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
    if matches!(
        attempt.terminal,
        Some(AttemptTerminal::PreAdmissionFailure { .. })
    ) {
        crate::config::RetryCause::PreAdmissionFailure
    } else if attempt.timed_out_at.is_some() {
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
        logical_budget_denied: lifecycle.logical_budget_denied,
        logical_outstanding_at_horizon: lifecycle.logical_outstanding_at_horizon,
        physical_attempts: lifecycle.physical_attempts,
        attempt_timeouts: lifecycle.attempt_timeouts,
        late_completions: lifecycle.attempt_late_completions,
        duplicate_completions: lifecycle.attempt_duplicate_completions,
        queue_rejections: lifecycle.attempt_queue_rejections,
        pre_admission_failures: lifecycle.attempt_pre_admission_failures,
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

    fn v2_fixture(probability: u64) -> crate::config::ResolvedRunConfig {
        let mut config = parse_resolved_run_config(SMOKE).unwrap();
        config.schema_version = "2.0.0".into();
        config.model_version = "retrycascade-des-v2.0.0".into();
        config.protocol_version = "retrycascade-protocol-v2.0.0".into();
        config
            .retry_contract
            .retryable_causes
            .push(crate::config::RetryCause::PreAdmissionFailure);
        config.extensions = Some(crate::config::V2Config {
            pre_admission_failure_basis_points: probability,
            failure_notification_delay_us: 1000,
            demand_identity: crate::config::DemandIdentity::RequestStable,
            budget_denial_finishes_logical_request: true,
            accept_timed_out_completion: false,
            observation: None,
        });
        config.validate().unwrap();
        config
    }

    #[test]
    fn pre_admission_failures_consume_no_server_work_or_timeouts() {
        let config = v2_fixture(10000);
        let summary = run_simulation(&config).unwrap();
        assert!(summary.counts.pre_admission_failures > 0);
        assert_eq!(summary.counts.attempt_timeouts, 0);
        assert!(summary.stages.iter().all(|stage| stage.admissions == 0));
        let metrics = summary.v2_metrics.unwrap();
        assert_eq!(metrics.stage_consumed_work_milli, [0; 3]);
        assert_eq!(metrics.stage_busy_worker_us, [0; 3]);
        assert!(summary.lifecycle_ledger_closed);
    }

    #[test]
    fn retries_can_rescue_transient_errors_and_share_failure_trace_with_no_retry() {
        let mut config = v2_fixture(3000);
        config.rho_basis_points = 1000;
        let retries = run_simulation(&config).unwrap();
        config.retry_contract.max_attempts_including_initial = 1;
        let no_retry = run_simulation(&config).unwrap();
        assert!(retries.counts.unique_goodput > no_retry.counts.unique_goodput);
        assert_eq!(retries.arrival_trace_sha256, no_retry.arrival_trace_sha256);
        assert_eq!(
            retries.potential_service_trace_sha256,
            no_retry.potential_service_trace_sha256
        );
        assert_eq!(
            retries.v2_metrics.unwrap().potential_failure_trace_sha256,
            no_retry.v2_metrics.unwrap().potential_failure_trace_sha256
        );
    }

    #[test]
    fn consumed_work_and_mature_outcomes_close_in_v2() {
        let config = v2_fixture(2000);
        let summary = run_simulation(&config).unwrap();
        let metrics = summary.v2_metrics.unwrap();
        assert_eq!(
            metrics.stage_consumed_work_milli.iter().sum::<u128>(),
            metrics.useful_attempt_work_milli
                + metrics.unsuccessful_terminal_attempt_work_milli
                + metrics.outstanding_attempt_work_milli
        );
        for cohort in metrics.outcome_cohorts {
            assert_eq!(
                cohort.arrivals,
                cohort.observed_outcomes + cohort.right_censored
            );
            assert_eq!(cohort.observed_outcomes, cohort.successes + cohort.failures);
        }
    }

    #[test]
    fn observation_is_passive_and_calendar_integrals_close() {
        let mut config = v2_fixture(2000);
        config.rho_basis_points = 1000;
        config.post_trigger_duration_us = 61_000_000;
        config.run_end_us = Tick::new(64_000_000);
        let without = run_simulation(&config).unwrap();
        config.extensions.as_mut().unwrap().observation =
            Some(crate::config::RecoveryObservationConfig {
                window_us: 30_000_000,
                reference_trigger_duration_us: config.trigger_duration_us,
            });
        let with = run_simulation(&config).unwrap();
        assert_eq!(with.counts, without.counts);
        assert_eq!(with.recovery, without.recovery);
        assert_eq!(with.arrival_trace_sha256, without.arrival_trace_sha256);
        assert_eq!(
            with.summary_schema_version,
            "retrycascade-run-summary-v2.1.0"
        );
        let metrics = with.v2_metrics.unwrap();
        let trace = metrics.recovery_trace.unwrap();
        assert_eq!(trace.windows[0].end_us, config.trigger_start_us.as_u64());
        assert!(!trace.windows[0].is_full_window);
        assert_eq!(
            trace.windows.last().unwrap().end_us,
            config.run_end_us.as_u64()
        );
        for stage in 0..3 {
            assert_eq!(
                trace
                    .windows
                    .iter()
                    .map(|w| w.stage_consumed_work_milli[stage])
                    .sum::<u128>(),
                metrics.stage_consumed_work_milli[stage]
            );
            assert_eq!(
                trace
                    .windows
                    .iter()
                    .map(|w| w.stage_busy_worker_us[stage])
                    .sum::<u128>(),
                metrics.stage_busy_worker_us[stage]
            );
        }
        assert_eq!(
            trace.windows.iter().map(|w| w.unique_goodput).sum::<u64>(),
            with.counts.unique_goodput
        );
        assert_eq!(
            trace
                .windows
                .iter()
                .map(|w| w.first_attempts + w.retry_attempts)
                .sum::<u64>(),
            with.counts.physical_attempts
        );
        assert_eq!(
            trace
                .windows
                .iter()
                .map(|w| w.logical_arrivals)
                .sum::<u64>(),
            with.counts.logical_arrivals
        );
        for window in &trace.windows {
            assert_eq!(
                window.cohort.arrivals,
                window.cohort.observed_outcomes + window.cohort.right_censored
            );
            assert_eq!(
                window.cohort.failures,
                window.cohort_budget_denied
                    + window.cohort_deadline_abandoned
                    + window.cohort_attempts_exhausted
            );
            assert_eq!(
                window.mature_available_at_us,
                window.end_us + config.retry_contract.overall_logical_deadline_us
            );
        }
        assert_eq!(trace.cohorts[0].start_us, 2_000_000);
        assert_eq!(trace.cohorts[0].end_us, 3_000_000);
        assert_eq!(trace.cohorts[1].end_us, 63_000_000);
        assert!(
            trace
                .windows
                .iter()
                .any(|window| window.end_us == 3_000_000)
        );
        assert!(
            trace
                .windows
                .iter()
                .any(|window| window.end_us == 63_000_000)
        );
    }

    #[test]
    fn disabled_trigger_uses_the_reference_incident_cohort() {
        let mut config = v2_fixture(0);
        config.trigger_enabled = false;
        config.trigger_duration_us = 0;
        config.trigger_end_us = config.trigger_start_us;
        config.post_trigger_duration_us = 4_000_000;
        config.extensions.as_mut().unwrap().observation =
            Some(crate::config::RecoveryObservationConfig {
                window_us: 30_000_000,
                reference_trigger_duration_us: 1_000_000,
            });
        let summary = run_simulation(&config).unwrap();
        let trace = summary.v2_metrics.unwrap().recovery_trace.unwrap();
        assert_eq!(trace.reference_trigger_end_us, 3_000_000);
        assert!(trace.cohorts[0].outcomes.arrivals > 0);
    }

    #[test]
    fn await_in_flight_contract_can_succeed_after_timeout_without_retry() {
        let mut config = v2_fixture(0);
        config.retry_contract.max_attempts_including_initial = 1;
        config.retry_contract.per_attempt_timeout_us = 1;
        config
            .extensions
            .as_mut()
            .unwrap()
            .accept_timed_out_completion = true;
        let mut engine = Engine::new(&config).unwrap();
        engine
            .handle_event(crate::event::Event {
                at: Tick::ZERO,
                kind: crate::event::EventKind::LogicalArrival {
                    logical_request_id: LogicalRequestId(1),
                },
                sequence: 0,
            })
            .unwrap();
        while let Some(event) = engine.scheduler.pop_next() {
            engine.handle_event(event).unwrap();
        }
        assert_eq!(engine.ledger.summary().logical_successes, 1);
        assert_eq!(engine.ledger.summary().attempt_timeouts, 1);
        assert_eq!(engine.ledger.summary().physical_attempts, 1);
        assert_eq!(engine.ledger.summary().logical_attempts_exhausted, 0);
    }

    #[test]
    fn denied_retry_can_leave_existing_attempt_open_until_success() {
        let mut config = v2_fixture(0);
        config.retry_contract.per_attempt_timeout_us = 1;
        let ext = config.extensions.as_mut().unwrap();
        ext.accept_timed_out_completion = true;
        ext.budget_denial_finishes_logical_request = false;
        config.policy = PolicyConfig::RetryBudget {
            base_us: 25_000,
            cap_us: 200_000,
            token_bucket: crate::config::TokenBucketConfig {
                units_per_token: 10,
                retry_cost_units: 10,
                refill_units_per_unique_on_time_success: 1,
                capacity_units: 100,
                initial_units: 0,
                token_denial_result: crate::config::TokenDenialResult::RetryDenied,
            },
        };
        let mut engine = Engine::new(&config).unwrap();
        engine
            .handle_event(crate::event::Event {
                at: Tick::ZERO,
                kind: crate::event::EventKind::LogicalArrival {
                    logical_request_id: LogicalRequestId(1),
                },
                sequence: 0,
            })
            .unwrap();
        while let Some(event) = engine.scheduler.pop_next() {
            engine.handle_event(event).unwrap();
        }
        assert_eq!(engine.policy.counters().retries_denied_by_budget, 1);
        assert_eq!(engine.ledger.summary().logical_successes, 1);
        assert_eq!(engine.ledger.summary().logical_budget_denied, 0);
        assert_eq!(engine.ledger.summary().physical_attempts, 1);
    }
}
