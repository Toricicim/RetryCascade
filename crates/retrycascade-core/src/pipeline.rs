use thiserror::Error;

use crate::event::{Event, EventKind};
use crate::scheduler::{Scheduler, SchedulerError};
use crate::stage::{AdmissionOutcome, CompletionOutcome, Stage, StageError, StageJob};
use crate::types::{AttemptId, Tick};

const MICROSECONDS_PER_MILLI_WORK_UNIT_AT_ONE_UNIT_PER_SECOND: u64 = 1_000;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PipelineError {
    #[error("service demand must be positive")]
    ZeroServiceDemand,
    #[error("service rate must be positive")]
    ZeroServiceRate,
    #[error("service duration arithmetic overflow")]
    ServiceDurationOverflow,
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
        PipelineError, ScheduledAdmission, admit_and_schedule_completion,
        complete_and_schedule_promoted, service_duration_us,
    };
    use crate::event::EventKind;
    use crate::scheduler::Scheduler;
    use crate::stage::{AdmissionOutcome, Stage, StageJob};
    use crate::types::{AttemptId, StageId, Tick};

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
}
