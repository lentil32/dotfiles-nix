use crate::core::state::{CursorColorProbeWitness, CursorColorSample};
use crate::core::types::Generation;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) enum CursorColorCacheLookup {
    Miss,
    Hit(Option<CursorColorSample>),
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct CursorColorProbeCacheEntry {
    witness: CursorColorProbeWitness,
    sample: Option<CursorColorSample>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct ProbeCacheState {
    colorscheme_generation: Generation,
    cursor_color: Option<CursorColorProbeCacheEntry>,
}

impl Default for ProbeCacheState {
    fn default() -> Self {
        Self {
            colorscheme_generation: Generation::INITIAL,
            cursor_color: None,
        }
    }
}

impl ProbeCacheState {
    pub(super) fn colorscheme_generation(&self) -> Generation {
        self.colorscheme_generation
    }

    pub(super) fn cached_cursor_color_sample(
        &self,
        witness: &CursorColorProbeWitness,
    ) -> CursorColorCacheLookup {
        self.cursor_color
            .as_ref()
            .filter(|entry| entry.witness == *witness)
            .map_or(CursorColorCacheLookup::Miss, |entry| {
                CursorColorCacheLookup::Hit(entry.sample.clone())
            })
    }

    pub(super) fn store_cursor_color_sample(
        &mut self,
        witness: CursorColorProbeWitness,
        sample: Option<CursorColorSample>,
    ) {
        self.cursor_color = Some(CursorColorProbeCacheEntry { witness, sample });
    }

    pub(super) fn note_cursor_color_colorscheme_change(&mut self) {
        self.colorscheme_generation = self.colorscheme_generation.next();
        self.cursor_color = None;
    }

    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::{CursorColorCacheLookup, ProbeCacheState};
    use crate::core::state::{CursorColorProbeWitness, CursorColorSample};
    use crate::core::types::{CursorCol, CursorPosition, CursorRow, Generation};

    fn cursor(row: u32, col: u32) -> Option<CursorPosition> {
        Some(CursorPosition {
            row: CursorRow(row),
            col: CursorCol(col),
        })
    }

    fn witness(
        buffer_handle: i64,
        changedtick: u64,
        mode: &str,
        cursor_position: Option<CursorPosition>,
        colorscheme_generation: u64,
    ) -> CursorColorProbeWitness {
        CursorColorProbeWitness::new(
            buffer_handle,
            changedtick,
            mode.to_string(),
            cursor_position,
            Generation::new(colorscheme_generation),
        )
    }

    #[test]
    fn probe_cache_state_returns_cursor_color_entry_only_for_identical_witness() {
        let mut cache = ProbeCacheState::default();
        let key = witness(22, 14, "n", cursor(7, 8), 0);
        let sample = Some(CursorColorSample::new("#abcdef".to_string()));
        cache.store_cursor_color_sample(key.clone(), sample.clone());

        assert_eq!(
            cache.cached_cursor_color_sample(&key),
            CursorColorCacheLookup::Hit(sample),
        );

        let changedtick = witness(22, 15, "n", cursor(7, 8), 0);
        assert_eq!(
            cache.cached_cursor_color_sample(&changedtick),
            CursorColorCacheLookup::Miss,
        );

        let moved_cursor = witness(22, 14, "n", cursor(7, 9), 0);
        assert_eq!(
            cache.cached_cursor_color_sample(&moved_cursor),
            CursorColorCacheLookup::Miss,
        );
    }

    #[test]
    fn probe_cache_state_invalidates_cursor_color_entry_when_colorscheme_changes() {
        let mut cache = ProbeCacheState::default();
        cache.store_cursor_color_sample(
            witness(22, 14, "n", cursor(7, 8), 0),
            Some(CursorColorSample::new("#abcdef".to_string())),
        );

        cache.note_cursor_color_colorscheme_change();

        assert_eq!(cache.colorscheme_generation(), Generation::new(1));
        assert_eq!(
            cache.cached_cursor_color_sample(&witness(22, 14, "n", cursor(7, 8), 1)),
            CursorColorCacheLookup::Miss,
        );
    }

    #[test]
    fn probe_cache_state_reset_clears_generation_and_entries() {
        let mut cache = ProbeCacheState::default();
        cache.store_cursor_color_sample(
            witness(22, 14, "i", cursor(5, 6), 0),
            Some(CursorColorSample::new("#fedcba".to_string())),
        );
        cache.note_cursor_color_colorscheme_change();

        cache.reset();

        assert_eq!(cache.colorscheme_generation(), Generation::INITIAL);
        assert_eq!(
            cache.cached_cursor_color_sample(&witness(22, 14, "i", cursor(5, 6), 0)),
            CursorColorCacheLookup::Miss,
        );
    }
}
