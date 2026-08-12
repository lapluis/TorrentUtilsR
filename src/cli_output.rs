use std::fmt::Arguments;
use std::io::{IsTerminal, Write, stderr, stdout};
use std::sync::Mutex;

use indicatif::{ProgressBar, ProgressStyle};
use torrent_utils::ProgressReporter;

pub(crate) fn write_error(prefix: &str, detail: Arguments<'_>, newline: bool) {
    let stderr = stderr();
    let mut stderr = stderr.lock();

    if stderr.is_terminal() {
        let _ = write!(stderr, "\x1b[31m{prefix}\x1b[0m");
    } else {
        let _ = write!(stderr, "{prefix}");
    }
    let _ = stderr.write_fmt(detail);
    if newline {
        let _ = writeln!(stderr);
    }
}

pub(crate) fn write_output(prefix: &str, detail: Arguments<'_>, color: &str) {
    let stdout = stdout();
    let mut stdout = stdout.lock();

    if stdout.is_terminal() {
        let _ = write!(stdout, "{color}{prefix}\x1b[0m");
    } else {
        let _ = write!(stdout, "{prefix}");
    }
    let _ = stdout.write_fmt(detail);
    let _ = writeln!(stdout);
}

macro_rules! errprint {
    ($prefix:expr, $($arg:tt)*) => {
        $crate::cli_output::write_error($prefix, format_args!($($arg)*), false)
    };
}

macro_rules! errprintln {
    ($prefix:expr, $($arg:tt)*) => {
        $crate::cli_output::write_error($prefix, format_args!($($arg)*), true)
    };
}

macro_rules! greenprintln {
    ($prefix:expr, $($arg:tt)*) => {
        $crate::cli_output::write_output($prefix, format_args!($($arg)*), "\x1b[32m")
    };
}

macro_rules! blueprintln {
    ($prefix:expr, $($arg:tt)*) => {
        $crate::cli_output::write_output($prefix, format_args!($($arg)*), "\x1b[34m")
    };
}

pub(crate) use {blueprintln, errprint, errprintln, greenprintln};

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
                "{spinner:.green} [{bar:40.cyan/blue}] [{pos}/{len}] pieces ({percent}%, eta: {eta})",
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
            println!(
                "\x1b[32m✓\x1b[0m [\x1b[36m########################################\x1b[0m] [{pieces_count}/{pieces_count}] pieces (100%, eta: 0s)"
            );
            println!("Processed {pieces_count} pieces in {elapsed:.2?}");
        }
    }
}
