use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::event::{Event, EventKind};
use crate::scheduler::{Scheduler, SchedulerError};
use crate::stage::{AdmissionOutcome, CompletionOutcome, Stage, StageError, StageJob};
use crate::types::{AttemptId, StageId, Tick};

const MICROSECONDS_PER_MILLI_WORK_UNIT_AT_ONE_UNIT_PER_SECOND: u64 = 1_000;
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
    #[error(transparent)]
    Stage(#[from] StageError),
    #[error(transparent)]
    Scheduler(#[from] SchedulerError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduledAdmission {
    pub outcome: AdmissionOutcome,
    pub completion_event: Option<Event>,
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

/// Tek bir stage için sayılan olayları ve gözlenen occupancy değerlerini tutar.
/// `occupancy`, worker'da çalışan veya kuyrukta bekleyen iş sayısıdır.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StageMetrics {
    pub admissions: u64,
    pub completions: u64,
    pub in_service: u64,
    pub waiting: u64,
    pub max_in_service: u64,
    pub max_waiting: u64,
}

/// Retry'siz pipeline koşusunun küçük, deterministik özeti.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PipelineSummary {
    pub logical_arrivals: u64,
    pub pipeline_completions: u64,
    pub last_pipeline_completion_at_us: Option<u64>,
    pub stages: [StageMetrics; STAGE_COUNT],
}

impl PipelineSummary {
    /// Alanları sabit sırada SHA-256'e besler. Böylece JSON biçimlendirmesi veya
    /// makinenin mimarisi checksum'u değiştirmez.
    pub fn checksum_sha256(self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"retrycascade-pipeline-summary-v1\0");
        hasher.update(self.logical_arrivals.to_le_bytes());
        hasher.update(self.pipeline_completions.to_le_bytes());

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
                stage.in_service,
                stage.waiting,
                stage.max_in_service,
                stage.max_waiting,
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

/// Retry ve trigger olmadan çalışan sabit Source -> Transform -> Sink hattı.
///
/// Üç elemanın her biri aynı generic `Stage` tipidir. Dizideki sıra, stage
/// kimliğiyle aynıdır: 0 Source, 1 Transform, 2 Sink.
#[derive(Debug)]
pub struct Pipeline {
    stages: [Stage; STAGE_COUNT],
    service_rates_work_units_per_second: [u64; STAGE_COUNT],
    summary: PipelineSummary,
}

impl Pipeline {
    pub fn new(specs: [StageRuntimeSpec; STAGE_COUNT]) -> Result<Self, PipelineError> {
        for spec in specs {
            if spec.service_rate_work_units_per_second == 0 {
                return Err(PipelineError::ZeroServiceRate);
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

        Ok(Self {
            stages,
            service_rates_work_units_per_second: [
                source.service_rate_work_units_per_second,
                transform.service_rate_work_units_per_second,
                sink.service_rate_work_units_per_second,
            ],
            summary: PipelineSummary::default(),
        })
    }

    /// Yeni bir attempt yalnız Source'dan hatta girebilir.
    pub fn admit_source(
        &mut self,
        scheduler: &mut Scheduler,
        now: Tick,
        job: StageJob,
    ) -> Result<ScheduledAdmission, PipelineError> {
        let admission = admit_and_schedule_completion(
            &mut self.stages[0],
            scheduler,
            now,
            self.service_rates_work_units_per_second[0],
            job,
        )?;

        self.summary.logical_arrivals = self.summary.logical_arrivals.checked_add(1).ok_or(
            PipelineError::MetricCounterOverflow {
                counter: "logical_arrivals",
            },
        )?;
        self.record_admission(0)?;

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
        let completed = complete_and_schedule_promoted(
            &mut self.stages[stage_index],
            scheduler,
            now,
            self.service_rates_work_units_per_second[stage_index],
            attempt_id,
        )?;
        self.record_completion(stage_index)?;

        let completed_job = completed.outcome.completed;
        let downstream_admission = if stage_index + 1 < STAGE_COUNT {
            let next_stage_index = stage_index + 1;
            let admission = admit_and_schedule_completion(
                &mut self.stages[stage_index + 1],
                scheduler,
                now,
                self.service_rates_work_units_per_second[stage_index + 1],
                completed_job,
            )?;
            self.record_admission(next_stage_index)?;
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

        Ok(PipelineStageCompletion {
            completed_stage: stage_id,
            completed_job,
            promoted_completion_event: completed.promoted_completion_event,
            downstream_admission,
            pipeline_completed: stage_index == STAGE_COUNT - 1,
        })
    }

    pub fn stage(&self, stage_id: StageId) -> Result<&Stage, PipelineError> {
        Ok(&self.stages[Self::stage_index(stage_id)?])
    }

    pub const fn summary(&self) -> PipelineSummary {
        self.summary
    }

    fn record_admission(&mut self, stage_index: usize) -> Result<(), PipelineError> {
        self.summary.stages[stage_index].admissions = self.summary.stages[stage_index]
            .admissions
            .checked_add(1)
            .ok_or(PipelineError::MetricCounterOverflow {
                counter: "stage_admissions",
            })?;
        self.observe_occupancy(stage_index)
    }

    fn record_completion(&mut self, stage_index: usize) -> Result<(), PipelineError> {
        self.summary.stages[stage_index].completions = self.summary.stages[stage_index]
            .completions
            .checked_add(1)
            .ok_or(PipelineError::MetricCounterOverflow {
                counter: "stage_completions",
            })?;
        self.observe_occupancy(stage_index)
    }

    fn observe_occupancy(&mut self, stage_index: usize) -> Result<(), PipelineError> {
        let in_service = u64::try_from(self.stages[stage_index].in_service_len())
            .map_err(|_| PipelineError::OccupancyOverflow)?;
        let waiting = u64::try_from(self.stages[stage_index].waiting_len())
            .map_err(|_| PipelineError::OccupancyOverflow)?;
        let metrics = &mut self.summary.stages[stage_index];

        metrics.in_service = in_service;
        metrics.waiting = waiting;
        metrics.max_in_service = metrics.max_in_service.max(in_service);
        metrics.max_waiting = metrics.max_waiting.max(waiting);
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
        ScheduledAdmission, StageMetrics, StageRuntimeSpec, TRANSFORM_STAGE_ID,
        admit_and_schedule_completion, complete_and_schedule_promoted, service_duration_us,
    };
    use crate::event::EventKind;
    use crate::scheduler::Scheduler;
    use crate::stage::{AdmissionOutcome, Stage, StageJob};
    use crate::types::{AttemptId, StageId, Tick};

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
                outcome: AdmissionOutcome::Queued(second),
                completion_event: None,
            }
        );
        assert_eq!(scheduler.len(), 1);

        assert_eq!(
            admit_and_schedule_completion(&mut stage, &mut scheduler, Tick::new(70), 100, third,)
                .unwrap(),
            ScheduledAdmission {
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
                logical_arrivals: 3,
                pipeline_completions: 3,
                last_pipeline_completion_at_us: Some(50_000),
                stages: [
                    StageMetrics {
                        admissions: 3,
                        completions: 3,
                        in_service: 0,
                        waiting: 0,
                        max_in_service: 1,
                        max_waiting: 2,
                    },
                    StageMetrics {
                        admissions: 3,
                        completions: 3,
                        in_service: 0,
                        waiting: 0,
                        max_in_service: 1,
                        max_waiting: 1,
                    },
                    StageMetrics {
                        admissions: 3,
                        completions: 3,
                        in_service: 0,
                        waiting: 0,
                        max_in_service: 1,
                        max_waiting: 1,
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
}
