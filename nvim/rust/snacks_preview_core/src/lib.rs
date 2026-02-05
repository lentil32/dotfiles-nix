use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BufKey(i64);

impl BufKey {
    pub const fn try_new(raw: i64) -> Option<Self> {
        if raw > 0 { Some(Self(raw)) } else { None }
    }

    pub const fn raw(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct DocPreviewState {
    pub token: i64,
    pub group: Option<u32>,
    pub cleanup: Option<i64>,
    pub name: String,
    pub preview_name: String,
    pub restore_name: bool,
}

#[derive(Debug, Default)]
pub struct PreviewRegistry {
    tokens: HashMap<BufKey, i64>,
    previews: HashMap<BufKey, DocPreviewState>,
}

impl PreviewRegistry {
    pub fn next_token(&mut self, key: BufKey) -> i64 {
        let entry = self.tokens.entry(key).or_insert(0);
        *entry += 1;
        *entry
    }

    pub fn is_token_current(&self, key: BufKey, token: i64) -> bool {
        self.previews
            .get(&key)
            .is_some_and(|entry| entry.token == token)
    }

    pub fn insert_preview(&mut self, key: BufKey, state: DocPreviewState) {
        self.previews.insert(key, state);
    }

    pub fn get_preview(&self, key: BufKey) -> Option<&DocPreviewState> {
        self.previews.get(&key)
    }

    pub fn get_preview_mut(&mut self, key: BufKey) -> Option<&mut DocPreviewState> {
        self.previews.get_mut(&key)
    }

    pub fn take_preview(&mut self, key: BufKey) -> Option<DocPreviewState> {
        self.tokens.remove(&key);
        self.previews.remove(&key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_token_increments_per_key() -> Result<(), &'static str> {
        let mut registry = PreviewRegistry::default();
        let key = BufKey::try_new(1).ok_or("expected valid key")?;
        assert_eq!(registry.next_token(key), 1);
        assert_eq!(registry.next_token(key), 2);
        Ok(())
    }

    #[test]
    fn token_resets_after_take() -> Result<(), &'static str> {
        let mut registry = PreviewRegistry::default();
        let key = BufKey::try_new(2).ok_or("expected valid key")?;
        let token = registry.next_token(key);
        registry.insert_preview(
            key,
            DocPreviewState {
                token,
                group: None,
                cleanup: None,
                name: "a".to_string(),
                preview_name: "b".to_string(),
                restore_name: false,
            },
        );
        let _ = registry.take_preview(key);
        assert_eq!(registry.next_token(key), 1);
        Ok(())
    }
}
