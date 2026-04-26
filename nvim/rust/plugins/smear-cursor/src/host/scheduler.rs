use super::NeovimHost;

pub(crate) trait SchedulerPort {
    fn schedule(&self, callback: Box<dyn FnOnce() + 'static>);
}

impl SchedulerPort for NeovimHost {
    fn schedule(&self, callback: Box<dyn FnOnce() + 'static>) {
        nvim_oxi::schedule(move |()| callback());
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum SchedulerCall {
    Schedule,
}

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct FakeSchedulerPort {
    calls: std::cell::RefCell<Vec<SchedulerCall>>,
}

#[cfg(test)]
impl FakeSchedulerPort {
    pub(crate) fn calls(&self) -> Vec<SchedulerCall> {
        self.calls.borrow().clone()
    }
}

#[cfg(test)]
impl SchedulerPort for FakeSchedulerPort {
    fn schedule(&self, callback: Box<dyn FnOnce() + 'static>) {
        self.calls.borrow_mut().push(SchedulerCall::Schedule);
        callback();
    }
}
