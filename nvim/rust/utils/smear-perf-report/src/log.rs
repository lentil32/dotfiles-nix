use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::path::Path;

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::error::ReportError;

const PERF_JSON_PREFIX: &str = "PERF_JSON ";
const PERF_JSON_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Schema {
    WindowSwitch,
    ParticleToggle,
}

impl Schema {
    pub(crate) fn parse(raw: &str) -> Result<Self, ReportError> {
        match raw {
            "window-switch" => Ok(Self::WindowSwitch),
            "particle-toggle" => Ok(Self::ParticleToggle),
            _ => Err(ReportError::UnsupportedSchema {
                schema: raw.to_owned(),
            }),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::WindowSwitch => "window-switch",
            Self::ParticleToggle => "particle-toggle",
        }
    }
}

impl std::fmt::Display for Schema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Deserialize)]
struct RawRecord {
    schema: String,
    version: u32,
    kind: String,
    payload: Value,
}

#[derive(Debug)]
struct PerfRecord {
    line: usize,
    schema: Schema,
    kind: String,
    payload: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ScenarioPayload {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    preset: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct LibraryPayload {
    module_path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PhasePayload {
    name: String,
    iterations: u64,
    elapsed_ms: f64,
    avg_us: f64,
    floating_windows: u64,
    visible_floating_windows: u64,
    smear_floating_windows: u64,
    visible_smear_floating_windows: u64,
    lua_memory_kib: f64,
}

#[derive(Clone, Debug, Deserialize)]
struct FieldsPayload {
    phase: String,
    raw: String,
    fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize)]
struct FieldsSnapshot {
    raw: String,
    #[serde(flatten)]
    fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RecoveryWaitPayload {
    mode: String,
    elapsed_ms: f64,
    reached_cold: bool,
    timed_out: bool,
    cleanup_thermal: String,
    compaction_target_reached: String,
    queue_total_backlog: String,
    pool_total_windows: String,
    pool_cached_budget: String,
    pool_peak_requested_capacity: String,
    pool_capacity_cap_hits: String,
    max_kept_windows: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RecoveryStatePayload {
    cleanup_thermal: String,
    compaction_target_reached: String,
    queue_total_backlog: String,
    delayed_ingress_pending: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct WindowCountsPayload {
    phase: String,
    floating_windows: u64,
    visible_floating_windows: u64,
    smear_floating_windows: u64,
    visible_smear_floating_windows: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StressSummaryPayload {
    max_avg_us: f64,
    tail_avg_us: f64,
    max_ratio: f64,
    tail_ratio: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct WindowSwitchSummaryPayload {
    baseline_avg_us: f64,
    recovery_avg_us: f64,
    recovery_ratio: f64,
    recovery_wait_mode: String,
    recovery_wait_elapsed_ms: f64,
    recovery_reached_cold: bool,
    recovery_timed_out: bool,
    post_wait_floating_windows: u64,
    post_wait_visible_floating_windows: u64,
    post_wait_smear_floating_windows: u64,
    post_wait_visible_smear_floating_windows: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ParticleToggleConfigPayload {
    warmup_iterations: u64,
    benchmark_iterations: u64,
    retarget_interval: u64,
    particles_enabled: bool,
    time_interval_ms: f64,
    particle_max_num: u64,
    anchor_count: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ParticleToggleSummaryPayload {
    avg_us: f64,
    avg_particles: f64,
    max_particles: u64,
    final_particles: u64,
    retargets: u64,
}

#[derive(Default)]
struct WindowSwitchAccumulator {
    scenario: Option<ScenarioPayload>,
    library: Option<LibraryPayload>,
    config: Option<BTreeMap<String, Value>>,
    phases: BTreeMap<String, PhasePayload>,
    diagnostics: BTreeMap<String, FieldsSnapshot>,
    validation: BTreeMap<String, FieldsSnapshot>,
    recovery_wait: Option<RecoveryWaitPayload>,
    recovery_state: Option<RecoveryStatePayload>,
    window_counts: BTreeMap<String, WindowCountsPayload>,
    stress_summary: Option<StressSummaryPayload>,
    summary: Option<WindowSwitchSummaryPayload>,
}

#[derive(Default)]
struct ParticleToggleAccumulator {
    scenario: Option<ScenarioPayload>,
    library: Option<LibraryPayload>,
    config: Option<ParticleToggleConfigPayload>,
    summary: Option<ParticleToggleSummaryPayload>,
}

#[derive(Debug, Serialize)]
struct WindowSwitchDocument {
    schema: &'static str,
    version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    scenario: Option<ScenarioPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    library: Option<LibraryPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    config: Option<BTreeMap<String, Value>>,
    phases: BTreeMap<String, PhasePayload>,
    diagnostics: BTreeMap<String, FieldsSnapshot>,
    validation: BTreeMap<String, FieldsSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recovery_wait: Option<RecoveryWaitPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recovery_state: Option<RecoveryStatePayload>,
    window_counts: BTreeMap<String, WindowCountsPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stress_summary: Option<StressSummaryPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<WindowSwitchSummaryPayload>,
}

#[derive(Debug, Serialize)]
struct ParticleToggleDocument {
    schema: &'static str,
    version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    scenario: Option<ScenarioPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    library: Option<LibraryPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    config: Option<ParticleToggleConfigPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<ParticleToggleSummaryPayload>,
}

pub(crate) fn load_summary_value(schema: Schema, log_path: &Path) -> Result<Value, ReportError> {
    let file = File::open(log_path).map_err(|source| ReportError::ReadFile {
        path: log_path.display().to_string(),
        source,
    })?;
    let reader = BufReader::new(file);
    load_summary_value_from_reader(schema, reader)
}

pub(crate) fn load_summary_value_from_reader<R>(
    schema: Schema,
    reader: R,
) -> Result<Value, ReportError>
where
    R: BufRead,
{
    let records = read_perf_records(schema, reader)?;
    match schema {
        Schema::WindowSwitch => load_window_switch_summary(records),
        Schema::ParticleToggle => load_particle_toggle_summary(records),
    }
}

fn read_perf_records<R>(schema: Schema, reader: R) -> Result<Vec<PerfRecord>, ReportError>
where
    R: BufRead,
{
    let mut records = Vec::new();
    for (index, line_result) in reader.lines().enumerate() {
        let line_number = index + 1;
        let line = line_result.map_err(|source| ReportError::ReadLogLine {
            line: line_number,
            source,
        })?;
        let Some(json_line) = line.strip_prefix(PERF_JSON_PREFIX) else {
            continue;
        };
        let raw_record = serde_json::from_str::<RawRecord>(json_line).map_err(|source| {
            ReportError::InvalidPerfJson {
                line: line_number,
                source,
            }
        })?;
        if raw_record.version != PERF_JSON_VERSION {
            return Err(ReportError::UnsupportedVersion {
                line: line_number,
                version: raw_record.version,
            });
        }
        let record_schema = Schema::parse(&raw_record.schema)?;
        if record_schema != schema {
            return Err(ReportError::UnexpectedSchema {
                expected: schema,
                found: raw_record.schema,
            });
        }
        records.push(PerfRecord {
            line: line_number,
            schema: record_schema,
            kind: raw_record.kind,
            payload: raw_record.payload,
        });
    }
    if records.is_empty() {
        return Err(ReportError::MissingPerfJsonEvents);
    }
    Ok(records)
}

fn load_window_switch_summary(records: Vec<PerfRecord>) -> Result<Value, ReportError> {
    let mut summary = WindowSwitchAccumulator::default();
    for record in records {
        match record.kind.as_str() {
            "scenario" => set_unique(
                &mut summary.scenario,
                "scenario",
                deserialize_payload(record)?,
            )?,
            "library" => set_unique(
                &mut summary.library,
                "library",
                deserialize_payload(record)?,
            )?,
            "config" => set_unique(&mut summary.config, "config", deserialize_payload(record)?)?,
            "phase" => {
                let payload: PhasePayload = deserialize_payload(record)?;
                insert_unique(&mut summary.phases, "phase", payload.name.clone(), payload)?;
            }
            "diagnostics" => {
                let payload: FieldsPayload = deserialize_payload(record)?;
                insert_unique(
                    &mut summary.diagnostics,
                    "diagnostics",
                    payload.phase.clone(),
                    payload.into_snapshot(),
                )?;
            }
            "validation" => {
                let payload: FieldsPayload = deserialize_payload(record)?;
                insert_unique(
                    &mut summary.validation,
                    "validation",
                    payload.phase.clone(),
                    payload.into_snapshot(),
                )?;
            }
            "recovery_wait" => set_unique(
                &mut summary.recovery_wait,
                "recovery_wait",
                deserialize_payload(record)?,
            )?,
            "recovery_state" => set_unique(
                &mut summary.recovery_state,
                "recovery_state",
                deserialize_payload(record)?,
            )?,
            "window_counts" => {
                let payload: WindowCountsPayload = deserialize_payload(record)?;
                insert_unique(
                    &mut summary.window_counts,
                    "window_counts",
                    payload.phase.clone(),
                    payload,
                )?;
            }
            "stress_summary" => set_unique(
                &mut summary.stress_summary,
                "stress_summary",
                deserialize_payload(record)?,
            )?,
            "summary" => set_unique(
                &mut summary.summary,
                "summary",
                deserialize_payload(record)?,
            )?,
            _ => {
                return Err(ReportError::UnsupportedEventKind {
                    schema: record.schema,
                    kind: record.kind,
                });
            }
        }
    }

    serde_json::to_value(WindowSwitchDocument {
        schema: Schema::WindowSwitch.as_str(),
        version: PERF_JSON_VERSION,
        scenario: summary.scenario,
        library: summary.library,
        config: summary.config,
        phases: summary.phases,
        diagnostics: summary.diagnostics,
        validation: summary.validation,
        recovery_wait: summary.recovery_wait,
        recovery_state: summary.recovery_state,
        window_counts: summary.window_counts,
        stress_summary: summary.stress_summary,
        summary: summary.summary,
    })
    .map_err(|source| ReportError::InvalidPerfJson { line: 0, source })
}

fn load_particle_toggle_summary(records: Vec<PerfRecord>) -> Result<Value, ReportError> {
    let mut summary = ParticleToggleAccumulator::default();
    for record in records {
        match record.kind.as_str() {
            "scenario" => set_unique(
                &mut summary.scenario,
                "scenario",
                deserialize_payload(record)?,
            )?,
            "library" => set_unique(
                &mut summary.library,
                "library",
                deserialize_payload(record)?,
            )?,
            "config" => set_unique(&mut summary.config, "config", deserialize_payload(record)?)?,
            "summary" => set_unique(
                &mut summary.summary,
                "summary",
                deserialize_payload(record)?,
            )?,
            _ => {
                return Err(ReportError::UnsupportedEventKind {
                    schema: record.schema,
                    kind: record.kind,
                });
            }
        }
    }

    serde_json::to_value(ParticleToggleDocument {
        schema: Schema::ParticleToggle.as_str(),
        version: PERF_JSON_VERSION,
        scenario: summary.scenario,
        library: summary.library,
        config: summary.config,
        summary: summary.summary,
    })
    .map_err(|source| ReportError::InvalidPerfJson { line: 0, source })
}

fn deserialize_payload<T>(record: PerfRecord) -> Result<T, ReportError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(record.payload).map_err(|source| ReportError::InvalidPerfJson {
        line: record.line,
        source,
    })
}

fn set_unique<T>(slot: &mut Option<T>, kind: &'static str, value: T) -> Result<(), ReportError> {
    if slot.is_some() {
        return Err(ReportError::DuplicateSingletonEvent { kind });
    }
    *slot = Some(value);
    Ok(())
}

fn insert_unique<T>(
    map: &mut BTreeMap<String, T>,
    collection: &'static str,
    key: String,
    value: T,
) -> Result<(), ReportError> {
    if map.insert(key.clone(), value).is_some() {
        return Err(ReportError::DuplicateKeyedEvent { collection, key });
    }
    Ok(())
}

impl FieldsPayload {
    fn into_snapshot(self) -> FieldsSnapshot {
        FieldsSnapshot {
            raw: self.raw,
            fields: self.fields,
        }
    }
}

#[cfg(test)]
mod tests;
