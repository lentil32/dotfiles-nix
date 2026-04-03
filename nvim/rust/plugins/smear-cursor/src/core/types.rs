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
pub(crate) struct SceneRevision(u64);

impl SceneRevision {
    #[cfg(test)]
    pub(crate) const INITIAL: Self = Self(0);
}

impl_u64_counter_methods!(
    SceneRevision,
    next = pub(crate) saturating_add;
    value = pub(crate),
);

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct ProjectorRevision(u64);

impl ProjectorRevision {
    pub(crate) const CURRENT: Self = Self(1);

    pub(crate) const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) enum TimerId {
    Animation,
    Ingress,
    Recovery,
    Cleanup,
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

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct CursorRow(pub(crate) u32);

impl CursorRow {
    pub(crate) const fn value(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct CursorCol(pub(crate) u32);

impl CursorCol {
    pub(crate) const fn value(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct CursorPosition {
    pub(crate) row: CursorRow,
    pub(crate) col: CursorCol,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct ViewportSnapshot {
    pub(crate) max_row: CursorRow,
    pub(crate) max_col: CursorCol,
}

impl ViewportSnapshot {
    pub(crate) const fn new(max_row: CursorRow, max_col: CursorCol) -> Self {
        Self { max_row, max_col }
    }
}
