#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum Ingress {
    Autocmd(AutocmdIngress),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum AutocmdIngress {
    BufWipeout,
    CmdlineChanged,
    CursorMoved,
    CursorMovedInsert,
    ModeChanged,
    OptionSet,
    TabClosed,
    TextChanged,
    TextChangedInsert,
    VimResized,
    WinEnter,
    WinClosed,
    WinScrolled,
    BufEnter,
    ColorScheme,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum AutocmdDispatchRoute {
    Cursor(CursorAutocmdIngress),
    NonCursor(NonCursorAutocmdIngress),
    ColorScheme,
    ShellOnlyTeardown(TeardownAutocmdIngress),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum CursorAutocmdIngress {
    CmdlineChanged,
    CursorMoved,
    CursorMovedInsert,
    ModeChanged,
    WinEnter,
    WinScrolled,
    BufEnter,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum NonCursorAutocmdIngress {
    OptionSet,
    TextChanged,
    TextChangedInsert,
    VimResized,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum TeardownAutocmdIngress {
    BufWipeout,
    TabClosed,
    WinClosed,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct AutocmdIngressMapping {
    event_name: &'static str,
    ingress: AutocmdIngress,
}

const AUTOCMD_INGRESS_MAPPINGS: [AutocmdIngressMapping; 15] = [
    AutocmdIngressMapping {
        event_name: "BufWipeout",
        ingress: AutocmdIngress::BufWipeout,
    },
    AutocmdIngressMapping {
        event_name: "CmdlineChanged",
        ingress: AutocmdIngress::CmdlineChanged,
    },
    AutocmdIngressMapping {
        event_name: "CursorMoved",
        ingress: AutocmdIngress::CursorMoved,
    },
    AutocmdIngressMapping {
        event_name: "CursorMovedI",
        ingress: AutocmdIngress::CursorMovedInsert,
    },
    AutocmdIngressMapping {
        event_name: "ModeChanged",
        ingress: AutocmdIngress::ModeChanged,
    },
    AutocmdIngressMapping {
        event_name: "OptionSet",
        ingress: AutocmdIngress::OptionSet,
    },
    AutocmdIngressMapping {
        event_name: "TabClosed",
        ingress: AutocmdIngress::TabClosed,
    },
    AutocmdIngressMapping {
        event_name: "TextChanged",
        ingress: AutocmdIngress::TextChanged,
    },
    AutocmdIngressMapping {
        event_name: "TextChangedI",
        ingress: AutocmdIngress::TextChangedInsert,
    },
    AutocmdIngressMapping {
        event_name: "VimResized",
        ingress: AutocmdIngress::VimResized,
    },
    // Surprising: switching windows in the same buffer may not emit CursorMoved.
    AutocmdIngressMapping {
        event_name: "WinEnter",
        ingress: AutocmdIngress::WinEnter,
    },
    AutocmdIngressMapping {
        event_name: "WinClosed",
        ingress: AutocmdIngress::WinClosed,
    },
    AutocmdIngressMapping {
        event_name: "WinScrolled",
        ingress: AutocmdIngress::WinScrolled,
    },
    AutocmdIngressMapping {
        event_name: "BufEnter",
        ingress: AutocmdIngress::BufEnter,
    },
    AutocmdIngressMapping {
        event_name: "ColorScheme",
        ingress: AutocmdIngress::ColorScheme,
    },
];

pub(super) fn parse_autocmd_ingress(event_name: &str) -> Option<AutocmdIngress> {
    AUTOCMD_INGRESS_MAPPINGS.iter().find_map(|mapping| {
        if mapping.event_name == event_name {
            Some(mapping.ingress)
        } else {
            None
        }
    })
}

pub(super) fn registered_autocmd_event_names() -> impl Iterator<Item = &'static str> {
    AUTOCMD_INGRESS_MAPPINGS
        .iter()
        .map(|mapping| mapping.event_name)
}

impl AutocmdIngress {
    pub(super) const fn dispatch_route(self) -> AutocmdDispatchRoute {
        match self {
            Self::BufWipeout => {
                AutocmdDispatchRoute::ShellOnlyTeardown(TeardownAutocmdIngress::BufWipeout)
            }
            Self::CmdlineChanged => {
                AutocmdDispatchRoute::Cursor(CursorAutocmdIngress::CmdlineChanged)
            }
            Self::CursorMoved => AutocmdDispatchRoute::Cursor(CursorAutocmdIngress::CursorMoved),
            Self::CursorMovedInsert => {
                AutocmdDispatchRoute::Cursor(CursorAutocmdIngress::CursorMovedInsert)
            }
            Self::ModeChanged => AutocmdDispatchRoute::Cursor(CursorAutocmdIngress::ModeChanged),
            Self::OptionSet => AutocmdDispatchRoute::NonCursor(NonCursorAutocmdIngress::OptionSet),
            Self::TabClosed => {
                AutocmdDispatchRoute::ShellOnlyTeardown(TeardownAutocmdIngress::TabClosed)
            }
            Self::TextChanged => {
                AutocmdDispatchRoute::NonCursor(NonCursorAutocmdIngress::TextChanged)
            }
            Self::TextChangedInsert => {
                AutocmdDispatchRoute::NonCursor(NonCursorAutocmdIngress::TextChangedInsert)
            }
            Self::VimResized => {
                AutocmdDispatchRoute::NonCursor(NonCursorAutocmdIngress::VimResized)
            }
            Self::WinEnter => AutocmdDispatchRoute::Cursor(CursorAutocmdIngress::WinEnter),
            Self::WinClosed => {
                AutocmdDispatchRoute::ShellOnlyTeardown(TeardownAutocmdIngress::WinClosed)
            }
            Self::WinScrolled => AutocmdDispatchRoute::Cursor(CursorAutocmdIngress::WinScrolled),
            Self::BufEnter => AutocmdDispatchRoute::Cursor(CursorAutocmdIngress::BufEnter),
            Self::ColorScheme => AutocmdDispatchRoute::ColorScheme,
        }
    }
}

impl CursorAutocmdIngress {
    pub(super) const fn requests_observation_base(self) -> bool {
        matches!(
            self,
            Self::CursorMoved
                | Self::CursorMovedInsert
                | Self::WinEnter
                | Self::WinScrolled
                | Self::CmdlineChanged
                | Self::ModeChanged
                | Self::BufEnter
        )
    }

    pub(super) const fn supports_unchanged_fast_path(self) -> bool {
        matches!(self, Self::WinEnter | Self::WinScrolled | Self::BufEnter)
    }
}

#[cfg(test)]
mod tests {
    use super::AutocmdDispatchRoute;
    use super::AutocmdIngress;
    use super::CursorAutocmdIngress;
    use super::NonCursorAutocmdIngress;
    use super::TeardownAutocmdIngress;
    use super::parse_autocmd_ingress;
    use super::registered_autocmd_event_names;
    use pretty_assertions::assert_eq;

    #[test]
    fn known_autocmd_names_round_trip_to_typed_ingress() {
        for event_name in registered_autocmd_event_names() {
            assert!(parse_autocmd_ingress(event_name).is_some());
        }
    }

    #[test]
    fn close_autocmd_names_map_to_resource_lifecycle_ingress() {
        assert_eq!(
            parse_autocmd_ingress("TabClosed"),
            Some(AutocmdIngress::TabClosed)
        );
        assert_eq!(
            parse_autocmd_ingress("WinClosed"),
            Some(AutocmdIngress::WinClosed)
        );
    }

    #[test]
    fn dispatch_route_classifies_each_autocmd_once() {
        let routes = [
            (
                AutocmdIngress::BufWipeout,
                AutocmdDispatchRoute::ShellOnlyTeardown(TeardownAutocmdIngress::BufWipeout),
            ),
            (
                AutocmdIngress::CmdlineChanged,
                AutocmdDispatchRoute::Cursor(CursorAutocmdIngress::CmdlineChanged),
            ),
            (
                AutocmdIngress::CursorMoved,
                AutocmdDispatchRoute::Cursor(CursorAutocmdIngress::CursorMoved),
            ),
            (
                AutocmdIngress::CursorMovedInsert,
                AutocmdDispatchRoute::Cursor(CursorAutocmdIngress::CursorMovedInsert),
            ),
            (
                AutocmdIngress::ModeChanged,
                AutocmdDispatchRoute::Cursor(CursorAutocmdIngress::ModeChanged),
            ),
            (
                AutocmdIngress::OptionSet,
                AutocmdDispatchRoute::NonCursor(NonCursorAutocmdIngress::OptionSet),
            ),
            (
                AutocmdIngress::TabClosed,
                AutocmdDispatchRoute::ShellOnlyTeardown(TeardownAutocmdIngress::TabClosed),
            ),
            (
                AutocmdIngress::TextChanged,
                AutocmdDispatchRoute::NonCursor(NonCursorAutocmdIngress::TextChanged),
            ),
            (
                AutocmdIngress::TextChangedInsert,
                AutocmdDispatchRoute::NonCursor(NonCursorAutocmdIngress::TextChangedInsert),
            ),
            (
                AutocmdIngress::VimResized,
                AutocmdDispatchRoute::NonCursor(NonCursorAutocmdIngress::VimResized),
            ),
            (
                AutocmdIngress::WinEnter,
                AutocmdDispatchRoute::Cursor(CursorAutocmdIngress::WinEnter),
            ),
            (
                AutocmdIngress::WinClosed,
                AutocmdDispatchRoute::ShellOnlyTeardown(TeardownAutocmdIngress::WinClosed),
            ),
            (
                AutocmdIngress::WinScrolled,
                AutocmdDispatchRoute::Cursor(CursorAutocmdIngress::WinScrolled),
            ),
            (
                AutocmdIngress::BufEnter,
                AutocmdDispatchRoute::Cursor(CursorAutocmdIngress::BufEnter),
            ),
            (
                AutocmdIngress::ColorScheme,
                AutocmdDispatchRoute::ColorScheme,
            ),
        ];

        assert_eq!(
            routes
                .into_iter()
                .map(|(ingress, _)| ingress)
                .collect::<Vec<_>>(),
            [
                AutocmdIngress::BufWipeout,
                AutocmdIngress::CmdlineChanged,
                AutocmdIngress::CursorMoved,
                AutocmdIngress::CursorMovedInsert,
                AutocmdIngress::ModeChanged,
                AutocmdIngress::OptionSet,
                AutocmdIngress::TabClosed,
                AutocmdIngress::TextChanged,
                AutocmdIngress::TextChangedInsert,
                AutocmdIngress::VimResized,
                AutocmdIngress::WinEnter,
                AutocmdIngress::WinClosed,
                AutocmdIngress::WinScrolled,
                AutocmdIngress::BufEnter,
                AutocmdIngress::ColorScheme,
            ]
        );
        assert_eq!(
            routes
                .into_iter()
                .map(|(ingress, _)| ingress.dispatch_route())
                .collect::<Vec<_>>(),
            routes
                .into_iter()
                .map(|(_, route)| route)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn unchanged_fast_path_stays_limited_to_window_surface_events() {
        for (ingress, expected) in [
            (CursorAutocmdIngress::CursorMoved, false),
            (CursorAutocmdIngress::CursorMovedInsert, false),
            (CursorAutocmdIngress::ModeChanged, false),
            (CursorAutocmdIngress::WinEnter, true),
            (CursorAutocmdIngress::WinScrolled, true),
            (CursorAutocmdIngress::BufEnter, true),
        ] {
            assert_eq!(
                ingress.supports_unchanged_fast_path(),
                expected,
                "unexpected unchanged-fast-path support for {ingress:?}"
            );
        }
    }
}
