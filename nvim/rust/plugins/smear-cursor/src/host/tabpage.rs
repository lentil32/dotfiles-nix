use super::NeovimHost;
use super::TabHandle;
use super::api;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HostTabSnapshot {
    pub(crate) tab_handle: TabHandle,
    pub(crate) tab_number: Option<u32>,
}

pub(crate) trait TabPagePort {
    fn live_tab_snapshot(&self) -> Vec<HostTabSnapshot>;
}

impl TabPagePort for NeovimHost {
    fn live_tab_snapshot(&self) -> Vec<HostTabSnapshot> {
        api::list_tabpages()
            .map(|tabpage| HostTabSnapshot {
                tab_handle: TabHandle::from_tabpage(&tabpage),
                tab_number: tabpage.get_number().ok(),
            })
            .collect()
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum TabPageCall {
    LiveTabSnapshot,
}

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct FakeTabPagePort {
    calls: std::cell::RefCell<Vec<TabPageCall>>,
    live_tabs: std::cell::RefCell<Vec<HostTabSnapshot>>,
}

#[cfg(test)]
impl FakeTabPagePort {
    pub(crate) fn set_live_tabs(&self, live_tabs: Vec<HostTabSnapshot>) {
        *self.live_tabs.borrow_mut() = live_tabs;
    }

    pub(crate) fn calls(&self) -> Vec<TabPageCall> {
        self.calls.borrow().clone()
    }
}

#[cfg(test)]
impl TabPagePort for FakeTabPagePort {
    fn live_tab_snapshot(&self) -> Vec<HostTabSnapshot> {
        self.calls.borrow_mut().push(TabPageCall::LiveTabSnapshot);
        self.live_tabs.borrow().clone()
    }
}
