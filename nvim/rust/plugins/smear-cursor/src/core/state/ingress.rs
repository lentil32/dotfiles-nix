use super::BufferPerfClass;
use crate::core::types::IngressSeq;
use crate::core::types::Millis;
use crate::core::types::ProposalId;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ExternalDemandKind {
    ExternalCursor,
    ModeChanged,
    BufferEntered,
    BoundaryRefresh,
}

impl ExternalDemandKind {
    pub(crate) const ALL: [Self; 4] = [
        Self::ExternalCursor,
        Self::ModeChanged,
        Self::BufferEntered,
        Self::BoundaryRefresh,
    ];

    pub(crate) const fn is_cursor(self) -> bool {
        matches!(self, Self::ExternalCursor)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ExternalDemand {
    seq: IngressSeq,
    kind: ExternalDemandKind,
    observed_at: Millis,
    buffer_perf_class: BufferPerfClass,
}

impl ExternalDemand {
    pub(crate) const fn new(
        seq: IngressSeq,
        kind: ExternalDemandKind,
        observed_at: Millis,
        buffer_perf_class: BufferPerfClass,
    ) -> Self {
        Self {
            seq,
            kind,
            observed_at,
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
    slots: DemandSlots,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
struct DemandSlots {
    latest_cursor: Option<QueuedDemand>,
    latest_mode_changed: Option<QueuedDemand>,
    latest_buffer_entered: Option<QueuedDemand>,
    latest_boundary_refresh: Option<QueuedDemand>,
}

impl DemandSlots {
    const fn get(&self, kind: ExternalDemandKind) -> Option<&QueuedDemand> {
        match kind {
            ExternalDemandKind::ExternalCursor => self.latest_cursor.as_ref(),
            ExternalDemandKind::ModeChanged => self.latest_mode_changed.as_ref(),
            ExternalDemandKind::BufferEntered => self.latest_buffer_entered.as_ref(),
            ExternalDemandKind::BoundaryRefresh => self.latest_boundary_refresh.as_ref(),
        }
    }

    fn get_mut(&mut self, kind: ExternalDemandKind) -> &mut Option<QueuedDemand> {
        match kind {
            ExternalDemandKind::ExternalCursor => &mut self.latest_cursor,
            ExternalDemandKind::ModeChanged => &mut self.latest_mode_changed,
            ExternalDemandKind::BufferEntered => &mut self.latest_buffer_entered,
            ExternalDemandKind::BoundaryRefresh => &mut self.latest_boundary_refresh,
        }
    }

    fn insert(&mut self, demand: QueuedDemand) -> bool {
        self.get_mut(demand.kind()).replace(demand).is_some()
    }

    fn take(&mut self, kind: ExternalDemandKind) -> Option<QueuedDemand> {
        self.get_mut(kind).take()
    }

    fn pending_len(&self) -> usize {
        [
            self.latest_cursor.is_some(),
            self.latest_mode_changed.is_some(),
            self.latest_buffer_entered.is_some(),
            self.latest_boundary_refresh.is_some(),
        ]
        .into_iter()
        .map(usize::from)
        .sum()
    }
}

impl DemandQueue {
    pub(crate) fn enqueue(mut self, demand: QueuedDemand) -> (Self, bool) {
        let coalesced = self.slots.insert(demand);
        (self, coalesced)
    }

    fn next_ready_kind(&self) -> Option<ExternalDemandKind> {
        ExternalDemandKind::ALL
            .into_iter()
            .filter_map(|kind| self.queued(kind).map(|demand| (kind, demand.seq())))
            .min_by_key(|&(_, seq)| seq)
            .map(|(kind, _)| kind)
    }

    fn take_ready_demand(&mut self, kind: ExternalDemandKind) -> Option<ExternalDemand> {
        self.slots.take(kind).map(QueuedDemand::into_ready)
    }

    pub(crate) fn dequeue_ready(mut self) -> (Self, Option<ExternalDemand>) {
        let Some(kind) = self.next_ready_kind() else {
            return (self, None);
        };

        let demand = self.take_ready_demand(kind);
        (self, demand)
    }

    pub(crate) const fn latest_cursor(&self) -> Option<&QueuedDemand> {
        self.queued(ExternalDemandKind::ExternalCursor)
    }

    pub(crate) const fn queued(&self, kind: ExternalDemandKind) -> Option<&QueuedDemand> {
        self.slots.get(kind)
    }

    pub(crate) fn pending_len(&self) -> usize {
        self.slots.pending_len()
    }
}

#[cfg(test)]
mod tests {
    use super::BufferPerfClass;
    use super::DemandQueue;
    use super::ExternalDemand;
    use super::ExternalDemandKind;
    use super::Millis;
    use super::QueuedDemand;
    use crate::core::types::IngressSeq;
    use pretty_assertions::assert_eq;

    fn queued_demand(seq: u64, kind: ExternalDemandKind) -> QueuedDemand {
        QueuedDemand::ready(ExternalDemand::new(
            IngressSeq::new(seq),
            kind,
            Millis::new(seq),
            BufferPerfClass::Full,
        ))
    }

    #[test]
    fn same_kind_enqueue_replaces_the_older_pending_demand() {
        let (queue, was_coalesced) =
            DemandQueue::default().enqueue(queued_demand(1, ExternalDemandKind::ModeChanged));
        assert!(!was_coalesced);

        let (queue, was_coalesced) =
            queue.enqueue(queued_demand(2, ExternalDemandKind::ModeChanged));
        assert!(was_coalesced);
        assert_eq!(
            queue.queued(ExternalDemandKind::ModeChanged),
            Some(&queued_demand(2, ExternalDemandKind::ModeChanged))
        );
        assert_eq!(queue.pending_len(), 1);
    }

    #[test]
    fn dequeue_ready_uses_the_oldest_sequence_across_occupied_slots() {
        let (queue, _) =
            DemandQueue::default().enqueue(queued_demand(4, ExternalDemandKind::ExternalCursor));
        let (queue, _) = queue.enqueue(queued_demand(2, ExternalDemandKind::ModeChanged));
        let (queue, _) = queue.enqueue(queued_demand(3, ExternalDemandKind::BufferEntered));

        let (queue, first) = queue.dequeue_ready();
        let (queue, second) = queue.dequeue_ready();
        let (_queue, third) = queue.dequeue_ready();

        assert_eq!(
            first.map(|demand| demand.kind()),
            Some(ExternalDemandKind::ModeChanged)
        );
        assert_eq!(
            second.map(|demand| demand.kind()),
            Some(ExternalDemandKind::BufferEntered)
        );
        assert_eq!(
            third.map(|demand| demand.kind()),
            Some(ExternalDemandKind::ExternalCursor)
        );
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
