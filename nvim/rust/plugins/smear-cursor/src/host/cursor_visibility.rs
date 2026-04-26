use super::NeovimHost;
use super::api;
use nvim_oxi::Result;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HostCursorVisibility {
    Hidden,
    Visible,
}

pub(crate) trait CursorVisibilityPort {
    fn guicursor(&self) -> Result<String>;
    fn set_guicursor(&self, value: &str) -> Result<()>;
    fn set_cursor_highlight_visibility(&self, visibility: HostCursorVisibility) -> Result<()>;
}

impl CursorVisibilityPort for NeovimHost {
    fn guicursor(&self) -> Result<String> {
        let opts = api::opts::OptionOpts::builder().build();
        Ok(api::get_option_value("guicursor", &opts)?)
    }

    fn set_guicursor(&self, value: &str) -> Result<()> {
        let opts = api::opts::OptionOpts::builder().build();
        api::set_option_value("guicursor", value.to_owned(), &opts)?;
        Ok(())
    }

    fn set_cursor_highlight_visibility(&self, visibility: HostCursorVisibility) -> Result<()> {
        let opts = match visibility {
            HostCursorVisibility::Hidden => api::opts::SetHighlightOpts::builder()
                .foreground("white")
                .blend(100)
                .build(),
            HostCursorVisibility::Visible => api::opts::SetHighlightOpts::builder()
                .foreground("none")
                .blend(0)
                .build(),
        };
        api::set_hl(0, "SmearCursorHideable", &opts)?;
        Ok(())
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum CursorVisibilityCall {
    Guicursor,
    SetGuicursor { value: String },
    SetCursorHighlightVisibility { visibility: HostCursorVisibility },
}

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct FakeCursorVisibilityPort {
    calls: std::cell::RefCell<Vec<CursorVisibilityCall>>,
    guicursor: std::cell::RefCell<String>,
}

#[cfg(test)]
impl FakeCursorVisibilityPort {
    pub(crate) fn set_initial_guicursor(&self, value: &str) {
        *self.guicursor.borrow_mut() = value.to_owned();
    }

    pub(crate) fn calls(&self) -> Vec<CursorVisibilityCall> {
        self.calls.borrow().clone()
    }

    fn record(&self, call: CursorVisibilityCall) {
        self.calls.borrow_mut().push(call);
    }
}

#[cfg(test)]
impl CursorVisibilityPort for FakeCursorVisibilityPort {
    fn guicursor(&self) -> Result<String> {
        self.record(CursorVisibilityCall::Guicursor);
        Ok(self.guicursor.borrow().clone())
    }

    fn set_guicursor(&self, value: &str) -> Result<()> {
        self.record(CursorVisibilityCall::SetGuicursor {
            value: value.to_owned(),
        });
        *self.guicursor.borrow_mut() = value.to_owned();
        Ok(())
    }

    fn set_cursor_highlight_visibility(&self, visibility: HostCursorVisibility) -> Result<()> {
        self.record(CursorVisibilityCall::SetCursorHighlightVisibility { visibility });
        Ok(())
    }
}
