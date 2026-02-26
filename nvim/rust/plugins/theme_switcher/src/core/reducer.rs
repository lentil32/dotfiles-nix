use std::collections::BTreeMap;

use nonempty::NonEmpty;
use nvim_oxi_utils::state_machine::{Machine, NoCommand, Transition};
use support::NonEmptyString;

/// Why constructing a theme entry failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeSpecError {
    EmptyName,
    EmptyColorscheme,
}

impl std::fmt::Display for ThemeSpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyName => write!(f, "theme name must be non-empty"),
            Self::EmptyColorscheme => write!(f, "theme colorscheme must be non-empty"),
        }
    }
}

impl std::error::Error for ThemeSpecError {}

/// Immutable theme data used by the switcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeSpec {
    name: NonEmptyString,
    colorscheme: NonEmptyString,
}

impl ThemeSpec {
    pub fn try_new(name: String, colorscheme: String) -> Result<Self, ThemeSpecError> {
        let name = NonEmptyString::try_new(name).map_err(|_| ThemeSpecError::EmptyName)?;
        let colorscheme =
            NonEmptyString::try_new(colorscheme).map_err(|_| ThemeSpecError::EmptyColorscheme)?;
        Ok(Self { name, colorscheme })
    }

    pub const fn name(&self) -> &NonEmptyString {
        &self.name
    }

    pub const fn colorscheme(&self) -> &NonEmptyString {
        &self.colorscheme
    }
}

/// Why constructing a catalog failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyThemeCatalogError;

impl std::fmt::Display for EmptyThemeCatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "theme catalog must contain at least one theme")
    }
}

impl std::error::Error for EmptyThemeCatalogError {}

/// Opaque index into `ThemeCatalog`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct ThemeIndex(usize);

impl ThemeIndex {
    pub const fn raw(self) -> usize {
        self.0
    }
}

/// Non-empty theme list with wrapped cursor navigation helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeCatalog {
    themes: NonEmpty<ThemeSpec>,
    colorscheme_to_index: BTreeMap<String, ThemeIndex>,
}

impl ThemeCatalog {
    fn build_colorscheme_index(themes: &NonEmpty<ThemeSpec>) -> BTreeMap<String, ThemeIndex> {
        let mut colorscheme_to_index = BTreeMap::new();
        for (raw, theme) in themes.iter().enumerate() {
            colorscheme_to_index
                .entry(theme.colorscheme().as_str().to_string())
                .or_insert(ThemeIndex(raw));
        }
        colorscheme_to_index
    }

    fn from_themes(themes: NonEmpty<ThemeSpec>) -> Self {
        let colorscheme_to_index = Self::build_colorscheme_index(&themes);
        Self {
            themes,
            colorscheme_to_index,
        }
    }

    pub fn try_from_vec(themes: Vec<ThemeSpec>) -> Result<Self, EmptyThemeCatalogError> {
        NonEmpty::from_vec(themes)
            .map(Self::from_themes)
            .ok_or(EmptyThemeCatalogError)
    }

    pub fn from_nonempty(themes: NonEmpty<ThemeSpec>) -> Self {
        Self::from_themes(themes)
    }

    pub fn len(&self) -> usize {
        self.themes.len()
    }

    pub fn first_index(&self) -> ThemeIndex {
        ThemeIndex(0)
    }

    pub fn last_index(&self) -> ThemeIndex {
        ThemeIndex(self.len() - 1)
    }

    pub fn index(&self, raw: usize) -> Option<ThemeIndex> {
        (raw < self.len()).then_some(ThemeIndex(raw))
    }

    pub fn get(&self, index: ThemeIndex) -> Option<&ThemeSpec> {
        self.themes.get(index.0)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ThemeSpec> {
        self.themes.iter()
    }

    pub fn find_by_colorscheme(&self, colorscheme: &str) -> Option<ThemeIndex> {
        self.colorscheme_to_index.get(colorscheme).copied()
    }

    fn next_wrapped(&self, current: ThemeIndex) -> Option<ThemeIndex> {
        self.get(current)?;
        let len = self.len();
        if len <= 1 {
            return Some(current);
        }
        Some(ThemeIndex((current.0 + 1) % len))
    }

    fn prev_wrapped(&self, current: ThemeIndex) -> Option<ThemeIndex> {
        self.get(current)?;
        let len = self.len();
        if len <= 1 {
            return Some(current);
        }
        let prev = if current.0 == 0 {
            len - 1
        } else {
            current.0 - 1
        };
        Some(ThemeIndex(prev))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerStatus {
    Active,
    Closed,
}

/// Input events for the theme picker state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeSwitcherEvent {
    MoveNext,
    MovePrev,
    Confirm,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeCycleDirection {
    Next,
    Prev,
}

/// Computes the one-step cycle target for a theme list.
///
/// Semantics:
/// - known current theme: move one step in the requested direction with wrap-around.
/// - unknown current theme: `Next` chooses first, `Prev` chooses last.
pub fn cycle_theme_index_from_index(
    catalog: &ThemeCatalog,
    current_index: Option<ThemeIndex>,
    direction: ThemeCycleDirection,
) -> ThemeIndex {
    match direction {
        ThemeCycleDirection::Next => match current_index {
            Some(index) => catalog.next_wrapped(index).unwrap_or(index),
            None => catalog.first_index(),
        },
        ThemeCycleDirection::Prev => match current_index {
            Some(index) => catalog.prev_wrapped(index).unwrap_or(index),
            None => catalog.last_index(),
        },
    }
}

/// Computes the one-step cycle target for a theme list from a colorscheme name.
///
/// Semantics:
/// - known current theme: move one step in the requested direction with wrap-around.
/// - unknown current theme: `Next` chooses first, `Prev` chooses last.
pub fn cycle_theme_index(
    catalog: &ThemeCatalog,
    current_colorscheme: Option<&str>,
    direction: ThemeCycleDirection,
) -> ThemeIndex {
    let current_index = current_colorscheme.and_then(|name| catalog.find_by_colorscheme(name));
    cycle_theme_index_from_index(catalog, current_index, direction)
}

/// Side-effects requested by one state transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeSwitcherEffect {
    PreviewTheme(ThemeIndex),
    PersistTheme(ThemeIndex),
    RestoreTheme(ThemeIndex),
    ClosePicker,
}

pub type ThemeSwitcherTransition = Transition<ThemeSwitcherEffect, NoCommand>;

/// Pure reducer for one picker session.
///
/// Invariants:
/// - `catalog` is non-empty.
/// - `persisted` and `cursor` are always catalog indexes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeSwitcherMachine {
    catalog: ThemeCatalog,
    persisted: ThemeIndex,
    cursor: ThemeIndex,
    status: PickerStatus,
}

impl ThemeSwitcherMachine {
    pub fn new(catalog: ThemeCatalog, persisted_index_raw: usize) -> Self {
        let persisted = catalog
            .index(persisted_index_raw)
            .unwrap_or_else(|| catalog.first_index());
        Self {
            catalog,
            persisted,
            cursor: persisted,
            status: PickerStatus::Active,
        }
    }

    pub const fn status(&self) -> PickerStatus {
        self.status
    }

    pub fn is_active(&self) -> bool {
        self.status == PickerStatus::Active
    }

    pub const fn cursor_index(&self) -> ThemeIndex {
        self.cursor
    }

    pub const fn persisted_index(&self) -> ThemeIndex {
        self.persisted
    }

    pub fn cursor_theme(&self) -> Option<&ThemeSpec> {
        self.catalog.get(self.cursor)
    }

    pub fn persisted_theme(&self) -> Option<&ThemeSpec> {
        self.catalog.get(self.persisted)
    }

    pub const fn catalog(&self) -> &ThemeCatalog {
        &self.catalog
    }

    fn reduce_move_next(&mut self) -> ThemeSwitcherTransition {
        let Some(next) = self.catalog.next_wrapped(self.cursor) else {
            return ThemeSwitcherTransition::default();
        };
        if next == self.cursor {
            return ThemeSwitcherTransition::default();
        }
        self.cursor = next;
        ThemeSwitcherTransition::with_effect(ThemeSwitcherEffect::PreviewTheme(next))
    }

    fn reduce_move_prev(&mut self) -> ThemeSwitcherTransition {
        let Some(prev) = self.catalog.prev_wrapped(self.cursor) else {
            return ThemeSwitcherTransition::default();
        };
        if prev == self.cursor {
            return ThemeSwitcherTransition::default();
        }
        self.cursor = prev;
        ThemeSwitcherTransition::with_effect(ThemeSwitcherEffect::PreviewTheme(prev))
    }

    fn reduce_confirm(&mut self) -> ThemeSwitcherTransition {
        self.persisted = self.cursor;
        self.status = PickerStatus::Closed;
        ThemeSwitcherTransition::with_effects(vec![
            ThemeSwitcherEffect::PersistTheme(self.persisted),
            ThemeSwitcherEffect::ClosePicker,
        ])
    }

    fn reduce_cancel(&mut self) -> ThemeSwitcherTransition {
        self.status = PickerStatus::Closed;
        let mut effects = Vec::new();
        if self.cursor != self.persisted {
            effects.push(ThemeSwitcherEffect::RestoreTheme(self.persisted));
        }
        self.cursor = self.persisted;
        effects.push(ThemeSwitcherEffect::ClosePicker);
        ThemeSwitcherTransition::with_effects(effects)
    }
}

impl Machine for ThemeSwitcherMachine {
    type Event = ThemeSwitcherEvent;
    type Effect = ThemeSwitcherEffect;
    type Command = NoCommand;

    fn reduce(&mut self, event: Self::Event) -> ThemeSwitcherTransition {
        if !self.is_active() {
            return ThemeSwitcherTransition::default();
        }
        match event {
            ThemeSwitcherEvent::MoveNext => self.reduce_move_next(),
            ThemeSwitcherEvent::MovePrev => self.reduce_move_prev(),
            ThemeSwitcherEvent::Confirm => self.reduce_confirm(),
            ThemeSwitcherEvent::Cancel => self.reduce_cancel(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme(value: &str) -> Result<ThemeSpec, ThemeSpecError> {
        ThemeSpec::try_new(value.to_string(), value.to_string())
    }

    fn catalog(values: &[&str]) -> Result<ThemeCatalog, &'static str> {
        let themes: Result<Vec<_>, _> = values.iter().map(|value| theme(value)).collect();
        let themes = themes.map_err(|_| "expected valid theme values")?;
        ThemeCatalog::try_from_vec(themes).map_err(|_| "expected non-empty theme catalog")
    }

    #[test]
    fn move_prev_wraps_and_emits_preview() -> Result<(), &'static str> {
        let mut machine = ThemeSwitcherMachine::new(catalog(&["a", "b", "c"])?, 0);
        let transition = machine.reduce(ThemeSwitcherEvent::MovePrev);
        assert_eq!(
            transition.effects,
            vec![ThemeSwitcherEffect::PreviewTheme(ThemeIndex(2))]
        );
        assert_eq!(machine.cursor_index().raw(), 2);
        Ok(())
    }

    #[test]
    fn move_next_wraps_and_emits_preview() -> Result<(), &'static str> {
        let mut machine = ThemeSwitcherMachine::new(catalog(&["a", "b", "c"])?, 2);
        let transition = machine.reduce(ThemeSwitcherEvent::MoveNext);
        assert_eq!(
            transition.effects,
            vec![ThemeSwitcherEffect::PreviewTheme(ThemeIndex(0))]
        );
        assert_eq!(machine.cursor_index().raw(), 0);
        Ok(())
    }

    #[test]
    fn confirm_persists_selected_theme_and_closes() -> Result<(), &'static str> {
        let mut machine = ThemeSwitcherMachine::new(catalog(&["a", "b", "c"])?, 0);
        let _ = machine.reduce(ThemeSwitcherEvent::MoveNext);
        let transition = machine.reduce(ThemeSwitcherEvent::Confirm);
        assert_eq!(
            transition.effects,
            vec![
                ThemeSwitcherEffect::PersistTheme(ThemeIndex(1)),
                ThemeSwitcherEffect::ClosePicker,
            ]
        );
        assert_eq!(machine.persisted_index().raw(), 1);
        assert_eq!(machine.status(), PickerStatus::Closed);
        assert!(machine.reduce(ThemeSwitcherEvent::MoveNext).is_empty());
        Ok(())
    }

    #[test]
    fn cancel_reverts_when_cursor_changed() -> Result<(), &'static str> {
        let mut machine = ThemeSwitcherMachine::new(catalog(&["a", "b", "c"])?, 1);
        let _ = machine.reduce(ThemeSwitcherEvent::MoveNext);
        let transition = machine.reduce(ThemeSwitcherEvent::Cancel);
        assert_eq!(
            transition.effects,
            vec![
                ThemeSwitcherEffect::RestoreTheme(ThemeIndex(1)),
                ThemeSwitcherEffect::ClosePicker,
            ]
        );
        assert_eq!(machine.cursor_index().raw(), 1);
        assert_eq!(machine.status(), PickerStatus::Closed);
        Ok(())
    }

    #[test]
    fn cancel_without_navigation_only_closes() -> Result<(), &'static str> {
        let mut machine = ThemeSwitcherMachine::new(catalog(&["a", "b", "c"])?, 1);
        let transition = machine.reduce(ThemeSwitcherEvent::Cancel);
        assert_eq!(transition.effects, vec![ThemeSwitcherEffect::ClosePicker]);
        assert_eq!(machine.cursor_index().raw(), 1);
        Ok(())
    }

    #[test]
    fn single_theme_navigation_is_noop() -> Result<(), &'static str> {
        let mut machine = ThemeSwitcherMachine::new(catalog(&["a"])?, 0);
        assert!(machine.reduce(ThemeSwitcherEvent::MoveNext).is_empty());
        assert!(machine.reduce(ThemeSwitcherEvent::MovePrev).is_empty());
        Ok(())
    }

    #[test]
    fn invalid_persisted_index_falls_back_to_first_theme() -> Result<(), &'static str> {
        let machine = ThemeSwitcherMachine::new(catalog(&["a", "b", "c"])?, 99);
        assert_eq!(machine.persisted_index().raw(), 0);
        assert_eq!(machine.cursor_index().raw(), 0);
        Ok(())
    }

    #[test]
    fn cycle_next_from_unknown_uses_first_theme() -> Result<(), &'static str> {
        let themes = catalog(&["a", "b", "c"])?;
        let next = cycle_theme_index(&themes, Some("missing"), ThemeCycleDirection::Next);
        assert_eq!(next.raw(), 0);
        Ok(())
    }

    #[test]
    fn cycle_prev_from_unknown_uses_last_theme() -> Result<(), &'static str> {
        let themes = catalog(&["a", "b", "c"])?;
        let prev = cycle_theme_index(&themes, Some("missing"), ThemeCycleDirection::Prev);
        assert_eq!(prev.raw(), 2);
        Ok(())
    }

    #[test]
    fn cycle_next_from_known_theme_advances_with_wrap() -> Result<(), &'static str> {
        let themes = catalog(&["a", "b", "c"])?;
        let next = cycle_theme_index(&themes, Some("c"), ThemeCycleDirection::Next);
        assert_eq!(next.raw(), 0);
        Ok(())
    }

    #[test]
    fn cycle_prev_from_known_theme_advances_with_wrap() -> Result<(), &'static str> {
        let themes = catalog(&["a", "b", "c"])?;
        let prev = cycle_theme_index(&themes, Some("a"), ThemeCycleDirection::Prev);
        assert_eq!(prev.raw(), 2);
        Ok(())
    }

    #[test]
    fn find_by_colorscheme_prefers_first_duplicate_entry() -> Result<(), &'static str> {
        let themes = vec![
            ThemeSpec::try_new("One".to_string(), "dup".to_string())
                .map_err(|_| "expected valid theme")?,
            ThemeSpec::try_new("Two".to_string(), "dup".to_string())
                .map_err(|_| "expected valid theme")?,
        ];
        let catalog =
            ThemeCatalog::try_from_vec(themes).map_err(|_| "expected non-empty catalog")?;
        assert_eq!(
            catalog.find_by_colorscheme("dup").map(ThemeIndex::raw),
            Some(0)
        );
        Ok(())
    }
}
