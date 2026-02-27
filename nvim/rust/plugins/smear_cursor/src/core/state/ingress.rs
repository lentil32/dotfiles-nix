use crate::core::types::{CursorPosition, IngressSeq, Millis, ProposalId};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ExternalDemandKind {
    ExternalCursor,
    KeyFallback,
    ModeChanged,
    BufferEntered,
}

impl ExternalDemandKind {
    pub(crate) const fn is_cursor(self) -> bool {
        matches!(self, Self::ExternalCursor | Self::KeyFallback)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ExternalDemand {
    seq: IngressSeq,
    kind: ExternalDemandKind,
    observed_at: Millis,
    requested_target: Option<CursorPosition>,
}

impl ExternalDemand {
    pub(crate) const fn new(
        seq: IngressSeq,
        kind: ExternalDemandKind,
        observed_at: Millis,
        requested_target: Option<CursorPosition>,
    ) -> Self {
        Self {
            seq,
            kind,
            observed_at,
            requested_target,
        }
    }

    pub(crate) const fn seq(&self) -> IngressSeq {
        self.seq
    }

    pub(crate) const fn kind(&self) -> ExternalDemandKind {
        self.kind
    }

    pub(crate) const fn observed_at(&self) -> Millis {
        self.observed_at
    }

    pub(crate) const fn requested_target(&self) -> Option<CursorPosition> {
        self.requested_target
    }

    pub(crate) const fn is_cursor(&self) -> bool {
        self.kind.is_cursor()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum QueuedDemand {
    Ready(ExternalDemand),
    PendingKeyFallback {
        seq: IngressSeq,
        due_at: Millis,
        requested_target: Option<CursorPosition>,
    },
}

impl QueuedDemand {
    pub(crate) fn ready(demand: ExternalDemand) -> Self {
        Self::Ready(demand)
    }

    pub(crate) const fn pending_key_fallback(
        seq: IngressSeq,
        due_at: Millis,
        requested_target: Option<CursorPosition>,
    ) -> Self {
        Self::PendingKeyFallback {
            seq,
            due_at,
            requested_target,
        }
    }

    pub(crate) const fn seq(&self) -> IngressSeq {
        match self {
            Self::Ready(demand) => demand.seq(),
            Self::PendingKeyFallback { seq, .. } => *seq,
        }
    }

    pub(crate) const fn kind(&self) -> ExternalDemandKind {
        match self {
            Self::Ready(demand) => demand.kind(),
            Self::PendingKeyFallback { .. } => ExternalDemandKind::KeyFallback,
        }
    }

    pub(crate) const fn due_at(&self) -> Option<Millis> {
        match self {
            Self::Ready(_) => None,
            Self::PendingKeyFallback { due_at, .. } => Some(*due_at),
        }
    }

    pub(crate) const fn is_cursor(&self) -> bool {
        self.kind().is_cursor()
    }

    pub(crate) fn is_ready_at(&self, observed_at: Millis) -> bool {
        match self {
            Self::Ready(_) => true,
            Self::PendingKeyFallback { due_at, .. } => due_at.value() <= observed_at.value(),
        }
    }

    pub(crate) fn into_ready(self, observed_at: Millis) -> ExternalDemand {
        match self {
            Self::Ready(demand) => demand,
            Self::PendingKeyFallback {
                seq,
                requested_target,
                ..
            } => ExternalDemand::new(
                seq,
                ExternalDemandKind::KeyFallback,
                observed_at,
                requested_target,
            ),
        }
    }
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub(crate) struct DemandQueue {
    latest_cursor: Option<QueuedDemand>,
    ordered: BTreeMap<IngressSeq, QueuedDemand>,
}

impl DemandQueue {
    pub(crate) fn enqueue(mut self, demand: QueuedDemand) -> (Self, bool) {
        if demand.is_cursor() {
            let coalesced = self.latest_cursor.replace(demand).is_some();
            return (self, coalesced);
        }

        self.ordered.insert(demand.seq(), demand);
        (self, false)
    }

    pub(crate) fn dequeue_ready(self, observed_at: Millis) -> (Self, Option<ExternalDemand>) {
        let ready_ordered_seq = self
            .ordered
            .iter()
            .find(|(_, demand)| demand.is_ready_at(observed_at))
            .map(|(seq, _)| *seq);
        match (
            self.latest_cursor
                .as_ref()
                .filter(|demand| demand.is_ready_at(observed_at))
                .map(QueuedDemand::seq),
            ready_ordered_seq,
        ) {
            (None, None) => (self, None),
            (Some(_), None) => {
                let mut next = self;
                let demand = next
                    .latest_cursor
                    .take()
                    .map(|queued| queued.into_ready(observed_at));
                (next, demand)
            }
            (None, Some(seq)) => {
                let mut next = self;
                let demand = next
                    .ordered
                    .remove(&seq)
                    .map(|queued| queued.into_ready(observed_at));
                (next, demand)
            }
            (Some(cursor_seq), Some(ordered_seq)) if cursor_seq < ordered_seq => {
                let mut next = self;
                let demand = next
                    .latest_cursor
                    .take()
                    .map(|queued| queued.into_ready(observed_at));
                (next, demand)
            }
            (Some(_), Some(ordered_seq)) => {
                let mut next = self;
                let demand = next
                    .ordered
                    .remove(&ordered_seq)
                    .map(|queued| queued.into_ready(observed_at));
                (next, demand)
            }
        }
    }

    pub(crate) fn next_due_at(&self) -> Option<Millis> {
        let cursor_due = self.latest_cursor.as_ref().and_then(QueuedDemand::due_at);
        let ordered_due = self.ordered.values().filter_map(QueuedDemand::due_at).min();
        match (cursor_due, ordered_due) {
            (Some(cursor_due), Some(ordered_due)) => {
                Some(if cursor_due.value() <= ordered_due.value() {
                    cursor_due
                } else {
                    ordered_due
                })
            }
            (Some(cursor_due), None) => Some(cursor_due),
            (None, Some(ordered_due)) => Some(ordered_due),
            (None, None) => None,
        }
    }

    pub(crate) const fn latest_cursor(&self) -> Option<&QueuedDemand> {
        self.latest_cursor.as_ref()
    }

    pub(crate) const fn ordered(&self) -> &BTreeMap<IngressSeq, QueuedDemand> {
        &self.ordered
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct EntropyState {
    next_proposal_id: ProposalId,
    next_ingress_seq: IngressSeq,
}

impl Default for EntropyState {
    fn default() -> Self {
        Self {
            next_proposal_id: ProposalId::INITIAL,
            next_ingress_seq: IngressSeq::INITIAL,
        }
    }
}

impl EntropyState {
    pub(crate) const fn next_proposal_id(self) -> ProposalId {
        self.next_proposal_id
    }

    pub(crate) const fn next_ingress_seq(self) -> IngressSeq {
        self.next_ingress_seq
    }

    pub(crate) fn allocate_proposal_id(self) -> (Self, ProposalId) {
        let proposal_id = self.next_proposal_id.next();
        (
            Self {
                next_proposal_id: proposal_id,
                ..self
            },
            proposal_id,
        )
    }

    pub(crate) fn allocate_ingress_seq(self) -> (Self, IngressSeq) {
        let seq = self.next_ingress_seq.next();
        (
            Self {
                next_ingress_seq: seq,
                ..self
            },
            seq,
        )
    }
}
