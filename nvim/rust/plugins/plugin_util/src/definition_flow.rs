use std::fmt;

use nvim_oxi::mlua;
use support::NonEmptyString;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferNumber(i64);

impl BufferNumber {
    pub const fn try_new(raw: i64) -> Option<Self> {
        if raw > 0 { Some(Self(raw)) } else { None }
    }

    pub const fn raw(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineNumber(i64);

impl LineNumber {
    pub const fn try_new(raw: i64) -> Option<Self> {
        if raw > 0 { Some(Self(raw)) } else { None }
    }

    pub const fn raw(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnNumber(i64);

impl ColumnNumber {
    pub const fn try_new(raw: i64) -> Option<Self> {
        if raw > 0 { Some(Self(raw)) } else { None }
    }

    pub const fn raw(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefinitionLocation {
    BufferOnly(BufferNumber),
    FileOnly(NonEmptyString),
    BufferAndFile {
        bufnr: BufferNumber,
        filename: NonEmptyString,
    },
}

impl DefinitionLocation {
    pub const fn bufnr(&self) -> Option<i64> {
        match self {
            Self::BufferOnly(bufnr) | Self::BufferAndFile { bufnr, .. } => Some(bufnr.raw()),
            Self::FileOnly(_) => None,
        }
    }

    pub fn filename(&self) -> Option<&str> {
        match self {
            Self::FileOnly(filename) | Self::BufferAndFile { filename, .. } => {
                Some(filename.as_str())
            }
            Self::BufferOnly(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionItem {
    location: DefinitionLocation,
    lnum: LineNumber,
    col: ColumnNumber,
}

impl DefinitionItem {
    pub const fn bufnr(&self) -> Option<i64> {
        self.location.bufnr()
    }

    pub fn filename(&self) -> Option<&str> {
        self.location.filename()
    }

    pub const fn lnum(&self) -> i64 {
        self.lnum.raw()
    }

    pub const fn col(&self) -> i64 {
        self.col.raw()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefinitionParseError {
    InvalidItemType { index: usize },
    MissingLocation { index: usize },
    MissingField { index: usize, field: &'static str },
    InvalidField { index: usize, field: &'static str },
}

impl fmt::Display for DefinitionParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidItemType { index } => {
                write!(f, "definition item #{index} has invalid type")
            }
            Self::MissingLocation { index } => {
                write!(f, "definition item #{index} is missing bufnr/filename")
            }
            Self::MissingField { index, field } => {
                write!(f, "definition item #{index} missing field `{field}`")
            }
            Self::InvalidField { index, field } => {
                write!(f, "definition item #{index} has invalid field `{field}`")
            }
        }
    }
}

type DefinitionParseResult<T> = std::result::Result<T, DefinitionParseError>;

fn parse_required_positive_field<T, F>(
    item: &mlua::Table,
    index: usize,
    field: &'static str,
    parse: F,
) -> DefinitionParseResult<T>
where
    F: FnOnce(i64) -> Option<T>,
{
    let value = item
        .get::<Option<i64>>(field)
        .map_err(|_| DefinitionParseError::InvalidField { index, field })?;
    let Some(value) = value else {
        return Err(DefinitionParseError::MissingField { index, field });
    };
    parse(value).ok_or(DefinitionParseError::InvalidField { index, field })
}

fn parse_optional_bufnr(
    item: &mlua::Table,
    index: usize,
) -> DefinitionParseResult<Option<BufferNumber>> {
    let value =
        item.get::<Option<i64>>("bufnr")
            .map_err(|_| DefinitionParseError::InvalidField {
                index,
                field: "bufnr",
            })?;
    Ok(value.and_then(BufferNumber::try_new))
}

fn parse_optional_filename(
    item: &mlua::Table,
    index: usize,
) -> DefinitionParseResult<Option<NonEmptyString>> {
    let value =
        item.get::<Option<String>>("filename")
            .map_err(|_| DefinitionParseError::InvalidField {
                index,
                field: "filename",
            })?;
    Ok(value.and_then(|entry| NonEmptyString::try_new(entry).ok()))
}

fn parse_definition_item(
    item: &mlua::Table,
    index: usize,
) -> DefinitionParseResult<DefinitionItem> {
    let bufnr = parse_optional_bufnr(item, index)?;
    let filename = parse_optional_filename(item, index)?;
    let location = match (bufnr, filename) {
        (None, None) => return Err(DefinitionParseError::MissingLocation { index }),
        (Some(bufnr), None) => DefinitionLocation::BufferOnly(bufnr),
        (None, Some(filename)) => DefinitionLocation::FileOnly(filename),
        (Some(bufnr), Some(filename)) => DefinitionLocation::BufferAndFile { bufnr, filename },
    };
    let lnum = parse_required_positive_field(item, index, "lnum", LineNumber::try_new)?;
    let col = parse_required_positive_field(item, index, "col", ColumnNumber::try_new)?;
    Ok(DefinitionItem {
        location,
        lnum,
        col,
    })
}

pub fn parse_definition_items(items: &mlua::Table) -> DefinitionParseResult<Vec<DefinitionItem>> {
    let len = items.raw_len();
    let mut parsed = Vec::with_capacity(len);
    for index in 1..=len {
        let entry = items
            .raw_get::<mlua::Table>(index)
            .map_err(|_| DefinitionParseError::InvalidItemType { index })?;
        parsed.push(parse_definition_item(&entry, index)?);
    }
    Ok(parsed)
}

pub fn parse_definition_title(opts: &mlua::Table) -> Option<String> {
    opts.get::<Option<String>>("title")
        .ok()
        .flatten()
        .filter(|title| !title.is_empty())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefinitionAction {
    CloseCreatedTarget,
    OpenPrimary(DefinitionItem),
    PushQuickfix {
        title: Option<String>,
        items: Vec<DefinitionItem>,
    },
}

pub fn plan_definition_actions(
    items: Vec<DefinitionItem>,
    title: Option<String>,
) -> Vec<DefinitionAction> {
    let Some((primary, rest)) = items.split_first() else {
        return vec![DefinitionAction::CloseCreatedTarget];
    };

    let mut actions = vec![DefinitionAction::OpenPrimary(primary.clone())];
    if !rest.is_empty() {
        actions.push(DefinitionAction::PushQuickfix { title, items });
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::{
        BufferNumber, ColumnNumber, DefinitionAction, DefinitionItem, DefinitionLocation,
        LineNumber, plan_definition_actions,
    };

    fn definition_item(bufnr: i64, lnum: i64, col: i64) -> Result<DefinitionItem, &'static str> {
        let bufnr = BufferNumber::try_new(bufnr).ok_or("expected valid bufnr")?;
        let lnum = LineNumber::try_new(lnum).ok_or("expected valid line")?;
        let col = ColumnNumber::try_new(col).ok_or("expected valid column")?;
        Ok(DefinitionItem {
            location: DefinitionLocation::BufferOnly(bufnr),
            lnum,
            col,
        })
    }

    #[test]
    fn plan_definition_actions_closes_when_empty() {
        assert_eq!(
            plan_definition_actions(Vec::new(), None),
            vec![DefinitionAction::CloseCreatedTarget]
        );
    }

    #[test]
    fn plan_definition_actions_opens_primary_for_single_item() -> Result<(), &'static str> {
        assert_eq!(
            plan_definition_actions(vec![definition_item(1, 2, 3)?], Some("defs".to_string())),
            vec![DefinitionAction::OpenPrimary(definition_item(1, 2, 3)?)]
        );
        Ok(())
    }

    #[test]
    fn plan_definition_actions_pushes_quickfix_for_multiple_items() -> Result<(), &'static str> {
        assert_eq!(
            plan_definition_actions(
                vec![definition_item(1, 2, 3)?, definition_item(4, 5, 6)?],
                Some("defs".to_string())
            ),
            vec![
                DefinitionAction::OpenPrimary(definition_item(1, 2, 3)?),
                DefinitionAction::PushQuickfix {
                    title: Some("defs".to_string()),
                    items: vec![definition_item(1, 2, 3)?, definition_item(4, 5, 6)?],
                }
            ]
        );
        Ok(())
    }
}
