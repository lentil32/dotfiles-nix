mod error;
mod log;
mod query;

use std::path::PathBuf;

use error::ReportError;
use log::Schema;
use log::load_summary_value;
use query::render_query_row;

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), ReportError> {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        return Err(usage_error());
    };
    let Some(schema) = args.next() else {
        return Err(usage_error());
    };
    let schema = Schema::parse(&schema)?;
    let Some(log_path) = args.next() else {
        return Err(usage_error());
    };
    let log_path = PathBuf::from(log_path);
    let summary = load_summary_value(schema, &log_path)?;

    match command.as_str() {
        "summary" => {
            if args.next().is_some() {
                return Err(usage_error());
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&summary)
                    .map_err(|source| { ReportError::InvalidPerfJson { line: 0, source } })?
            );
            Ok(())
        }
        "query" => {
            let field_specs = args.collect::<Vec<_>>();
            if field_specs.is_empty() {
                return Err(usage_error());
            }
            println!("{}", render_query_row(&summary, &field_specs)?);
            Ok(())
        }
        _ => Err(usage_error()),
    }
}

fn usage_error() -> ReportError {
    ReportError::Usage(
        "usage: nvimrs-smear-perf-report <summary|query> <window-switch|particle-toggle> <log-file> [field[=default] ...]"
            .to_owned(),
    )
}
