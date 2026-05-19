use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const FIXED_NOW: &str = "2026-05-17T00:00:00.000Z";

#[derive(Clone, Copy)]
struct BenchCase {
    id: &'static str,
    groups: &'static [&'static str],
    args: &'static [&'static str],
}

const BENCHMARK_CASES: &[BenchCase] = &[
    BenchCase {
        id: "auth-status-json",
        groups: &["auth"],
        args: &["auth", "status", "--auth-file", "{authFile}", "--json"],
    },
    BenchCase {
        id: "doctor-json",
        groups: &["doctor"],
        args: &[
            "doctor",
            "--auth-file",
            "{authFile}",
            "--codex-home",
            "{codexHome}",
            "--sessions-dir",
            "{sessionsDir}",
            "--cycle-file",
            "{cycleFile}",
            "--json",
        ],
    },
    BenchCase {
        id: "stat-json-poc",
        groups: &["stat", "poc"],
        args: &[
            "stat",
            "--all",
            "--format",
            "json",
            "--sessions-dir",
            "{sessionsDir}",
        ],
    },
    BenchCase {
        id: "stat-table",
        groups: &["stat"],
        args: &[
            "stat",
            "--all",
            "--format",
            "table",
            "--sessions-dir",
            "{sessionsDir}",
        ],
    },
    BenchCase {
        id: "stat-model-json",
        groups: &["stat"],
        args: &[
            "stat",
            "--all",
            "--group-by",
            "model",
            "--reasoning-effort",
            "--sort",
            "tokens",
            "--limit",
            "10",
            "--format",
            "json",
            "--sessions-dir",
            "{sessionsDir}",
        ],
    },
    BenchCase {
        id: "stat-account-json",
        groups: &["stat"],
        args: &[
            "stat",
            "--all",
            "--group-by",
            "account",
            "--format",
            "json",
            "--auth-file",
            "{authFile}",
            "--account-history-file",
            "{accountHistoryFile}",
            "--sessions-dir",
            "{sessionsDir}",
        ],
    },
    BenchCase {
        id: "stat-sessions-json",
        groups: &["stat"],
        args: &[
            "stat",
            "sessions",
            "--all",
            "--top",
            "10",
            "--format",
            "json",
            "--sessions-dir",
            "{sessionsDir}",
        ],
    },
    BenchCase {
        id: "cycle-current-json",
        groups: &["cycle"],
        args: &[
            "cycle",
            "current",
            "--cycle-file",
            "{cycleFile}",
            "--account-id",
            "account-fixture",
            "--sessions-dir",
            "{sessionsDir}",
            "--format",
            "json",
        ],
    },
    BenchCase {
        id: "cycle-history-json",
        groups: &["cycle"],
        args: &[
            "cycle",
            "history",
            "--cycle-file",
            "{cycleFile}",
            "--account-id",
            "account-fixture",
            "--sessions-dir",
            "{sessionsDir}",
            "--all",
            "--format",
            "json",
        ],
    },
];

#[derive(Debug)]
struct Options {
    fixture: PathBuf,
    runs: usize,
    matrix: String,
    rust_binary: PathBuf,
}

struct Sandbox {
    root: PathBuf,
    home: PathBuf,
    codex_home: PathBuf,
    auth_file: PathBuf,
    sessions_dir: PathBuf,
    account_history_file: PathBuf,
    cycle_file: PathBuf,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct TimedOutput {
    output: Output,
    elapsed: Duration,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let options = parse_args(env::args().skip(1).collect())?;
    ensure_file(&options.rust_binary, "Rust binary")?;
    ensure_dir(&options.fixture, "fixture")?;

    let cases = select_cases(&options.matrix)?;
    let sandbox = prepare_sandbox(&options.fixture)?;

    println!("codex-ops Rust benchmark smoke");
    println!("fixture={}", display_path(&options.fixture));
    println!("rustBinary={}", display_path(&options.rust_binary));
    println!("runs={}", options.runs);
    println!("cases={}", cases.len());

    let mut failures = Vec::new();
    for case in cases {
        let args = resolve_case_args(case, &sandbox);
        let cold = run_case(&options.rust_binary, &args, &sandbox)?;
        let cold_status = status_label(&cold.output);

        if !cold.output.status.success() {
            failures.push(format!(
                "{} cold failed status={} stdout={} stderr={}",
                case.id,
                cold_status,
                preview(&cold.output.stdout),
                preview(&cold.output.stderr)
            ));
            println!(
                "case={} status={} coldMs={:.2} runs={} warmMinMs=0.00 warmMedianMs=0.00 warmMeanMs=0.00 warmMaxMs=0.00 stdoutBytes={} stderrBytes={}",
                case.id,
                cold_status,
                millis(cold.elapsed),
                options.runs,
                cold.output.stdout.len(),
                cold.output.stderr.len()
            );
            continue;
        }

        let mut warm = Vec::new();
        for _ in 0..options.runs {
            let timed = run_case(&options.rust_binary, &args, &sandbox)?;
            if !timed.output.status.success() {
                failures.push(format!(
                    "{} warm failed status={} stdout={} stderr={}",
                    case.id,
                    status_label(&timed.output),
                    preview(&timed.output.stdout),
                    preview(&timed.output.stderr)
                ));
                break;
            }
            warm.push(timed.elapsed);
        }

        let stats = TimingStats::from_durations(&warm);
        println!(
            "case={} status={} coldMs={:.2} runs={} warmMinMs={:.2} warmMedianMs={:.2} warmMeanMs={:.2} warmMaxMs={:.2} stdoutBytes={} stderrBytes={}",
            case.id,
            cold_status,
            millis(cold.elapsed),
            warm.len(),
            stats.min_ms,
            stats.median_ms,
            stats.mean_ms,
            stats.max_ms,
            cold.output.stdout.len(),
            cold.output.stderr.len()
        );
    }

    if !failures.is_empty() {
        return Err(format!(
            "{} benchmark case(s) failed:\n{}",
            failures.len(),
            failures.join("\n")
        ));
    }

    println!("benchmark smoke passed");
    Ok(())
}

fn parse_args(args: Vec<String>) -> Result<Options, String> {
    let mut options = Options {
        fixture: resolve_input_path("test/fixtures/rust-run"),
        runs: 1,
        matrix: "all".to_string(),
        rust_binary: resolve_input_path(default_binary_path()),
    };

    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--fixture" => {
                index += 1;
                options.fixture = resolve_input_path(read_arg(&args, index, "--fixture")?);
            }
            "--runs" => {
                index += 1;
                options.runs = parse_runs(read_arg(&args, index, "--runs")?)?;
            }
            "--matrix" => {
                index += 1;
                options.matrix = read_arg(&args, index, "--matrix")?.to_string();
            }
            "--rust-binary" => {
                index += 1;
                options.rust_binary = resolve_input_path(read_arg(&args, index, "--rust-binary")?);
            }
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            _ if arg.starts_with("--fixture=") => {
                options.fixture = resolve_input_path(&arg["--fixture=".len()..]);
            }
            _ if arg.starts_with("--runs=") => {
                options.runs = parse_runs(&arg["--runs=".len()..])?;
            }
            _ if arg.starts_with("--matrix=") => {
                options.matrix = arg["--matrix=".len()..].to_string();
            }
            _ if arg.starts_with("--rust-binary=") => {
                options.rust_binary = resolve_input_path(&arg["--rust-binary=".len()..]);
            }
            _ => return Err(format!("Unsupported argument: {arg}")),
        }
        index += 1;
    }

    Ok(options)
}

fn print_help() {
    println!(
        "\
Usage: codex-ops-bench [options]

Runs the production Rust binary against a synthetic fixture and prints a timing
summary. This is a smoke benchmark, not a historical JS comparison harness.

Options:
  --fixture <path>       Fixture root, default test/fixtures/rust-run
  --runs <n>             Warm runs per command, default 1
  --matrix <groups>      all, auth, doctor, stat, cycle, poc; comma-separated
  --rust-binary <path>   Production Rust binary, default target/release/codex-ops
  -h, --help             Print help"
    );
}

fn select_cases(matrix: &str) -> Result<Vec<&'static BenchCase>, String> {
    let requested: Vec<&str> = matrix
        .split(',')
        .map(str::trim)
        .filter(|group| !group.is_empty())
        .collect();

    if requested.is_empty() || requested.contains(&"all") {
        return Ok(BENCHMARK_CASES.iter().collect());
    }

    for group in &requested {
        if !BENCHMARK_CASES
            .iter()
            .any(|case| case.groups.contains(group))
        {
            return Err(format!("Unknown matrix group: {group}"));
        }
    }

    Ok(BENCHMARK_CASES
        .iter()
        .filter(|case| case.groups.iter().any(|group| requested.contains(group)))
        .collect())
}

fn prepare_sandbox(fixture: &Path) -> Result<Sandbox, String> {
    let root = temp_root()?;
    let fixture_copy = root.join("fixture");
    let home = root.join("home");

    copy_dir(fixture, &fixture_copy)?;
    fs::create_dir_all(&home).map_err(|error| error.to_string())?;

    let codex_home = fixture_copy.join("codex-home");
    let helper_dir = codex_home.join("codex-ops");

    Ok(Sandbox {
        root,
        home,
        auth_file: codex_home.join("auth.json"),
        sessions_dir: codex_home.join("sessions"),
        account_history_file: helper_dir.join("auth-account-history.json"),
        cycle_file: helper_dir.join("stat-cycles.json"),
        codex_home,
    })
}

fn run_case(binary: &Path, args: &[OsString], sandbox: &Sandbox) -> Result<TimedOutput, String> {
    let started = Instant::now();
    let output = Command::new(binary)
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("CODEX_HOME", &sandbox.codex_home)
        .env("CODEX_OPS_FIXED_NOW", FIXED_NOW)
        .env("HOME", &sandbox.home)
        .output()
        .map_err(|error| format!("Failed to start {}: {error}", display_path(binary)))?;

    Ok(TimedOutput {
        output,
        elapsed: started.elapsed(),
    })
}

fn resolve_case_args(case: &BenchCase, sandbox: &Sandbox) -> Vec<OsString> {
    case.args
        .iter()
        .map(|arg| match *arg {
            "{authFile}" => sandbox.auth_file.as_os_str().to_owned(),
            "{codexHome}" => sandbox.codex_home.as_os_str().to_owned(),
            "{sessionsDir}" => sandbox.sessions_dir.as_os_str().to_owned(),
            "{accountHistoryFile}" => sandbox.account_history_file.as_os_str().to_owned(),
            "{cycleFile}" => sandbox.cycle_file.as_os_str().to_owned(),
            value => OsString::from(value),
        })
        .collect()
}

#[derive(Default)]
struct TimingStats {
    min_ms: f64,
    median_ms: f64,
    mean_ms: f64,
    max_ms: f64,
}

impl TimingStats {
    fn from_durations(durations: &[Duration]) -> Self {
        if durations.is_empty() {
            return Self::default();
        }

        let mut values: Vec<f64> = durations.iter().map(|duration| millis(*duration)).collect();
        values.sort_by(|left, right| left.total_cmp(right));
        let sum: f64 = values.iter().sum();
        let middle = values.len() / 2;
        let median_ms = if values.len() % 2 == 0 {
            (values[middle - 1] + values[middle]) / 2.0
        } else {
            values[middle]
        };

        Self {
            min_ms: values[0],
            median_ms,
            mean_ms: sum / values.len() as f64,
            max_ms: values[values.len() - 1],
        }
    }
}

fn read_arg<'a>(args: &'a [String], index: usize, name: &str) -> Result<&'a str, String> {
    args.get(index)
        .filter(|value| !value.starts_with("--"))
        .map(String::as_str)
        .ok_or_else(|| format!("Missing value for {name}"))
}

fn parse_runs(value: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|_| "Invalid --runs value. Expected a non-negative integer.".to_string())
}

fn ensure_file(path: &Path, label: &str) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("{label} not found: {}", display_path(path)));
    }
    Ok(())
}

fn ensure_dir(path: &Path, label: &str) -> Result<(), String> {
    if !path.is_dir() {
        return Err(format!(
            "{label} directory not found: {}",
            display_path(path)
        ));
    }
    Ok(())
}

fn copy_dir(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        let next_destination = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir(&entry.path(), &next_destination)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), next_destination).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn temp_root() -> Result<PathBuf, String> {
    let mut root = env::temp_dir();
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    root.push(format!("codex-ops-bench-{pid}-{nanos}"));
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    Ok(root)
}

fn resolve_input_path(value: impl AsRef<OsStr>) -> PathBuf {
    let value = value.as_ref();
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
    }
}

fn default_binary_path() -> &'static str {
    if cfg!(windows) {
        "target/release/codex-ops.exe"
    } else {
        "target/release/codex-ops"
    }
}

fn status_label(output: &Output) -> String {
    output
        .status
        .code()
        .map(|code| code.to_string())
        .or_else(|| {
            output
                .status
                .signal()
                .map(|signal| format!("signal:{signal}"))
        })
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(unix)]
trait ExitSignal {
    fn signal(&self) -> Option<i32>;
}

#[cfg(unix)]
impl ExitSignal for std::process::ExitStatus {
    fn signal(&self) -> Option<i32> {
        std::os::unix::process::ExitStatusExt::signal(self)
    }
}

#[cfg(not(unix))]
trait ExitSignal {
    fn signal(&self) -> Option<i32>;
}

#[cfg(not(unix))]
impl ExitSignal for std::process::ExitStatus {
    fn signal(&self) -> Option<i32> {
        None
    }
}

fn preview(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut result = String::new();
    for ch in text.chars().take(500) {
        result.push(ch);
    }
    result.replace('\n', "\\n")
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}
