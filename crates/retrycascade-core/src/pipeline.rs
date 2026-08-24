use std::collections::HashMap;

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::event::{Event, EventKind, TriggerTransition};
use crate::scheduler::{Scheduler, SchedulerError};
use crate::stage::{AdmissionOutcome, CompletionOutcome, Stage, StageError, StageJob};
use crate::types::{AttemptId, StageId, Tick};

const MICROSECONDS_PER_MILLI_WORK_UNIT_AT_ONE_UNIT_PER_SECOND: u64 = 1_000;
const BASIS_POINTS_PER_ONE: u64 = 10_000;
const SCALED_WORK_PER_MILLI_WORK_UNIT: u128 = 10_000_000;
const STAGE_COUNT: usize = 3;

pub const SOURCE_STAGE_ID: StageId = StageId(0);
pub const TRANSFORM_STAGE_ID: StageId = StageId(1);
pub const SINK_STAGE_ID: StageId = StageId(2);

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PipelineError {
    #[error("service demand must be positive")]
    ZeroServiceDemand,
    #[error("service rate must be positive")]
    ZeroServiceRate,
    #[error("service duration arithmetic overflow")]
    ServiceDurationOverflow,
    #[error("unknown stage id {stage_id:?}")]
    UnknownStageId { stage_id: StageId },
    #[error("pipeline metric counter overflow: {counter}")]
    MetricCounterOverflow { counter: &'static str },
    #[error("stage occupancy cannot be represented as u64")]
    OccupancyOverflow,
    #[error("queue-area arithmetic overflow")]
    QueueAreaOverflow,
    #[error("trigger multiplier must be in 1..=10000 basis points")]
    InvalidTriggerMultiplier,
    #[error("trigger start must be before trigger end")]
    InvalidTriggerInterval,
    #[error("unexpected trigger transition {transition:?} at {at:?}")]
    UnexpectedTriggerTransition {
        transition: TriggerTransition,
        at: Tick,
    },
    #[error("service runtime already exists for attempt {attempt_id:?} at stage {stage_id:?}")]
    DuplicateServiceRuntime {
        attempt_id: AttemptId,
        stage_id: StageId,
    },
    #[error("no service epoch is known for attempt {attempt_id:?} at stage {stage_id:?}")]
    UnknownServiceEpoch {
        attempt_id: AttemptId,
        stage_id: StageId,
    },
    #[error(
        "current completion event has no active service for attempt {attempt_id:?} at stage {stage_id:?}"
    )]
    MissingActiveService {
        attempt_id: AttemptId,
        stage_id: StageId,
    },
    #[error(
        "completion for attempt {attempt_id:?} at stage {stage_id:?} was expected at {expected:?}, got {actual:?}"
    )]
    UnexpectedCompletionTime {
        attempt_id: AttemptId,
        stage_id: StageId,
        expected: Tick,
        actual: Tick,
    },
    #[error("service progress arithmetic overflow or invalid event order")]
    ServiceProgressError,
    #[error(transparent)]
    Stage(#[from] StageError),
    #[error(transparent)]
    Scheduler(#[from] SchedulerError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduledAdmission {
    pub stage_id: StageId,
    pub outcome: AdmissionOutcome,
    pub completion_event: Option<Event>,
}

impl ScheduledAdmission {
    pub fn queue_rejected_stage(self) -> Option<StageId> {
        matches!(self.outcome, AdmissionOutcome::Rejected(_)).then_some(self.stage_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduledCompletion {
    pub outcome: CompletionOutcome,
    pub promoted_completion_event: Option<Event>,
}

/// Tek bir pipeline aşamasını çalıştırmak için gereken deterministik kapasite bilgisi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageRuntimeSpec {
    pub workers: usize,
    pub queue_capacity_waiting: usize,
    pub service_rate_work_units_per_second: u64,
}

/// Sink'in kapasitesini `[start, end)` aralığında geçici olarak değiştirir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinkTriggerSpec {
    pub start: Tick,
    pub end: Tick,
    pub capacity_multiplier_basis_points: u64,
}

/// Tek bir stage için sayılan olayları ve gözlenen occupancy değerlerini tutar.
/// `occupancy`, worker'da çalışan veya kuyrukta bekleyen iş sayısıdır.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StageMetrics {
    pub admissions: u64,
    pub completions: u64,
    pub queue_rejections: u64,
    pub in_service: u64,
    pub waiting: u64,
    pub max_in_service: u64,
    pub max_waiting: u64,
    pub waiting_area_slot_us: u64,
}

/// Retry'siz pipeline koşusunun küçük, deterministik özeti.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PipelineSummary {
    pub source_arrival_attempts: u64,
    pub pipeline_completions: u64,
    pub last_pipeline_completion_at_us: Option<u64>,
    pub stale_completion_events: u64,
    pub trigger_reschedules: u64,
    pub stages: [StageMetrics; STAGE_COUNT],
}

impl PipelineSummary {
    /// Alanları sabit sırada SHA-256'e besler. Böylece JSON biçimlendirmesi veya
    /// makinenin mimarisi checksum'u değiştirmez.
    pub fn checksum_sha256(self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"retrycascade-pipeline-summary-v2\0");
        hasher.update(self.source_arrival_attempts.to_le_bytes());
        hasher.update(self.pipeline_completions.to_le_bytes());
        hasher.update(self.stale_completion_events.to_le_bytes());
        hasher.update(self.trigger_reschedules.to_le_bytes());

        match self.last_pipeline_completion_at_us {
            Some(at_us) => {
                hasher.update([1]);
                hasher.update(at_us.to_le_bytes());
            }
            None => hasher.update([0]),
        }

        for stage in self.stages {
            for value in [
                stage.admissions,
                stage.completions,
                stage.queue_rejections,
                stage.in_service,
                stage.waiting,
                stage.max_in_service,
                stage.max_waiting,
                stage.waiting_area_slot_us,
            ] {
                hasher.update(value.to_le_bytes());
            }
        }

        format!("{:x}", hasher.finalize())
    }
}

/// Bir stage completion olayının hem aynı stage'de hem de sonraki stage'de
/// oluşturduğu sonucu birlikte taşır.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PipelineStageCompletion {
    pub completed_stage: StageId,
    pub completed_job: StageJob,
    pub promoted_completion_event: Option<Event>,
    pub downstream_admission: Option<ScheduledAdmission>,
    pub pipeline_completed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageCompletionHandling {
    Applied(PipelineStageCompletion),
    Stale {
        attempt_id: AttemptId,
        stage_id: StageId,
        event_epoch: u64,
        current_epoch: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ActiveService {
    remaining_work_scaled: u128,
    last_updated_at: Tick,
    rate_scaled_per_us: u128,
    epoch: u64,
    completion_at: Tick,
}

/// Retry ve trigger olmadan çalışan sabit Source -> Transform -> Sink hattı.
///
/// Üç elemanın her biri aynı generic `Stage` tipidir. Dizideki sıra, stage
/// kimliğiyle aynıdır: 0 Source, 1 Transform, 2 Sink.
#[derive(Debug)]
pub struct Pipeline {
    stages: [Stage; STAGE_COUNT],
    nominal_rates_scaled_per_us: [u128; STAGE_COUNT],
    effective_rates_scaled_per_us: [u128; STAGE_COUNT],
    sink_trigger: Option<SinkTriggerSpec>,
    trigger_active: bool,
    active_services: [HashMap<AttemptId, ActiveService>; STAGE_COUNT],
    known_service_epochs: [HashMap<AttemptId, u64>; STAGE_COUNT],
    queue_area_last_updated_at: [Tick; STAGE_COUNT],
    summary: PipelineSummary,
}

impl Pipeline {
    pub fn new(specs: [StageRuntimeSpec; STAGE_COUNT]) -> Result<Self, PipelineError> {
        Self::new_with_sink_trigger(specs, None)
    }

    pub fn new_with_sink_trigger(
        specs: [StageRuntimeSpec; STAGE_COUNT],
        sink_trigger: Option<SinkTriggerSpec>,
    ) -> Result<Self, PipelineError> {
        for spec in specs {
            if spec.service_rate_work_units_per_second == 0 {
                return Err(PipelineError::ZeroServiceRate);
            }
        }
        if let Some(trigger) = sink_trigger {
            if trigger.capacity_multiplier_basis_points == 0
                || trigger.capacity_multiplier_basis_points > BASIS_POINTS_PER_ONE
            {
                return Err(PipelineError::InvalidTriggerMultiplier);
            }
            if trigger.start >= trigger.end {
                return Err(PipelineError::InvalidTriggerInterval);
            }
        }

        let [source, transform, sink] = specs;
        let stages = [
            Stage::new(
                SOURCE_STAGE_ID,
                source.workers,
                source.queue_capacity_waiting,
            )?,
            Stage::new(
                TRANSFORM_STAGE_ID,
                transform.workers,
                transform.queue_capacity_waiting,
            )?,
            Stage::new(SINK_STAGE_ID, sink.workers, sink.queue_capacity_waiting)?,
        ];
        let nominal_rates_scaled_per_us = [
            u128::from(source.service_rate_work_units_per_second)
                * u128::from(BASIS_POINTS_PER_ONE),
            u128::from(transform.service_rate_work_units_per_second)
                * u128::from(BASIS_POINTS_PER_ONE),
            u128::from(sink.service_rate_work_units_per_second) * u128::from(BASIS_POINTS_PER_ONE),
        ];

        Ok(Self {
            stages,
            nominal_rates_scaled_per_us,
            effective_rates_scaled_per_us: nominal_rates_scaled_per_us,
            sink_trigger,
            trigger_active: false,
            active_services: std::array::from_fn(|_| HashMap::new()),
            known_service_epochs: std::array::from_fn(|_| HashMap::new()),
            queue_area_last_updated_at: [Tick::ZERO; STAGE_COUNT],
            summary: PipelineSummary::default(),
        })
    }

    pub fn schedule_trigger_boundaries(
        &self,
        scheduler: &mut Scheduler,
    ) -> Result<Vec<Event>, PipelineError> {
        let Some(trigger) = self.sink_trigger else {
            return Ok(Vec::new());
        };

        Ok(vec![
            scheduler.schedule_at(
                trigger.start,
                EventKind::TriggerBoundary {
                    transition: TriggerTransition::Start,
                },
            )?,
            scheduler.schedule_at(
                trigger.end,
                EventKind::TriggerBoundary {
                    transition: TriggerTransition::End,
                },
            )?,
        ])
    }

    /// Yeni bir attempt yalnız Source'dan hatta girebilir.
    pub fn admit_source(
        &mut self,
        scheduler: &mut Scheduler,
        now: Tick,
        job: StageJob,
    ) -> Result<ScheduledAdmission, PipelineError> {
        let admission = self.admit_stage(0, scheduler, now, job)?;

        self.summary.source_arrival_attempts =
            self.summary.source_arrival_attempts.checked_add(1).ok_or(
                PipelineError::MetricCounterOverflow {
                    counter: "source_arrival_attempts",
                },
            )?;
        self.record_admission(0, now, admission.outcome)?;

        Ok(admission)
    }

    /// Bir stage'deki işi tamamlar, aynı stage'in FIFO kuyruğunu ilerletir ve
    /// tamamlanan işi varsa bir sonraki stage'e kabul ettirir.
    pub fn complete_stage(
        &mut self,
        scheduler: &mut Scheduler,
        now: Tick,
        stage_id: StageId,
        attempt_id: AttemptId,
    ) -> Result<PipelineStageCompletion, PipelineError> {
        let stage_index = Self::stage_index(stage_id)?;
        let service_epoch = *self.known_service_epochs[stage_index]
            .get(&attempt_id)
            .ok_or(PipelineError::UnknownServiceEpoch {
                attempt_id,
                stage_id,
            })?;

        match self.handle_stage_completion(scheduler, now, stage_id, attempt_id, service_epoch)? {
            StageCompletionHandling::Applied(completed) => Ok(completed),
            StageCompletionHandling::Stale { .. } => Err(PipelineError::ServiceProgressError),
        }
    }

    pub fn handle_stage_completion(
        &mut self,
        scheduler: &mut Scheduler,
        now: Tick,
        stage_id: StageId,
        attempt_id: AttemptId,
        event_epoch: u64,
    ) -> Result<StageCompletionHandling, PipelineError> {
        let stage_index = Self::stage_index(stage_id)?;
        let current_epoch = *self.known_service_epochs[stage_index]
            .get(&attempt_id)
            .ok_or(PipelineError::UnknownServiceEpoch {
                attempt_id,
                stage_id,
            })?;

        if event_epoch != current_epoch {
            self.summary.stale_completion_events =
                self.summary.stale_completion_events.checked_add(1).ok_or(
                    PipelineError::MetricCounterOverflow {
                        counter: "stale_completion_events",
                    },
                )?;
            return Ok(StageCompletionHandling::Stale {
                attempt_id,
                stage_id,
                event_epoch,
                current_epoch,
            });
        }

        let active = self.active_services[stage_index]
            .get(&attempt_id)
            .copied()
            .ok_or(PipelineError::MissingActiveService {
                attempt_id,
                stage_id,
            })?;
        if active.completion_at != now {
            return Err(PipelineError::UnexpectedCompletionTime {
                attempt_id,
                stage_id,
                expected: active.completion_at,
                actual: now,
            });
        }

        self.active_services[stage_index].remove(&attempt_id);
        let outcome = self.stages[stage_index].complete(attempt_id)?;
        let promoted_completion_event = outcome
            .started_next
            .map(|job| self.start_service(stage_index, scheduler, now, job))
            .transpose()?;
        self.record_completion(stage_index, now)?;

        let completed_job = outcome.completed;
        let downstream_admission = if stage_index + 1 < STAGE_COUNT {
            let next_stage_index = stage_index + 1;
            let admission = self.admit_stage(next_stage_index, scheduler, now, completed_job)?;
            self.record_admission(next_stage_index, now, admission.outcome)?;
            Some(admission)
        } else {
            self.summary.pipeline_completions =
                self.summary.pipeline_completions.checked_add(1).ok_or(
                    PipelineError::MetricCounterOverflow {
                        counter: "pipeline_completions",
                    },
                )?;
            self.summary.last_pipeline_completion_at_us = Some(now.as_u64());
            None
        };

        Ok(StageCompletionHandling::Applied(PipelineStageCompletion {
            completed_stage: stage_id,
            completed_job,
            promoted_completion_event,
            downstream_admission,
            pipeline_completed: stage_index == STAGE_COUNT - 1,
        }))
    }

    pub fn handle_trigger_boundary(
        &mut self,
        scheduler: &mut Scheduler,
        now: Tick,
        transition: TriggerTransition,
    ) -> Result<(), PipelineError> {
        let trigger = self
            .sink_trigger
            .ok_or(PipelineError::UnexpectedTriggerTransition {
                transition,
                at: now,
            })?;

        let new_rate = match transition {
            TriggerTransition::Start if now == trigger.start && !self.trigger_active => {
                self.trigger_active = true;
                (self.nominal_rates_scaled_per_us[2] / u128::from(BASIS_POINTS_PER_ONE))
                    * u128::from(trigger.capacity_multiplier_basis_points)
            }
            TriggerTransition::End if now == trigger.end && self.trigger_active => {
                self.trigger_active = false;
                self.nominal_rates_scaled_per_us[2]
            }
            _ => {
                return Err(PipelineError::UnexpectedTriggerTransition {
                    transition,
                    at: now,
                });
            }
        };

        self.change_sink_rate(scheduler, now, new_rate)
    }

    pub fn stage(&self, stage_id: StageId) -> Result<&Stage, PipelineError> {
        Ok(&self.stages[Self::stage_index(stage_id)?])
    }

    pub const fn summary(&self) -> PipelineSummary {
        self.summary
    }

    pub fn summary_at(&self, now: Tick) -> Result<PipelineSummary, PipelineError> {
        let mut summary = self.summary;
        for stage_index in 0..STAGE_COUNT {
            let elapsed_us = now
                .as_u64()
                .checked_sub(self.queue_area_last_updated_at[stage_index].as_u64())
                .ok_or(PipelineError::ServiceProgressError)?;
            let tail_area = summary.stages[stage_index]
                .waiting
                .checked_mul(elapsed_us)
                .ok_or(PipelineError::QueueAreaOverflow)?;
            summary.stages[stage_index].waiting_area_slot_us = summary.stages[stage_index]
                .waiting_area_slot_us
                .checked_add(tail_area)
                .ok_or(PipelineError::QueueAreaOverflow)?;
        }
        Ok(summary)
    }

    fn admit_stage(
        &mut self,
        stage_index: usize,
        scheduler: &mut Scheduler,
        now: Tick,
        job: StageJob,
    ) -> Result<ScheduledAdmission, PipelineError> {
        let stage_id = self.stages[stage_index].id();
        let outcome = self.stages[stage_index].admit(job);
        let completion_event = match outcome {
            AdmissionOutcome::Started(started) => {
                Some(self.start_service(stage_index, scheduler, now, started)?)
            }
            AdmissionOutcome::Queued(_) | AdmissionOutcome::Rejected(_) => None,
        };

        Ok(ScheduledAdmission {
            stage_id,
            outcome,
            completion_event,
        })
    }

    fn start_service(
        &mut self,
        stage_index: usize,
        scheduler: &mut Scheduler,
        now: Tick,
        job: StageJob,
    ) -> Result<Event, PipelineError> {
        let stage_id = self.stages[stage_index].id();
        if self.active_services[stage_index].contains_key(&job.attempt_id)
            || self.known_service_epochs[stage_index].contains_key(&job.attempt_id)
        {
            return Err(PipelineError::DuplicateServiceRuntime {
                attempt_id: job.attempt_id,
                stage_id,
            });
        }

        let remaining_work_scaled = u128::from(job.service_demand_milli)
            .checked_mul(SCALED_WORK_PER_MILLI_WORK_UNIT)
            .ok_or(PipelineError::ServiceDurationOverflow)?;
        if remaining_work_scaled == 0 {
            return Err(PipelineError::ZeroServiceDemand);
        }
        let rate_scaled_per_us = self.effective_rates_scaled_per_us[stage_index];
        let delay_us = duration_from_scaled_work(remaining_work_scaled, rate_scaled_per_us)?;
        let event = scheduler.schedule_after(
            now,
            delay_us,
            EventKind::StageCompletion {
                attempt_id: job.attempt_id,
                stage_id,
                service_epoch: job.service_epoch,
            },
        )?;

        self.active_services[stage_index].insert(
            job.attempt_id,
            ActiveService {
                remaining_work_scaled,
                last_updated_at: now,
                rate_scaled_per_us,
                epoch: job.service_epoch,
                completion_at: event.at,
            },
        );
        self.known_service_epochs[stage_index].insert(job.attempt_id, job.service_epoch);
        Ok(event)
    }

    fn change_sink_rate(
        &mut self,
        scheduler: &mut Scheduler,
        now: Tick,
        new_rate_scaled_per_us: u128,
    ) -> Result<(), PipelineError> {
        let stage_index = usize::from(SINK_STAGE_ID.0);
        let old_rate = self.effective_rates_scaled_per_us[stage_index];
        if old_rate == new_rate_scaled_per_us {
            return Ok(());
        }
        if new_rate_scaled_per_us == 0 {
            return Err(PipelineError::ZeroServiceRate);
        }

        let mut attempt_ids: Vec<_> = self.active_services[stage_index].keys().copied().collect();
        attempt_ids.sort_unstable();

        for attempt_id in attempt_ids {
            let active = self.active_services[stage_index]
                .get(&attempt_id)
                .copied()
                .ok_or(PipelineError::MissingActiveService {
                    attempt_id,
                    stage_id: SINK_STAGE_ID,
                })?;
            let elapsed_us = now
                .as_u64()
                .checked_sub(active.last_updated_at.as_u64())
                .ok_or(PipelineError::ServiceProgressError)?;
            let processed_work = active
                .rate_scaled_per_us
                .checked_mul(u128::from(elapsed_us))
                .ok_or(PipelineError::ServiceProgressError)?;
            let remaining_work_scaled = active
                .remaining_work_scaled
                .checked_sub(processed_work)
                .filter(|remaining| *remaining > 0)
                .ok_or(PipelineError::ServiceProgressError)?;
            let epoch =
                active
                    .epoch
                    .checked_add(1)
                    .ok_or(PipelineError::MetricCounterOverflow {
                        counter: "service_epoch",
                    })?;
            let delay_us =
                duration_from_scaled_work(remaining_work_scaled, new_rate_scaled_per_us)?;
            let event = scheduler.schedule_after(
                now,
                delay_us,
                EventKind::StageCompletion {
                    attempt_id,
                    stage_id: SINK_STAGE_ID,
                    service_epoch: epoch,
                },
            )?;

            self.active_services[stage_index].insert(
                attempt_id,
                ActiveService {
                    remaining_work_scaled,
                    last_updated_at: now,
                    rate_scaled_per_us: new_rate_scaled_per_us,
                    epoch,
                    completion_at: event.at,
                },
            );
            self.known_service_epochs[stage_index].insert(attempt_id, epoch);
            self.summary.trigger_reschedules =
                self.summary.trigger_reschedules.checked_add(1).ok_or(
                    PipelineError::MetricCounterOverflow {
                        counter: "trigger_reschedules",
                    },
                )?;
        }

        self.effective_rates_scaled_per_us[stage_index] = new_rate_scaled_per_us;
        Ok(())
    }

    fn record_admission(
        &mut self,
        stage_index: usize,
        now: Tick,
        outcome: AdmissionOutcome,
    ) -> Result<(), PipelineError> {
        self.summary.stages[stage_index].admissions = self.summary.stages[stage_index]
            .admissions
            .checked_add(1)
            .ok_or(PipelineError::MetricCounterOverflow {
                counter: "stage_admissions",
            })?;
        if matches!(outcome, AdmissionOutcome::Rejected(_)) {
            self.summary.stages[stage_index].queue_rejections = self.summary.stages[stage_index]
                .queue_rejections
                .checked_add(1)
                .ok_or(PipelineError::MetricCounterOverflow {
                    counter: "queue_rejections",
                })?;
        }
        self.observe_occupancy(stage_index, now)
    }

    fn record_completion(&mut self, stage_index: usize, now: Tick) -> Result<(), PipelineError> {
        self.summary.stages[stage_index].completions = self.summary.stages[stage_index]
            .completions
            .checked_add(1)
            .ok_or(PipelineError::MetricCounterOverflow {
                counter: "stage_completions",
            })?;
        self.observe_occupancy(stage_index, now)
    }

    fn observe_occupancy(&mut self, stage_index: usize, now: Tick) -> Result<(), PipelineError> {
        let in_service = u64::try_from(self.stages[stage_index].in_service_len())
            .map_err(|_| PipelineError::OccupancyOverflow)?;
        let waiting = u64::try_from(self.stages[stage_index].waiting_len())
            .map_err(|_| PipelineError::OccupancyOverflow)?;
        let metrics = &mut self.summary.stages[stage_index];
        let elapsed_us = now
            .as_u64()
            .checked_sub(self.queue_area_last_updated_at[stage_index].as_u64())
            .ok_or(PipelineError::ServiceProgressError)?;
        let area_delta = metrics
            .waiting
            .checked_mul(elapsed_us)
            .ok_or(PipelineError::QueueAreaOverflow)?;

        metrics.waiting_area_slot_us = metrics
            .waiting_area_slot_us
            .checked_add(area_delta)
            .ok_or(PipelineError::QueueAreaOverflow)?;
        metrics.in_service = in_service;
        metrics.waiting = waiting;
        metrics.max_in_service = metrics.max_in_service.max(in_service);
        metrics.max_waiting = metrics.max_waiting.max(waiting);
        self.queue_area_last_updated_at[stage_index] = now;
        Ok(())
    }

    fn stage_index(stage_id: StageId) -> Result<usize, PipelineError> {
        let index = usize::from(stage_id.0);
        if index < STAGE_COUNT {
            Ok(index)
        } else {
            Err(PipelineError::UnknownStageId { stage_id })
        }
    }
}

fn duration_from_scaled_work(
    remaining_work_scaled: u128,
    rate_scaled_per_us: u128,
) -> Result<u64, PipelineError> {
    if remaining_work_scaled == 0 {
        return Err(PipelineError::ZeroServiceDemand);
    }
    if rate_scaled_per_us == 0 {
        return Err(PipelineError::ZeroServiceRate);
    }

    let whole_ticks = remaining_work_scaled / rate_scaled_per_us;
    let has_fraction = !remaining_work_scaled.is_multiple_of(rate_scaled_per_us);
    let duration = whole_ticks
        .checked_add(u128::from(has_fraction))
        .ok_or(PipelineError::ServiceDurationOverflow)?;
    u64::try_from(duration).map_err(|_| PipelineError::ServiceDurationOverflow)
}

/// Milli-work-unit cinsinden işi, verilen etkin hız altında integer microsecond'a
/// çevirir. Kesirli tick yukarı yuvarlanır; iş olduğundan erken tamamlanmaz.
pub fn service_duration_us(
    service_demand_milli: u64,
    service_rate_work_units_per_second: u64,
) -> Result<u64, PipelineError> {
    if service_demand_milli == 0 {
        return Err(PipelineError::ZeroServiceDemand);
    }
    if service_rate_work_units_per_second == 0 {
        return Err(PipelineError::ZeroServiceRate);
    }

    let numerator = service_demand_milli
        .checked_mul(MICROSECONDS_PER_MILLI_WORK_UNIT_AT_ONE_UNIT_PER_SECOND)
        .ok_or(PipelineError::ServiceDurationOverflow)?;
    let whole_ticks = numerator / service_rate_work_units_per_second;
    let has_fraction = numerator % service_rate_work_units_per_second != 0;

    whole_ticks
        .checked_add(u64::from(has_fraction))
        .ok_or(PipelineError::ServiceDurationOverflow)
}

fn schedule_completion(
    scheduler: &mut Scheduler,
    stage: &Stage,
    now: Tick,
    service_rate_work_units_per_second: u64,
    job: StageJob,
) -> Result<Event, PipelineError> {
    let delay_us =
        service_duration_us(job.service_demand_milli, service_rate_work_units_per_second)?;

    Ok(scheduler.schedule_after(
        now,
        delay_us,
        EventKind::StageCompletion {
            attempt_id: job.attempt_id,
            stage_id: stage.id(),
            service_epoch: job.service_epoch,
        },
    )?)
}

/// Job'ı stage'e kabul ettirir; yalnız gerçekten worker'da başlayan job için
/// completion event'i üretir.
pub fn admit_and_schedule_completion(
    stage: &mut Stage,
    scheduler: &mut Scheduler,
    now: Tick,
    service_rate_work_units_per_second: u64,
    job: StageJob,
) -> Result<ScheduledAdmission, PipelineError> {
    let outcome = stage.admit(job);
    let completion_event = match outcome {
        AdmissionOutcome::Started(started) => Some(schedule_completion(
            scheduler,
            stage,
            now,
            service_rate_work_units_per_second,
            started,
        )?),
        AdmissionOutcome::Queued(_) | AdmissionOutcome::Rejected(_) => None,
    };

    Ok(ScheduledAdmission {
        stage_id: stage.id(),
        outcome,
        completion_event,
    })
}

/// Çalışan job'ı tamamlar; FIFO kuyruğundan yeni bir job worker'a geçtiyse
/// yalnız o job için yeni completion event'i üretir.
pub fn complete_and_schedule_promoted(
    stage: &mut Stage,
    scheduler: &mut Scheduler,
    now: Tick,
    service_rate_work_units_per_second: u64,
    attempt_id: AttemptId,
) -> Result<ScheduledCompletion, PipelineError> {
    let outcome = stage.complete(attempt_id)?;
    let promoted_completion_event = outcome
        .started_next
        .map(|started| {
            schedule_completion(
                scheduler,
                stage,
                now,
                service_rate_work_units_per_second,
                started,
            )
        })
        .transpose()?;

    Ok(ScheduledCompletion {
        outcome,
        promoted_completion_event,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        Pipeline, PipelineError, PipelineSummary, SINK_STAGE_ID, SOURCE_STAGE_ID,
        ScheduledAdmission, SinkTriggerSpec, StageCompletionHandling, StageMetrics,
        StageRuntimeSpec, TRANSFORM_STAGE_ID, admit_and_schedule_completion,
        complete_and_schedule_promoted, duration_from_scaled_work, service_duration_us,
    };
    use crate::event::{Event, EventKind, TriggerTransition};
    use crate::scheduler::Scheduler;
    use crate::stage::{AdmissionOutcome, Stage, StageJob};
    use crate::types::{AttemptId, StageId, Tick};

    fn dispatch_pipeline_event(
        pipeline: &mut Pipeline,
        scheduler: &mut Scheduler,
        event: Event,
    ) -> Option<StageCompletionHandling> {
        match event.kind {
            EventKind::StageCompletion {
                attempt_id,
                stage_id,
                service_epoch,
            } => Some(
                pipeline
                    .handle_stage_completion(
                        scheduler,
                        event.at,
                        stage_id,
                        attempt_id,
                        service_epoch,
                    )
                    .unwrap(),
            ),
            EventKind::TriggerBoundary { transition } => {
                pipeline
                    .handle_trigger_boundary(scheduler, event.at, transition)
                    .unwrap();
                None
            }
            other => panic!("unexpected event in retry-free fixture: {other:?}"),
        }
    }

    fn fast_upstream_specs() -> [StageRuntimeSpec; 3] {
        [
            StageRuntimeSpec {
                workers: 2,
                queue_capacity_waiting: 2,
                service_rate_work_units_per_second: 1_000,
            },
            StageRuntimeSpec {
                workers: 2,
                queue_capacity_waiting: 2,
                service_rate_work_units_per_second: 1_000,
            },
            StageRuntimeSpec {
                workers: 1,
                queue_capacity_waiting: 2,
                service_rate_work_units_per_second: 100,
            },
        ]
    }

    fn run_single_job_with_trigger(trigger: Option<SinkTriggerSpec>) -> PipelineSummary {
        let mut pipeline = Pipeline::new_with_sink_trigger(fast_upstream_specs(), trigger).unwrap();
        let mut scheduler = Scheduler::new();
        pipeline
            .schedule_trigger_boundaries(&mut scheduler)
            .unwrap();
        pipeline
            .admit_source(
                &mut scheduler,
                Tick::ZERO,
                StageJob::new(AttemptId(1), 1_000),
            )
            .unwrap();

        while let Some(event) = scheduler.pop_next() {
            dispatch_pipeline_event(&mut pipeline, &mut scheduler, event);
        }
        pipeline.summary()
    }

    fn run_three_request_fifo_fixture() -> (PipelineSummary, String) {
        let specs = [
            StageRuntimeSpec {
                workers: 1,
                queue_capacity_waiting: 2,
                service_rate_work_units_per_second: 100,
            },
            StageRuntimeSpec {
                workers: 1,
                queue_capacity_waiting: 2,
                service_rate_work_units_per_second: 100,
            },
            StageRuntimeSpec {
                workers: 1,
                queue_capacity_waiting: 2,
                service_rate_work_units_per_second: 100,
            },
        ];
        let mut pipeline = Pipeline::new(specs).unwrap();
        let mut scheduler = Scheduler::new();
        let first = StageJob::new(AttemptId(1), 1_000);
        let second = StageJob::new(AttemptId(2), 1_000);
        let third = StageJob::new(AttemptId(3), 1_000);

        assert_eq!(
            pipeline
                .admit_source(&mut scheduler, Tick::ZERO, first)
                .unwrap()
                .outcome,
            AdmissionOutcome::Started(first)
        );
        assert_eq!(
            pipeline
                .admit_source(&mut scheduler, Tick::ZERO, second)
                .unwrap()
                .outcome,
            AdmissionOutcome::Queued(second)
        );
        assert_eq!(
            pipeline
                .admit_source(&mut scheduler, Tick::ZERO, third)
                .unwrap()
                .outcome,
            AdmissionOutcome::Queued(third)
        );

        while let Some(event) = scheduler.pop_next() {
            match event.kind {
                EventKind::StageCompletion {
                    attempt_id,
                    stage_id,
                    ..
                } => {
                    pipeline
                        .complete_stage(&mut scheduler, event.at, stage_id, attempt_id)
                        .unwrap();
                }
                other => panic!("unexpected event in retry-free fixture: {other:?}"),
            }
        }

        let summary = pipeline.summary();
        (summary, summary.checksum_sha256())
    }

    #[test]
    fn service_duration_uses_integer_ceiling() {
        assert_eq!(service_duration_us(1_000, 100), Ok(10_000));
        assert_eq!(service_duration_us(1_000, 140), Ok(7_143));
        assert_eq!(
            service_duration_us(0, 100),
            Err(PipelineError::ZeroServiceDemand)
        );
        assert_eq!(
            service_duration_us(1_000, 0),
            Err(PipelineError::ZeroServiceRate)
        );
        assert_eq!(duration_from_scaled_work(10, 3), Ok(4));
    }

    #[test]
    fn only_a_started_job_receives_a_completion_event() {
        let mut stage = Stage::new(StageId(2), 1, 1).unwrap();
        let mut scheduler = Scheduler::new();
        let first = StageJob::new(AttemptId(1), 1_000);
        let second = StageJob::new(AttemptId(2), 1_000);
        let third = StageJob::new(AttemptId(3), 1_000);

        let started =
            admit_and_schedule_completion(&mut stage, &mut scheduler, Tick::new(50), 100, first)
                .unwrap();
        assert_eq!(started.outcome, AdmissionOutcome::Started(first));
        assert_eq!(started.completion_event.unwrap().at, Tick::new(10_050));
        assert_eq!(scheduler.len(), 1);

        assert_eq!(
            admit_and_schedule_completion(&mut stage, &mut scheduler, Tick::new(60), 100, second,)
                .unwrap(),
            ScheduledAdmission {
                stage_id: StageId(2),
                outcome: AdmissionOutcome::Queued(second),
                completion_event: None,
            }
        );
        assert_eq!(scheduler.len(), 1);

        assert_eq!(
            admit_and_schedule_completion(&mut stage, &mut scheduler, Tick::new(70), 100, third,)
                .unwrap(),
            ScheduledAdmission {
                stage_id: StageId(2),
                outcome: AdmissionOutcome::Rejected(third),
                completion_event: None,
            }
        );
        assert_eq!(scheduler.len(), 1);
    }

    #[test]
    fn completion_schedules_only_the_job_promoted_from_the_fifo_queue() {
        let mut stage = Stage::new(StageId(2), 1, 2).unwrap();
        let mut scheduler = Scheduler::new();
        let first = StageJob::new(AttemptId(1), 1_000);
        let second = StageJob::new(AttemptId(2), 500);

        admit_and_schedule_completion(&mut stage, &mut scheduler, Tick::ZERO, 100, first).unwrap();
        admit_and_schedule_completion(&mut stage, &mut scheduler, Tick::ZERO, 100, second).unwrap();

        let first_completion = scheduler.pop_next().unwrap();
        assert_eq!(first_completion.at, Tick::new(10_000));

        let completed = complete_and_schedule_promoted(
            &mut stage,
            &mut scheduler,
            first_completion.at,
            100,
            first.attempt_id,
        )
        .unwrap();

        let promoted_event = completed.promoted_completion_event.unwrap();
        assert_eq!(promoted_event.at, Tick::new(15_000));
        assert_eq!(
            promoted_event.kind,
            EventKind::StageCompletion {
                attempt_id: second.attempt_id,
                stage_id: StageId(2),
                service_epoch: 0,
            }
        );
        assert_eq!(scheduler.len(), 1);
    }

    #[test]
    fn one_request_flows_through_three_stages_at_hand_calculated_times() {
        let mut pipeline = Pipeline::new([
            StageRuntimeSpec {
                workers: 1,
                queue_capacity_waiting: 1,
                service_rate_work_units_per_second: 100,
            },
            StageRuntimeSpec {
                workers: 1,
                queue_capacity_waiting: 1,
                service_rate_work_units_per_second: 200,
            },
            StageRuntimeSpec {
                workers: 1,
                queue_capacity_waiting: 1,
                service_rate_work_units_per_second: 250,
            },
        ])
        .unwrap();
        let mut scheduler = Scheduler::new();
        let job = StageJob::new(AttemptId(1), 1_000);

        let admitted = pipeline
            .admit_source(&mut scheduler, Tick::ZERO, job)
            .unwrap();
        assert_eq!(admitted.outcome, AdmissionOutcome::Started(job));

        let source_completion = scheduler.pop_next().unwrap();
        assert_eq!(source_completion.at, Tick::new(10_000));
        assert_eq!(
            source_completion.kind,
            EventKind::StageCompletion {
                attempt_id: job.attempt_id,
                stage_id: SOURCE_STAGE_ID,
                service_epoch: 0,
            }
        );
        let source_result = pipeline
            .complete_stage(
                &mut scheduler,
                source_completion.at,
                SOURCE_STAGE_ID,
                job.attempt_id,
            )
            .unwrap();
        assert!(!source_result.pipeline_completed);
        assert_eq!(
            source_result.downstream_admission.unwrap().outcome,
            AdmissionOutcome::Started(job)
        );
        let source = pipeline.stage(SOURCE_STAGE_ID).unwrap();
        assert_eq!(source.in_service_len(), 0);
        assert_eq!(source.waiting_len(), 0);

        let transform = pipeline.stage(TRANSFORM_STAGE_ID).unwrap();
        assert_eq!(transform.in_service_len(), 1);
        assert_eq!(transform.waiting_len(), 0);

        let transform_completion = scheduler.pop_next().unwrap();
        assert_eq!(transform_completion.at, Tick::new(15_000));
        assert_eq!(
            transform_completion.kind,
            EventKind::StageCompletion {
                attempt_id: job.attempt_id,
                stage_id: TRANSFORM_STAGE_ID,
                service_epoch: 0,
            }
        );
        let transform_result = pipeline
            .complete_stage(
                &mut scheduler,
                transform_completion.at,
                TRANSFORM_STAGE_ID,
                job.attempt_id,
            )
            .unwrap();
        assert!(!transform_result.pipeline_completed);
        assert_eq!(
            transform_result.downstream_admission.unwrap().outcome,
            AdmissionOutcome::Started(job)
        );

        let sink_completion = scheduler.pop_next().unwrap();
        assert_eq!(sink_completion.at, Tick::new(19_000));
        assert_eq!(
            sink_completion.kind,
            EventKind::StageCompletion {
                attempt_id: job.attempt_id,
                stage_id: SINK_STAGE_ID,
                service_epoch: 0,
            }
        );
        let sink_result = pipeline
            .complete_stage(
                &mut scheduler,
                sink_completion.at,
                SINK_STAGE_ID,
                job.attempt_id,
            )
            .unwrap();
        assert!(sink_result.pipeline_completed);
        assert_eq!(sink_result.downstream_admission, None);
        assert!(scheduler.is_empty());

        for stage_id in [SOURCE_STAGE_ID, TRANSFORM_STAGE_ID, SINK_STAGE_ID] {
            let stage = pipeline.stage(stage_id).unwrap();
            assert_eq!(stage.in_service_len(), 0);
            assert_eq!(stage.waiting_len(), 0);
        }
    }

    #[test]
    fn counters_and_occupancy_stay_within_capacity_for_three_requests() {
        let (summary, _) = run_three_request_fifo_fixture();

        assert_eq!(
            summary,
            PipelineSummary {
                source_arrival_attempts: 3,
                pipeline_completions: 3,
                last_pipeline_completion_at_us: Some(50_000),
                stale_completion_events: 0,
                trigger_reschedules: 0,
                stages: [
                    StageMetrics {
                        admissions: 3,
                        completions: 3,
                        queue_rejections: 0,
                        in_service: 0,
                        waiting: 0,
                        max_in_service: 1,
                        max_waiting: 2,
                        waiting_area_slot_us: 30_000,
                    },
                    StageMetrics {
                        admissions: 3,
                        completions: 3,
                        queue_rejections: 0,
                        in_service: 0,
                        waiting: 0,
                        max_in_service: 1,
                        max_waiting: 1,
                        waiting_area_slot_us: 0,
                    },
                    StageMetrics {
                        admissions: 3,
                        completions: 3,
                        queue_rejections: 0,
                        in_service: 0,
                        waiting: 0,
                        max_in_service: 1,
                        max_waiting: 1,
                        waiting_area_slot_us: 0,
                    },
                ],
            }
        );

        for stage in summary.stages {
            assert!(stage.in_service <= 1);
            assert!(stage.waiting <= 2);
            assert!(stage.max_in_service <= 1);
            assert!(stage.max_waiting <= 2);
        }
    }

    #[test]
    fn same_fixture_produces_the_same_summary_and_checksum() {
        let first = run_three_request_fifo_fixture();
        let second = run_three_request_fifo_fixture();

        assert_eq!(first, second);
        assert_eq!(first.1.len(), 64);
    }

    #[test]
    fn full_queue_reports_the_rejecting_stage() {
        let mut source_pipeline = Pipeline::new([
            StageRuntimeSpec {
                workers: 1,
                queue_capacity_waiting: 1,
                service_rate_work_units_per_second: 100,
            },
            StageRuntimeSpec {
                workers: 1,
                queue_capacity_waiting: 1,
                service_rate_work_units_per_second: 100,
            },
            StageRuntimeSpec {
                workers: 1,
                queue_capacity_waiting: 1,
                service_rate_work_units_per_second: 100,
            },
        ])
        .unwrap();
        let mut source_scheduler = Scheduler::new();
        for attempt_id in [AttemptId(1), AttemptId(2)] {
            source_pipeline
                .admit_source(
                    &mut source_scheduler,
                    Tick::ZERO,
                    StageJob::new(attempt_id, 1_000),
                )
                .unwrap();
        }
        let source_rejection = source_pipeline
            .admit_source(
                &mut source_scheduler,
                Tick::ZERO,
                StageJob::new(AttemptId(3), 1_000),
            )
            .unwrap();
        assert_eq!(
            source_rejection.queue_rejected_stage(),
            Some(SOURCE_STAGE_ID)
        );
        assert_eq!(source_pipeline.summary().stages[0].queue_rejections, 1);

        let mut transform_pipeline = Pipeline::new([
            StageRuntimeSpec {
                workers: 3,
                queue_capacity_waiting: 1,
                service_rate_work_units_per_second: 1_000,
            },
            StageRuntimeSpec {
                workers: 1,
                queue_capacity_waiting: 1,
                service_rate_work_units_per_second: 100,
            },
            StageRuntimeSpec {
                workers: 1,
                queue_capacity_waiting: 1,
                service_rate_work_units_per_second: 100,
            },
        ])
        .unwrap();
        let mut transform_scheduler = Scheduler::new();
        for attempt_id in [AttemptId(1), AttemptId(2), AttemptId(3)] {
            transform_pipeline
                .admit_source(
                    &mut transform_scheduler,
                    Tick::ZERO,
                    StageJob::new(attempt_id, 1_000),
                )
                .unwrap();
        }
        let mut transform_rejection = None;
        while transform_rejection.is_none() {
            let event = transform_scheduler.pop_next().unwrap();
            if let Some(StageCompletionHandling::Applied(completed)) =
                dispatch_pipeline_event(&mut transform_pipeline, &mut transform_scheduler, event)
            {
                transform_rejection = completed
                    .downstream_admission
                    .and_then(ScheduledAdmission::queue_rejected_stage);
            }
        }
        assert_eq!(transform_rejection, Some(TRANSFORM_STAGE_ID));

        let mut sink_pipeline = Pipeline::new([
            StageRuntimeSpec {
                workers: 3,
                queue_capacity_waiting: 1,
                service_rate_work_units_per_second: 1_000,
            },
            StageRuntimeSpec {
                workers: 3,
                queue_capacity_waiting: 1,
                service_rate_work_units_per_second: 1_000,
            },
            StageRuntimeSpec {
                workers: 1,
                queue_capacity_waiting: 1,
                service_rate_work_units_per_second: 100,
            },
        ])
        .unwrap();
        let mut sink_scheduler = Scheduler::new();
        for attempt_id in [AttemptId(1), AttemptId(2), AttemptId(3)] {
            sink_pipeline
                .admit_source(
                    &mut sink_scheduler,
                    Tick::ZERO,
                    StageJob::new(attempt_id, 1_000),
                )
                .unwrap();
        }
        let mut sink_rejection = None;
        while sink_rejection.is_none() {
            let event = sink_scheduler.pop_next().unwrap();
            if let Some(StageCompletionHandling::Applied(completed)) =
                dispatch_pipeline_event(&mut sink_pipeline, &mut sink_scheduler, event)
            {
                sink_rejection = completed
                    .downstream_admission
                    .and_then(ScheduledAdmission::queue_rejected_stage);
            }
        }
        assert_eq!(sink_rejection, Some(SINK_STAGE_ID));
    }

    #[test]
    fn sink_trigger_reschedules_remaining_work_and_stale_events_are_no_ops() {
        let trigger = SinkTriggerSpec {
            start: Tick::new(4_000),
            end: Tick::new(8_000),
            capacity_multiplier_basis_points: 5_000,
        };
        let mut pipeline =
            Pipeline::new_with_sink_trigger(fast_upstream_specs(), Some(trigger)).unwrap();
        let mut scheduler = Scheduler::new();
        let boundaries = pipeline
            .schedule_trigger_boundaries(&mut scheduler)
            .unwrap();
        assert_eq!(boundaries[0].at, Tick::new(4_000));
        assert_eq!(boundaries[1].at, Tick::new(8_000));
        assert_eq!(
            boundaries[0].kind,
            EventKind::TriggerBoundary {
                transition: TriggerTransition::Start,
            }
        );
        assert_eq!(
            boundaries[1].kind,
            EventKind::TriggerBoundary {
                transition: TriggerTransition::End,
            }
        );
        for attempt_id in [AttemptId(1), AttemptId(2)] {
            pipeline
                .admit_source(&mut scheduler, Tick::ZERO, StageJob::new(attempt_id, 1_000))
                .unwrap();
        }

        let mut sink_completion_times = Vec::new();
        while let Some(event) = scheduler.pop_next() {
            match event.kind {
                EventKind::TriggerBoundary { transition } => {
                    pipeline
                        .handle_trigger_boundary(&mut scheduler, event.at, transition)
                        .unwrap();
                    if transition == TriggerTransition::End {
                        let sink = pipeline.stage(SINK_STAGE_ID).unwrap();
                        assert_eq!(sink.in_service_len(), 1);
                        assert_eq!(sink.waiting_len(), 1);
                    }
                }
                EventKind::StageCompletion {
                    attempt_id,
                    stage_id,
                    service_epoch,
                } => {
                    let before = (
                        pipeline.stage(stage_id).unwrap().in_service_len(),
                        pipeline.stage(stage_id).unwrap().waiting_len(),
                    );
                    let handled = pipeline
                        .handle_stage_completion(
                            &mut scheduler,
                            event.at,
                            stage_id,
                            attempt_id,
                            service_epoch,
                        )
                        .unwrap();
                    match handled {
                        StageCompletionHandling::Applied(completed)
                            if completed.completed_stage == SINK_STAGE_ID =>
                        {
                            sink_completion_times.push(event.at)
                        }
                        StageCompletionHandling::Stale { .. } => {
                            let after = (
                                pipeline.stage(stage_id).unwrap().in_service_len(),
                                pipeline.stage(stage_id).unwrap().waiting_len(),
                            );
                            assert_eq!(before, after);
                        }
                        StageCompletionHandling::Applied(_) => {}
                    }
                }
                other => panic!("unexpected event in trigger fixture: {other:?}"),
            }
        }

        assert_eq!(
            sink_completion_times,
            vec![Tick::new(14_000), Tick::new(24_000)]
        );
        let summary = pipeline.summary();
        assert_eq!(summary.pipeline_completions, 2);
        assert_eq!(summary.trigger_reschedules, 2);
        assert_eq!(summary.stale_completion_events, 2);
        assert_eq!(summary.stages[2].waiting_area_slot_us, 12_000);
        for stage in summary.stages {
            assert_eq!(stage.admissions, 2);
            assert_eq!(stage.completions, 2);
            assert_eq!(stage.queue_rejections, 0);
            assert_eq!(stage.in_service, 0);
            assert_eq!(stage.waiting, 0);
        }
    }

    #[test]
    fn one_x_trigger_is_equivalent_to_no_trigger() {
        let no_trigger = run_single_job_with_trigger(None);
        let one_x_trigger = run_single_job_with_trigger(Some(SinkTriggerSpec {
            start: Tick::new(4_000),
            end: Tick::new(8_000),
            capacity_multiplier_basis_points: 10_000,
        }));

        assert_eq!(one_x_trigger, no_trigger);
        assert_eq!(
            one_x_trigger.checksum_sha256(),
            no_trigger.checksum_sha256()
        );
    }
}
