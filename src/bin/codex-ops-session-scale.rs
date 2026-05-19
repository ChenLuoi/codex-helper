use serde_json::{json, Value};
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

const DEFAULT_SOURCE: &str = ".local-bench-data/real-sessions-backup";
const DEFAULT_OUTPUT: &str = ".local-bench-data/sessions-100x";

#[derive(Debug, Clone)]
struct Options {
    backup_from: Option<PathBuf>,
    source: PathBuf,
    output: PathBuf,
    copies: usize,
    clean: bool,
    clean_backup: bool,
    padding: PaddingMode,
    progress_every: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum PaddingMode {
    Preserve,
    Minimal,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum RelevantKind {
    SessionMeta,
    TurnContext,
    TokenCount,
}

#[derive(Debug, Default)]
struct ScaleReport {
    source_files: usize,
    source_lines: u64,
    source_token_events: u64,
    written_files: u64,
    written_lines: u64,
    written_token_events: u64,
    written_bytes: u64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let options = parse_args(env::args().skip(1).collect())?;

    if let Some(backup_from) = options.backup_from.as_ref() {
        ensure_dir(backup_from, "backup source")?;
        if options.source.exists() {
            if !options.clean_backup {
                return Err(format!(
                    "Backup destination already exists: {}. Pass --clean-backup to replace it.",
                    display_path(&options.source)
                ));
            }
            fs::remove_dir_all(&options.source).map_err(|error| error.to_string())?;
        }
        if let Some(parent) = options.source.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        println!(
            "Copying real sessions backup: {} -> {}",
            display_path(backup_from),
            display_path(&options.source)
        );
        copy_dir_recursive(backup_from, &options.source)?;
    }

    ensure_dir(&options.source, "source sessions directory")?;
    if options.output.exists() {
        if !options.clean {
            return Err(format!(
                "Output already exists: {}. Pass --clean to replace it.",
                display_path(&options.output)
            ));
        }
        fs::remove_dir_all(&options.output).map_err(|error| error.to_string())?;
    }
    fs::create_dir_all(&options.output).map_err(|error| error.to_string())?;

    let files = list_jsonl_files(&options.source)?;
    if files.is_empty() {
        return Err(format!(
            "No .jsonl files found in {}.",
            display_path(&options.source)
        ));
    }

    println!(
        "Generating {}x simulated sessions from {} source file(s).",
        options.copies,
        files.len()
    );
    println!("Source: {}", display_path(&options.source));
    println!("Output: {}", display_path(&options.output));
    println!("Padding: {}", options.padding.as_str());

    let started = Instant::now();
    let mut report = ScaleReport {
        source_files: files.len(),
        ..ScaleReport::default()
    };

    for (index, file) in files.iter().enumerate() {
        generate_scaled_file(file, &options, &mut report)?;

        let completed = index + 1;
        if completed % options.progress_every == 0 || completed == files.len() {
            println!(
                "Progress {completed}/{} source files writtenFiles={} writtenLines={} tokenEvents={} elapsed={:.1}s",
                files.len(),
                report.written_files,
                report.written_lines,
                report.written_token_events,
                started.elapsed().as_secs_f64()
            );
        }
    }

    write_scale_report(&options, &report)?;
    println!("Done.");
    println!(
        "Scale report: {}",
        display_path(&options.output.join("scale-report.json"))
    );
    Ok(())
}

fn parse_args(args: Vec<String>) -> Result<Options, String> {
    let mut options = Options {
        backup_from: None,
        source: resolve_input_path(DEFAULT_SOURCE),
        output: resolve_input_path(DEFAULT_OUTPUT),
        copies: 100,
        clean: false,
        clean_backup: false,
        padding: PaddingMode::Preserve,
        progress_every: 10,
    };

    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];

        match arg.as_str() {
            "--backup-from" => {
                index += 1;
                options.backup_from =
                    Some(resolve_input_path(read_arg(&args, index, "--backup-from")?));
            }
            "--source" => {
                index += 1;
                options.source = resolve_input_path(read_arg(&args, index, "--source")?);
            }
            "--output" => {
                index += 1;
                options.output = resolve_input_path(read_arg(&args, index, "--output")?);
            }
            "--copies" => {
                index += 1;
                options.copies =
                    parse_positive_usize(read_arg(&args, index, "--copies")?, "--copies")?;
            }
            "--padding" => {
                index += 1;
                options.padding = PaddingMode::parse(read_arg(&args, index, "--padding")?)?;
            }
            "--progress-every" => {
                index += 1;
                options.progress_every = parse_positive_usize(
                    read_arg(&args, index, "--progress-every")?,
                    "--progress-every",
                )?;
            }
            "--clean" => options.clean = true,
            "--clean-backup" => options.clean_backup = true,
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            _ if arg.starts_with("--backup-from=") => {
                options.backup_from = Some(resolve_input_path(&arg["--backup-from=".len()..]));
            }
            _ if arg.starts_with("--source=") => {
                options.source = resolve_input_path(&arg["--source=".len()..]);
            }
            _ if arg.starts_with("--output=") => {
                options.output = resolve_input_path(&arg["--output=".len()..]);
            }
            _ if arg.starts_with("--copies=") => {
                options.copies = parse_positive_usize(&arg["--copies=".len()..], "--copies")?;
            }
            _ if arg.starts_with("--padding=") => {
                options.padding = PaddingMode::parse(&arg["--padding=".len()..])?;
            }
            _ if arg.starts_with("--progress-every=") => {
                options.progress_every =
                    parse_positive_usize(&arg["--progress-every=".len()..], "--progress-every")?;
            }
            _ => return Err(format!("Unsupported argument: {arg}")),
        }

        index += 1;
    }

    Ok(options)
}

fn read_arg<'a>(args: &'a [String], index: usize, name: &str) -> Result<&'a str, String> {
    let value = args
        .get(index)
        .ok_or_else(|| format!("Missing value for {name}"))?;
    if value.starts_with("--") {
        return Err(format!("Missing value for {name}"));
    }
    Ok(value)
}

fn parse_positive_usize(value: &str, name: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("Invalid {name}. Expected a positive integer."))?;
    if parsed == 0 {
        return Err(format!("Invalid {name}. Expected a positive integer."));
    }
    Ok(parsed)
}

fn print_help() {
    println!(
        "Usage: codex-ops-session-scale [options]\n\n\
         Copies a local sessions backup when requested, then generates scaled simulated\n\
         sessions from that backup. Stats-relevant fields are preserved; unrelated raw\n\
         lines are replaced with simulated padding.\n\n\
         Options:\n\
           --backup-from <path>      Copy real sessions into --source before generation\n\
           --source <path>           Source backup, default {DEFAULT_SOURCE}\n\
           --output <path>           Output sessions dir, default {DEFAULT_OUTPUT}\n\
           --copies <n>              Copies per source file, default 100\n\
           --padding <mode>          preserve or minimal, default preserve\n\
           --progress-every <n>      Progress interval in source files, default 10\n\
           --clean                   Replace output if it already exists\n\
           --clean-backup            Replace backup destination if it already exists\n\
           -h, --help                Print help"
    );
}

impl PaddingMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "preserve" => Ok(Self::Preserve),
            "minimal" => Ok(Self::Minimal),
            _ => Err("Invalid --padding. Expected preserve or minimal.".to_string()),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Preserve => "preserve",
            Self::Minimal => "minimal",
        }
    }
}

fn resolve_input_path(value: &str) -> PathBuf {
    if value == "~" {
        return home_dir();
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn ensure_dir(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("{} not found at {}: {}", label, display_path(path), error))?;
    if !metadata.is_dir() {
        return Err(format!(
            "{} is not a directory: {}",
            label,
            display_path(path)
        ));
    }
    Ok(())
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|error| error.to_string())?;
    let mut entries = fs::read_dir(source)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_dir() {
            copy_dir_recursive(&source_path, &target_path)?;
        } else if file_type.is_file() {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::copy(&source_path, &target_path).map_err(|error| error.to_string())?;
        }
    }

    Ok(())
}

fn list_jsonl_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    collect_jsonl_files(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_jsonl_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let mut entries = fs::read_dir(root)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_dir() {
            collect_jsonl_files(&path, files)?;
        } else if file_type.is_file()
            && path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|extension| extension == "jsonl")
        {
            files.push(path);
        }
    }

    Ok(())
}

fn generate_scaled_file(
    source_file: &Path,
    options: &Options,
    report: &mut ScaleReport,
) -> Result<(), String> {
    let relative_path = source_file
        .strip_prefix(&options.source)
        .map_err(|error| error.to_string())?;
    let output_paths = (1..=options.copies)
        .map(|copy| output_file_path(&options.output, relative_path, copy))
        .collect::<Vec<_>>();

    for output_path in &output_paths {
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
    }

    let mut writers = output_paths
        .iter()
        .map(|path| {
            File::create(path)
                .map(BufWriter::new)
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;

    let file = File::open(source_file).map_err(|error| error.to_string())?;
    let reader = BufReader::new(file);

    for (line_index, line_result) in reader.lines().enumerate() {
        let line = line_result.map_err(|error| error.to_string())?;
        let descriptor = describe_line(&line);
        report.source_lines += 1;
        if descriptor.kind == Some(RelevantKind::TokenCount) {
            report.source_token_events += 1;
        }

        for (copy_index, writer) in writers.iter_mut().enumerate() {
            let copy = copy_index + 1;
            let written =
                write_simulated_line(writer, &descriptor, copy, line_index, options.padding)?;
            report.written_lines += 1;
            report.written_bytes += written as u64;
            if descriptor.kind == Some(RelevantKind::TokenCount) {
                report.written_token_events += 1;
            }
        }
    }

    for writer in &mut writers {
        writer.flush().map_err(|error| error.to_string())?;
    }

    report.written_files += options.copies as u64;
    Ok(())
}

fn output_file_path(output_root: &Path, relative_path: &Path, copy: usize) -> PathBuf {
    let directory = relative_path.parent().unwrap_or_else(|| Path::new(""));
    let stem = relative_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("rollout-simulated");
    output_root
        .join(directory)
        .join(format!("{stem}-sim{copy:03}.jsonl"))
}

#[derive(Debug)]
struct LineDescriptor<'a> {
    kind: Option<RelevantKind>,
    value: Option<Value>,
    byte_len: usize,
    raw: &'a str,
}

fn describe_line(line: &str) -> LineDescriptor<'_> {
    let mut descriptor = LineDescriptor {
        kind: None,
        value: None,
        byte_len: line.len(),
        raw: line,
    };

    if !line.contains("\"token_count\"")
        && !line.contains("\"session_meta\"")
        && !line.contains("\"turn_context\"")
    {
        return descriptor;
    }

    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return descriptor;
    };
    let event_type = value.get("type").and_then(Value::as_str);
    let payload_type = value
        .get("payload")
        .and_then(|payload| payload.get("type"))
        .and_then(Value::as_str);

    descriptor.kind = match (event_type, payload_type) {
        (Some("session_meta"), _) => Some(RelevantKind::SessionMeta),
        (Some("turn_context"), _) => Some(RelevantKind::TurnContext),
        (Some("event_msg"), Some("token_count")) => Some(RelevantKind::TokenCount),
        _ => None,
    };
    descriptor.value = descriptor.kind.map(|_| value);
    descriptor
}

fn write_simulated_line(
    writer: &mut BufWriter<File>,
    descriptor: &LineDescriptor<'_>,
    copy: usize,
    line_index: usize,
    padding: PaddingMode,
) -> Result<usize, String> {
    match (descriptor.kind, descriptor.value.as_ref()) {
        (Some(kind), Some(value)) => write_relevant_line(writer, value, kind, copy),
        _ => write_noise_line(
            writer,
            descriptor.raw,
            descriptor.byte_len,
            copy,
            line_index,
            padding,
        ),
    }
}

fn write_relevant_line(
    writer: &mut BufWriter<File>,
    value: &Value,
    kind: RelevantKind,
    copy: usize,
) -> Result<usize, String> {
    let mut event = value.clone();
    let suffix = format!("sim{copy:03}");

    if kind == RelevantKind::SessionMeta {
        if let Some(payload) = event.get_mut("payload").and_then(Value::as_object_mut) {
            if let Some(Value::String(id)) = payload.get_mut("id") {
                id.push('-');
                id.push_str(&suffix);
            }
        }
    }

    if kind == RelevantKind::TokenCount {
        if let Some(payload) = event.get_mut("payload").and_then(Value::as_object_mut) {
            payload.insert("synthetic_copy".to_string(), json!(copy));
        }
    }

    let line = serde_json::to_string(&event).map_err(|error| error.to_string())?;
    writer
        .write_all(line.as_bytes())
        .and_then(|_| writer.write_all(b"\n"))
        .map_err(|error| error.to_string())?;
    Ok(line.len() + 1)
}

fn write_noise_line(
    writer: &mut BufWriter<File>,
    raw: &str,
    byte_len: usize,
    copy: usize,
    line_index: usize,
    padding: PaddingMode,
) -> Result<usize, String> {
    if padding == PaddingMode::Minimal {
        let line = format!(r#"{{"type":"simulated_noise","copy":{copy},"line":{line_index}}}"#);
        writer
            .write_all(line.as_bytes())
            .and_then(|_| writer.write_all(b"\n"))
            .map_err(|error| error.to_string())?;
        return Ok(line.len() + 1);
    }

    let prefix =
        format!(r#"{{"type":"simulated_noise","copy":{copy},"line":{line_index},"padding":""#);
    let suffix = r#""}"#;
    let target = byte_len.max(prefix.len() + suffix.len());
    let padding_len = target - prefix.len() - suffix.len();

    writer
        .write_all(prefix.as_bytes())
        .map_err(|error| error.to_string())?;
    write_padding(writer, raw, padding_len)?;
    writer
        .write_all(suffix.as_bytes())
        .and_then(|_| writer.write_all(b"\n"))
        .map_err(|error| error.to_string())?;
    Ok(prefix.len() + padding_len + suffix.len() + 1)
}

fn write_padding(writer: &mut BufWriter<File>, raw: &str, len: usize) -> Result<(), String> {
    let bytes = raw.as_bytes();
    if bytes.is_empty() {
        write_repeated_byte(writer, b'x', len)?;
        return Ok(());
    }

    let safe = bytes
        .iter()
        .copied()
        .filter(|byte| byte.is_ascii_graphic() && *byte != b'"' && *byte != b'\\')
        .collect::<Vec<_>>();
    if safe.is_empty() {
        write_repeated_byte(writer, b'x', len)?;
        return Ok(());
    }

    let mut remaining = len;
    while remaining > 0 {
        let chunk = remaining.min(safe.len());
        writer
            .write_all(&safe[..chunk])
            .map_err(|error| error.to_string())?;
        remaining -= chunk;
    }
    Ok(())
}

fn write_repeated_byte(writer: &mut BufWriter<File>, byte: u8, len: usize) -> Result<(), String> {
    const CHUNK: usize = 8192;
    let buffer = vec![byte; CHUNK.min(len.max(1))];
    let mut remaining = len;
    while remaining > 0 {
        let chunk = remaining.min(buffer.len());
        writer
            .write_all(&buffer[..chunk])
            .map_err(|error| error.to_string())?;
        remaining -= chunk;
    }
    Ok(())
}

fn write_scale_report(options: &Options, report: &ScaleReport) -> Result<(), String> {
    let output_path = options.output.join("scale-report.json");
    let source_line_scale = if report.source_lines == 0 {
        0.0
    } else {
        report.written_lines as f64 / report.source_lines as f64
    };
    let token_event_scale = if report.source_token_events == 0 {
        0.0
    } else {
        report.written_token_events as f64 / report.source_token_events as f64
    };

    let value = json!({
        "source": display_path(&options.source),
        "output": display_path(&options.output),
        "copies": options.copies,
        "padding": options.padding.as_str(),
        "sourceFiles": report.source_files,
        "sourceLines": report.source_lines,
        "sourceTokenEvents": report.source_token_events,
        "writtenFiles": report.written_files,
        "writtenLines": report.written_lines,
        "writtenTokenEvents": report.written_token_events,
        "writtenBytes": report.written_bytes,
        "approximateScale": {
            "files": report.written_files as f64 / report.source_files as f64,
            "lines": source_line_scale,
            "tokenCountEvents": token_event_scale
        },
        "privacy": "Stats-relevant session_meta, turn_context, and token_count fields are preserved. Other raw lines are replaced with simulated padding."
    });
    let mut writer = BufWriter::new(File::create(output_path).map_err(|error| error.to_string())?);
    serde_json::to_writer_pretty(&mut writer, &value).map_err(|error| error.to_string())?;
    writer.write_all(b"\n").map_err(|error| error.to_string())?;
    writer.flush().map_err(|error| error.to_string())
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().to_string()
}
