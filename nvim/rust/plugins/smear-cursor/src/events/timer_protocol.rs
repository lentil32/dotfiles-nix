use nvim_oxi::Result;
use std::num::NonZeroI64;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(super) struct HostCallbackId(NonZeroI64);

impl HostCallbackId {
    pub(super) fn try_new(value: i64) -> Result<Self> {
        try_positive_id(value, "callback id").map(Self)
    }

    pub(super) const fn get(self) -> i64 {
        self.0.get()
    }

    pub(super) fn next(counter: &mut u64) -> Self {
        *counter = counter.saturating_add(1);
        let value = i64::try_from(*counter).unwrap_or(i64::MAX);
        let Some(value) = NonZeroI64::new(value) else {
            unreachable!("saturating positive callback id allocator must stay non-zero");
        };
        Self(value)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(super) struct HostTimerId(NonZeroI64);

impl HostTimerId {
    pub(super) fn try_new(value: i64) -> Result<Self> {
        try_positive_id(value, "timer id").map(Self)
    }

    pub(super) const fn get(self) -> i64 {
        self.0.get()
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct FiredHostTimer {
    host_callback_id: HostCallbackId,
    host_timer_id: HostTimerId,
}

impl FiredHostTimer {
    pub(super) fn try_from_raw(host_callback_id: i64, host_timer_id: i64) -> Result<Self> {
        Ok(Self {
            host_callback_id: HostCallbackId::try_new(host_callback_id)?,
            host_timer_id: HostTimerId::try_new(host_timer_id)?,
        })
    }

    #[cfg(test)]
    pub(super) const fn new(host_callback_id: HostCallbackId, host_timer_id: HostTimerId) -> Self {
        Self {
            host_callback_id,
            host_timer_id,
        }
    }

    pub(super) const fn host_callback_id(self) -> HostCallbackId {
        self.host_callback_id
    }

    pub(super) const fn host_timer_id(self) -> HostTimerId {
        self.host_timer_id
    }
}

fn try_positive_id(value: i64, label: &'static str) -> Result<NonZeroI64> {
    NonZeroI64::new(value)
        .filter(|id| id.get() > 0)
        .map_or_else(
            || {
                Err(nvim_oxi::api::Error::Other(format!(
                    "host bridge returned invalid {label}: {value}"
                ))
                .into())
            },
            Ok,
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn host_callback_id(value: i64) -> HostCallbackId {
        HostCallbackId::try_new(value).expect("test callback id must be positive")
    }

    fn host_timer_id(value: i64) -> HostTimerId {
        HostTimerId::try_new(value).expect("test timer id must be positive")
    }

    #[test]
    fn host_callback_id_rejects_non_positive_values() {
        assert!(HostCallbackId::try_new(0).is_err());
        assert!(HostCallbackId::try_new(-7).is_err());
        assert_eq!(host_callback_id(13).get(), 13);
    }

    #[test]
    fn host_timer_id_rejects_non_positive_values() {
        assert!(HostTimerId::try_new(0).is_err());
        assert!(HostTimerId::try_new(-7).is_err());
        assert_eq!(host_timer_id(13).get(), 13);
    }

    #[test]
    fn next_host_callback_id_allocates_positive_monotone_ids() {
        let mut counter = 0;

        let first = HostCallbackId::next(&mut counter);
        let second = HostCallbackId::next(&mut counter);

        assert_eq!(first.get(), 1);
        assert_eq!(second.get(), 2);
    }

    #[test]
    fn fired_host_timer_roundtrip_validates_both_host_witnesses() {
        assert_eq!(
            FiredHostTimer::try_from_raw(7, 17).expect("positive host timer payload should decode"),
            FiredHostTimer::new(host_callback_id(7), host_timer_id(17))
        );

        assert!(FiredHostTimer::try_from_raw(0, 17).is_err());
        assert!(FiredHostTimer::try_from_raw(7, 0).is_err());
    }
}
