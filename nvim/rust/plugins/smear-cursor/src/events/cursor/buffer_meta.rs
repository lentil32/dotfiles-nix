use nvim_oxi::Result;
use nvim_oxi::api;
use nvim_oxi::api::opts::OptionOpts;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct BufferMetadata {
    filetype: String,
    buftype: String,
    buflisted: bool,
    line_count: usize,
}

impl BufferMetadata {
    pub(crate) fn read(buffer: &api::Buffer) -> Result<Self> {
        let opts = OptionOpts::builder().buf(buffer.clone()).build();
        let filetype: String = api::get_option_value("filetype", &opts)?;
        let buftype: String = api::get_option_value("buftype", &opts)?;
        let buflisted: bool = api::get_option_value("buflisted", &opts)?;

        Ok(Self {
            filetype,
            buftype,
            buflisted,
            line_count: buffer.line_count()?,
        })
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        filetype: &str,
        buftype: &str,
        buflisted: bool,
        line_count: usize,
    ) -> Self {
        Self {
            filetype: filetype.to_string(),
            buftype: buftype.to_string(),
            buflisted,
            line_count,
        }
    }

    pub(crate) fn filetype(&self) -> &str {
        &self.filetype
    }

    pub(crate) fn buftype(&self) -> &str {
        &self.buftype
    }

    pub(crate) const fn buflisted(&self) -> bool {
        self.buflisted
    }

    pub(crate) const fn line_count(&self) -> usize {
        self.line_count
    }
}
