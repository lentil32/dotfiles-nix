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

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct WindowId(i64);

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum WindowIdError {
    NonPositive(i64),
}

impl WindowId {
    pub(crate) fn try_new(value: i64) -> Result<Self, WindowIdError> {
        if value > 0 {
            Ok(Self(value))
        } else {
            Err(WindowIdError::NonPositive(value))
        }
    }

    pub(crate) const fn value(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct BufferId(i64);

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum BufferIdError {
    NonPositive(i64),
}

impl BufferId {
    pub(crate) fn try_new(value: i64) -> Result<Self, BufferIdError> {
        if value > 0 {
            Ok(Self(value))
        } else {
            Err(BufferIdError::NonPositive(value))
        }
    }

    pub(crate) const fn value(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct TabId(i32);

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum TabIdError {
    NonPositive(i32),
}

impl TabId {
    pub(crate) fn try_new(value: i32) -> Result<Self, TabIdError> {
        if value > 0 {
            Ok(Self(value))
        } else {
            Err(TabIdError::NonPositive(value))
        }
    }

    pub(crate) const fn value(self) -> i32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct Generation(u64);

impl Generation {
    pub(crate) const INITIAL: Self = Self(0);

    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(crate) const fn value(self) -> u64 {
        self.0
    }

    pub(crate) fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct IngressSeq(u64);

impl IngressSeq {
    pub(crate) const INITIAL: Self = Self(0);

    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(crate) const fn value(self) -> u64 {
        self.0
    }

    pub(crate) fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

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
    pub(crate) const INITIAL: Self = Self(0);

    pub(crate) const fn value(self) -> u64 {
        self.0
    }

    pub(crate) fn next(self) -> Self {
        Self(self.0.saturating_add(1))
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

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) enum TimerId {
    Animation,
    Ingress,
    Recovery,
    Cleanup,
}

impl TimerId {
    pub(crate) const fn fingerprint(self) -> u64 {
        match self {
            Self::Animation => 1_u64,
            Self::Ingress => 2_u64,
            Self::Recovery => 3_u64,
            Self::Cleanup => 4_u64,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct TimerGeneration(u64);

impl TimerGeneration {
    pub(crate) const INITIAL: Self = Self(0);

    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(crate) const fn value(self) -> u64 {
        self.0
    }

    pub(crate) fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

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

    pub(crate) const fn fingerprint(self) -> u64 {
        let id_mix = self.id.fingerprint().wrapping_mul(0x9E37_79B1_85EB_CA87);
        let generation_mix = self.generation.value().wrapping_mul(0x94D0_49BB_1331_11EB);
        id_mix.rotate_left(17) ^ generation_mix
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct FrameId(u64);

impl FrameId {
    pub(crate) const INITIAL: Self = Self(0);

    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(crate) const fn value(self) -> u64 {
        self.0
    }

    pub(crate) fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct ProposalId(u64);

impl ProposalId {
    pub(crate) const INITIAL: Self = Self(0);

    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(crate) const fn value(self) -> u64 {
        self.0
    }

    pub(crate) fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

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

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct NonNegativeMs(u64);

impl NonNegativeMs {
    // This is intentionally distinct from `Millis` even though both wrap `u64`.
    // The type separates absolute timestamps from normalized non-negative durations.
    pub(crate) const ZERO: Self = Self(0);

    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(crate) const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct Q16(i32);

impl Q16 {
    pub(crate) const ZERO: Self = Self(0);
    pub(crate) const FRACTION_BITS: u32 = 16;
    pub(crate) const SCALE: i32 = 1 << Self::FRACTION_BITS;

    pub(crate) const fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub(crate) fn from_int(value: i32) -> Self {
        Self(value.saturating_mul(Self::SCALE))
    }

    pub(crate) const fn raw(self) -> i32 {
        self.0
    }

    pub(crate) fn trunc_to_int(self) -> i32 {
        self.0 / Self::SCALE
    }

    pub(crate) fn saturating_add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }

    pub(crate) fn saturating_sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct StepIndex(u64);

impl StepIndex {
    pub(crate) const INITIAL: Self = Self(0);

    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(crate) const fn value(self) -> u64 {
        self.0
    }

    pub(crate) fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct StrokeId(u64);

impl StrokeId {
    pub(crate) const INITIAL: Self = Self(0);

    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(crate) const fn value(self) -> u64 {
        self.0
    }

    pub(crate) fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct ArcLenQ16(u32);

impl ArcLenQ16 {
    pub(crate) const ZERO: Self = Self(0);

    pub(crate) const fn new(raw_q16: u32) -> Self {
        Self(raw_q16)
    }

    pub(crate) const fn value(self) -> u32 {
        self.0
    }

    pub(crate) fn saturating_add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct DelayBudgetMs(u64);

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum DelayBudgetMsError {
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

impl CursorPosition {
    pub(crate) fn fingerprint(self) -> u64 {
        (u64::from(self.row.value()) << 32) | u64::from(self.col.value())
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) enum CursorMode {
    Normal,
    Insert,
    Visual,
    Select,
    Replace,
    Command,
    Terminal,
    Other,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct CursorSnapshot {
    pub(crate) position: CursorPosition,
    pub(crate) window_id: WindowId,
    pub(crate) buffer_id: BufferId,
    pub(crate) tab_id: TabId,
    pub(crate) mode: CursorMode,
}

impl CursorSnapshot {
    pub(crate) const fn new(
        position: CursorPosition,
        window_id: WindowId,
        buffer_id: BufferId,
        tab_id: TabId,
        mode: CursorMode,
    ) -> Self {
        Self {
            position,
            window_id,
            buffer_id,
            tab_id,
            mode,
        }
    }
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

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum PoolSnapshotError {
    InUseExceedsTotal {
        total_windows: u32,
        in_use_windows: u32,
    },
    AvailableExceedsTotal {
        total_windows: u32,
        available_windows: u32,
    },
    CapacityMismatch {
        total_windows: u32,
        in_use_windows: u32,
        available_windows: u32,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct PoolSnapshot {
    pub(crate) total_windows: u32,
    pub(crate) in_use_windows: u32,
    pub(crate) available_windows: u32,
}

impl PoolSnapshot {
    pub(crate) fn try_new(
        total_windows: u32,
        in_use_windows: u32,
        available_windows: u32,
    ) -> Result<Self, PoolSnapshotError> {
        if in_use_windows > total_windows {
            return Err(PoolSnapshotError::InUseExceedsTotal {
                total_windows,
                in_use_windows,
            });
        }

        if available_windows > total_windows {
            return Err(PoolSnapshotError::AvailableExceedsTotal {
                total_windows,
                available_windows,
            });
        }

        let expected_available = total_windows - in_use_windows;
        if available_windows != expected_available {
            return Err(PoolSnapshotError::CapacityMismatch {
                total_windows,
                in_use_windows,
                available_windows,
            });
        }

        Ok(Self {
            total_windows,
            in_use_windows,
            available_windows,
        })
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct RenderRequest {
    pub(crate) frame_id: FrameId,
    pub(crate) cursor: CursorSnapshot,
    pub(crate) viewport: ViewportSnapshot,
    pub(crate) pool: PoolSnapshot,
}

impl RenderRequest {
    pub(crate) const fn new(
        frame_id: FrameId,
        cursor: CursorSnapshot,
        viewport: ViewportSnapshot,
        pool: PoolSnapshot,
    ) -> Self {
        Self {
            frame_id,
            cursor,
            viewport,
            pool,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) enum RenderOutcome {
    AppliedFully,
    Degraded,
    Failed,
}

impl RenderOutcome {
    pub(crate) const fn is_success(self) -> bool {
        match self {
            Self::AppliedFully | Self::Degraded => true,
            Self::Failed => false,
        }
    }
}

pub(crate) fn phase1_types_fingerprint() -> u64 {
    let generation = Generation::new(1).next().value();
    let timer_generation = TimerGeneration::new(4).next();
    let timer_token_animation = TimerToken::new(TimerId::Animation, timer_generation);
    let timer_token_ingress = TimerToken::new(TimerId::Ingress, TimerGeneration::new(2));
    let timer_token_recovery = TimerToken::new(TimerId::Recovery, TimerGeneration::INITIAL.next());
    let timer_token_cleanup = TimerToken::new(TimerId::Cleanup, TimerGeneration::new(3));
    let proposal_id = ProposalId::new(9).next();
    let ingress_seq = IngressSeq::new(2).next();
    let observation_id = ObservationId::from_ingress_seq(ingress_seq);
    let scene_revision = SceneRevision::INITIAL.next();
    let projector_revision = ProjectorRevision::CURRENT;
    let timer_seed = timer_generation.value()
        ^ timer_token_animation.fingerprint()
        ^ timer_token_ingress.fingerprint()
        ^ timer_token_recovery.fingerprint()
        ^ timer_token_cleanup.fingerprint()
        ^ timer_token_animation.generation().value()
        ^ timer_token_ingress.id().fingerprint()
        ^ timer_token_recovery.id().fingerprint()
        ^ timer_token_cleanup.id().fingerprint()
        ^ proposal_id.value()
        ^ ingress_seq.value()
        ^ observation_id.value()
        ^ scene_revision.value()
        ^ projector_revision.value();
    let frame_from_initial = FrameId::INITIAL.next();
    let frame_explicit = FrameId::new(9);
    let frame_seed = frame_from_initial.value() ^ frame_explicit.value();

    let timestamp_seed =
        Millis::new(33).value() ^ NonNegativeMs::ZERO.value() ^ NonNegativeMs::new(7).value();
    let q16_seed = u64::from(Q16::ZERO.raw().unsigned_abs())
        ^ u64::from(Q16::from_int(3).trunc_to_int().unsigned_abs())
        ^ u64::from(
            Q16::from_raw(-16_384)
                .saturating_add(Q16::from_raw(8_192))
                .saturating_sub(Q16::from_raw(4_096))
                .raw()
                .unsigned_abs(),
        );
    let step_seed = StepIndex::INITIAL.next().value() ^ StepIndex::new(17).next().value();
    let stroke_seed = StrokeId::INITIAL.next().value() ^ StrokeId::new(u64::MAX).next().value();
    let arc_seed = u64::from(
        ArcLenQ16::ZERO
            .saturating_add(ArcLenQ16::new(128))
            .saturating_add(ArcLenQ16::new(256))
            .value(),
    );

    let window_id = match WindowId::try_new(10) {
        Ok(value) => value,
        Err(_) => return generation ^ frame_seed ^ timestamp_seed,
    };
    let buffer_id = match BufferId::try_new(20) {
        Ok(value) => value,
        Err(_) => return generation ^ frame_seed ^ timestamp_seed,
    };
    let tab_id = match TabId::try_new(30) {
        Ok(value) => value,
        Err(_) => return generation ^ frame_seed ^ timestamp_seed,
    };

    let window_error_seed = match WindowId::try_new(0) {
        Ok(value) => value.value().unsigned_abs(),
        Err(WindowIdError::NonPositive(value)) => value.unsigned_abs(),
    };
    let buffer_error_seed = match BufferId::try_new(0) {
        Ok(value) => value.value().unsigned_abs(),
        Err(BufferIdError::NonPositive(value)) => value.unsigned_abs(),
    };
    let tab_error_seed = match TabId::try_new(0) {
        Ok(value) => u64::from(value.value().unsigned_abs()),
        Err(TabIdError::NonPositive(value)) => u64::from(value.unsigned_abs()),
    };

    let delay_ok = match DelayBudgetMs::try_new(8) {
        Ok(value) => value.value(),
        Err(_) => 0,
    };
    let delay_error_seed = match DelayBudgetMs::try_new(0) {
        Ok(value) => value.value(),
        Err(DelayBudgetMsError::MustBePositive) => 1,
    };

    let cursor = CursorSnapshot::new(
        CursorPosition {
            row: CursorRow(5),
            col: CursorCol(8),
        },
        window_id,
        buffer_id,
        tab_id,
        CursorMode::Normal,
    );
    let viewport = ViewportSnapshot::new(CursorRow(120), CursorCol(240));

    let pool = match PoolSnapshot::try_new(6, 2, 4) {
        Ok(value) => value,
        Err(_) => return generation ^ frame_seed ^ timestamp_seed ^ delay_ok ^ delay_error_seed,
    };
    let pool_in_use_error = match PoolSnapshot::try_new(4, 5, 0) {
        Ok(_) => 0,
        Err(PoolSnapshotError::InUseExceedsTotal {
            total_windows,
            in_use_windows,
        }) => u64::from(total_windows) ^ u64::from(in_use_windows),
        Err(_) => 0,
    };
    let pool_available_error = match PoolSnapshot::try_new(4, 1, 5) {
        Ok(_) => 0,
        Err(PoolSnapshotError::AvailableExceedsTotal {
            total_windows,
            available_windows,
        }) => u64::from(total_windows) ^ u64::from(available_windows),
        Err(_) => 0,
    };
    let pool_capacity_error = match PoolSnapshot::try_new(4, 1, 1) {
        Ok(_) => 0,
        Err(PoolSnapshotError::CapacityMismatch {
            total_windows,
            in_use_windows,
            available_windows,
        }) => u64::from(total_windows) ^ u64::from(in_use_windows) ^ u64::from(available_windows),
        Err(_) => 0,
    };

    let request = RenderRequest::new(frame_from_initial, cursor, viewport, pool);

    let mode_seed = [
        CursorMode::Normal,
        CursorMode::Insert,
        CursorMode::Visual,
        CursorMode::Select,
        CursorMode::Replace,
        CursorMode::Command,
        CursorMode::Terminal,
        CursorMode::Other,
    ]
    .iter()
    .copied()
    .enumerate()
    .fold(0_u64, |acc, (index, mode)| {
        let mode_value = match mode {
            CursorMode::Normal => 1_u64,
            CursorMode::Insert => 2_u64,
            CursorMode::Visual => 3_u64,
            CursorMode::Select => 4_u64,
            CursorMode::Replace => 5_u64,
            CursorMode::Command => 6_u64,
            CursorMode::Terminal => 7_u64,
            CursorMode::Other => 8_u64,
        };
        acc ^ ((index as u64 + 1) * mode_value)
    });

    let outcome_seed = [
        RenderOutcome::AppliedFully,
        RenderOutcome::Degraded,
        RenderOutcome::Failed,
    ]
    .iter()
    .copied()
    .enumerate()
    .fold(0_u64, |acc, (index, outcome)| {
        let success = if outcome.is_success() { 1_u64 } else { 0_u64 };
        acc ^ ((index as u64 + 1) ^ success)
    });

    generation
        ^ timer_seed
        ^ frame_seed
        ^ timestamp_seed
        ^ window_error_seed
        ^ buffer_error_seed
        ^ tab_error_seed
        ^ delay_ok
        ^ delay_error_seed
        ^ pool_in_use_error
        ^ pool_available_error
        ^ pool_capacity_error
        ^ request.frame_id.value()
        ^ request.cursor.window_id.value().unsigned_abs()
        ^ request.cursor.buffer_id.value().unsigned_abs()
        ^ u64::from(request.cursor.tab_id.value().unsigned_abs())
        ^ q16_seed
        ^ step_seed
        ^ stroke_seed
        ^ arc_seed
        ^ mode_seed
        ^ outcome_seed
}
