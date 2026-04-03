use super::BufferPerfClass;
use crate::core::types::CursorPosition;
use crate::core::types::IngressSeq;
use crate::core::types::Millis;
use crate::core::types::ProposalId;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ExternalDemandKind {
    ExternalCursor,
    ModeChanged,
    BufferEntered,
    BoundaryRefresh,
}

impl ExternalDemandKind {
    pub(crate) const fn is_cursor(self) -> bool {
        matches!(self, Self::ExternalCursor)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ExternalDemand {
    seq: IngressSeq,
    kind: ExternalDemandKind,
    observed_at: Millis,
    requested_target: Option<CursorPosition>,
    buffer_perf_class: BufferPerfClass,
}

impl ExternalDemand {
    pub(crate) const fn new(
        seq: IngressSeq,
        kind: ExternalDemandKind,
        observed_at: Millis,
        requested_target: Option<CursorPosition>,
        buffer_perf_class: BufferPerfClass,
    ) -> Self {
        Self {
            seq,
            kind,
            observed_at,
            requested_target,
            buffer_perf_class,
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

    pub(crate) const fn buffer_perf_class(&self) -> BufferPerfClass {
        self.buffer_perf_class
    }

    pub(crate) const fn is_cursor(&self) -> bool {
        self.kind.is_cursor()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct QueuedDemand(ExternalDemand);

impl QueuedDemand {
    pub(crate) fn ready(demand: ExternalDemand) -> Self {
        Self(demand)
    }

    pub(crate) const fn as_demand(&self) -> &ExternalDemand {
        &self.0
    }

    pub(crate) const fn seq(&self) -> IngressSeq {
        self.0.seq()
    }

    pub(crate) const fn kind(&self) -> ExternalDemandKind {
        self.0.kind()
    }

    pub(crate) const fn is_cursor(&self) -> bool {
        self.kind().is_cursor()
    }

    pub(crate) fn into_ready(self) -> ExternalDemand {
        self.0
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

    pub(crate) fn dequeue_ready(self) -> (Self, Option<ExternalDemand>) {
        match (
            self.latest_cursor.as_ref().map(QueuedDemand::seq),
            self.ordered.keys().next().copied(),
        ) {
            (None, None) => (self, None),
            (Some(_), None) => {
                let mut next = self;
                let demand = next.latest_cursor.take().map(QueuedDemand::into_ready);
                (next, demand)
            }
            (None, Some(seq)) => {
                let mut next = self;
                let demand = next.ordered.remove(&seq).map(QueuedDemand::into_ready);
                (next, demand)
            }
            (Some(cursor_seq), Some(ordered_seq)) if cursor_seq < ordered_seq => {
                let mut next = self;
                let demand = next.latest_cursor.take().map(QueuedDemand::into_ready);
                (next, demand)
            }
            (Some(_), Some(ordered_seq)) => {
                let mut next = self;
                let demand = next
                    .ordered
                    .remove(&ordered_seq)
                    .map(QueuedDemand::into_ready);
                (next, demand)
            }
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
