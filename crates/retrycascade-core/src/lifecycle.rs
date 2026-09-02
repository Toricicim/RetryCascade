use std::collections::BTreeMap;

use thiserror::Error;

use crate::pipeline::{Pipeline, PipelineError, PipelineStageCompletion, ScheduledAdmission};
use crate::scheduler::{Scheduler, SchedulerError};
use crate::stage::{AdmissionOutcome, StageJob};
use crate::types::{AttemptId, LogicalRequestId, StageId, Tick};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleSpec {
    pub per_attempt_timeout_us: u64,
    pub overall_logical_deadline_us: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalTerminal {
    Succeeded {
        at: Tick,
        winning_attempt_id: AttemptId,
    },
    DeadlineAbandoned {
        at: Tick,
    },
    AttemptsExhausted {
        at: Tick,
    },
    OutstandingAtHorizon {
        at: Tick,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogicalRequest {
    pub id: LogicalRequestId,
    pub arrived_at: Tick,
    pub deadline_at: Tick,
    pub terminal: Option<LogicalTerminal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptTerminal {
    OnTimeSuccess { at: Tick },
    LateCompletion { at: Tick },
    DuplicateCompletion { at: Tick },
    QueueRejected { at: Tick, stage_id: StageId },
    OutstandingAtHorizon { at: Tick, timed_out: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attempt {
    pub id: AttemptId,
    pub logical_request_id: LogicalRequestId,
    pub attempt_no: u32,
    pub source_admitted: bool,
    pub timeout_at: Option<Tick>,
    pub timed_out_at: Option<Tick>,
    pub terminal: Option<AttemptTerminal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleAdmission {
    pub pipeline_admission: ScheduledAdmission,
    pub timeout_event: Option<crate::event::Event>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeoutHandling {
    TimedOut,
    NoOpTerminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeadlineHandling {
    Abandoned,
    NoOpSucceeded,
    NoOpTerminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptProgress {
    Advanced,
    OnTimeSuccess,
    LateCompletion,
    DuplicateCompletion,
    QueueRejected { stage_id: StageId },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LifecycleSummary {
    pub logical_arrivals: u64,
    pub logical_successes: u64,
    pub logical_deadline_abandoned: u64,
    pub logical_attempts_exhausted: u64,
    pub logical_outstanding_at_horizon: u64,
    pub physical_attempts: u64,
    pub attempt_timeouts: u64,
    pub attempt_on_time_successes: u64,
    pub attempt_late_completions: u64,
    pub attempt_duplicate_completions: u64,
    pub attempt_queue_rejections: u64,
    pub attempt_outstanding_at_horizon: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConservationLedger {
    pub logical_total: u64,
    pub logical_terminal: u64,
    pub logical_open: u64,
    pub attempt_total: u64,
    pub attempt_terminal: u64,
    pub attempt_open: u64,
}

impl ConservationLedger {
    pub const fn closes(self) -> bool {
        self.logical_total == self.logical_terminal + self.logical_open
            && self.attempt_total == self.attempt_terminal + self.attempt_open
    }

    pub const fn all_terminal(self) -> bool {
        self.closes() && self.logical_open == 0 && self.attempt_open == 0
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LifecycleError {
    #[error("timeout and deadline durations must be positive")]
    ZeroDuration,
    #[error("lifecycle time arithmetic overflow")]
    TimeOverflow,
    #[error("attempt number must start at one")]
    ZeroAttemptNumber,
    #[error("logical request {0:?} already exists")]
    DuplicateLogicalRequest(LogicalRequestId),
    #[error("attempt {0:?} already exists")]
    DuplicateAttempt(AttemptId),
    #[error("unknown logical request {0:?}")]
    UnknownLogicalRequest(LogicalRequestId),
    #[error("unknown attempt {0:?}")]
    UnknownAttempt(AttemptId),
    #[error("logical request {0:?} is already terminal")]
    LogicalAlreadyTerminal(LogicalRequestId),
    #[error("attempt {0:?} is already terminal")]
    AttemptAlreadyTerminal(AttemptId),
    #[error("event for {entity} was expected at {expected:?}, got {actual:?}")]
    UnexpectedEventTime {
        entity: &'static str,
        expected: Tick,
        actual: Tick,
    },
    #[error("lifecycle counter overflow: {0}")]
    CounterOverflow(&'static str),
    #[error("completion occurred after an unprocessed logical deadline")]
    DeadlineOrderingViolation,
    #[error("lifecycle conservation ledger does not close")]
    ConservationViolation,
    #[error(transparent)]
    Pipeline(#[from] PipelineError),
    #[error(transparent)]
    Scheduler(#[from] SchedulerError),
}

#[derive(Debug)]
pub struct LifecycleLedger {
    spec: LifecycleSpec,
    logical_requests: BTreeMap<LogicalRequestId, LogicalRequest>,
    attempts: BTreeMap<AttemptId, Attempt>,
    summary: LifecycleSummary,
}

impl LifecycleLedger {
    pub fn new(spec: LifecycleSpec) -> Result<Self, LifecycleError> {
        if spec.per_attempt_timeout_us == 0 || spec.overall_logical_deadline_us == 0 {
            return Err(LifecycleError::ZeroDuration);
        }
        Ok(Self {
            spec,
            logical_requests: BTreeMap::new(),
            attempts: BTreeMap::new(),
            summary: LifecycleSummary::default(),
        })
    }

    pub fn register_logical_request(
        &mut self,
        scheduler: &mut Scheduler,
        now: Tick,
        logical_request_id: LogicalRequestId,
    ) -> Result<crate::event::Event, LifecycleError> {
        if self.logical_requests.contains_key(&logical_request_id) {
            return Err(LifecycleError::DuplicateLogicalRequest(logical_request_id));
        }
        let deadline_at = now
            .checked_add(self.spec.overall_logical_deadline_us)
            .ok_or(LifecycleError::TimeOverflow)?;
        let event = scheduler.schedule_at(
            deadline_at,
            crate::event::EventKind::LogicalDeadline { logical_request_id },
        )?;
        self.logical_requests.insert(
            logical_request_id,
            LogicalRequest {
                id: logical_request_id,
                arrived_at: now,
                deadline_at,
                terminal: None,
            },
        );
        Self::increment(&mut self.summary.logical_arrivals, "logical_arrivals")?;
        Ok(event)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn admit_attempt(
        &mut self,
        pipeline: &mut Pipeline,
        scheduler: &mut Scheduler,
        now: Tick,
        logical_request_id: LogicalRequestId,
        attempt_id: AttemptId,
        attempt_no: u32,
        service_demand_milli: u64,
    ) -> Result<LifecycleAdmission, LifecycleError> {
        self.admit_attempt_with_stage_demands(
            pipeline,
            scheduler,
            now,
            logical_request_id,
            attempt_id,
            attempt_no,
            [service_demand_milli; 3],
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn admit_attempt_with_stage_demands(
        &mut self,
        pipeline: &mut Pipeline,
        scheduler: &mut Scheduler,
        now: Tick,
        logical_request_id: LogicalRequestId,
        attempt_id: AttemptId,
        attempt_no: u32,
        service_demands_milli: [u64; 3],
    ) -> Result<LifecycleAdmission, LifecycleError> {
        if attempt_no == 0 {
            return Err(LifecycleError::ZeroAttemptNumber);
        }
        let logical = self
            .logical_requests
            .get(&logical_request_id)
            .ok_or(LifecycleError::UnknownLogicalRequest(logical_request_id))?;
        if logical.terminal.is_some() {
            return Err(LifecycleError::LogicalAlreadyTerminal(logical_request_id));
        }
        if self.attempts.contains_key(&attempt_id) {
            return Err(LifecycleError::DuplicateAttempt(attempt_id));
        }

        let pipeline_admission = pipeline.admit_source(
            scheduler,
            now,
            StageJob::with_stage_demands(attempt_id, service_demands_milli),
        )?;
        let (source_admitted, timeout_at, timeout_event, terminal) =
            match pipeline_admission.outcome {
                AdmissionOutcome::Started(_) | AdmissionOutcome::Queued(_) => {
                    let timeout_at = now
                        .checked_add(self.spec.per_attempt_timeout_us)
                        .ok_or(LifecycleError::TimeOverflow)?;
                    let timeout_event = scheduler.schedule_at(
                        timeout_at,
                        crate::event::EventKind::AttemptTimeout { attempt_id },
                    )?;
                    (true, Some(timeout_at), Some(timeout_event), None)
                }
                AdmissionOutcome::Rejected(_) => {
                    Self::increment(
                        &mut self.summary.attempt_queue_rejections,
                        "attempt_queue_rejections",
                    )?;
                    (
                        false,
                        None,
                        None,
                        Some(AttemptTerminal::QueueRejected {
                            at: now,
                            stage_id: pipeline_admission.stage_id,
                        }),
                    )
                }
            };

        self.attempts.insert(
            attempt_id,
            Attempt {
                id: attempt_id,
                logical_request_id,
                attempt_no,
                source_admitted,
                timeout_at,
                timed_out_at: None,
                terminal,
            },
        );
        Self::increment(&mut self.summary.physical_attempts, "physical_attempts")?;
        Ok(LifecycleAdmission {
            pipeline_admission,
            timeout_event,
        })
    }

    pub fn handle_attempt_timeout(
        &mut self,
        now: Tick,
        attempt_id: AttemptId,
    ) -> Result<TimeoutHandling, LifecycleError> {
        let attempt = self
            .attempts
            .get_mut(&attempt_id)
            .ok_or(LifecycleError::UnknownAttempt(attempt_id))?;
        if attempt.terminal.is_some() {
            return Ok(TimeoutHandling::NoOpTerminal);
        }
        let timeout_at = attempt
            .timeout_at
            .ok_or(LifecycleError::UnexpectedEventTime {
                entity: "attempt without timeout",
                expected: now,
                actual: now,
            })?;
        if timeout_at != now {
            return Err(LifecycleError::UnexpectedEventTime {
                entity: "attempt timeout",
                expected: timeout_at,
                actual: now,
            });
        }
        if attempt.timed_out_at.is_some() {
            return Ok(TimeoutHandling::NoOpTerminal);
        }

        attempt.timed_out_at = Some(now);
        Self::increment(&mut self.summary.attempt_timeouts, "attempt_timeouts")?;
        Ok(TimeoutHandling::TimedOut)
    }

    pub fn handle_logical_deadline(
        &mut self,
        now: Tick,
        logical_request_id: LogicalRequestId,
    ) -> Result<DeadlineHandling, LifecycleError> {
        let logical = self
            .logical_requests
            .get_mut(&logical_request_id)
            .ok_or(LifecycleError::UnknownLogicalRequest(logical_request_id))?;
        if matches!(logical.terminal, Some(LogicalTerminal::Succeeded { .. })) {
            return Ok(DeadlineHandling::NoOpSucceeded);
        }
        if logical.terminal.is_some() {
            return Ok(DeadlineHandling::NoOpTerminal);
        }
        if logical.deadline_at != now {
            return Err(LifecycleError::UnexpectedEventTime {
                entity: "logical deadline",
                expected: logical.deadline_at,
                actual: now,
            });
        }
        logical.terminal = Some(LogicalTerminal::DeadlineAbandoned { at: now });
        Self::increment(
            &mut self.summary.logical_deadline_abandoned,
            "logical_deadline_abandoned",
        )?;
        Ok(DeadlineHandling::Abandoned)
    }

    /// Marks a logical request terminal when the configured attempt limit has
    /// been consumed. Physical work already in service remains in the pipeline
    /// and will later be classified as late work.
    pub fn mark_attempts_exhausted(
        &mut self,
        now: Tick,
        logical_request_id: LogicalRequestId,
    ) -> Result<bool, LifecycleError> {
        let logical = self
            .logical_requests
            .get_mut(&logical_request_id)
            .ok_or(LifecycleError::UnknownLogicalRequest(logical_request_id))?;
        if logical.terminal.is_some() {
            return Ok(false);
        }
        logical.terminal = Some(LogicalTerminal::AttemptsExhausted { at: now });
        Self::increment(
            &mut self.summary.logical_attempts_exhausted,
            "logical_attempts_exhausted",
        )?;
        Ok(true)
    }

    pub fn observe_pipeline_progress(
        &mut self,
        now: Tick,
        completed: PipelineStageCompletion,
    ) -> Result<AttemptProgress, LifecycleError> {
        let attempt_id = completed.completed_job.attempt_id;
        let (logical_request_id, timed_out, attempt_terminal) = {
            let attempt = self
                .attempts
                .get(&attempt_id)
                .ok_or(LifecycleError::UnknownAttempt(attempt_id))?;
            (
                attempt.logical_request_id,
                attempt.timed_out_at.is_some(),
                attempt.terminal,
            )
        };
        if attempt_terminal.is_some() {
            return Err(LifecycleError::AttemptAlreadyTerminal(attempt_id));
        }

        if let Some(admission) = completed.downstream_admission
            && let Some(stage_id) = admission.queue_rejected_stage()
        {
            self.attempts.get_mut(&attempt_id).unwrap().terminal =
                Some(AttemptTerminal::QueueRejected { at: now, stage_id });
            Self::increment(
                &mut self.summary.attempt_queue_rejections,
                "attempt_queue_rejections",
            )?;
            return Ok(AttemptProgress::QueueRejected { stage_id });
        }

        if !completed.pipeline_completed {
            return Ok(AttemptProgress::Advanced);
        }

        let logical = self
            .logical_requests
            .get(&logical_request_id)
            .ok_or(LifecycleError::UnknownLogicalRequest(logical_request_id))?;
        let classification = match logical.terminal {
            Some(LogicalTerminal::Succeeded { .. }) => AttemptProgress::DuplicateCompletion,
            Some(LogicalTerminal::DeadlineAbandoned { .. }) => AttemptProgress::LateCompletion,
            Some(LogicalTerminal::AttemptsExhausted { .. }) => AttemptProgress::LateCompletion,
            Some(LogicalTerminal::OutstandingAtHorizon { .. }) => {
                return Err(LifecycleError::LogicalAlreadyTerminal(logical_request_id));
            }
            None if timed_out => AttemptProgress::LateCompletion,
            None if now > logical.deadline_at => {
                return Err(LifecycleError::DeadlineOrderingViolation);
            }
            None => AttemptProgress::OnTimeSuccess,
        };

        match classification {
            AttemptProgress::OnTimeSuccess => {
                self.attempts.get_mut(&attempt_id).unwrap().terminal =
                    Some(AttemptTerminal::OnTimeSuccess { at: now });
                self.logical_requests
                    .get_mut(&logical_request_id)
                    .unwrap()
                    .terminal = Some(LogicalTerminal::Succeeded {
                    at: now,
                    winning_attempt_id: attempt_id,
                });
                Self::increment(
                    &mut self.summary.attempt_on_time_successes,
                    "attempt_on_time_successes",
                )?;
                Self::increment(&mut self.summary.logical_successes, "logical_successes")?;
            }
            AttemptProgress::LateCompletion => {
                self.attempts.get_mut(&attempt_id).unwrap().terminal =
                    Some(AttemptTerminal::LateCompletion { at: now });
                Self::increment(
                    &mut self.summary.attempt_late_completions,
                    "attempt_late_completions",
                )?;
            }
            AttemptProgress::DuplicateCompletion => {
                self.attempts.get_mut(&attempt_id).unwrap().terminal =
                    Some(AttemptTerminal::DuplicateCompletion { at: now });
                Self::increment(
                    &mut self.summary.attempt_duplicate_completions,
                    "attempt_duplicate_completions",
                )?;
            }
            AttemptProgress::Advanced | AttemptProgress::QueueRejected { .. } => unreachable!(),
        }
        Ok(classification)
    }

    pub fn close_horizon(&mut self, now: Tick) -> Result<ConservationLedger, LifecycleError> {
        for logical in self.logical_requests.values_mut() {
            if logical.terminal.is_none() {
                logical.terminal = Some(LogicalTerminal::OutstandingAtHorizon { at: now });
                Self::increment(
                    &mut self.summary.logical_outstanding_at_horizon,
                    "logical_outstanding_at_horizon",
                )?;
            }
        }
        for attempt in self.attempts.values_mut() {
            if attempt.terminal.is_none() {
                attempt.terminal = Some(AttemptTerminal::OutstandingAtHorizon {
                    at: now,
                    timed_out: attempt.timed_out_at.is_some(),
                });
                Self::increment(
                    &mut self.summary.attempt_outstanding_at_horizon,
                    "attempt_outstanding_at_horizon",
                )?;
            }
        }

        let conservation = self.conservation();
        if !conservation.all_terminal() {
            return Err(LifecycleError::ConservationViolation);
        }
        Ok(conservation)
    }

    pub fn conservation(&self) -> ConservationLedger {
        let logical_terminal = self
            .logical_requests
            .values()
            .filter(|logical| logical.terminal.is_some())
            .count() as u64;
        let attempt_terminal = self
            .attempts
            .values()
            .filter(|attempt| attempt.terminal.is_some())
            .count() as u64;
        ConservationLedger {
            logical_total: self.logical_requests.len() as u64,
            logical_terminal,
            logical_open: self.logical_requests.len() as u64 - logical_terminal,
            attempt_total: self.attempts.len() as u64,
            attempt_terminal,
            attempt_open: self.attempts.len() as u64 - attempt_terminal,
        }
    }

    pub fn logical_request(&self, id: LogicalRequestId) -> Option<&LogicalRequest> {
        self.logical_requests.get(&id)
    }

    pub fn attempt(&self, id: AttemptId) -> Option<&Attempt> {
        self.attempts.get(&id)
    }

    pub const fn summary(&self) -> LifecycleSummary {
        self.summary
    }

    fn increment(value: &mut u64, name: &'static str) -> Result<(), LifecycleError> {
        *value = value
            .checked_add(1)
            .ok_or(LifecycleError::CounterOverflow(name))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AttemptProgress, AttemptTerminal, LifecycleLedger, LifecycleSpec, LogicalTerminal,
    };
    use crate::config::RetryCause;
    use crate::event::EventKind;
    use crate::pipeline::{Pipeline, StageCompletionHandling, StageRuntimeSpec};
    use crate::scheduler::Scheduler;
    use crate::types::{AttemptId, LogicalRequestId, Tick};

    fn pipeline_with_capacity(workers: usize, queue_capacity: usize) -> Pipeline {
        Pipeline::new([
            StageRuntimeSpec {
                workers,
                queue_capacity_waiting: queue_capacity,
                service_rate_work_units_per_second: 1_000,
            },
            StageRuntimeSpec {
                workers,
                queue_capacity_waiting: queue_capacity,
                service_rate_work_units_per_second: 1_000,
            },
            StageRuntimeSpec {
                workers,
                queue_capacity_waiting: queue_capacity,
                service_rate_work_units_per_second: 100,
            },
        ])
        .unwrap()
    }

    fn drive_retry_free_events(
        ledger: &mut LifecycleLedger,
        pipeline: &mut Pipeline,
        scheduler: &mut Scheduler,
    ) -> Vec<AttemptProgress> {
        let mut progress = Vec::new();
        while let Some(event) = scheduler.pop_next() {
            match event.kind {
                EventKind::StageCompletion {
                    attempt_id,
                    stage_id,
                    service_epoch,
                } => match pipeline
                    .handle_stage_completion(
                        scheduler,
                        event.at,
                        stage_id,
                        attempt_id,
                        service_epoch,
                    )
                    .unwrap()
                {
                    StageCompletionHandling::Applied(completed) => progress.push(
                        ledger
                            .observe_pipeline_progress(event.at, completed)
                            .unwrap(),
                    ),
                    StageCompletionHandling::Stale { .. } => {}
                },
                EventKind::AttemptTimeout { attempt_id } => {
                    ledger.handle_attempt_timeout(event.at, attempt_id).unwrap();
                }
                EventKind::LogicalDeadline { logical_request_id } => {
                    ledger
                        .handle_logical_deadline(event.at, logical_request_id)
                        .unwrap();
                }
                EventKind::TriggerBoundary { transition } => {
                    pipeline
                        .handle_trigger_boundary(scheduler, event.at, transition)
                        .unwrap();
                }
                other => panic!("unexpected lifecycle fixture event: {other:?}"),
            }
        }
        progress
    }

    #[test]
    fn completion_wins_over_timeout_at_the_same_tick() {
        let mut pipeline = pipeline_with_capacity(1, 2);
        let mut scheduler = Scheduler::new();
        let mut ledger = LifecycleLedger::new(LifecycleSpec {
            per_attempt_timeout_us: 12_000,
            overall_logical_deadline_us: 20_000,
        })
        .unwrap();
        let logical_id = LogicalRequestId(1);
        let attempt_id = AttemptId(1);

        ledger
            .register_logical_request(&mut scheduler, Tick::ZERO, logical_id)
            .unwrap();
        ledger
            .admit_attempt(
                &mut pipeline,
                &mut scheduler,
                Tick::ZERO,
                logical_id,
                attempt_id,
                1,
                1_000,
            )
            .unwrap();
        let progress = drive_retry_free_events(&mut ledger, &mut pipeline, &mut scheduler);

        assert!(progress.contains(&AttemptProgress::OnTimeSuccess));
        assert_eq!(
            ledger.attempt(attempt_id).unwrap().terminal,
            Some(AttemptTerminal::OnTimeSuccess {
                at: Tick::new(12_000),
            })
        );
        assert_eq!(ledger.attempt(attempt_id).unwrap().timed_out_at, None);
        assert_eq!(ledger.summary().attempt_timeouts, 0);
        assert_eq!(ledger.summary().logical_successes, 1);
        assert!(ledger.conservation().all_terminal());
    }

    #[test]
    fn timed_out_work_keeps_running_and_finishes_late() {
        let mut pipeline = pipeline_with_capacity(1, 2);
        let mut scheduler = Scheduler::new();
        let mut ledger = LifecycleLedger::new(LifecycleSpec {
            per_attempt_timeout_us: 5_000,
            overall_logical_deadline_us: 20_000,
        })
        .unwrap();
        let logical_id = LogicalRequestId(1);
        let attempt_id = AttemptId(1);

        ledger
            .register_logical_request(&mut scheduler, Tick::ZERO, logical_id)
            .unwrap();
        ledger
            .admit_attempt(
                &mut pipeline,
                &mut scheduler,
                Tick::ZERO,
                logical_id,
                attempt_id,
                1,
                1_000,
            )
            .unwrap();
        let progress = drive_retry_free_events(&mut ledger, &mut pipeline, &mut scheduler);

        assert!(progress.contains(&AttemptProgress::LateCompletion));
        assert_eq!(
            ledger.attempt(attempt_id).unwrap().timed_out_at,
            Some(Tick::new(5_000))
        );
        assert_eq!(
            ledger.attempt(attempt_id).unwrap().terminal,
            Some(AttemptTerminal::LateCompletion {
                at: Tick::new(12_000),
            })
        );
        assert_eq!(
            ledger.logical_request(logical_id).unwrap().terminal,
            Some(LogicalTerminal::DeadlineAbandoned {
                at: Tick::new(20_000),
            })
        );
        assert_eq!(pipeline.summary().pipeline_completions, 1);
        assert_eq!(ledger.summary().logical_successes, 0);
        assert_eq!(ledger.summary().attempt_late_completions, 1);
        assert!(ledger.conservation().all_terminal());
    }

    #[test]
    fn first_success_is_goodput_and_later_completion_is_duplicate() {
        let mut pipeline = pipeline_with_capacity(2, 2);
        let mut scheduler = Scheduler::new();
        let mut ledger = LifecycleLedger::new(LifecycleSpec {
            per_attempt_timeout_us: 30_000,
            overall_logical_deadline_us: 40_000,
        })
        .unwrap();
        let logical_id = LogicalRequestId(1);

        ledger
            .register_logical_request(&mut scheduler, Tick::ZERO, logical_id)
            .unwrap();
        ledger
            .admit_attempt(
                &mut pipeline,
                &mut scheduler,
                Tick::ZERO,
                logical_id,
                AttemptId(1),
                1,
                1_000,
            )
            .unwrap();
        ledger
            .admit_attempt(
                &mut pipeline,
                &mut scheduler,
                Tick::ZERO,
                logical_id,
                AttemptId(2),
                2,
                2_000,
            )
            .unwrap();
        let progress = drive_retry_free_events(&mut ledger, &mut pipeline, &mut scheduler);

        assert!(progress.contains(&AttemptProgress::OnTimeSuccess));
        assert!(progress.contains(&AttemptProgress::DuplicateCompletion));
        assert_eq!(ledger.summary().logical_successes, 1);
        assert_eq!(ledger.summary().attempt_on_time_successes, 1);
        assert_eq!(ledger.summary().attempt_duplicate_completions, 1);
        assert!(ledger.conservation().all_terminal());
    }

    #[test]
    fn source_rejection_has_no_timeout_and_horizon_closes_every_record() {
        let mut pipeline = pipeline_with_capacity(1, 1);
        let mut scheduler = Scheduler::new();
        let mut ledger = LifecycleLedger::new(LifecycleSpec {
            per_attempt_timeout_us: 30_000,
            overall_logical_deadline_us: 40_000,
        })
        .unwrap();
        let logical_id = LogicalRequestId(1);
        ledger
            .register_logical_request(&mut scheduler, Tick::ZERO, logical_id)
            .unwrap();

        for (attempt_id, attempt_no) in [(AttemptId(1), 1), (AttemptId(2), 2), (AttemptId(3), 3)] {
            ledger
                .admit_attempt(
                    &mut pipeline,
                    &mut scheduler,
                    Tick::ZERO,
                    logical_id,
                    attempt_id,
                    attempt_no,
                    1_000,
                )
                .unwrap();
        }

        let rejected = ledger.attempt(AttemptId(3)).unwrap();
        assert!(!rejected.source_admitted);
        assert_eq!(rejected.timeout_at, None);
        assert!(matches!(
            rejected.terminal,
            Some(AttemptTerminal::QueueRejected { .. })
        ));

        let conservation = ledger.close_horizon(Tick::ZERO).unwrap();
        assert!(conservation.all_terminal());
        assert_eq!(ledger.summary().logical_outstanding_at_horizon, 1);
        assert_eq!(ledger.summary().attempt_outstanding_at_horizon, 2);
        assert_eq!(ledger.summary().attempt_queue_rejections, 1);
    }

    #[test]
    fn deadline_precedes_retry_ready_at_the_same_tick() {
        let mut scheduler = Scheduler::new();
        let logical_request_id = LogicalRequestId(1);
        let parent_attempt_id = AttemptId(1);
        let at = Tick::new(10);
        scheduler
            .schedule_at(
                at,
                EventKind::RetryReady {
                    logical_request_id,
                    parent_attempt_id,
                    cause: RetryCause::AttemptTimeout,
                },
            )
            .unwrap();
        scheduler
            .schedule_at(at, EventKind::LogicalDeadline { logical_request_id })
            .unwrap();

        assert!(matches!(
            scheduler.pop_next().unwrap().kind,
            EventKind::LogicalDeadline { .. }
        ));
        assert!(matches!(
            scheduler.pop_next().unwrap().kind,
            EventKind::RetryReady { .. }
        ));
    }
}
