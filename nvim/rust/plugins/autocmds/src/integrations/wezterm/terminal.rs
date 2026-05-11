use std::io::Error;
use std::io::ErrorKind;
use std::io::Write;
use std::process::ExitStatus;
use std::thread;
use std::time::Duration;

use nvimrs_support::TabTitle;
use percent_encoding::AsciiSet;
use percent_encoding::NON_ALPHANUMERIC;
use percent_encoding::percent_encode;

use super::WeztermCommandRunner;

const TERMINAL_WRITE_MAX_WOULD_BLOCK_RETRIES: usize = 16;
const TERMINAL_WRITE_RETRY_DELAY: Duration = Duration::from_millis(5);
const FILE_URL_PATH_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

#[cfg(unix)]
fn successful_exit_status() -> ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    ExitStatus::from_raw(0)
}

#[cfg(windows)]
fn successful_exit_status() -> ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    ExitStatus::from_raw(0)
}

fn tmux_passthrough_enabled() -> bool {
    std::env::var_os("TMUX").is_some()
}

fn with_tmux_passthrough(osc: &str, passthrough_enabled: bool) -> String {
    if !passthrough_enabled {
        return osc.to_string();
    }
    let mut tmux_passthrough = String::from("\u{1b}Ptmux;");
    for ch in osc.chars() {
        if ch == '\u{1b}' {
            tmux_passthrough.push(ch);
        }
        tmux_passthrough.push(ch);
    }
    tmux_passthrough.push_str("\u{1b}\\");
    tmux_passthrough.push_str(osc);
    tmux_passthrough
}

fn build_tab_title_sequence(title: &TabTitle) -> String {
    let osc = format!("\u{1b}]1;{}\u{1b}\\", title.as_str());
    with_tmux_passthrough(&osc, tmux_passthrough_enabled())
}

fn build_working_dir_sequence(cwd: &str) -> std::io::Result<String> {
    let cwd = std::path::Path::new(cwd);
    if !cwd.is_absolute() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("cwd {cwd:?} is not an absolute path"),
        ));
    }
    let host = hostname::get()
        .ok()
        .and_then(|host| host.into_string().ok())
        .filter(|host| !host.is_empty())
        .unwrap_or_else(|| "localhost".to_string());
    #[cfg(unix)]
    let encoded_path = {
        use std::os::unix::ffi::OsStrExt;
        percent_encode(cwd.as_os_str().as_bytes(), FILE_URL_PATH_ENCODE_SET).to_string()
    };
    #[cfg(windows)]
    let encoded_path = {
        let normalized = cwd.to_string_lossy().replace('\\', "/");
        percent_encode(normalized.as_bytes(), FILE_URL_PATH_ENCODE_SET).to_string()
    };

    let osc = format!("\u{1b}]7;file://{host}{encoded_path}\u{1b}\\");
    Ok(with_tmux_passthrough(&osc, tmux_passthrough_enabled()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TerminalWriteRetry {
    max_would_block_retries: usize,
    delay: Duration,
}

impl TerminalWriteRetry {
    const DEFAULT: Self = Self {
        max_would_block_retries: TERMINAL_WRITE_MAX_WOULD_BLOCK_RETRIES,
        delay: TERMINAL_WRITE_RETRY_DELAY,
    };
}

#[derive(Debug, Clone, Copy)]
struct WouldBlockRetryBudget {
    remaining: usize,
    max: usize,
    delay: Duration,
}

impl WouldBlockRetryBudget {
    const fn new(retry: TerminalWriteRetry) -> Self {
        Self {
            remaining: retry.max_would_block_retries,
            max: retry.max_would_block_retries,
            delay: retry.delay,
        }
    }

    const fn reset(&mut self) {
        self.remaining = self.max;
    }

    fn retry_or_fail(&mut self, err: Error) -> std::io::Result<()> {
        if self.remaining == 0 {
            return Err(err);
        }
        self.remaining -= 1;
        if !self.delay.is_zero() {
            thread::sleep(self.delay);
        }
        Ok(())
    }
}

fn write_all_with_retry<W>(
    writer: &mut W,
    mut bytes: &[u8],
    retry: TerminalWriteRetry,
) -> std::io::Result<()>
where
    W: Write + ?Sized,
{
    let mut retry_budget = WouldBlockRetryBudget::new(retry);
    while !bytes.is_empty() {
        match writer.write(bytes) {
            Ok(0) => {
                return Err(Error::new(
                    ErrorKind::WriteZero,
                    "failed to write terminal escape sequence",
                ));
            }
            Ok(written) => {
                bytes = &bytes[written..];
                retry_budget.reset();
            }
            Err(err) if err.kind() == ErrorKind::Interrupted => {}
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                retry_budget.retry_or_fail(err)?;
            }
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

fn flush_with_retry<W>(writer: &mut W, retry: TerminalWriteRetry) -> std::io::Result<()>
where
    W: Write + ?Sized,
{
    let mut retry_budget = WouldBlockRetryBudget::new(retry);
    loop {
        match writer.flush() {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == ErrorKind::Interrupted => {}
            Err(err) if err.kind() == ErrorKind::WouldBlock => retry_budget.retry_or_fail(err)?,
            Err(err) => return Err(err),
        }
    }
}

fn write_sequence_to<W>(writer: &mut W, sequence: &str) -> std::io::Result<()>
where
    W: Write + ?Sized,
{
    write_all_with_retry(writer, sequence.as_bytes(), TerminalWriteRetry::DEFAULT)?;
    flush_with_retry(writer, TerminalWriteRetry::DEFAULT)
}

fn write_sequence_to_stdout(sequence: &str) -> std::io::Result<()> {
    let mut stdout = std::io::stdout().lock();
    write_sequence_to(&mut stdout, sequence)
}

#[cfg(unix)]
fn write_terminal_sequence(sequence: &str) -> std::io::Result<()> {
    match std::fs::OpenOptions::new().write(true).open("/dev/tty") {
        Ok(mut tty) => write_sequence_to(&mut tty, sequence),
        Err(_) => write_sequence_to_stdout(sequence),
    }
}

#[cfg(not(unix))]
fn write_terminal_sequence(sequence: &str) -> std::io::Result<()> {
    write_sequence_to_stdout(sequence)
}

#[derive(Debug, Default)]
pub(super) struct EscapeSequenceWeztermCommandRunner;

impl WeztermCommandRunner for EscapeSequenceWeztermCommandRunner {
    fn run_tab_title(&self, title: &TabTitle) -> std::io::Result<ExitStatus> {
        let sequence = build_tab_title_sequence(title);
        write_terminal_sequence(&sequence)?;
        Ok(successful_exit_status())
    }

    fn run_working_dir(&self, cwd: &str) -> std::io::Result<ExitStatus> {
        let sequence = build_working_dir_sequence(cwd)?;
        write_terminal_sequence(&sequence)?;
        Ok(successful_exit_status())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    type TestResult<T = ()> = std::result::Result<T, &'static str>;

    #[derive(Debug, Default)]
    struct WouldBlockWriter {
        write_attempts: usize,
        flush_attempts: usize,
        block_next_write: bool,
        block_next_flush: bool,
        bytes: Vec<u8>,
    }

    impl Write for WouldBlockWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.write_attempts += 1;
            if self.block_next_write {
                self.block_next_write = false;
                return Err(Error::new(ErrorKind::WouldBlock, "try write again"));
            }
            self.bytes.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.flush_attempts += 1;
            if self.block_next_flush {
                self.block_next_flush = false;
                return Err(Error::new(ErrorKind::WouldBlock, "try flush again"));
            }
            Ok(())
        }
    }

    #[test]
    fn terminal_sequence_writer_retries_would_block_write() -> TestResult {
        let mut writer = WouldBlockWriter {
            block_next_write: true,
            ..WouldBlockWriter::default()
        };
        write_all_with_retry(
            &mut writer,
            b"\x1b]7;file://host/tmp\x1b\\",
            TerminalWriteRetry {
                max_would_block_retries: 1,
                delay: Duration::ZERO,
            },
        )
        .map_err(|_| "expected retry to recover")?;

        assert_eq!(writer.write_attempts, 2);
        assert_eq!(writer.bytes, b"\x1b]7;file://host/tmp\x1b\\");
        Ok(())
    }

    #[test]
    fn terminal_sequence_writer_retries_would_block_flush() -> TestResult {
        let mut writer = WouldBlockWriter {
            block_next_flush: true,
            ..WouldBlockWriter::default()
        };
        write_sequence_to(&mut writer, "\x1b]7;file://host/tmp\x1b\\")
            .map_err(|_| "expected flush retry to recover")?;

        assert_eq!(writer.flush_attempts, 2);
        assert_eq!(writer.bytes, b"\x1b]7;file://host/tmp\x1b\\");
        Ok(())
    }

    #[test]
    fn with_tmux_passthrough_noop_when_disabled() {
        let osc = "\u{1b}]1;tab-title\u{1b}\\";
        assert_eq!(with_tmux_passthrough(osc, false), osc);
    }

    #[test]
    fn with_tmux_passthrough_wraps_when_enabled() {
        let osc = "\u{1b}]1;tab-title\u{1b}\\";
        let wrapped = with_tmux_passthrough(osc, true);
        assert!(wrapped.starts_with("\u{1b}Ptmux;"));
        assert!(wrapped.ends_with(osc));
    }

    #[test]
    fn working_dir_sequence_contains_osc7_payload() -> TestResult {
        let sequence =
            build_working_dir_sequence("/tmp").map_err(|_| "expected valid OSC7 sequence")?;
        assert!(sequence.contains("]7;file://"));
        Ok(())
    }
}
