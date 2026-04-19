use crate::log::Schema;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ReportError {
    #[error("{0}")]
    Usage(String),
    #[error("failed to read {path}: {source}")]
    ReadFile {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read log line {line}: {source}")]
    ReadLogLine {
        line: usize,
        #[source]
        source: std::io::Error,
    },
    #[error("log line {line} contained invalid PERF_JSON: {source}")]
    InvalidPerfJson {
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error("unsupported perf schema {schema}")]
    UnsupportedSchema { schema: String },
    #[error("expected {expected} PERF_JSON events, found {found}")]
    UnexpectedSchema { expected: Schema, found: String },
    #[error("no PERF_JSON events found in log")]
    MissingPerfJsonEvents,
    #[error("unsupported PERF_JSON version {version} on line {line}")]
    UnsupportedVersion { line: usize, version: u32 },
    #[error("unsupported PERF_JSON event kind {kind} for {schema}")]
    UnsupportedEventKind { schema: Schema, kind: String },
    #[error("duplicate singleton PERF_JSON event {kind}")]
    DuplicateSingletonEvent { kind: &'static str },
    #[error("duplicate {collection} PERF_JSON event for key {key}")]
    DuplicateKeyedEvent {
        collection: &'static str,
        key: String,
    },
    #[error("missing required PERF_JSON field {path}")]
    MissingField { path: String },
    #[error("PERF_JSON field {path} is not a scalar value")]
    NonScalarField { path: String },
}
