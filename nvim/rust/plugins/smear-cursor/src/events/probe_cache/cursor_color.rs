use super::LruCache;
use crate::core::effect::ProbePolicy;
use crate::core::state::CursorColorProbeWitness;
use crate::core::state::CursorColorSample;
use crate::core::state::ProbeReuse;

pub(super) const CURSOR_COLOR_CACHE_CAPACITY: usize = 16;
const CURSOR_COLOR_MOTION_CACHE_CAPACITY: usize = 16;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum CursorColorCacheLookup {
    Miss,
    Hit(Option<CursorColorSample>),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct CachedCursorColorProbeSample {
    reuse: ProbeReuse,
    sample: Option<CursorColorSample>,
}

impl CachedCursorColorProbeSample {
    const fn new(reuse: ProbeReuse, sample: Option<CursorColorSample>) -> Self {
        Self { reuse, sample }
    }

    pub(crate) const fn reuse(self) -> ProbeReuse {
        self.reuse
    }

    pub(crate) const fn sample(self) -> Option<CursorColorSample> {
        self.sample
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct CursorColorMotionCacheKey {
    buffer_handle: i64,
    changedtick: u64,
    mode: String,
    line: u32,
    colorscheme_generation: crate::core::types::Generation,
}

impl CursorColorMotionCacheKey {
    fn from_witness(witness: &CursorColorProbeWitness) -> Option<Self> {
        let line = witness.cursor_position()?.row.value();
        Some(Self {
            buffer_handle: witness.buffer_handle(),
            changedtick: witness.changedtick(),
            mode: witness.mode().to_owned(),
            line,
            colorscheme_generation: witness.colorscheme_generation(),
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct CursorColorProbeCache {
    exact: LruCache<CursorColorProbeWitness, Option<CursorColorSample>>,
    motion: LruCache<CursorColorMotionCacheKey, Option<CursorColorSample>>,
}

impl Default for CursorColorProbeCache {
    fn default() -> Self {
        Self {
            exact: LruCache::new(CURSOR_COLOR_CACHE_CAPACITY),
            motion: LruCache::new(CURSOR_COLOR_MOTION_CACHE_CAPACITY),
        }
    }
}

impl CursorColorProbeCache {
    pub(super) fn cached_sample(
        &mut self,
        witness: &CursorColorProbeWitness,
    ) -> CursorColorCacheLookup {
        self.exact
            .get_cloned(witness)
            .map_or(CursorColorCacheLookup::Miss, CursorColorCacheLookup::Hit)
    }

    pub(super) fn cached_sample_for_probe(
        &mut self,
        witness: &CursorColorProbeWitness,
        probe_policy: ProbePolicy,
        reuse: ProbeReuse,
    ) -> Option<CachedCursorColorProbeSample> {
        if let CursorColorCacheLookup::Hit(sample) = self.cached_sample(witness) {
            return Some(CachedCursorColorProbeSample::new(reuse, sample));
        }

        if reuse != ProbeReuse::Compatible || !probe_policy.allows_compatible_cursor_color_reuse() {
            return None;
        }

        let motion_key = CursorColorMotionCacheKey::from_witness(witness)?;
        self.motion
            .get_cloned(&motion_key)
            .map(|sample| CachedCursorColorProbeSample::new(ProbeReuse::Compatible, sample))
    }

    pub(super) fn store_sample(
        &mut self,
        witness: CursorColorProbeWitness,
        sample: Option<CursorColorSample>,
    ) {
        let motion_key = CursorColorMotionCacheKey::from_witness(&witness);
        self.exact.insert(witness, sample);
        if let Some(motion_key) = motion_key {
            self.motion.insert(motion_key, sample);
        }
    }

    pub(super) fn note_colorscheme_change(&mut self) {
        self.exact.clear();
        self.motion.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::CachedCursorColorProbeSample;
    use super::CursorColorCacheLookup;
    use super::CursorColorProbeCache;
    use crate::core::effect::CursorColorFallbackMode;
    use crate::core::effect::CursorColorReuseMode;
    use crate::core::effect::CursorPositionProbeMode;
    use crate::core::effect::ProbePolicy;
    use crate::core::effect::ProbeQuality;
    use crate::core::state::CursorColorProbeWitness;
    use crate::core::state::CursorColorSample;
    use crate::core::state::ProbeReuse;
    use crate::core::types::CursorCol;
    use crate::core::types::CursorPosition;
    use crate::core::types::CursorRow;
    use crate::core::types::Generation;
    use pretty_assertions::assert_eq;

    fn cursor(row: u32, col: u32) -> CursorPosition {
        CursorPosition {
            row: CursorRow(row),
            col: CursorCol(col),
        }
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
    fn cursor_color_probe_cache_returns_exact_entry_only_for_identical_witness() {
        let mut cache = CursorColorProbeCache::default();
        let key = witness(22, 14, "n", Some(cursor(7, 8)), 0);
        let sample = Some(CursorColorSample::new(0x00AB_CDEF));
        cache.store_sample(key.clone(), sample);

        assert_eq!(
            cache.cached_sample(&key),
            CursorColorCacheLookup::Hit(sample)
        );
        assert_eq!(
            cache.cached_sample(&witness(22, 15, "n", Some(cursor(7, 8)), 0)),
            CursorColorCacheLookup::Miss,
        );
        assert_eq!(
            cache.cached_sample(&witness(22, 14, "n", Some(cursor(7, 9)), 0)),
            CursorColorCacheLookup::Miss,
        );
    }

    #[test]
    fn cursor_color_probe_cache_keeps_exact_entries_distinct_while_motion_cache_reuses_line_scope()
    {
        let mut cache = CursorColorProbeCache::default();
        let left = witness(22, 14, "n", Some(cursor(7, 8)), 0);
        let right = witness(22, 14, "n", Some(cursor(7, 9)), 0);
        let left_sample = Some(CursorColorSample::new(0x00AB_CDEF));
        let right_sample = Some(CursorColorSample::new(0x00FE_DCBA));
        cache.store_sample(left.clone(), left_sample);
        cache.store_sample(right.clone(), right_sample);

        assert_eq!(
            cache.cached_sample(&left),
            CursorColorCacheLookup::Hit(left_sample)
        );
        assert_eq!(
            cache.cached_sample(&right),
            CursorColorCacheLookup::Hit(right_sample)
        );
        assert_eq!(
            cache.cached_sample_for_probe(
                &witness(22, 14, "n", Some(cursor(7, 10)), 0),
                ProbePolicy::new(ProbeQuality::FastMotion),
                ProbeReuse::Compatible,
            ),
            Some(CachedCursorColorProbeSample {
                reuse: ProbeReuse::Compatible,
                sample: right_sample,
            }),
        );
    }

    #[test]
    fn cursor_color_probe_cache_motion_key_stays_scoped_to_line_and_mode() {
        let mut cache = CursorColorProbeCache::default();
        let sample = Some(CursorColorSample::new(0x00AB_CDEF));
        cache.store_sample(witness(22, 14, "n", Some(cursor(7, 8)), 0), sample);

        assert_eq!(
            cache.cached_sample_for_probe(
                &witness(22, 14, "n", Some(cursor(7, 9)), 0),
                ProbePolicy::new(ProbeQuality::FastMotion),
                ProbeReuse::Compatible,
            ),
            Some(CachedCursorColorProbeSample {
                reuse: ProbeReuse::Compatible,
                sample,
            }),
        );
        assert_eq!(
            cache.cached_sample_for_probe(
                &witness(22, 14, "n", Some(cursor(8, 1)), 0),
                ProbePolicy::new(ProbeQuality::FastMotion),
                ProbeReuse::Compatible,
            ),
            None,
        );
        assert_eq!(
            cache.cached_sample_for_probe(
                &witness(22, 14, "i", Some(cursor(7, 9)), 0),
                ProbePolicy::new(ProbeQuality::FastMotion),
                ProbeReuse::Compatible,
            ),
            None,
        );
    }

    #[test]
    fn cursor_color_probe_cache_motion_reuse_requires_compatible_reuse_and_matching_reuse_state() {
        let mut cache = CursorColorProbeCache::default();
        let sample = Some(CursorColorSample::new(0x00AB_CDEF));
        cache.store_sample(witness(22, 14, "n", Some(cursor(7, 8)), 0), sample);
        let moved = witness(22, 14, "n", Some(cursor(7, 9)), 0);
        let exact_compatible_policy = ProbePolicy::from_modes(
            CursorPositionProbeMode::Exact,
            CursorColorReuseMode::CompatibleWithinLine,
            CursorColorFallbackMode::SyntaxThenExtmarks,
        );

        assert_eq!(
            cache.cached_sample_for_probe(
                &moved,
                ProbePolicy::new(ProbeQuality::Exact),
                ProbeReuse::Compatible,
            ),
            None,
        );
        assert_eq!(
            cache.cached_sample_for_probe(&moved, exact_compatible_policy, ProbeReuse::Exact,),
            None,
        );
        assert_eq!(
            cache.cached_sample_for_probe(&moved, exact_compatible_policy, ProbeReuse::Compatible),
            Some(CachedCursorColorProbeSample {
                reuse: ProbeReuse::Compatible,
                sample,
            }),
        );
    }

    #[test]
    fn cursor_color_probe_cache_clears_exact_and_motion_entries_on_colorscheme_change() {
        let mut cache = CursorColorProbeCache::default();
        let original = witness(22, 14, "n", Some(cursor(7, 8)), 0);
        cache.store_sample(original.clone(), Some(CursorColorSample::new(0x00AB_CDEF)));

        cache.note_colorscheme_change();

        assert_eq!(cache.cached_sample(&original), CursorColorCacheLookup::Miss);
        assert_eq!(
            cache.cached_sample_for_probe(
                &witness(22, 14, "n", Some(cursor(7, 9)), 0),
                ProbePolicy::new(ProbeQuality::FastMotion),
                ProbeReuse::Compatible,
            ),
            None,
        );
    }
}
