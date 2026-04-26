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
