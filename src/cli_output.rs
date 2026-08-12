use std::fmt::Arguments;
use std::io::{IsTerminal, Write, stderr, stdout};
use std::sync::Mutex;

use indicatif::{ProgressBar, ProgressStyle};
use torrent_utils::ProgressReporter;

const COLOR_RESET: &str = "\x1b[0m";
const COLOR_SUCCESS: &str = "\x1b[92m";
pub(crate) const COLOR_LABEL: &str = "\x1b[94m";
const COLOR_WARNING: &str = "\x1b[93m";
const COLOR_ERROR: &str = "\x1b[91m";

#[derive(Clone, Copy)]
pub(crate) enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Clone, Copy)]
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

fn colorize_torrent_info(text: &str, color: bool) -> String {
    if !color {
        return text.to_string();
    }

    text.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with('-') || trimmed.starts_with('[') {
                return line.to_string();
            }
            match line.find(':') {
                Some(index) => format!(
                    "{COLOR_LABEL}{}{COLOR_RESET}{}",
                    &line[..=index],
                    &line[index + 1..]
                ),
                None => line.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn print_torrent_info(text: &str) {
    println!("{}", colorize_torrent_info(text, stdout().is_terminal()));
}

pub(crate) struct CliProgress {
    bar: Mutex<Option<ProgressBar>>,
}

impl CliProgress {
    pub(crate) const fn new() -> Self {
        Self {
            bar: Mutex::new(None),
        }
    }
}

impl ProgressReporter for CliProgress {
    fn begin(&self, total: usize) {
        let pb = ProgressBar::new(total as u64);
        pb.set_style(
            ProgressStyle::with_template(
                "{spinner} [{bar:40}] [{pos}/{len}] pieces ({percent}%, eta: {eta})",
            )
            .expect("the progress bar template is valid")
            .progress_chars("#>-"),
        );
        *self.bar.lock().expect("progress lock poisoned") = Some(pb);
    }

    fn advance(&self, delta: usize) {
        if let Some(pb) = self.bar.lock().expect("progress lock poisoned").as_ref() {
            pb.inc(delta as u64);
        }
    }

    fn finish(&self) {
        if let Some(pb) = self.bar.lock().expect("progress lock poisoned").take() {
            let elapsed = pb.elapsed();
            let pieces_count = pb.length().unwrap_or_default();
            pb.finish_and_clear();
            if stdout().is_terminal() {
                println!(
                    "{COLOR_SUCCESS}✓{COLOR_RESET} [{COLOR_SUCCESS}########################################{COLOR_RESET}] [{pieces_count}/{pieces_count}] pieces (100%, eta: 0s)"
                );
            } else {
                println!(
                    "✓ [########################################] [{pieces_count}/{pieces_count}] pieces (100%, eta: 0s)"
                );
            }
            println!("Processed {pieces_count} pieces in {elapsed:.2?}");
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
    fn torrent_info_colorization_leaves_values_uncolored() {
        let output = colorize_torrent_info("Torrent Info:\n  Name: example", true);
        assert_eq!(
            output,
            "\x1b[94mTorrent Info:\x1b[0m\n\x1b[94m  Name:\x1b[0m example"
        );
    }
}
