use std::io::{Write, stdin, stdout};
use std::path::Path;

use chrono::Local;
use torrent_utils::{
    CreateOptions, ProgressReporter, Torrent, VerificationOptions, VerificationReport, WalkMode,
};

use crate::cli_output::{
    CliProgress, OutputStream, OutputStyle, outputln, print_styled_lines, print_verification_line,
};
use crate::render::{file_tree, human_size, torrent_info};
use crate::{Args, Config, NAME_VERSION};

pub(crate) struct CliError {
    style: OutputStyle,
    message: String,
}

impl CliError {
    fn error(message: impl Into<String>) -> Self {
        Self {
            style: OutputStyle::Error,
            message: message.into(),
        }
    }

    fn warning(message: impl Into<String>) -> Self {
        Self {
            style: OutputStyle::Warning,
            message: message.into(),
        }
    }

    pub(crate) fn report(&self) {
        outputln!(OutputStream::Stderr, self.style, "{}", self.message);
    }
}

fn with_progress<T>(quiet: bool, operation: impl FnOnce(Option<&dyn ProgressReporter>) -> T) -> T {
    let progress = (!quiet).then(CliProgress::new);
    let reporter = progress
        .as_ref()
        .map(|progress| progress as &dyn ProgressReporter);
    let result = operation(reporter);
    drop(progress);
    result
}

pub(crate) fn run(args: &Args, config: &Config) -> Result<(), CliError> {
    match args.input.len() {
        1 if args.input[0].ends_with(".torrent") => run_info(&args.input[0], args),
        1 => run_create(&args.input[0], args, config),
        2 => run_verify(&args.input, args, config),
        _ => Err(CliError::error(
            "Error: Please provide one target (create), one .torrent (info), or a .torrent plus target (verify).",
        )),
    }
}

fn run_info(input: &str, args: &Args) -> Result<(), CliError> {
    if !args.quiet {
        outputln!(plain, "I:", " Info mode.");
        outputln!(label, "Torrent:", " {input}");
    }

    let torrent = Torrent::read_torrent(input)
        .map_err(|e| CliError::error(format!("Error reading torrent file: {e}")))?;

    if args.print_tree {
        print!("{}", file_tree(&torrent));
    } else {
        print_styled_lines(&torrent_info(&torrent));
    }

    Ok(())
}

fn run_create(input: &str, args: &Args, config: &Config) -> Result<(), CliError> {
    if !args.quiet {
        outputln!(plain, "I:", " Create mode.");
    }

    let piece_size = args.piece_size.unwrap_or(config.piece_size);
    let piece_length = 1usize
        << match piece_size {
            14..=27 => piece_size,
            _ => {
                return Err(CliError::error(
                    "Error: Piece size must be between 14 and 27.",
                ));
            }
        };

    let walk_mode = match args.walk_mode.unwrap_or(config.walk_mode) {
        0 => WalkMode::Default,
        1 => WalkMode::Alphabetical,
        2 => WalkMode::BreadthFirstAlphabetical,
        3 => WalkMode::BreadthFirstLevel,
        4 => WalkMode::FileSize,
        _ => return Err(CliError::error("Error: Invalid walk mode.")),
    };

    let tr_config = CreateOptions {
        piece_length,
        private: args.private || config.private,
        n_jobs: config.n_jobs,
        walk_mode,
        source: args
            .source
            .clone()
            .or_else(|| config.source.clone())
            .filter(|s| !s.is_empty()),
    };

    let tracker_list = if args.announce.is_empty() {
        config.tracker_list.clone()
    } else if args.announce.iter().any(|s| s.is_empty()) {
        Vec::new()
    } else {
        args.announce.clone()
    };

    let torrent_path = match args.output.as_deref() {
        Some(path) => {
            if !path.ends_with(".torrent") {
                return Err(CliError::error("Error: Output path must end with .torrent"));
            }
            let path_obj = Path::new(path);
            let has_directory = path_obj
                .parent()
                .is_some_and(|parent| !parent.as_os_str().is_empty());
            if path_obj.is_absolute() || has_directory {
                path.to_string()
            } else {
                let target_path = Path::new(input);
                let parent_path = target_path.parent().unwrap_or_else(|| Path::new("."));
                parent_path.join(path).to_string_lossy().to_string()
            }
        }
        None => format!("{input}.torrent"),
    };

    let mut force_overwrite = args.force;
    if !force_overwrite && Path::new(&torrent_path).exists() {
        if config.confirm_overwrite && confirm_overwrite(&torrent_path) {
            force_overwrite = true;
        } else if config.confirm_overwrite {
            return Err(CliError::warning(
                "⚠ Creation cancelled; existing torrent file was not changed.",
            ));
        } else {
            return Err(CliError::error(
                "Error writing torrent file: File already exists, use -f to overwrite",
            ));
        }
    }

    if !args.quiet {
        outputln!(label, "Target:", "  {input}");
        outputln!(label, "Torrent:", " {torrent_path}");
        outputln!(
            label,
            "Piece Length:",
            " {} bytes [{}]",
            tr_config.piece_length,
            human_size(tr_config.piece_length)
        );
        if tr_config.private {
            println!("Private Torrent");
        }
    }

    let announce_list: Vec<Vec<String>> =
        tracker_list.iter().map(|url| vec![url.clone()]).collect();

    let mut torrent = Torrent::new(
        announce_list.first().and_then(|tier| tier.first()).cloned(),
        if announce_list.is_empty() {
            None
        } else {
            Some(announce_list)
        },
        args.comment.clone(),
        Some(NAME_VERSION.to_string()),
        if args.no_date {
            None
        } else {
            Some(Local::now().timestamp())
        },
        Some(String::from("UTF-8")),
    );

    with_progress(args.quiet, |reporter| {
        torrent.create_torrent(input, &tr_config, reporter)
    })
    .map_err(|e| CliError::error(format!("Error creating torrent: {e}")))?;

    torrent
        .write_to_file(torrent_path, force_overwrite)
        .map_err(|e| CliError::error(format!("Error writing torrent file: {e}")))?;

    Ok(())
}

fn run_verify(inputs: &[String], args: &Args, config: &Config) -> Result<(), CliError> {
    let (torrent_path, target_path) = if inputs[0].ends_with(".torrent") {
        (inputs[0].as_str(), inputs[1].as_str())
    } else if inputs[1].ends_with(".torrent") {
        (inputs[1].as_str(), inputs[0].as_str())
    } else {
        return Err(CliError::error(
            "Error: Please provide a .torrent file as one of the arguments.",
        ));
    };

    if !args.quiet {
        outputln!(plain, "I:", " Verify mode.");
        outputln!(label, "Target:", "  {target_path}");
        outputln!(label, "Torrent:", " {torrent_path}");
    }

    let torrent = Torrent::read_torrent(torrent_path)
        .map_err(|e| CliError::error(format!("Error reading torrent file: {e}")))?;
    let tr_info = torrent.get_info().ok_or_else(|| {
        CliError::error("Error: Torrent file does not contain valid info section")
    })?;

    let base_path = Path::new(target_path);
    let name = base_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let tr_name = tr_info.get_name().unwrap_or(String::from("<unknown>"));
    if name != tr_name {
        return Err(CliError::error(format!(
            "Error: Target name '{name}' does not match torrent name '{tr_name}'"
        )));
    }

    let full_path = base_path.parent().unwrap_or_else(|| Path::new(""));
    if !full_path.join(&tr_name).exists() {
        return Err(CliError::error(format!(
            "Error: Target path '{}' does not exist",
            full_path.join(&tr_name).display()
        )));
    }

    let verify_options = VerificationOptions {
        n_jobs: config.n_jobs,
    };
    let report = with_progress(args.quiet, |reporter| {
        tr_info.verify(target_path, &verify_options, reporter)
    })
    .map_err(|e| CliError::error(format!("Error during verification: {e}")))?;

    print_verification_report(&report);
    Ok(())
}

fn confirm_overwrite(torrent_path: &str) -> bool {
    print!("Torrent file '{torrent_path}' already exists. Overwrite? [y/N]: ");
    let _ = stdout().flush();

    let mut answer = String::new();
    if stdin().read_line(&mut answer).is_err() {
        return false;
    }

    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn print_verification_report(report: &VerificationReport) {
    let failed_piece_count = report.failed_pieces.len();
    let failed_file_count = report.failed_files.len();

    outputln!(label, "Verification Result:", "");
    print_verification_line(
        "Pieces:",
        report.total_pieces,
        report.passed_pieces(),
        failed_piece_count,
    );
    print_verification_line(
        "Files: ",
        report.total_files,
        report.passed_files(),
        failed_file_count,
    );

    println!();

    if report.is_ok() {
        outputln!(
            OutputStream::Stdout,
            OutputStyle::Success,
            "✓ All files are OK."
        );
    } else {
        outputln!(
            OutputStream::Stdout,
            OutputStyle::Warning,
            "⚠ Some files failed verification:"
        );
        for file in &report.failed_files {
            let known_issue = if file.missing_or_size_mismatch {
                " [missing or size mismatch]"
            } else {
                ""
            };
            println!(
                "- {} ({} [{}]){}",
                file.path,
                file.length,
                human_size(file.length),
                known_issue
            );
        }
    }
}
