use std::collections::VecDeque;

use thiserror::Error;

use crate::types::{AttemptId, StageId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageJob {
    pub attempt_id: AttemptId,
    pub service_demand_milli: u64,
    pub service_epoch: u64,
}

impl StageJob {
    pub const fn new(attempt_id: AttemptId, service_demand_milli: u64) -> Self {
        Self {
            attempt_id,
            service_demand_milli,
            service_epoch: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionOutcome {
    Started(StageJob),
    Queued(StageJob),
    Rejected(StageJob),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StageError {
    #[error("worker capacity must be positive")]
    ZeroWorkerCapacity,
    #[error("waiting queue capacity must be positive")]
    ZeroWaitingCapacity,
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
    use super::{AdmissionOutcome, Stage, StageJob};
    use crate::types::{AttemptId, StageId};

    #[test]
    fn admitted_job_starts_when_worker_is_available() {
        let mut stage = Stage::new(StageId(0), 1, 2).unwrap();
        let job = StageJob::new(AttemptId(1), 1_000);

        assert_eq!(stage.admit(job), AdmissionOutcome::Started(job));
        assert_eq!(stage.in_service_len(), 1);
        assert_eq!(stage.waiting_len(), 0);
    }
}
