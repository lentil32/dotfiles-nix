use super::TabHandle;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HostTabSnapshot {
    pub(crate) tab_handle: TabHandle,
    pub(crate) tab_number: Option<u32>,
}
