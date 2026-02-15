use nvim_oxi_utils::state_machine::{Machine, Transition};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BufKey(i64);

impl BufKey {
    pub const fn try_new(raw: i64) -> Option<Self> {
        if raw > 0 { Some(Self(raw)) } else { None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PreviewToken(i64);

impl PreviewToken {
    pub const fn try_new(raw: i64) -> Option<Self> {
        if raw > 0 { Some(Self(raw)) } else { None }
    }

    pub const fn raw(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct DocPreviewState {
    pub token: PreviewToken,
    pub group: Option<u32>,
    pub cleanup: Option<i64>,
    pub restore_name_plan: Option<RestoreNamePlan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreNamePlan {
    pub name: String,
    pub preview_name: String,
}

impl RestoreNamePlan {
    fn from_state(state: &DocPreviewState) -> Option<Self> {
        state.restore_name_plan.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewEffect {
    RestoreName(RestoreNamePlan),
    DeleteAugroup(u32),
    CloseCleanup(i64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewCommand {
    RequestDocFind(PreviewToken),
}

pub type PreviewTransition = Transition<PreviewEffect, PreviewCommand>;

#[derive(Debug, Clone)]
pub enum PreviewEvent {
    Close {
        key: BufKey,
    },
    Register {
        key: BufKey,
        group: u32,
        restore_name_plan: Option<RestoreNamePlan>,
    },
    DocFindArrived {
        key: BufKey,
        token: PreviewToken,
    },
    CleanupOpened {
        key: BufKey,
        token: PreviewToken,
        cleanup_id: i64,
    },
}

#[derive(Debug, Default)]
pub struct PreviewRegistry {
    next_token: i64,
    previews: HashMap<BufKey, DocPreviewState>,
}

impl PreviewRegistry {
    pub const fn next_token(&mut self) -> PreviewToken {
        self.next_token = if self.next_token == i64::MAX {
            1
        } else {
            self.next_token + 1
        };
        PreviewToken(self.next_token)
    }

    pub fn is_token_current(&self, key: BufKey, token: PreviewToken) -> bool {
        self.previews
            .get(&key)
            .is_some_and(|entry| entry.token == token)
    }

    #[cfg(test)]
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
        self.previews.remove(&key)
    }

    fn close_effects(state: &DocPreviewState) -> Vec<PreviewEffect> {
        let mut effects = Vec::new();
        if let Some(plan) = RestoreNamePlan::from_state(state) {
            effects.push(PreviewEffect::RestoreName(plan));
        }
        if let Some(group) = state.group {
            effects.push(PreviewEffect::DeleteAugroup(group));
        }
        if let Some(cleanup_id) = state.cleanup {
            effects.push(PreviewEffect::CloseCleanup(cleanup_id));
        }
        effects
    }

    fn replace_effects(state: &DocPreviewState, new_group: u32) -> Vec<PreviewEffect> {
        let mut effects = Vec::new();
        if let Some(group) = state.group
            && group != new_group
        {
            effects.push(PreviewEffect::DeleteAugroup(group));
        }
        if let Some(cleanup_id) = state.cleanup {
            effects.push(PreviewEffect::CloseCleanup(cleanup_id));
        }
        effects
    }

    pub fn reduce(&mut self, event: PreviewEvent) -> PreviewTransition {
        match event {
            PreviewEvent::Close { key } => self
                .take_preview(key)
                .map(|old| Self::close_effects(&old))
                .map_or_else(PreviewTransition::default, PreviewTransition::with_effects),
            PreviewEvent::Register {
                key,
                group,
                restore_name_plan,
            } => {
                let token = self.next_token();
                let replaced = self.previews.insert(
                    key,
                    DocPreviewState {
                        token,
                        group: Some(group),
                        cleanup: None,
                        restore_name_plan,
                    },
                );
                let effects =
                    replaced.map_or_else(Vec::new, |old| Self::replace_effects(&old, group));
                let mut transition = PreviewTransition::with_effects(effects);
                transition.set_command(PreviewCommand::RequestDocFind(token));
                transition
            }
            PreviewEvent::DocFindArrived { key, token } => self
                .get_preview(key)
                .filter(|entry| entry.token == token)
                .and_then(RestoreNamePlan::from_state)
                .map(|plan| vec![PreviewEffect::RestoreName(plan)])
                .map_or_else(PreviewTransition::default, PreviewTransition::with_effects),
            PreviewEvent::CleanupOpened {
                key,
                token,
                cleanup_id,
            } => {
                let Some(entry) = self.get_preview_mut(key) else {
                    return PreviewTransition::with_effects(vec![PreviewEffect::CloseCleanup(
                        cleanup_id,
                    )]);
                };
                if entry.token != token {
                    return PreviewTransition::with_effects(vec![PreviewEffect::CloseCleanup(
                        cleanup_id,
                    )]);
                }
                let replaced = entry.cleanup.replace(cleanup_id);
                let effects =
                    replaced.map_or_else(Vec::new, |old| vec![PreviewEffect::CloseCleanup(old)]);
                PreviewTransition::with_effects(effects)
            }
        }
    }
}

impl Machine for PreviewRegistry {
    type Event = PreviewEvent;
    type Effect = PreviewEffect;
    type Command = PreviewCommand;

    fn reduce(&mut self, event: Self::Event) -> PreviewTransition {
        Self::reduce(self, event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(raw: i64) -> Result<BufKey, &'static str> {
        BufKey::try_new(raw).ok_or("expected valid key")
    }

    fn token(raw: i64) -> Result<PreviewToken, &'static str> {
        PreviewToken::try_new(raw).ok_or("expected valid token")
    }

    fn assert_registry_invariants(registry: &PreviewRegistry) {
        assert!(registry.next_token >= 0);
        for state in registry.previews.values() {
            assert!(state.token.raw() > 0);
            if let Some(cleanup_id) = state.cleanup {
                assert!(cleanup_id > 0);
            }
            if let Some(plan) = &state.restore_name_plan {
                assert!(!plan.name.is_empty());
                assert!(!plan.preview_name.is_empty());
                assert_ne!(plan.name, plan.preview_name);
            }
        }
    }

    #[test]
    fn next_token_increments_monotonically() {
        let mut registry = PreviewRegistry::default();
        assert_eq!(registry.next_token(), PreviewToken(1));
        assert_eq!(registry.next_token(), PreviewToken(2));
    }

    #[test]
    fn token_continues_after_take() -> Result<(), &'static str> {
        let mut registry = PreviewRegistry::default();
        let key = BufKey::try_new(2).ok_or("expected valid key")?;
        let first_token = registry.next_token();
        registry.insert_preview(
            key,
            DocPreviewState {
                token: first_token,
                group: None,
                cleanup: None,
                restore_name_plan: None,
            },
        );
        let _ = registry.take_preview(key);
        assert_eq!(registry.next_token(), token(2)?);
        Ok(())
    }

    #[test]
    fn close_emits_restore_group_and_cleanup_effects() -> Result<(), &'static str> {
        let mut registry = PreviewRegistry::default();
        let key = BufKey::try_new(3).ok_or("expected valid key")?;
        registry.insert_preview(
            key,
            DocPreviewState {
                token: token(9)?,
                group: Some(17),
                cleanup: Some(23),
                restore_name_plan: Some(RestoreNamePlan {
                    name: "a".to_string(),
                    preview_name: "b".to_string(),
                }),
            },
        );

        let transition = registry.reduce(PreviewEvent::Close { key });
        assert_eq!(
            transition.effects,
            vec![
                PreviewEffect::RestoreName(RestoreNamePlan {
                    name: "a".to_string(),
                    preview_name: "b".to_string(),
                }),
                PreviewEffect::DeleteAugroup(17),
                PreviewEffect::CloseCleanup(23),
            ]
        );
        assert_eq!(transition.command, None);
        Ok(())
    }

    #[test]
    fn register_requests_doc_find_and_tracks_token() -> Result<(), &'static str> {
        let mut registry = PreviewRegistry::default();
        let key = BufKey::try_new(4).ok_or("expected valid key")?;

        let transition = registry.reduce(PreviewEvent::Register {
            key,
            group: 11,
            restore_name_plan: Some(RestoreNamePlan {
                name: "n".to_string(),
                preview_name: "p".to_string(),
            }),
        });

        assert_eq!(transition.effects, Vec::<PreviewEffect>::new());
        assert_eq!(
            transition.command,
            Some(PreviewCommand::RequestDocFind(token(1)?))
        );
        assert!(registry.is_token_current(key, token(1)?));
        Ok(())
    }

    #[test]
    fn cleanup_opened_replaces_prior_cleanup() -> Result<(), &'static str> {
        let mut registry = PreviewRegistry::default();
        let key = BufKey::try_new(5).ok_or("expected valid key")?;
        let _ = registry.reduce(PreviewEvent::Register {
            key,
            group: 1,
            restore_name_plan: Some(RestoreNamePlan {
                name: "n".to_string(),
                preview_name: "p".to_string(),
            }),
        });

        let first = registry.reduce(PreviewEvent::CleanupOpened {
            key,
            token: token(1)?,
            cleanup_id: 10,
        });
        assert!(first.effects.is_empty());
        assert_eq!(first.command, None);

        let second = registry.reduce(PreviewEvent::CleanupOpened {
            key,
            token: token(1)?,
            cleanup_id: 11,
        });
        assert_eq!(second.effects, vec![PreviewEffect::CloseCleanup(10)]);
        assert_eq!(second.command, None);
        Ok(())
    }

    #[test]
    fn cleanup_opened_for_stale_token_closes_new_cleanup() -> Result<(), &'static str> {
        let mut registry = PreviewRegistry::default();
        let key = BufKey::try_new(6).ok_or("expected valid key")?;
        let _ = registry.reduce(PreviewEvent::Register {
            key,
            group: 1,
            restore_name_plan: Some(RestoreNamePlan {
                name: "n".to_string(),
                preview_name: "p".to_string(),
            }),
        });

        let transition = registry.reduce(PreviewEvent::CleanupOpened {
            key,
            token: token(99)?,
            cleanup_id: 12,
        });
        assert_eq!(transition.effects, vec![PreviewEffect::CloseCleanup(12)]);
        assert_eq!(transition.command, None);
        Ok(())
    }

    #[test]
    fn close_missing_preview_is_noop() -> Result<(), &'static str> {
        let mut registry = PreviewRegistry::default();
        let key = BufKey::try_new(7).ok_or("expected valid key")?;

        let transition = registry.reduce(PreviewEvent::Close { key });

        assert!(transition.effects.is_empty());
        assert_eq!(transition.command, None);
        Ok(())
    }

    #[test]
    fn register_replaces_existing_preview_and_cleans_prior_resources() -> Result<(), &'static str> {
        let mut registry = PreviewRegistry::default();
        let key = BufKey::try_new(8).ok_or("expected valid key")?;
        registry.insert_preview(
            key,
            DocPreviewState {
                token: token(1)?,
                group: Some(70),
                cleanup: Some(80),
                restore_name_plan: Some(RestoreNamePlan {
                    name: "orig".to_string(),
                    preview_name: "orig.preview".to_string(),
                }),
            },
        );

        let transition = registry.reduce(PreviewEvent::Register {
            key,
            group: 71,
            restore_name_plan: Some(RestoreNamePlan {
                name: "new".to_string(),
                preview_name: "new.preview".to_string(),
            }),
        });

        assert_eq!(
            transition.effects,
            vec![
                PreviewEffect::DeleteAugroup(70),
                PreviewEffect::CloseCleanup(80)
            ]
        );
        assert_eq!(
            transition.command,
            Some(PreviewCommand::RequestDocFind(token(1)?))
        );
        assert_eq!(
            registry.get_preview(key).and_then(|entry| entry.group),
            Some(71)
        );
        assert_eq!(
            registry.get_preview(key).and_then(|entry| entry.cleanup),
            None
        );
        Ok(())
    }

    #[test]
    fn register_replacement_same_group_does_not_delete_current_group() -> Result<(), &'static str> {
        let mut registry = PreviewRegistry::default();
        let key = BufKey::try_new(11).ok_or("expected valid key")?;
        registry.insert_preview(
            key,
            DocPreviewState {
                token: token(1)?,
                group: Some(70),
                cleanup: Some(80),
                restore_name_plan: None,
            },
        );

        let transition = registry.reduce(PreviewEvent::Register {
            key,
            group: 70,
            restore_name_plan: None,
        });

        assert_eq!(transition.effects, vec![PreviewEffect::CloseCleanup(80)]);
        assert_eq!(
            transition.command,
            Some(PreviewCommand::RequestDocFind(token(1)?))
        );
        Ok(())
    }

    #[test]
    fn doc_find_arrived_without_restore_plan_is_noop() -> Result<(), &'static str> {
        let mut registry = PreviewRegistry::default();
        let key = BufKey::try_new(9).ok_or("expected valid key")?;
        registry.insert_preview(
            key,
            DocPreviewState {
                token: token(33)?,
                group: Some(1),
                cleanup: None,
                restore_name_plan: None,
            },
        );

        let transition = registry.reduce(PreviewEvent::DocFindArrived {
            key,
            token: token(33)?,
        });

        assert!(transition.effects.is_empty());
        assert_eq!(transition.command, None);
        Ok(())
    }

    #[test]
    fn doc_find_arrived_for_stale_token_is_noop() -> Result<(), &'static str> {
        let mut registry = PreviewRegistry::default();
        let key = BufKey::try_new(12).ok_or("expected valid key")?;

        let first = registry.reduce(PreviewEvent::Register {
            key,
            group: 2,
            restore_name_plan: Some(RestoreNamePlan {
                name: "old".to_string(),
                preview_name: "old.preview".to_string(),
            }),
        });
        assert_eq!(
            first.command,
            Some(PreviewCommand::RequestDocFind(token(1)?))
        );

        let second = registry.reduce(PreviewEvent::Register {
            key,
            group: 3,
            restore_name_plan: Some(RestoreNamePlan {
                name: "new".to_string(),
                preview_name: "new.preview".to_string(),
            }),
        });
        assert_eq!(
            second.command,
            Some(PreviewCommand::RequestDocFind(token(2)?))
        );

        let stale = registry.reduce(PreviewEvent::DocFindArrived {
            key,
            token: token(1)?,
        });
        assert!(stale.effects.is_empty());
        assert_eq!(stale.command, None);

        let current = registry.reduce(PreviewEvent::DocFindArrived {
            key,
            token: token(2)?,
        });
        assert_eq!(
            current.effects,
            vec![PreviewEffect::RestoreName(RestoreNamePlan {
                name: "new".to_string(),
                preview_name: "new.preview".to_string(),
            })]
        );
        assert_eq!(current.command, None);
        Ok(())
    }

    #[test]
    fn cleanup_opened_without_preview_closes_cleanup() -> Result<(), &'static str> {
        let mut registry = PreviewRegistry::default();
        let key = BufKey::try_new(10).ok_or("expected valid key")?;

        let transition = registry.reduce(PreviewEvent::CleanupOpened {
            key,
            token: token(1)?,
            cleanup_id: 99,
        });

        assert_eq!(transition.effects, vec![PreviewEffect::CloseCleanup(99)]);
        assert_eq!(transition.command, None);
        Ok(())
    }

    #[derive(Clone, Copy)]
    enum Step {
        Register,
        Close,
        DocFindCurrent,
        DocFindStale,
        CleanupCurrent,
        CleanupStale,
    }

    impl Step {
        const ALL: [Self; 6] = [
            Self::Register,
            Self::Close,
            Self::DocFindCurrent,
            Self::DocFindStale,
            Self::CleanupCurrent,
            Self::CleanupStale,
        ];
    }

    fn current_token(registry: &PreviewRegistry, key: BufKey) -> Option<PreviewToken> {
        registry.get_preview(key).map(|entry| entry.token)
    }

    fn apply_step(
        registry: &mut PreviewRegistry,
        key: BufKey,
        step: Step,
        next_cleanup_id: &mut i64,
    ) -> Result<(), &'static str> {
        let stale_token = token(99_999)?;
        let transition = match step {
            Step::Register => registry.reduce(PreviewEvent::Register {
                key,
                group: 7,
                restore_name_plan: Some(RestoreNamePlan {
                    name: "name".to_string(),
                    preview_name: "name.preview".to_string(),
                }),
            }),
            Step::Close => registry.reduce(PreviewEvent::Close { key }),
            Step::DocFindCurrent => {
                let Some(current) = current_token(registry, key) else {
                    return Ok(());
                };
                registry.reduce(PreviewEvent::DocFindArrived {
                    key,
                    token: current,
                })
            }
            Step::DocFindStale => registry.reduce(PreviewEvent::DocFindArrived {
                key,
                token: stale_token,
            }),
            Step::CleanupCurrent => {
                let Some(current) = current_token(registry, key) else {
                    return Ok(());
                };
                let cleanup_id = *next_cleanup_id;
                *next_cleanup_id += 1;
                registry.reduce(PreviewEvent::CleanupOpened {
                    key,
                    token: current,
                    cleanup_id,
                })
            }
            Step::CleanupStale => {
                let cleanup_id = *next_cleanup_id;
                *next_cleanup_id += 1;
                registry.reduce(PreviewEvent::CleanupOpened {
                    key,
                    token: stale_token,
                    cleanup_id,
                })
            }
        };
        for effect in transition.effects {
            if let PreviewEffect::CloseCleanup(cleanup_id) = effect {
                assert!(cleanup_id > 0);
            }
        }
        if let Some(PreviewCommand::RequestDocFind(token)) = transition.command {
            assert!(token.raw() > 0);
        }
        assert_registry_invariants(registry);
        Ok(())
    }

    fn run_sequences(
        sequence: &mut Vec<Step>,
        remaining: usize,
        key: BufKey,
    ) -> Result<(), &'static str> {
        if remaining == 0 {
            let mut registry = PreviewRegistry::default();
            assert_registry_invariants(&registry);
            let mut next_cleanup_id = 1;
            for step in sequence {
                apply_step(&mut registry, key, *step, &mut next_cleanup_id)?;
            }
            return Ok(());
        }
        for step in Step::ALL {
            sequence.push(step);
            run_sequences(sequence, remaining - 1, key)?;
            let _ = sequence.pop();
        }
        Ok(())
    }

    #[test]
    fn reduce_preserves_invariants_over_bounded_sequences() -> Result<(), &'static str> {
        let key = key(13)?;
        run_sequences(&mut Vec::new(), 4, key)?;
        Ok(())
    }
}
