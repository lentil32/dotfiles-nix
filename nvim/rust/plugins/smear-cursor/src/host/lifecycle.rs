use super::NeovimHost;
use super::api;
use nvim_oxi::Result;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AutocmdGroupId(u32);

pub(crate) trait LifecyclePort {
    fn clear_autocmd_group(&self, group_name: &str) -> Result<()>;
    fn create_autocmd_group(&self, group_name: &str) -> Result<AutocmdGroupId>;
    fn create_autocmd_dispatch(
        &self,
        group: AutocmdGroupId,
        event: &str,
        command: &str,
    ) -> Result<()>;
    fn delete_user_command(&self, name: &str) -> Result<()>;
    fn create_string_user_command(&self, name: &str, command: &str) -> Result<()>;
}

impl LifecyclePort for NeovimHost {
    fn clear_autocmd_group(&self, group_name: &str) -> Result<()> {
        let opts = api::opts::CreateAugroupOpts::builder().clear(true).build();
        api::create_augroup(group_name, &opts)?;
        Ok(())
    }

    fn create_autocmd_group(&self, group_name: &str) -> Result<AutocmdGroupId> {
        let opts = api::opts::CreateAugroupOpts::builder().clear(true).build();
        Ok(AutocmdGroupId(api::create_augroup(group_name, &opts)?))
    }

    fn create_autocmd_dispatch(
        &self,
        group: AutocmdGroupId,
        event: &str,
        command: &str,
    ) -> Result<()> {
        let opts = api::opts::CreateAutocmdOpts::builder()
            .group(group.0)
            .command(command)
            .build();
        api::create_autocmd([event], &opts)?;
        Ok(())
    }

    fn delete_user_command(&self, name: &str) -> Result<()> {
        api::del_user_command(name)?;
        Ok(())
    }

    fn create_string_user_command(&self, name: &str, command: &str) -> Result<()> {
        api::create_user_command(
            name,
            command,
            &api::opts::CreateCommandOpts::builder().build(),
        )?;
        Ok(())
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum LifecycleCall {
    ClearAutocmdGroup {
        group_name: String,
    },
    CreateAutocmdGroup {
        group_name: String,
    },
    CreateAutocmdDispatch {
        group: AutocmdGroupId,
        event: String,
        command: String,
    },
    DeleteUserCommand {
        name: String,
    },
    CreateStringUserCommand {
        name: String,
        command: String,
    },
}

#[cfg(test)]
#[derive(Debug)]
pub(crate) struct FakeLifecyclePort {
    calls: std::cell::RefCell<Vec<LifecycleCall>>,
    group_id: std::cell::Cell<AutocmdGroupId>,
}

#[cfg(test)]
impl Default for FakeLifecyclePort {
    fn default() -> Self {
        Self {
            calls: std::cell::RefCell::new(Vec::new()),
            group_id: std::cell::Cell::new(AutocmdGroupId(1)),
        }
    }
}

#[cfg(test)]
impl FakeLifecyclePort {
    pub(crate) fn set_group_id(&self, group_id: u32) {
        self.group_id.set(AutocmdGroupId(group_id));
    }

    pub(crate) fn calls(&self) -> Vec<LifecycleCall> {
        self.calls.borrow().clone()
    }

    fn record(&self, call: LifecycleCall) {
        self.calls.borrow_mut().push(call);
    }
}

#[cfg(test)]
impl LifecyclePort for FakeLifecyclePort {
    fn clear_autocmd_group(&self, group_name: &str) -> Result<()> {
        self.record(LifecycleCall::ClearAutocmdGroup {
            group_name: group_name.to_owned(),
        });
        Ok(())
    }

    fn create_autocmd_group(&self, group_name: &str) -> Result<AutocmdGroupId> {
        self.record(LifecycleCall::CreateAutocmdGroup {
            group_name: group_name.to_owned(),
        });
        Ok(self.group_id.get())
    }

    fn create_autocmd_dispatch(
        &self,
        group: AutocmdGroupId,
        event: &str,
        command: &str,
    ) -> Result<()> {
        self.record(LifecycleCall::CreateAutocmdDispatch {
            group,
            event: event.to_owned(),
            command: command.to_owned(),
        });
        Ok(())
    }

    fn delete_user_command(&self, name: &str) -> Result<()> {
        self.record(LifecycleCall::DeleteUserCommand {
            name: name.to_owned(),
        });
        Ok(())
    }

    fn create_string_user_command(&self, name: &str, command: &str) -> Result<()> {
        self.record(LifecycleCall::CreateStringUserCommand {
            name: name.to_owned(),
            command: command.to_owned(),
        });
        Ok(())
    }
}
