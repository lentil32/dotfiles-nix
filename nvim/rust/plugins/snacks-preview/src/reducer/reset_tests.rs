use pretty_assertions::assert_eq;

use super::*;

fn key(raw: i64) -> Result<BufKey, &'static str> {
    BufKey::try_new(raw).ok_or("expected valid buffer key")
}

fn win(raw: i64) -> Result<WinKey, &'static str> {
    WinKey::try_new(raw).ok_or("expected valid window key")
}

#[test]
fn reset_closes_every_owned_resource_on_its_buffer() -> Result<(), &'static str> {
    let mut registry = PreviewRegistry::default();
    let key_a = key(11)?;
    let key_b = key(22)?;
    let _ = registry.reduce(PreviewEvent::Register {
        key: key_b,
        win: win(2)?,
        group: 202,
        restore_name_plan: Some(RestoreNamePlan {
            name: "b.md".to_string(),
            preview_name: "b.md.snacks-preview".to_string(),
        }),
    });
    let _ = registry.reduce(PreviewEvent::Register {
        key: key_a,
        win: win(1)?,
        group: 101,
        restore_name_plan: Some(RestoreNamePlan {
            name: String::new(),
            preview_name: "a.md.snacks-preview".to_string(),
        }),
    });
    let _ = registry.reduce(PreviewEvent::CleanupOpened {
        key: key_a,
        token: PreviewToken::try_new(2).ok_or("expected valid token")?,
        cleanup_id: 303,
    });

    let transition = registry.reduce(PreviewEvent::Reset);

    assert_eq!(
        (transition, registry.previews.len()),
        (
            PreviewTransition::with_effects(vec![
                PreviewEffect::RestoreName {
                    key: key_a,
                    plan: RestoreNamePlan {
                        name: String::new(),
                        preview_name: "a.md.snacks-preview".to_string(),
                    },
                },
                PreviewEffect::DeleteAugroup(101),
                PreviewEffect::CloseCleanup(303),
                PreviewEffect::RestoreName {
                    key: key_b,
                    plan: RestoreNamePlan {
                        name: "b.md".to_string(),
                        preview_name: "b.md.snacks-preview".to_string(),
                    },
                },
                PreviewEffect::DeleteAugroup(202),
            ]),
            0,
        )
    );
    Ok(())
}

#[test]
fn reset_preserves_token_generation_against_stale_callbacks() -> Result<(), &'static str> {
    let mut registry = PreviewRegistry::default();
    let key = key(33)?;
    let first = registry.reduce(PreviewEvent::Register {
        key,
        win: win(3)?,
        group: 1,
        restore_name_plan: None,
    });
    let _ = registry.reduce(PreviewEvent::Reset);

    let second = registry.reduce(PreviewEvent::Register {
        key,
        win: win(3)?,
        group: 2,
        restore_name_plan: None,
    });
    let stale = registry.reduce(PreviewEvent::CloseByToken {
        token: PreviewToken::try_new(1).ok_or("expected valid token")?,
    });

    assert_eq!(
        (first.command, second.command, stale),
        (
            Some(PreviewCommand::RequestDocFind(PreviewToken(1))),
            Some(PreviewCommand::RequestDocFind(PreviewToken(2))),
            PreviewTransition::default(),
        )
    );
    Ok(())
}
