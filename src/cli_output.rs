use std::fmt::Arguments;
use std::io::{IsTerminal, Write, stderr, stdout};
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use anstyle_progress::{TermProgress, supports_term_progress};
use indicatif::{ProgressBar, ProgressStyle};
use torrent_utils::ProgressReporter;

const COLOR_RESET: &str = "\x1b[0m";
const COLOR_SUCCESS: &str = "\x1b[92m";
const COLOR_PROGRESS_GREEN: &str = "\x1b[32m";
const COLOR_PROGRESS_CYAN: &str = "\x1b[36m";
pub(crate) const COLOR_LABEL: &str = "\x1b[94m";
const COLOR_WARNING: &str = "\x1b[93m";
const COLOR_ERROR: &str = "\x1b[91m";

#[derive(Clone, Copy)]
pub(crate) enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutputStyle {
    Plain,
    Success,
    Label,
    Warning,
    Error,
}

impl OutputStyle {
    const fn ansi(self) -> &'static str {
        match self {
            Self::Plain => "",
            Self::Success => COLOR_SUCCESS,
            Self::Label => COLOR_LABEL,
            Self::Warning => COLOR_WARNING,
            Self::Error => COLOR_ERROR,
        }
    }
}

pub(crate) fn print(
    stream: OutputStream,
    style: OutputStyle,
    prefix: Arguments<'_>,
    detail: Arguments<'_>,
    color_detail: bool,
    newline: bool,
) {
    match stream {
        OutputStream::Stdout => {
            let stdout = stdout();
            write_message(
                stdout.lock(),
                stdout.is_terminal(),
                style,
                prefix,
                detail,
                color_detail,
                newline,
            );
        }
        OutputStream::Stderr => {
            let stderr = stderr();
            write_message(
                stderr.lock(),
                stderr.is_terminal(),
                style,
                prefix,
                detail,
                color_detail,
                newline,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn write_message(
    mut output: impl Write,
    is_terminal: bool,
    style: OutputStyle,
    prefix: Arguments<'_>,
    detail: Arguments<'_>,
    color_detail: bool,
    newline: bool,
) {
    let color = style.ansi();
    let colored = is_terminal && !color.is_empty();
    if colored {
        let _ = write!(output, "{color}");
    }
    let _ = output.write_fmt(prefix);
    if colored && !color_detail {
        let _ = write!(output, "{COLOR_RESET}");
    }
    let _ = output.write_fmt(detail);
    if colored && color_detail {
        let _ = write!(output, "{COLOR_RESET}");
    }
    if newline {
        let _ = writeln!(output);
    }
}

macro_rules! outputln {
    (error, $prefix:expr, $($arg:tt)*) => {
        $crate::cli_output::print(
            $crate::cli_output::OutputStream::Stderr,
            $crate::cli_output::OutputStyle::Error,
            format_args!($prefix),
            format_args!($($arg)*),
            true,
            true,
        )
    };
    (plain, $prefix:expr, $($arg:tt)*) => {
        $crate::cli_output::print(
            $crate::cli_output::OutputStream::Stdout,
            $crate::cli_output::OutputStyle::Plain,
            format_args!($prefix),
            format_args!($($arg)*),
            false,
            true,
        )
    };
    (label, $prefix:expr, $($arg:tt)*) => {
        $crate::cli_output::print(
            $crate::cli_output::OutputStream::Stdout,
            $crate::cli_output::OutputStyle::Label,
            format_args!($prefix),
            format_args!($($arg)*),
            false,
            true,
        )
    };
    ($stream:expr, $style:expr, $($arg:tt)*) => {
        $crate::cli_output::print(
            $stream,
            $style,
            format_args!($($arg)*),
            format_args!(""),
            true,
            true,
        )
    };
}

pub(crate) use outputln;

fn verification_line(
    label: &str,
    total: usize,
    passed: usize,
    failed: usize,
    color: bool,
) -> String {
    let status = if failed == 0 {
        format!("{passed:8} passed")
    } else {
        format!("{failed:8} failed")
    };
    let colored_status = if color {
        let status_color = if failed == 0 {
            COLOR_SUCCESS
        } else {
            COLOR_ERROR
        };
        format!("{status_color}{status}{COLOR_RESET}")
    } else {
        status
    };

    let label = if color {
        format!("{COLOR_LABEL}{label}{COLOR_RESET}")
    } else {
        label.to_string()
    };

    if failed == 0 {
        format!("{label} {total:8} total = {colored_status} + {failed:8} failed")
    } else {
        format!("{label} {total:8} total = {passed:8} passed + {colored_status}")
    }
}

pub(crate) fn print_verification_line(label: &str, total: usize, passed: usize, failed: usize) {
    println!(
        "{}",
        verification_line(label, total, passed, failed, stdout().is_terminal())
    );
}

pub(crate) struct StyledSpan {
    pub(crate) style: OutputStyle,
    pub(crate) text: String,
}

impl StyledSpan {
    pub(crate) fn plain(text: impl Into<String>) -> Self {
        Self {
            style: OutputStyle::Plain,
            text: text.into(),
        }
    }

    pub(crate) fn label(text: impl Into<String>) -> Self {
        Self {
            style: OutputStyle::Label,
            text: text.into(),
        }
    }
}

pub(crate) type StyledLine = Vec<StyledSpan>;

fn write_styled_line(output: &mut impl Write, line: &[StyledSpan], color: bool) {
    for span in line {
        let ansi = span.style.ansi();
        if color && !ansi.is_empty() {
            let _ = write!(output, "{ansi}{}{COLOR_RESET}", span.text);
        } else {
            let _ = write!(output, "{}", span.text);
        }
    }
    let _ = writeln!(output);
}

pub(crate) fn print_styled_lines(lines: &[StyledLine]) {
    let stdout = stdout();
    let color = stdout.is_terminal();
    let mut output = stdout.lock();
    for line in lines {
        write_styled_line(&mut output, line, color);
    }
}

const NO_PERCENT: u8 = u8::MAX;

fn progress_percent(position: u64, length: u64) -> Option<u8> {
    (length > 0).then(|| (u128::from(position) * 100 / u128::from(length)).min(100) as u8)
}

fn write_terminal_progress(progress: TermProgress) {
    let mut output = stderr().lock();
    let _ = write!(output, "{progress}");
    let _ = output.flush();
}

pub(crate) struct CliProgress {
    bar: RwLock<Option<ProgressBar>>,
    terminal_enabled: AtomicBool,
    last_percent: AtomicU8,
}

impl CliProgress {
    pub(crate) const fn new() -> Self {
        Self {
            bar: RwLock::new(None),
            terminal_enabled: AtomicBool::new(false),
            last_percent: AtomicU8::new(NO_PERCENT),
        }
    }

    fn take_bar(&self) -> Option<ProgressBar> {
        match self.bar.write() {
            Ok(mut bar) => bar.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        }
    }

    fn clear_terminal(&self) {
        if self.terminal_enabled.swap(false, Ordering::AcqRel) {
            write_terminal_progress(TermProgress::remove());
        }
        self.last_percent.store(NO_PERCENT, Ordering::Release);
    }
}

impl ProgressReporter for CliProgress {
    fn begin(&self, total: usize) {
        let pb = ProgressBar::new(total as u64);
        pb.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{bar:40.cyan/blue}] [{pos}/{len}] pieces ({percent}%, eta: {eta})",
            )
            .expect("the progress bar template is valid")
            .progress_chars("#>-"),
        );
        *self.bar.write().expect("progress lock poisoned") = Some(pb);

        if self.terminal_enabled.swap(false, Ordering::AcqRel) {
            write_terminal_progress(TermProgress::remove());
        }

        let enabled = total > 0 && supports_term_progress(stderr().is_terminal());
        if enabled {
            self.last_percent.store(0, Ordering::Release);
            self.terminal_enabled.store(true, Ordering::Release);
            write_terminal_progress(TermProgress::start().percent(0));
        } else {
            self.last_percent.store(NO_PERCENT, Ordering::Release);
        }
    }

    fn advance(&self, delta: usize) {
        let percent = {
            let bar = self.bar.read().expect("progress lock poisoned");
            let Some(pb) = bar.as_ref() else {
                return;
            };
            pb.inc(delta as u64);
            progress_percent(pb.position(), pb.length().unwrap_or_default())
        };
        let Some(percent) = percent else {
            return;
        };

        if !self.terminal_enabled.load(Ordering::Acquire) {
            return;
        }

        let mut prev = self.last_percent.load(Ordering::Acquire);
        while prev == NO_PERCENT || percent > prev {
            match self.last_percent.compare_exchange_weak(
                prev,
                percent,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    write_terminal_progress(TermProgress::start().percent(percent));
                    break;
                }
                Err(actual) => prev = actual,
            }
        }
    }

    fn finish(&self) {
        if let Some(pb) = self.take_bar() {
            let elapsed = pb.elapsed();
            let pieces_count = pb.length().unwrap_or_default();
            pb.finish_and_clear();
            if stderr().is_terminal() {
                eprintln!(
                    "{COLOR_PROGRESS_GREEN}✓{COLOR_RESET} [{COLOR_PROGRESS_CYAN}########################################{COLOR_RESET}] [{pieces_count}/{pieces_count}] pieces (100%, eta: 0s)"
                );
            }
            eprintln!("Processed {pieces_count} pieces in {elapsed:.2?}");
        }

        self.clear_terminal();
    }

    fn abandon(&self) {
        if let Some(pb) = self.take_bar() {
            pb.finish_and_clear();
        }
        self.clear_terminal();
    }
}

impl Drop for CliProgress {
    fn drop(&mut self) {
        if *self.terminal_enabled.get_mut() {
            write_terminal_progress(TermProgress::remove());
            *self.terminal_enabled.get_mut() = false;
        }

        let bar = match self.bar.get_mut() {
            Ok(bar) => bar,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(pb) = bar.take() {
            pb.finish_and_clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_colors_only_the_relevant_result() {
        let passed = verification_line("Pieces:", 2, 2, 0, true);
        assert!(passed.contains("\x1b[92m       2 passed\x1b[0m"));
        assert!(!passed.contains("\x1b[91m"));

        let failed = verification_line("Pieces:", 2, 1, 1, true);
        assert!(failed.contains("\x1b[91m       1 failed\x1b[0m"));
        assert!(!failed.contains("\x1b[92m"));
    }

    #[test]
    fn progress_percent_is_bounded() {
        assert_eq!(progress_percent(0, 10), Some(0));
        assert_eq!(progress_percent(5, 10), Some(50));
        assert_eq!(progress_percent(10, 10), Some(100));
        assert_eq!(progress_percent(11, 10), Some(100));
        assert_eq!(progress_percent(0, 0), None);
    }

    #[test]
    fn styled_lines_color_only_labeled_spans() {
        let line = vec![StyledSpan::label("  Name:"), StyledSpan::plain(" example")];

        let mut colored = Vec::new();
        write_styled_line(&mut colored, &line, true);
        assert_eq!(
            String::from_utf8(colored).expect("styled output is UTF-8"),
            "\x1b[94m  Name:\x1b[0m example\n"
        );

        let mut plain = Vec::new();
        write_styled_line(&mut plain, &line, false);
        assert_eq!(
            String::from_utf8(plain).expect("styled output is UTF-8"),
            "  Name: example\n"
        );
    }
}
