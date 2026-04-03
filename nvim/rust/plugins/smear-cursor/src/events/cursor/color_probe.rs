use super::CursorParseError;
use super::CursorReadError;
use super::cursor_parse_error;
use crate::core::effect::ProbePolicy;
use crate::core::state::ProbeKind;
use crate::core::types::Generation;
use crate::events::host_bridge::installed_host_bridge;
use crate::events::runtime::record_probe_extmark_fallback;
use crate::lua::bool_from_object_typed;
use crate::lua::i64_from_object_typed;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::Result;
use nvim_oxi::String as NvimString;
use nvim_oxi::conversion::FromObject;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct CursorColorProbeResult {
    color: Option<u32>,
    used_extmark_fallback: bool,
}

impl CursorColorProbeResult {
    const fn new(color: Option<u32>, used_extmark_fallback: bool) -> Self {
        Self {
            color,
            used_extmark_fallback,
        }
    }
}

fn cursor_color_at_current_position(
    colorscheme_generation: Generation,
    probe_policy: ProbePolicy,
) -> Result<Option<u32>> {
    let value = installed_host_bridge()?.cursor_color_at_cursor(
        colorscheme_generation,
        probe_policy.allows_cursor_color_extmark_fallback(),
    )?;
    let probe_result = parse_cursor_color_probe_result(value)?;
    if probe_result.used_extmark_fallback {
        record_probe_extmark_fallback(ProbeKind::CursorColor);
    }
    Ok(probe_result.color)
}

fn parse_cursor_color_probe_result(value: Object) -> Result<CursorColorProbeResult> {
    if value.is_nil() {
        return Ok(CursorColorProbeResult::new(None, false));
    }

    if let Ok(dict) = Dictionary::from_object(value.clone()) {
        let context = "cursor_color_host_bridge";
        let field = "used_extmark_fallback";
        let used_extmark_fallback =
            dict.get(&NvimString::from(field))
                .cloned()
                .ok_or(CursorReadError::from(
                    CursorParseError::DictionaryMissingField { context, field },
                ))?;
        let used_extmark_fallback = bool_from_object_typed(
            "cursor_color_host_bridge.used_extmark_fallback",
            used_extmark_fallback,
        )
        .map_err(|source| {
            nvim_oxi::Error::from(cursor_parse_error(
                "cursor_color_host_bridge.used_extmark_fallback",
                source,
            ))
        })?;
        let color = dict
            .get(&NvimString::from("color"))
            .cloned()
            .filter(|value| !value.is_nil())
            .map(parse_cursor_color_host_bridge_color)
            .transpose()?;
        return Ok(CursorColorProbeResult::new(color, used_extmark_fallback));
    }

    Ok(CursorColorProbeResult::new(
        Some(parse_cursor_color_host_bridge_color(value)?),
        false,
    ))
}

fn parse_cursor_color_host_bridge_color(value: Object) -> Result<u32> {
    let parsed = i64_from_object_typed("cursor_color_host_bridge", value).map_err(|source| {
        nvim_oxi::Error::from(cursor_parse_error("cursor_color_host_bridge", source))
    })?;
    Ok(u32::try_from(parsed).map_err(|_| {
        nvim_oxi::api::Error::Other(
            "cursor_color_host_bridge parse failed: color out of range".into(),
        )
    })?)
}

pub(crate) fn sampled_cursor_color_at_current_position(
    colorscheme_generation: Generation,
    probe_policy: ProbePolicy,
) -> Result<Option<u32>> {
    cursor_color_at_current_position(colorscheme_generation, probe_policy)
}

#[cfg(test)]
mod tests {
    use super::CursorColorProbeResult;
    use super::parse_cursor_color_probe_result;
    use nvim_oxi::Dictionary;
    use nvim_oxi::Object;
    use pretty_assertions::assert_eq;

    fn cursor_color_probe_result_object(color: Option<i64>, used_extmark_fallback: bool) -> Object {
        let mut dict = Dictionary::new();
        dict.insert("color", color.map_or_else(Object::nil, Object::from));
        dict.insert("used_extmark_fallback", Object::from(used_extmark_fallback));
        Object::from(dict)
    }

    #[test]
    fn cursor_color_probe_result_accepts_structured_payloads() {
        assert_eq!(
            parse_cursor_color_probe_result(cursor_color_probe_result_object(
                Some(0x00AB_CDEF),
                true,
            ))
            .expect("structured cursor color payload should parse"),
            CursorColorProbeResult::new(Some(0x00AB_CDEF), true),
        );
    }

    #[test]
    fn cursor_color_probe_result_accepts_legacy_scalar_payloads() {
        assert_eq!(
            parse_cursor_color_probe_result(Object::from(0x00AB_CDEF_i64))
                .expect("legacy scalar cursor color payload should parse"),
            CursorColorProbeResult::new(Some(0x00AB_CDEF), false),
        );
    }
}
