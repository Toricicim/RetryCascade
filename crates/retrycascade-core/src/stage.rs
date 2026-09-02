use std::collections::VecDeque;

use thiserror::Error;

use crate::types::{AttemptId, StageId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageJob {
    pub attempt_id: AttemptId,
    pub service_demands_milli: [u64; 3],
    pub service_epoch: u64,
}

impl StageJob {
    pub const fn new(attempt_id: AttemptId, service_demand_milli: u64) -> Self {
        Self {
            attempt_id,
            service_demands_milli: [service_demand_milli; 3],
            service_epoch: 0,
        }
    }

    pub const fn with_stage_demands(
        attempt_id: AttemptId,
        service_demands_milli: [u64; 3],
    ) -> Self {
        Self {
            attempt_id,
            service_demands_milli,
            service_epoch: 0,
        }
    }

    pub fn demand_for_stage(self, stage_id: StageId) -> Option<u64> {
        self.service_demands_milli
            .get(usize::from(stage_id.0))
            .copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionOutcome {
    Started(StageJob),
    Queued(StageJob),
    Rejected(StageJob),
}

/// Bir worker'daki iş tamamlandığında stage'in yaptığı iki değişikliği birlikte döndürür.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompletionOutcome {
    pub completed: StageJob,
    pub started_next: Option<StageJob>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StageError {
    #[error("worker capacity must be positive")]
    ZeroWorkerCapacity,
    #[error("waiting queue capacity must be positive")]
    ZeroWaitingCapacity,
    #[error("attempt {attempt_id:?} is not currently in service")]
    AttemptNotInService { attempt_id: AttemptId },
}

/// Bir pipeline aşamasının çalışan işlerini ve FIFO bekleme kuyruğunu tutar.
#[derive(Debug)]
pub struct Stage {
    id: StageId,
    worker_capacity: usize,
    queue_capacity_waiting: usize,
    in_service: Vec<StageJob>,
    waiting: VecDeque<StageJob>,
}

impl Stage {
    pub fn new(
        id: StageId,
        worker_capacity: usize,
        queue_capacity_waiting: usize,
    ) -> Result<Self, StageError> {
        if worker_capacity == 0 {
            return Err(StageError::ZeroWorkerCapacity);
        }
        if queue_capacity_waiting == 0 {
            return Err(StageError::ZeroWaitingCapacity);
        }

        Ok(Self {
            id,
            worker_capacity,
            queue_capacity_waiting,
            in_service: Vec::with_capacity(worker_capacity),
            waiting: VecDeque::with_capacity(queue_capacity_waiting),
        })
    }

    pub fn admit(&mut self, job: StageJob) -> AdmissionOutcome {
        if self.in_service.len() < self.worker_capacity {
            self.in_service.push(job);
            return AdmissionOutcome::Started(job);
        }

        if self.waiting.len() < self.queue_capacity_waiting {
            self.waiting.push_back(job);
            return AdmissionOutcome::Queued(job);
        }

        AdmissionOutcome::Rejected(job)
    }

    /// Çalışan attempt'i tamamlar ve varsa FIFO kuyruğunun başındaki işi başlatır.
    pub fn complete(&mut self, attempt_id: AttemptId) -> Result<CompletionOutcome, StageError> {
        let completed_position = self
            .in_service
            .iter()
            .position(|job| job.attempt_id == attempt_id)
            .ok_or(StageError::AttemptNotInService { attempt_id })?;

        let completed = self.in_service.remove(completed_position);
        let started_next = self.waiting.pop_front();

        if let Some(job) = started_next {
            self.in_service.push(job);
        }

        debug_assert!(self.in_service.len() <= self.worker_capacity);
        debug_assert!(self.waiting.len() <= self.queue_capacity_waiting);

        Ok(CompletionOutcome {
            completed,
            started_next,
        })
    }

    pub const fn id(&self) -> StageId {
        self.id
    }

    pub fn in_service_len(&self) -> usize {
        self.in_service.len()
    }

    pub fn waiting_len(&self) -> usize {
        self.waiting.len()
    }

    pub fn worker_capacity(&self) -> usize {
        self.worker_capacity
    }

    pub fn queue_capacity_waiting(&self) -> usize {
        self.queue_capacity_waiting
    }
}

#[cfg(test)]
mod tests {
    use super::{AdmissionOutcome, CompletionOutcome, Stage, StageError, StageJob};
    use crate::types::{AttemptId, StageId};

    #[test]
    fn admitted_job_starts_when_worker_is_available() {
        let mut stage = Stage::new(StageId(0), 1, 2).unwrap();
        let job = StageJob::new(AttemptId(1), 1_000);

        assert_eq!(stage.admit(job), AdmissionOutcome::Started(job));
        assert_eq!(stage.in_service_len(), 1);
        assert_eq!(stage.waiting_len(), 0);
    }

    #[test]
    fn jobs_wait_and_are_rejected_when_worker_and_queue_are_full() {
        let mut stage = Stage::new(StageId(0), 1, 2).unwrap();
        let first = StageJob::new(AttemptId(1), 1_000);
        let second = StageJob::new(AttemptId(2), 1_000);
        let third = StageJob::new(AttemptId(3), 1_000);
        let fourth = StageJob::new(AttemptId(4), 1_000);

        assert_eq!(stage.admit(first), AdmissionOutcome::Started(first));
        assert_eq!(stage.admit(second), AdmissionOutcome::Queued(second));
        assert_eq!(stage.admit(third), AdmissionOutcome::Queued(third));
        assert_eq!(stage.admit(fourth), AdmissionOutcome::Rejected(fourth));
        assert_eq!(stage.in_service_len(), 1);
        assert_eq!(stage.waiting_len(), 2);
    }

    #[test]
    fn completion_promotes_waiting_jobs_in_fifo_order() {
        let mut stage = Stage::new(StageId(0), 1, 2).unwrap();
        let first = StageJob::new(AttemptId(1), 1_000);
        let second = StageJob::new(AttemptId(2), 1_000);
        let third = StageJob::new(AttemptId(3), 1_000);

        stage.admit(first);
        stage.admit(second);
        stage.admit(third);

        assert_eq!(
            stage.complete(first.attempt_id),
            Ok(CompletionOutcome {
                completed: first,
                started_next: Some(second),
            })
        );
        assert_eq!(stage.in_service_len(), 1);
        assert_eq!(stage.waiting_len(), 1);

        assert_eq!(
            stage.complete(second.attempt_id),
            Ok(CompletionOutcome {
                completed: second,
                started_next: Some(third),
            })
        );
        assert_eq!(stage.in_service_len(), 1);
        assert_eq!(stage.waiting_len(), 0);
    }

    #[test]
    fn completion_without_a_waiting_job_leaves_worker_idle() {
        let mut stage = Stage::new(StageId(0), 1, 2).unwrap();
        let job = StageJob::new(AttemptId(1), 1_000);
        stage.admit(job);

        assert_eq!(
            stage.complete(job.attempt_id),
            Ok(CompletionOutcome {
                completed: job,
                started_next: None,
            })
        );
        assert_eq!(stage.in_service_len(), 0);
        assert_eq!(stage.waiting_len(), 0);
    }

    #[test]
    fn completing_an_unknown_attempt_is_an_error_and_preserves_state() {
        let mut stage = Stage::new(StageId(0), 1, 2).unwrap();
        let running = StageJob::new(AttemptId(1), 1_000);
        let waiting = StageJob::new(AttemptId(2), 1_000);
        stage.admit(running);
        stage.admit(waiting);

        assert_eq!(
            stage.complete(AttemptId(99)),
            Err(StageError::AttemptNotInService {
                attempt_id: AttemptId(99),
            })
        );
        assert_eq!(stage.in_service_len(), 1);
        assert_eq!(stage.waiting_len(), 1);
    }
}
