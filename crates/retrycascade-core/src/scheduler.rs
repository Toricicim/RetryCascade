use std::collections::BinaryHeap;

use thiserror::Error;

use crate::event::{Event, EventKind};
use crate::types::Tick;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SchedulerError {
    #[error("event time overflow")]
    TimeOverflow,
    #[error("event sequence overflow")]
    SequenceOverflow,
}

/// Gelecekteki olayları sanal zaman, olay önceliği ve eklenme sırasına göre tutar.
#[derive(Debug, Default)]
pub struct Scheduler {
    events: BinaryHeap<Event>,
    next_sequence: u64,
}

impl Scheduler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn schedule_at(&mut self, at: Tick, kind: EventKind) -> Result<Event, SchedulerError> {
        let sequence = self.next_sequence;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(SchedulerError::SequenceOverflow)?;

        let event = Event::new(at, kind, sequence);
        self.events.push(event);
        Ok(event)
    }

    pub fn schedule_after(
        &mut self,
        now: Tick,
        delay_us: u64,
        kind: EventKind,
    ) -> Result<Event, SchedulerError> {
        let at = now
            .checked_add(delay_us)
            .ok_or(SchedulerError::TimeOverflow)?;
        self.schedule_at(at, kind)
    }

    pub fn pop_next(&mut self) -> Option<Event> {
        self.events.pop()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BinaryHeap;

    use super::{Scheduler, SchedulerError};
    use crate::config::RetryCause;
    use crate::event::{EventKind, TriggerTransition};
    use crate::types::{AttemptId, LogicalRequestId, StageId, Tick};

    #[test]
    fn empty_scheduler_is_safe() {
        let mut scheduler = Scheduler::new();

        assert!(scheduler.is_empty());
        assert_eq!(scheduler.len(), 0);
        assert_eq!(scheduler.pop_next(), None);
    }

    #[test]
    fn schedule_after_rejects_time_overflow_without_adding_an_event() {
        let mut scheduler = Scheduler::new();

        let result = scheduler.schedule_after(Tick::new(u64::MAX), 1, EventKind::QueueObservation);

        assert_eq!(result, Err(SchedulerError::TimeOverflow));
        assert!(scheduler.is_empty());
    }

    #[test]
    fn earlier_event_is_popped_first() {
        let mut scheduler = Scheduler::new();

        scheduler
            .schedule_at(Tick::new(20), EventKind::QueueObservation)
            .unwrap();

        scheduler
            .schedule_at(Tick::new(10), EventKind::QueueObservation)
            .unwrap();

        let next = scheduler
            .pop_next()
            .expect("scheduler should contain an event");

        assert_eq!(next.at, Tick::new(10));
    }
    #[test]
    fn same_time_and_priority_preserves_insertion_order() {
        let mut scheduler = Scheduler::new();

        let first = scheduler
            .schedule_at(Tick::new(10), EventKind::QueueObservation)
            .unwrap();

        let second = scheduler
            .schedule_at(Tick::new(10), EventKind::QueueObservation)
            .unwrap();

        assert_eq!(first.sequence, 0);
        assert_eq!(second.sequence, 1);

        let first_popped = scheduler.pop_next().unwrap();
        let second_popped = scheduler.pop_next().unwrap();

        assert_eq!(first_popped.sequence, 0);
        assert_eq!(second_popped.sequence, 1);
    }

    #[test]
    fn all_event_kinds_follow_the_frozen_same_tick_priority() {
        let mut scheduler = Scheduler::new();
        let at = Tick::new(10);
        let logical_request_id = LogicalRequestId(1);
        let attempt_id = AttemptId(1);

        // Bilerek ters sırada eklenir; scheduler ekleme sırasına güvenmemelidir.
        let kinds = [
            EventKind::QueueObservation,
            EventKind::LogicalArrival { logical_request_id },
            EventKind::RetryReady {
                logical_request_id,
                parent_attempt_id: attempt_id,
                cause: RetryCause::AttemptTimeout,
            },
            EventKind::TriggerBoundary {
                transition: TriggerTransition::Start,
            },
            EventKind::LogicalDeadline { logical_request_id },
            EventKind::AttemptTimeout { attempt_id },
            EventKind::StageCompletion {
                attempt_id,
                stage_id: StageId(0),
                service_epoch: 0,
            },
        ];

        for kind in kinds {
            scheduler.schedule_at(at, kind).unwrap();
        }

        let priorities: Vec<u8> = std::iter::from_fn(|| scheduler.pop_next())
            .map(|event| event.kind.priority())
            .collect();

        assert_eq!(priorities, vec![0, 1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn sequence_overflow_is_reported_without_adding_an_event() {
        let mut scheduler = Scheduler {
            events: BinaryHeap::new(),
            next_sequence: u64::MAX,
        };

        let result = scheduler.schedule_at(Tick::new(10), EventKind::QueueObservation);

        assert_eq!(result, Err(SchedulerError::SequenceOverflow));
        assert!(scheduler.is_empty());
    }
}
