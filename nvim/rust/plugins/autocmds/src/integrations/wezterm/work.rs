use std::process::ExitStatus;

use crate::machines::wezterm::WeztermCommand;
use nvimrs_support::TabTitle;

use super::WeztermCommandRunner;

#[derive(Debug)]
pub(super) enum WeztermCommandResult {
    TabTitle {
        title: TabTitle,
        status: std::io::Result<ExitStatus>,
    },
    WorkingDir {
        cwd: String,
        status: std::io::Result<ExitStatus>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WeztermCommandKind {
    TabTitle,
    WorkingDir,
}

impl WeztermCommand {
    const fn kind(&self) -> WeztermCommandKind {
        match self {
            Self::SetTabTitle(_) => WeztermCommandKind::TabTitle,
            Self::SetWorkingDir(_) => WeztermCommandKind::WorkingDir,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WeztermCommandBatch {
    pub(super) first: WeztermCommand,
    pub(super) second: WeztermCommand,
}

impl WeztermCommandBatch {
    fn new(first: WeztermCommand, second: WeztermCommand) -> Self {
        debug_assert!(
            first.kind() != second.kind(),
            "batch pairs are expected to contain distinct command kinds"
        );
        Self { first, second }
    }

    fn for_each<F>(self, mut f: F)
    where
        F: FnMut(WeztermCommand),
    {
        f(self.first);
        f(self.second);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WeztermWorkItem {
    Single(WeztermCommand),
    Batch(WeztermCommandBatch),
}

impl WeztermWorkItem {
    pub(super) fn from_command(command: WeztermCommand) -> Self {
        Self::Single(command)
    }

    pub(super) fn from_optional(
        first: Option<WeztermCommand>,
        second: Option<WeztermCommand>,
    ) -> Option<Self> {
        match (first, second) {
            (Some(first), Some(second)) => {
                Some(Self::Batch(WeztermCommandBatch::new(first, second)))
            }
            (Some(command), None) | (None, Some(command)) => Some(Self::Single(command)),
            (None, None) => None,
        }
    }

    pub(super) fn for_each_command<F>(self, mut f: F)
    where
        F: FnMut(WeztermCommand),
    {
        match self {
            Self::Single(command) => f(command),
            Self::Batch(batch) => batch.for_each(f),
        }
    }
}

pub(super) fn run_wezterm_command(
    command: WeztermCommand,
    runner: &dyn WeztermCommandRunner,
) -> WeztermCommandResult {
    match command {
        WeztermCommand::SetTabTitle(title) => WeztermCommandResult::TabTitle {
            status: runner.run_tab_title(&title),
            title,
        },
        WeztermCommand::SetWorkingDir(cwd) => WeztermCommandResult::WorkingDir {
            status: runner.run_working_dir(&cwd),
            cwd,
        },
    }
}

pub(super) fn command_error_result(
    command: WeztermCommand,
    err: std::io::Error,
) -> WeztermCommandResult {
    match command {
        WeztermCommand::SetTabTitle(title) => WeztermCommandResult::TabTitle {
            title,
            status: Err(err),
        },
        WeztermCommand::SetWorkingDir(cwd) => WeztermCommandResult::WorkingDir {
            cwd,
            status: Err(err),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    type TestResult<T = ()> = std::result::Result<T, &'static str>;

    fn title(value: &str) -> TestResult<TabTitle> {
        TabTitle::try_new(value.to_string()).map_err(|_| "expected non-empty tab title")
    }

    #[test]
    fn work_item_from_optional_preserves_pair_order() -> TestResult {
        let tab = title("batched")?;
        let first = WeztermCommand::SetTabTitle(tab);
        let second = WeztermCommand::SetWorkingDir("/tmp".to_string());
        let Some(work_item) =
            WeztermWorkItem::from_optional(Some(first.clone()), Some(second.clone()))
        else {
            return Err("expected work item");
        };
        let mut seen = Vec::new();
        work_item.for_each_command(|command| seen.push(command));
        assert_eq!(seen, vec![first, second]);
        Ok(())
    }

    #[test]
    fn work_item_from_optional_keeps_single_command_as_single() -> TestResult {
        let command = WeztermCommand::SetTabTitle(title("single")?);

        let work_item = WeztermWorkItem::from_optional(Some(command.clone()), None);

        assert_eq!(work_item, Some(WeztermWorkItem::Single(command)));
        Ok(())
    }
}
