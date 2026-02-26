use super::reducer::{ThemeCatalog, ThemeIndex};

/// Resolves the effective theme index from two sources.
///
/// Priority:
/// 1) persisted colorscheme
/// 2) current colorscheme
///
/// Only colorschemes present in `catalog` are considered valid.
pub fn resolve_effective_theme_index(
    catalog: &ThemeCatalog,
    persisted_colorscheme: Option<&str>,
    current_colorscheme: Option<&str>,
) -> Option<ThemeIndex> {
    persisted_colorscheme
        .and_then(|name| catalog.find_by_colorscheme(name))
        .or_else(|| current_colorscheme.and_then(|name| catalog.find_by_colorscheme(name)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{ThemeCatalog, ThemeSpec};

    fn theme(value: &str) -> Result<ThemeSpec, &'static str> {
        ThemeSpec::try_new(value.to_string(), value.to_string()).map_err(|_| "invalid theme")
    }

    fn catalog(values: &[&str]) -> Result<ThemeCatalog, &'static str> {
        let themes: Result<Vec<_>, _> = values.iter().map(|value| theme(value)).collect();
        let themes = themes?;
        ThemeCatalog::try_from_vec(themes).map_err(|_| "empty catalog")
    }

    #[test]
    fn prefers_persisted_when_both_match() -> Result<(), &'static str> {
        let catalog = catalog(&["a", "b", "c"])?;
        let index = resolve_effective_theme_index(&catalog, Some("c"), Some("a"));
        assert_eq!(index.map(ThemeIndex::raw), Some(2));
        Ok(())
    }

    #[test]
    fn falls_back_to_current_when_persisted_is_missing() -> Result<(), &'static str> {
        let catalog = catalog(&["a", "b", "c"])?;
        let index = resolve_effective_theme_index(&catalog, None, Some("b"));
        assert_eq!(index.map(ThemeIndex::raw), Some(1));
        Ok(())
    }

    #[test]
    fn falls_back_to_current_when_persisted_is_unknown() -> Result<(), &'static str> {
        let catalog = catalog(&["a", "b", "c"])?;
        let index = resolve_effective_theme_index(&catalog, Some("missing"), Some("b"));
        assert_eq!(index.map(ThemeIndex::raw), Some(1));
        Ok(())
    }

    #[test]
    fn returns_none_when_both_sources_are_unknown() -> Result<(), &'static str> {
        let catalog = catalog(&["a", "b", "c"])?;
        let index = resolve_effective_theme_index(&catalog, Some("x"), Some("y"));
        assert_eq!(index, None);
        Ok(())
    }
}
