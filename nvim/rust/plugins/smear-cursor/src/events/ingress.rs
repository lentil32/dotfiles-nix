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
    TextChanged,
    TextChangedInsert,
    VimResized,
    WinEnter,
    WinScrolled,
    BufEnter,
    ColorScheme,
    Unknown,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct AutocmdIngressMapping {
    event_name: &'static str,
    ingress: AutocmdIngress,
}

const AUTOCMD_INGRESS_MAPPINGS: [AutocmdIngressMapping; 13] = [
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

pub(super) fn parse_autocmd_ingress(event_name: &str) -> AutocmdIngress {
    AUTOCMD_INGRESS_MAPPINGS
        .iter()
        .find_map(|mapping| {
            if mapping.event_name == event_name {
                Some(mapping.ingress)
            } else {
                None
            }
        })
        .unwrap_or(AutocmdIngress::Unknown)
}

pub(super) fn registered_autocmd_event_names() -> impl Iterator<Item = &'static str> {
    AUTOCMD_INGRESS_MAPPINGS
        .iter()
        .map(|mapping| mapping.event_name)
}

impl AutocmdIngress {
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
    use super::parse_autocmd_ingress;
    use super::registered_autocmd_event_names;
    use super::AutocmdIngress;

    #[test]
    fn known_autocmd_names_round_trip_to_typed_ingress() {
        for event_name in registered_autocmd_event_names() {
            let ingress = parse_autocmd_ingress(event_name);
            assert_ne!(ingress, AutocmdIngress::Unknown);
        }
    }

    #[test]
    fn unknown_autocmd_name_maps_to_explicit_noop_variant() {
        assert_eq!(
            parse_autocmd_ingress("DefinitelyNotReal"),
            AutocmdIngress::Unknown
        );
    }

    #[test]
    fn unchanged_fast_path_stays_limited_to_window_surface_events() {
        for (ingress, expected) in [
            (AutocmdIngress::CursorMoved, false),
            (AutocmdIngress::CursorMovedInsert, false),
            (AutocmdIngress::ModeChanged, false),
            (AutocmdIngress::WinEnter, true),
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
