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
pub(super) enum AutocmdHostPhase {
    ImmediateHostAllowed,
    ShellOnlyTeardown,
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
    pub(super) const fn host_phase(self) -> AutocmdHostPhase {
        match self {
            Self::BufWipeout | Self::TabClosed | Self::WinClosed => {
                AutocmdHostPhase::ShellOnlyTeardown
            }
            Self::CmdlineChanged
            | Self::CursorMoved
            | Self::CursorMovedInsert
            | Self::ModeChanged
            | Self::OptionSet
            | Self::TextChanged
            | Self::TextChangedInsert
            | Self::VimResized
            | Self::WinEnter
            | Self::WinScrolled
            | Self::BufEnter
            | Self::ColorScheme => AutocmdHostPhase::ImmediateHostAllowed,
        }
    }

    pub(super) const fn teardown_ingress(self) -> Option<TeardownAutocmdIngress> {
        match self {
            Self::BufWipeout => Some(TeardownAutocmdIngress::BufWipeout),
            Self::TabClosed => Some(TeardownAutocmdIngress::TabClosed),
            Self::WinClosed => Some(TeardownAutocmdIngress::WinClosed),
            Self::CmdlineChanged
            | Self::CursorMoved
            | Self::CursorMovedInsert
            | Self::ModeChanged
            | Self::OptionSet
            | Self::TextChanged
            | Self::TextChangedInsert
            | Self::VimResized
            | Self::WinEnter
            | Self::WinScrolled
            | Self::BufEnter
            | Self::ColorScheme => None,
        }
    }

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
    use super::AutocmdHostPhase;
    use super::AutocmdIngress;
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
    fn close_autocmds_are_the_only_shell_only_teardown_ingresses() {
        for ingress in [
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
        ] {
            let expected_phase = match ingress {
                AutocmdIngress::BufWipeout
                | AutocmdIngress::TabClosed
                | AutocmdIngress::WinClosed => AutocmdHostPhase::ShellOnlyTeardown,
                AutocmdIngress::CmdlineChanged
                | AutocmdIngress::CursorMoved
                | AutocmdIngress::CursorMovedInsert
                | AutocmdIngress::ModeChanged
                | AutocmdIngress::OptionSet
                | AutocmdIngress::TextChanged
                | AutocmdIngress::TextChangedInsert
                | AutocmdIngress::VimResized
                | AutocmdIngress::WinEnter
                | AutocmdIngress::WinScrolled
                | AutocmdIngress::BufEnter
                | AutocmdIngress::ColorScheme => AutocmdHostPhase::ImmediateHostAllowed,
            };
            assert_eq!(ingress.host_phase(), expected_phase);
        }
    }

    #[test]
    fn teardown_ingress_projection_is_explicit() {
        assert_eq!(
            AutocmdIngress::BufWipeout.teardown_ingress(),
            Some(TeardownAutocmdIngress::BufWipeout)
        );
        assert_eq!(
            AutocmdIngress::TabClosed.teardown_ingress(),
            Some(TeardownAutocmdIngress::TabClosed)
        );
        assert_eq!(
            AutocmdIngress::WinClosed.teardown_ingress(),
            Some(TeardownAutocmdIngress::WinClosed)
        );
        assert_eq!(AutocmdIngress::OptionSet.teardown_ingress(), None);
    }

    #[test]
    fn unchanged_fast_path_stays_limited_to_window_surface_events() {
        for (ingress, expected) in [
            (AutocmdIngress::CursorMoved, false),
            (AutocmdIngress::CursorMovedInsert, false),
            (AutocmdIngress::ModeChanged, false),
            (AutocmdIngress::TabClosed, false),
            (AutocmdIngress::WinEnter, true),
            (AutocmdIngress::WinClosed, false),
            (AutocmdIngress::WinScrolled, true),
            (AutocmdIngress::BufEnter, true),
        ] {
            assert_eq!(
                ingress.supports_unchanged_fast_path(),
                expected,
                "unexpected unchanged-fast-path support for {ingress:?}"
            );
        }
    }
}
