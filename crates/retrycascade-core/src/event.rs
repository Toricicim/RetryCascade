use std::cmp::Ordering;

use crate::config::RetryCause;
use crate::types::{AttemptId, LogicalRequestId, StageId, Tick};

/// Sink kapasite trigger'ının hangi sınırına gelindiğini belirtir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerTransition {
    Start,
    End,
}

/// Simülasyonun işleyebildiği olaylar ve olay gerçekleştiğinde gereken kimlikler.
///
/// Örneğin bir completion olayı yalnızca "iş bitti" demez; hangi attempt'in,
/// hangi stage'deki ve hangi service epoch'undaki işinin bittiğini de taşır.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    PreAdmissionFailure {
        attempt_id: AttemptId,
    },
    StageCompletion {
        attempt_id: AttemptId,
        stage_id: StageId,
        service_epoch: u64,
    },
    AttemptTimeout {
        attempt_id: AttemptId,
    },
    LogicalDeadline {
        logical_request_id: LogicalRequestId,
    },
    TriggerBoundary {
        transition: TriggerTransition,
    },
    RetryReady {
        logical_request_id: LogicalRequestId,
        parent_attempt_id: AttemptId,
        cause: RetryCause,
    },
    LogicalArrival {
        logical_request_id: LogicalRequestId,
    },
    QueueObservation,
}

impl EventKind {
    /// Küçük sayı, aynı tick içinde daha yüksek öncelik demektir.
    pub const fn priority(self) -> u8 {
        match self {
            Self::StageCompletion { .. } | Self::PreAdmissionFailure { .. } => 0,
            Self::AttemptTimeout { .. } => 1,
            Self::LogicalDeadline { .. } => 2,
            Self::TriggerBoundary { .. } => 3,
            Self::RetryReady { .. } => 4,
            Self::LogicalArrival { .. } => 5,
            Self::QueueObservation => 6,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    pub at: Tick,
    pub kind: EventKind,
    pub sequence: u64,
}

impl Event {
    pub const fn new(at: Tick, kind: EventKind, sequence: u64) -> Self {
        Self { at, kind, sequence }
    }
}

/// BinaryHeap en "büyük" öğeyi çıkarır. Bu karşılaştırma erken zamanı,
/// düşük priority değerini ve düşük sequence değerini heap açısından büyük yapar.
impl Ord for Event {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .at
            .cmp(&self.at)
            .then_with(|| other.kind.priority().cmp(&self.kind.priority()))
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

impl PartialOrd for Event {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::EventKind;
    use crate::types::AttemptId;

    #[test]
    fn completion_has_priority_over_timeout() {
        let completion = EventKind::StageCompletion {
            attempt_id: AttemptId(1),
            stage_id: crate::types::StageId(0),
            service_epoch: 0,
        };
        let timeout = EventKind::AttemptTimeout {
            attempt_id: AttemptId(1),
        };

        assert!(completion.priority() < timeout.priority());
    }
}
