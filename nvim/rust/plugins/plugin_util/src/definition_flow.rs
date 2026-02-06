use std::fmt;

use nvim_oxi::mlua;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionItem {
    pub bufnr: Option<i64>,
    pub filename: Option<String>,
    pub lnum: i64,
    pub col: i64,
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

fn parse_required_positive_field(
    item: &mlua::Table,
    index: usize,
    field: &'static str,
) -> DefinitionParseResult<i64> {
    let value = item
        .get::<Option<i64>>(field)
        .map_err(|_| DefinitionParseError::InvalidField { index, field })?;
    let Some(value) = value else {
        return Err(DefinitionParseError::MissingField { index, field });
    };
    if value <= 0 {
        return Err(DefinitionParseError::InvalidField { index, field });
    }
    Ok(value)
}

fn parse_optional_bufnr(item: &mlua::Table, index: usize) -> DefinitionParseResult<Option<i64>> {
    let value =
        item.get::<Option<i64>>("bufnr")
            .map_err(|_| DefinitionParseError::InvalidField {
                index,
                field: "bufnr",
            })?;
    Ok(value.filter(|entry| *entry > 0))
}

fn parse_optional_filename(
    item: &mlua::Table,
    index: usize,
) -> DefinitionParseResult<Option<String>> {
    let value =
        item.get::<Option<String>>("filename")
            .map_err(|_| DefinitionParseError::InvalidField {
                index,
                field: "filename",
            })?;
    Ok(value.filter(|entry| !entry.is_empty()))
}

fn parse_definition_item(
    item: &mlua::Table,
    index: usize,
) -> DefinitionParseResult<DefinitionItem> {
    let bufnr = parse_optional_bufnr(item, index)?;
    let filename = parse_optional_filename(item, index)?;
    if bufnr.is_none() && filename.is_none() {
        return Err(DefinitionParseError::MissingLocation { index });
    }
    let lnum = parse_required_positive_field(item, index, "lnum")?;
    let col = parse_required_positive_field(item, index, "col")?;
    Ok(DefinitionItem {
        bufnr,
        filename,
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
    use super::{DefinitionAction, DefinitionItem, plan_definition_actions};

    fn definition_item(bufnr: i64, lnum: i64, col: i64) -> DefinitionItem {
        DefinitionItem {
            bufnr: Some(bufnr),
            filename: None,
            lnum,
            col,
        }
    }

    #[test]
    fn plan_definition_actions_closes_when_empty() {
        assert_eq!(
            plan_definition_actions(Vec::new(), None),
            vec![DefinitionAction::CloseCreatedTarget]
        );
    }

    #[test]
    fn plan_definition_actions_opens_primary_for_single_item() {
        assert_eq!(
            plan_definition_actions(vec![definition_item(1, 2, 3)], Some("defs".to_string())),
            vec![DefinitionAction::OpenPrimary(definition_item(1, 2, 3))]
        );
    }

    #[test]
    fn plan_definition_actions_pushes_quickfix_for_multiple_items() {
        assert_eq!(
            plan_definition_actions(
                vec![definition_item(1, 2, 3), definition_item(4, 5, 6)],
                Some("defs".to_string())
            ),
            vec![
                DefinitionAction::OpenPrimary(definition_item(1, 2, 3)),
                DefinitionAction::PushQuickfix {
                    title: Some("defs".to_string()),
                    items: vec![definition_item(1, 2, 3), definition_item(4, 5, 6)],
                }
            ]
        );
    }
}
