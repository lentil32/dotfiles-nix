use nvimrs_nvim_oxi_utils::handles::BufHandle;

const MAX_RECENT_BUFFERS: usize = 64;

#[derive(Debug, Default, Clone)]
pub struct BufferMruState {
    recent: Vec<BufHandle>,
}

impl BufferMruState {
    pub fn enter(&mut self, buf: BufHandle) {
        self.recent.retain(|existing| *existing != buf);
        self.recent.insert(0, buf);
        self.recent.truncate(MAX_RECENT_BUFFERS);
    }

    pub fn wipe(&mut self, buf: BufHandle) {
        self.recent.retain(|existing| *existing != buf);
    }

    pub fn previous_buffer<F>(&mut self, current: BufHandle, is_valid: F) -> Option<BufHandle>
    where
        F: Fn(BufHandle) -> bool,
    {
        self.recent.retain(|buf| is_valid(*buf));
        self.recent.iter().copied().find(|buf| *buf != current)
    }

    #[cfg(test)]
    fn recent_raw(&self) -> Vec<i64> {
        self.recent.iter().map(|buf| buf.raw()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(id: i64) -> Result<BufHandle, &'static str> {
        BufHandle::try_from_i64(id).ok_or("expected valid buffer handle")
    }

    #[test]
    fn enter_tracks_most_recent_first() -> Result<(), &'static str> {
        let mut state = BufferMruState::default();
        let b10 = buf(10)?;
        let b20 = buf(20)?;

        state.enter(b10);
        state.enter(b20);

        assert_eq!(state.recent_raw(), vec![20, 10]);
        assert_eq!(state.previous_buffer(b20, |_| true), Some(b10));
        Ok(())
    }

    #[test]
    fn reenter_moves_buffer_to_front_without_duplicates() -> Result<(), &'static str> {
        let mut state = BufferMruState::default();
        let b10 = buf(10)?;
        let b20 = buf(20)?;
        let b30 = buf(30)?;

        state.enter(b10);
        state.enter(b20);
        state.enter(b30);
        state.enter(b10);

        assert_eq!(state.recent_raw(), vec![10, 30, 20]);
        assert_eq!(state.previous_buffer(b10, |_| true), Some(b30));
        Ok(())
    }

    #[test]
    fn previous_from_untracked_current_uses_latest_buffer() -> Result<(), &'static str> {
        let mut state = BufferMruState::default();
        let b10 = buf(10)?;
        let b20 = buf(20)?;
        let untracked = buf(99)?;

        state.enter(b10);
        state.enter(b20);

        assert_eq!(state.previous_buffer(untracked, |_| true), Some(b20));
        Ok(())
    }

    #[test]
    fn wipe_removes_buffer() -> Result<(), &'static str> {
        let mut state = BufferMruState::default();
        let b10 = buf(10)?;
        let b20 = buf(20)?;

        state.enter(b10);
        state.enter(b20);
        state.wipe(b20);

        assert_eq!(state.recent_raw(), vec![10]);
        assert_eq!(state.previous_buffer(b20, |_| true), Some(b10));
        Ok(())
    }

    #[test]
    fn previous_prunes_invalid_buffers() -> Result<(), &'static str> {
        let mut state = BufferMruState::default();
        let b10 = buf(10)?;
        let b20 = buf(20)?;
        let b30 = buf(30)?;

        state.enter(b10);
        state.enter(b20);
        state.enter(b30);

        assert_eq!(
            state.previous_buffer(b30, |buf| buf == b10 || buf == b30),
            Some(b10)
        );
        assert_eq!(state.recent_raw(), vec![30, 10]);
        Ok(())
    }
}
