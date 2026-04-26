#[cfg(test)]
use super::BufferHandle;
use super::NeovimHost;
use super::api;
use nvim_oxi::Result;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BufferMetadataSnapshot {
    filetype: String,
    buftype: String,
    buflisted: bool,
    line_count: usize,
}

impl BufferMetadataSnapshot {
    pub(crate) fn new(
        filetype: impl Into<String>,
        buftype: impl Into<String>,
        buflisted: bool,
        line_count: usize,
    ) -> Self {
        Self {
            filetype: filetype.into(),
            buftype: buftype.into(),
            buflisted,
            line_count,
        }
    }

    pub(crate) fn into_parts(self) -> (String, String, bool, usize) {
        (self.filetype, self.buftype, self.buflisted, self.line_count)
    }
}

pub(crate) trait BufferMetadataPort {
    fn buffer_metadata(&self, buffer: &api::Buffer) -> Result<BufferMetadataSnapshot>;
}

impl BufferMetadataPort for NeovimHost {
    fn buffer_metadata(&self, buffer: &api::Buffer) -> Result<BufferMetadataSnapshot> {
        let opts = api::opts::OptionOpts::builder().buf(buffer.clone()).build();
        let filetype: String = api::get_option_value("filetype", &opts)?;
        let buftype: String = api::get_option_value("buftype", &opts)?;
        let buflisted: bool = api::get_option_value("buflisted", &opts)?;

        Ok(BufferMetadataSnapshot::new(
            filetype,
            buftype,
            buflisted,
            buffer.line_count()?,
        ))
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum BufferMetadataCall {
    BufferMetadata { buffer_handle: BufferHandle },
}

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct FakeBufferMetadataPort {
    calls: std::cell::RefCell<Vec<BufferMetadataCall>>,
    metadata_results:
        std::cell::RefCell<std::collections::VecDeque<Result<BufferMetadataSnapshot>>>,
}

#[cfg(test)]
impl FakeBufferMetadataPort {
    pub(crate) fn push_buffer_metadata(&self, metadata: BufferMetadataSnapshot) {
        self.metadata_results.borrow_mut().push_back(Ok(metadata));
    }

    pub(crate) fn calls(&self) -> Vec<BufferMetadataCall> {
        self.calls.borrow().clone()
    }

    fn record(&self, call: BufferMetadataCall) {
        self.calls.borrow_mut().push(call);
    }
}

#[cfg(test)]
impl BufferMetadataPort for FakeBufferMetadataPort {
    fn buffer_metadata(&self, buffer: &api::Buffer) -> Result<BufferMetadataSnapshot> {
        self.record(BufferMetadataCall::BufferMetadata {
            buffer_handle: BufferHandle::from_buffer(buffer),
        });
        super::pop_fake_response(&self.metadata_results, "buffer metadata")
    }
}
