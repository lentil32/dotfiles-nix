use serde_json::Value;

use crate::error::ReportError;

pub(crate) fn render_query_row(
    summary: &Value,
    field_specs: &[String],
) -> Result<String, ReportError> {
    field_specs
        .iter()
        .map(|field_spec| resolve_field_spec(summary, field_spec))
        .collect::<Result<Vec<_>, _>>()
        .map(|values| values.join("\t"))
}

fn resolve_field_spec(summary: &Value, field_spec: &str) -> Result<String, ReportError> {
    let (path, default) = field_spec
        .split_once('=')
        .map_or((field_spec, None), |(path, default)| (path, Some(default)));
    let Some(value) = lookup_path(summary, path) else {
        return default.map_or_else(
            || {
                Err(ReportError::MissingField {
                    path: path.to_owned(),
                })
            },
            |value| Ok(value.to_owned()),
        );
    };
    stringify_scalar(path, value)
}

fn lookup_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cursor = value;
    for segment in path.split('.') {
        cursor = cursor.as_object()?.get(segment)?;
    }
    Some(cursor)
}

fn stringify_scalar(path: &str, value: &Value) -> Result<String, ReportError> {
    match value {
        Value::Null => Ok(String::new()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        Value::String(value) => Ok(value.clone()),
        Value::Array(_) | Value::Object(_) => Err(ReportError::NonScalarField {
            path: path.to_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::render_query_row;

    #[test]
    fn render_query_row_returns_scalar_values_in_requested_order() {
        let summary = json!({
            "summary": {
                "baseline_avg_us": 12.5,
                "recovery_ratio": 1.1
            },
            "diagnostics": {
                "post_recovery": {
                    "perf_class": "fast"
                }
            }
        });

        let row = render_query_row(
            &summary,
            &[
                "summary.baseline_avg_us".to_owned(),
                "diagnostics.post_recovery.perf_class".to_owned(),
                "summary.recovery_ratio".to_owned(),
            ],
        )
        .expect("query row should render");

        assert_eq!(row, "12.5\tfast\t1.1");
    }

    #[test]
    fn render_query_row_uses_defaults_for_missing_optional_fields() {
        let summary = json!({
            "summary": {
                "baseline_avg_us": 12.5
            }
        });

        let row = render_query_row(
            &summary,
            &[
                "summary.baseline_avg_us".to_owned(),
                "validation.post_recovery.prh=0".to_owned(),
                "diagnostics.post_recovery.perf_class=na".to_owned(),
            ],
        )
        .expect("query row should render");

        assert_eq!(row, "12.5\t0\tna");
    }
}
