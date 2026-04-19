use thiserror::Error;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum Lifecycle {
    Idle,
    Primed,
    Observing,
    Ready,
    Planning,
    Applying,
    Recovering,
}

macro_rules! impl_u64_counter_methods {
    (
        $name:ident,
        next = $next_vis:vis $next_method:ident;
        $(new = $(#[$new_meta:meta])* $new_vis:vis,)?
        $(value = $(#[$value_meta:meta])* $value_vis:vis,)?
    ) => {
        impl_u64_counter_methods!(
            @impl $name,
            ($next_vis) $next_method;
            $(new = $(#[$new_meta])* $new_vis,)?
            $(value = $(#[$value_meta])* $value_vis,)?
        );
    };
    (
        $name:ident,
        next = $next_method:ident;
        $(new = $(#[$new_meta:meta])* $new_vis:vis,)?
        $(value = $(#[$value_meta:meta])* $value_vis:vis,)?
    ) => {
        impl_u64_counter_methods!(
            @impl $name,
            () $next_method;
            $(new = $(#[$new_meta])* $new_vis,)?
            $(value = $(#[$value_meta])* $value_vis,)?
        );
    };
    (
        @impl $name:ident,
        ($($next_vis:tt)*) $next_method:ident;
        $(new = $(#[$new_meta:meta])* $new_vis:vis,)?
        $(value = $(#[$value_meta:meta])* $value_vis:vis,)?
    ) => {
        impl $name {
            $(
                $(#[$new_meta])*
                $new_vis const fn new(value: u64) -> Self {
                    Self(value)
                }
            )?

            $(
                $(#[$value_meta])*
                $value_vis const fn value(self) -> u64 {
                    self.0
                }
            )?

            $($next_vis)* fn next(self) -> Self {
                Self(self.0.$next_method(1))
            }
        }
    };
}

pub(crate) use impl_u64_counter_methods;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct Generation(u64);

impl Generation {
    pub(crate) const INITIAL: Self = Self(0);
}

impl_u64_counter_methods!(
    Generation,
    next = pub(crate) saturating_add;
    new = #[cfg(test)] pub(crate),
    value = pub(crate),
);

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct IngressSeq(u64);

impl IngressSeq {
    pub(crate) const INITIAL: Self = Self(0);
}

impl_u64_counter_methods!(
    IngressSeq,
    next = pub(crate) saturating_add;
    new = #[cfg(test)] pub(crate),
    value = pub(crate),
);

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct ObservationId(u64);

impl ObservationId {
    pub(crate) const fn from_ingress_seq(seq: IngressSeq) -> Self {
        Self(seq.value())
    }

    pub(crate) const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct ProbeRequestId(u64);

impl ProbeRequestId {
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(crate) const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct MotionRevision(u64);

impl MotionRevision {
    pub(crate) const INITIAL: Self = Self(0);
}

impl_u64_counter_methods!(
    MotionRevision,
    next = pub(crate) saturating_add;
    value = pub(crate),
);

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct SemanticRevision(u64);

impl SemanticRevision {
    pub(crate) const INITIAL: Self = Self(0);
}

impl_u64_counter_methods!(
    SemanticRevision,
    next = pub(crate) saturating_add;
    value = pub(crate),
);

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct ConfigRevision(u64);

impl ConfigRevision {
    pub(crate) const INITIAL: Self = Self(0);
}

impl_u64_counter_methods!(
    ConfigRevision,
    next = pub(crate) saturating_add;
);

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct ProjectionPolicyRevision(u64);

impl ProjectionPolicyRevision {
    pub(crate) const INITIAL: Self = Self(0);
}

impl_u64_counter_methods!(
    ProjectionPolicyRevision,
    next = pub(crate) saturating_add;
);

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct RenderRevision {
    motion: MotionRevision,
    semantics: SemanticRevision,
}

impl RenderRevision {
    pub(crate) const INITIAL: Self = Self::new(MotionRevision::INITIAL, SemanticRevision::INITIAL);

    pub(crate) const fn new(motion: MotionRevision, semantics: SemanticRevision) -> Self {
        Self { motion, semantics }
    }

    pub(crate) const fn motion(self) -> MotionRevision {
        self.motion
    }

    pub(crate) const fn semantics(self) -> SemanticRevision {
        self.semantics
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct ProjectorRevision(u64);

impl ProjectorRevision {
    pub(crate) const CURRENT: Self = Self(1);

    pub(crate) const fn value(self) -> u64 {
        self.0
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) enum TimerId {
    Animation,
    Ingress,
    Recovery,
    Cleanup,
}

impl TimerId {
    pub(crate) const ALL: [Self; 4] = [
        Self::Animation,
        Self::Ingress,
        Self::Recovery,
        Self::Cleanup,
    ];
    const NAMES: [&str; 4] = ["animation", "ingress", "recovery", "cleanup"];

    pub(crate) const COUNT: usize = Self::ALL.len();

    pub(crate) const fn slot_index(self) -> usize {
        self as usize
    }

    pub(crate) const fn name(self) -> &'static str {
        Self::NAMES[self.slot_index()]
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct TimerSlots<T> {
    slots: [T; TimerId::COUNT],
}

impl<T> TimerSlots<T>
where
    T: Copy,
{
    pub(crate) const fn filled(value: T) -> Self {
        Self {
            slots: [value; TimerId::COUNT],
        }
    }
}

impl<T> TimerSlots<T> {
    pub(crate) fn from_fn(mut build: impl FnMut(TimerId) -> T) -> Self {
        Self {
            slots: std::array::from_fn(|index| build(TimerId::ALL[index])),
        }
    }

    pub(crate) fn get(&self, timer_id: TimerId) -> &T {
        &self.slots[timer_id.slot_index()]
    }

    pub(crate) fn get_mut(&mut self, timer_id: TimerId) -> &mut T {
        &mut self.slots[timer_id.slot_index()]
    }

    pub(crate) fn replace(&mut self, timer_id: TimerId, value: T) -> T {
        std::mem::replace(self.get_mut(timer_id), value)
    }

    pub(crate) fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.slots.iter_mut()
    }
}

impl<T> TimerSlots<T>
where
    T: Copy,
{
    pub(crate) fn copied(&self, timer_id: TimerId) -> T {
        *self.get(timer_id)
    }
}

impl<T> TimerSlots<Option<T>> {
    pub(crate) fn take(&mut self, timer_id: TimerId) -> Option<T> {
        self.replace(timer_id, None)
    }

    pub(crate) fn clear(&mut self) {
        for slot in self.iter_mut() {
            *slot = None;
        }
    }

    pub(crate) fn take_all(&mut self) -> Vec<T> {
        self.iter_mut().filter_map(Option::take).collect()
    }
}

impl<T> Default for TimerSlots<T>
where
    T: Default,
{
    fn default() -> Self {
        Self::from_fn(|_| T::default())
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct TimerGeneration(u64);

impl TimerGeneration {
    pub(crate) const INITIAL: Self = Self(0);
}

impl_u64_counter_methods!(
    TimerGeneration,
    next = pub(crate) saturating_add;
    new = #[cfg(test)] pub(crate),
    value = pub(crate),
);

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct TimerToken {
    id: TimerId,
    generation: TimerGeneration,
}

impl TimerToken {
    pub(crate) const fn new(id: TimerId, generation: TimerGeneration) -> Self {
        Self { id, generation }
    }

    pub(crate) const fn id(self) -> TimerId {
        self.id
    }

    pub(crate) const fn generation(self) -> TimerGeneration {
        self.generation
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct ProposalId(u64);

impl ProposalId {
    pub(crate) const INITIAL: Self = Self(0);
}

impl_u64_counter_methods!(
    ProposalId,
    next = pub(crate) saturating_add;
    new = #[cfg(test)] pub(crate),
    value = pub(crate),
);

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct Millis(u64);

impl Millis {
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(crate) const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct StepIndex(u64);

impl_u64_counter_methods!(
    StepIndex,
    next = pub(crate) saturating_add;
    new = #[cfg(test)] pub(crate),
    value = pub(crate),
);

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct StrokeId(u64);

impl StrokeId {
    pub(crate) const INITIAL: Self = Self(0);
}

impl_u64_counter_methods!(
    StrokeId,
    next = pub(crate) wrapping_add;
    new = #[cfg(test)] pub(crate),
    value = #[cfg(test)] pub(crate),
);

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct ArcLenQ16(u32);

impl ArcLenQ16 {
    pub(crate) const ZERO: Self = Self(0);

    pub(crate) const fn new(raw_q16: u32) -> Self {
        Self(raw_q16)
    }

    #[cfg(test)]
    pub(crate) const fn value(self) -> u32 {
        self.0
    }

    pub(crate) fn saturating_add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct DelayBudgetMs(u64);

#[derive(Debug, Clone, Copy, Eq, PartialEq, Error)]
pub(crate) enum DelayBudgetMsError {
    #[error("delay budget must be positive")]
    MustBePositive,
}

impl DelayBudgetMs {
    pub(crate) const DEFAULT_ANIMATION: Self = Self(8);

    pub(crate) fn try_new(value: u64) -> Result<Self, DelayBudgetMsError> {
        if value == 0 {
            Err(DelayBudgetMsError::MustBePositive)
        } else {
            Ok(Self(value))
        }
    }

    pub(crate) const fn value(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn timer_slots_follow_the_canonical_timer_id_order() {
        let slots = TimerSlots::from_fn(TimerId::slot_index);

        for timer_id in TimerId::ALL {
            assert_eq!(slots.copied(timer_id), timer_id.slot_index());
        }
    }

    #[test]
    fn timer_id_slot_indices_are_dense_and_zero_based() {
        for timer_id in TimerId::ALL {
            assert_eq!(timer_id.slot_index(), usize::from(timer_id as u8));
        }

        assert_eq!(TimerId::COUNT, 4);
    }

    #[test]
    fn timer_slots_option_helpers_follow_canonical_timer_order() {
        let mut slots = TimerSlots::from_fn(|timer_id| Some(timer_id.slot_index()));

        assert_eq!(
            slots.take(TimerId::Ingress),
            Some(TimerId::Ingress.slot_index())
        );
        assert_eq!(slots.take(TimerId::Ingress), None);
        assert_eq!(
            slots.take_all(),
            vec![
                TimerId::Animation.slot_index(),
                TimerId::Recovery.slot_index(),
                TimerId::Cleanup.slot_index(),
            ]
        );

        slots = TimerSlots::from_fn(|timer_id| Some(timer_id.slot_index()));
        slots.clear();
        assert_eq!(slots.take_all(), Vec::<usize>::new());
    }
}
