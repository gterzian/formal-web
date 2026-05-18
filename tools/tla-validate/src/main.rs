use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Output};

const DEFAULT_TRACES_DIR: &str = "tla-traces";
const DEFAULT_SPECS_DIR: &str = "tla_specs";
const DEFAULT_TLA2TOOLS_JAR: &str = "tla2tools.jar";
const DEFAULT_FAILURE_CONTEXT: usize = 3;

#[derive(Parser, Debug)]
#[command(name = "tla-validate")]
#[command(about = "Validate recorded engine traces against TLA+ trace specs")]
struct Cli {
    #[command(subcommand)]
    command: Option<CommandKind>,

    #[command(flatten)]
    options: ValidateArgs,
}

#[derive(Subcommand, Debug)]
enum CommandKind {
    Diagnose(DiagnoseArgs),
}

#[derive(Args, Debug, Clone)]
struct ValidateArgs {
    #[arg(long, default_value = DEFAULT_TRACES_DIR, value_name = "DIR")]
    traces: PathBuf,

    #[arg(long, default_value = DEFAULT_SPECS_DIR, value_name = "DIR")]
    specs: PathBuf,

    #[arg(long, value_name = "SPEC_A,SPEC_B")]
    only: Option<String>,

    #[arg(long, default_value = DEFAULT_TLA2TOOLS_JAR, value_name = "JAR")]
    tla2tools: PathBuf,

    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct DiagnoseArgs {
    spec: String,

    #[arg(long, default_value = DEFAULT_TRACES_DIR, value_name = "DIR")]
    traces: PathBuf,

    #[arg(long, default_value = DEFAULT_SPECS_DIR, value_name = "DIR")]
    specs: PathBuf,

    #[arg(long, default_value = DEFAULT_TLA2TOOLS_JAR, value_name = "JAR")]
    tla2tools: PathBuf,

    #[arg(long, default_value_t = DEFAULT_FAILURE_CONTEXT)]
    context: usize,
}

#[derive(Debug, Clone)]
struct SpecLayout {
    name: String,
    working_dir: PathBuf,
    base_spec: PathBuf,
    trace_spec: PathBuf,
    config: Option<PathBuf>,
}

#[derive(Debug)]
struct TlcRun {
    success: bool,
    diameter: Option<usize>,
    raw_output: String,
}

#[derive(Debug, Clone)]
struct TraceWindow {
    failing_entry: Option<Value>,
    context: Vec<Value>,
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum SpecStatus {
    Skip,
    None,
    Ok,
    Fail,
}

#[derive(Debug, Serialize)]
struct ValidationReport {
    results: Vec<SpecReport>,
}

#[derive(Debug, Serialize)]
struct SpecReport {
    spec: String,
    status: SpecStatus,
    message: Option<String>,
    trace_length: Option<usize>,
    tlc_diameter: Option<usize>,
    trace_file: Option<String>,
    trace_spec_file: Option<String>,
    failing_entry: Option<Value>,
    context: Vec<Value>,
    tlc_raw_output: Option<String>,
}

#[derive(Debug, Serialize)]
struct DiagnoseReport {
    spec: String,
    status: SpecStatus,
    trace_length: Option<usize>,
    tlc_diameter: Option<usize>,
    failing_entry: Option<Value>,
    context: Vec<Value>,
    source_file: Option<String>,
    source_line: Option<u64>,
    trace_event: Option<String>,
    naming_mismatch: Option<bool>,
    trace_spec_excerpt: Option<SpecExcerpt>,
    base_spec_excerpt: Option<SpecExcerpt>,
    tlc_raw_output: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Serialize)]
struct SpecExcerpt {
    file: String,
    line: usize,
    snippet: String,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(exit_code) => exit_code,
        Err(error) => {
            eprintln!("tla-validate: {error}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, String> {
    match cli.command {
        Some(CommandKind::Diagnose(args)) => run_diagnose(args),
        None => run_validate(cli.options),
    }
}

fn run_validate(options: ValidateArgs) -> Result<ExitCode, String> {
    let selected_specs = select_specs(&options.specs, options.only.as_deref())?;
    let tla2tools = resolve_process_path(&options.tla2tools)?;

    let mut any_failures = false;
    let mut results = Vec::with_capacity(selected_specs.len());
    for spec in selected_specs {
        let report = validate_spec(&spec, &options.traces, &tla2tools, DEFAULT_FAILURE_CONTEXT)?;
        if report.status == SpecStatus::Fail {
            any_failures = true;
        }
        results.push(report);
    }

    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ValidationReport { results })
                .map_err(|error| format!("failed to encode validation report: {error}"))?
        );
    } else {
        print_human_report(&results);
    }

    Ok(ExitCode::from(if any_failures { 1 } else { 0 }))
}

fn run_diagnose(args: DiagnoseArgs) -> Result<ExitCode, String> {
    let specs = discover_specs(&args.specs)?;
    let spec = specs
        .into_iter()
        .find(|candidate| candidate.name == args.spec)
        .ok_or_else(|| format!("unknown spec {}", args.spec))?;
    let tla2tools = resolve_process_path(&args.tla2tools)?;
    let report = validate_spec(&spec, &args.traces, &tla2tools, args.context)?;
    let diagnosis = build_diagnose_report(&spec, report)?;
    let failed = diagnosis.status == SpecStatus::Fail;
    println!(
        "{}",
        serde_json::to_string_pretty(&diagnosis)
            .map_err(|error| format!("failed to encode diagnose report: {error}"))?
    );
    Ok(ExitCode::from(if failed { 1 } else { 0 }))
}

fn select_specs(spec_root: &Path, only: Option<&str>) -> Result<Vec<SpecLayout>, String> {
    let specs = discover_specs(spec_root)?;
    let Some(filter) = parse_only_filter(only)? else {
        return Ok(specs);
    };

    let selected = specs
        .into_iter()
        .filter(|spec| filter.contains(&spec.name))
        .collect::<Vec<_>>();
    let known = selected
        .iter()
        .map(|spec| spec.name.clone())
        .collect::<BTreeSet<_>>();
    let missing = filter
        .difference(&known)
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!("unknown spec filter(s): {}", missing.join(", ")));
    }

    Ok(selected)
}

fn discover_specs(spec_root: &Path) -> Result<Vec<SpecLayout>, String> {
    let entries = fs::read_dir(spec_root)
        .map_err(|error| format!("failed to read spec directory {}: {error}", spec_root.display()))?;
    let mut specs = BTreeMap::<String, SpecLayout>::new();

    for entry_result in entries {
        let entry = entry_result
            .map_err(|error| format!("failed to read entry in {}: {error}", spec_root.display()))?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| {
            format!(
                "failed to determine file type for {}: {error}",
                path.display()
            )
        })?;

        if file_type.is_dir() {
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| format!("non-utf8 spec directory name: {}", path.display()))?;
            specs.entry(name.clone()).or_insert_with(|| SpecLayout::nested(spec_root, &name));
            continue;
        }

        if !file_type.is_file() || path.extension() != Some(OsStr::new("tla")) {
            continue;
        }

        let Some(stem) = path.file_stem().and_then(OsStr::to_str) else {
            continue;
        };
        if stem.ends_with("Trace") {
            continue;
        }

        let name = stem.to_owned();
        specs.entry(name.clone()).or_insert_with(|| SpecLayout::flat(spec_root, &name));
    }

    Ok(specs.into_values().collect())
}

impl SpecLayout {
    fn nested(spec_root: &Path, name: &str) -> Self {
        let working_dir = spec_root.join(name);
        let config = working_dir.join(format!("{name}.cfg"));
        Self {
            name: name.to_owned(),
            working_dir: working_dir.clone(),
            base_spec: working_dir.join(format!("{name}.tla")),
            trace_spec: working_dir.join(format!("{name}Trace.tla")),
            config: config.exists().then_some(config),
        }
    }

    fn flat(spec_root: &Path, name: &str) -> Self {
        let config = spec_root.join(format!("{name}.cfg"));
        Self {
            name: name.to_owned(),
            working_dir: spec_root.to_path_buf(),
            base_spec: spec_root.join(format!("{name}.tla")),
            trace_spec: spec_root.join(format!("{name}Trace.tla")),
            config: config.exists().then_some(config),
        }
    }

    fn trace_module_name(&self) -> Result<String, String> {
        self.trace_spec
            .file_stem()
            .and_then(OsStr::to_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| format!("invalid trace spec path {}", self.trace_spec.display()))
    }

    fn trace_file_path(&self, traces_root: &Path) -> PathBuf {
        traces_root.join(format!("{}.ndjson", self.name))
    }
}

fn validate_spec(
    spec: &SpecLayout,
    traces_root: &Path,
    tla2tools: &Path,
    failure_context: usize,
) -> Result<SpecReport, String> {
    if !spec.trace_spec.exists() {
        return Ok(SpecReport {
            spec: spec.name.clone(),
            status: SpecStatus::Skip,
            message: Some(String::from("trace spec not written yet")),
            trace_length: None,
            tlc_diameter: None,
            trace_file: None,
            trace_spec_file: Some(path_string(&spec.trace_spec)),
            failing_entry: None,
            context: Vec::new(),
            tlc_raw_output: None,
        });
    }

    let trace_file = spec.trace_file_path(traces_root);
    if !trace_file.exists() {
        return Ok(SpecReport {
            spec: spec.name.clone(),
            status: SpecStatus::None,
            message: Some(String::from("no trace file for this spec in the current run")),
            trace_length: None,
            tlc_diameter: None,
            trace_file: Some(path_string(&trace_file)),
            trace_spec_file: Some(path_string(&spec.trace_spec)),
            failing_entry: None,
            context: Vec::new(),
            tlc_raw_output: None,
        });
    }

    let trace_length = count_trace_entries(&trace_file)?;
    let tlc_run = run_tlc(spec, &trace_file, tla2tools)?;
    let matches_trace = tlc_run.diameter == Some(trace_length) && tlc_run.success;
    let mut report = SpecReport {
        spec: spec.name.clone(),
        status: if matches_trace {
            SpecStatus::Ok
        } else {
            SpecStatus::Fail
        },
        message: if matches_trace {
            None
        } else {
            Some(failure_message(trace_length, tlc_run.diameter, tlc_run.success))
        },
        trace_length: Some(trace_length),
        tlc_diameter: tlc_run.diameter,
        trace_file: Some(path_string(&trace_file)),
        trace_spec_file: Some(path_string(&spec.trace_spec)),
        failing_entry: None,
        context: Vec::new(),
        tlc_raw_output: Some(tlc_run.raw_output),
    };

    if report.status == SpecStatus::Fail {
        let failing_index = report.tlc_diameter.unwrap_or(trace_length);
        let window = read_trace_window(&trace_file, failing_index, failure_context)?;
        report.failing_entry = window.failing_entry;
        report.context = window.context;
    }

    Ok(report)
}

fn run_tlc(spec: &SpecLayout, trace_file: &Path, tla2tools: &Path) -> Result<TlcRun, String> {
    let trace_path = resolve_process_path(trace_file)?;
    let mut command = Command::new("java");
    command
        .arg(format!("-DTRACE_PATH={}", trace_path.display()))
        .arg("-jar")
        .arg(tla2tools)
        .arg("-deadlock");
    if let Some(config_path) = spec.config.as_ref() {
        let Some(config_name) = config_path.file_name() else {
            return Err(format!("invalid config path {}", config_path.display()));
        };
        command.arg("-config").arg(config_name);
    }
    command
        .arg(spec.trace_module_name()?)
        .current_dir(&spec.working_dir);

    let output = command.output().map_err(|error| {
        format!(
            "failed to run TLC for spec {} from {}: {error}",
            spec.name,
            spec.working_dir.display()
        )
    })?;

    let raw_output = combine_output(&output);
    Ok(TlcRun {
        success: output.status.success(),
        diameter: parse_tlc_diameter(&raw_output),
        raw_output,
    })
}

fn count_trace_entries(path: &Path) -> Result<usize, String> {
    let file = File::open(path)
        .map_err(|error| format!("failed to open trace file {}: {error}", path.display()))?;
    let reader = BufReader::new(file);
    let mut count = 0usize;
    for line in reader.lines() {
        line.map_err(|error| format!("failed to read trace file {}: {error}", path.display()))?;
        count += 1;
    }
    Ok(count)
}

fn read_trace_window(path: &Path, index: usize, context_size: usize) -> Result<TraceWindow, String> {
    let file = File::open(path)
        .map_err(|error| format!("failed to open trace file {}: {error}", path.display()))?;
    let reader = BufReader::new(file);
    let mut context = VecDeque::with_capacity(context_size);
    let mut failing_entry = None;

    for (line_index, line_result) in reader.lines().enumerate() {
        let line = line_result
            .map_err(|error| format!("failed to read trace file {}: {error}", path.display()))?;
        let entry = serde_json::from_str::<Value>(&line).map_err(|error| {
            format!(
                "failed to decode NDJSON entry {} from {}: {error}",
                line_index,
                path.display()
            )
        })?;
        if line_index < index {
            if context_size > 0 {
                if context.len() == context_size {
                    context.pop_front();
                }
                context.push_back(entry);
            }
            continue;
        }

        if line_index == index {
            failing_entry = Some(entry);
            break;
        }
    }

    Ok(TraceWindow {
        failing_entry,
        context: context.into_iter().collect(),
    })
}

fn build_diagnose_report(spec: &SpecLayout, report: SpecReport) -> Result<DiagnoseReport, String> {
    let failing_entry = report.failing_entry.clone();
    let source_file = failing_entry
        .as_ref()
        .and_then(|entry| entry.get("source_file"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let source_line = failing_entry
        .as_ref()
        .and_then(|entry| entry.get("source_line"))
        .and_then(Value::as_u64);
    let trace_event = failing_entry
        .as_ref()
        .and_then(|entry| entry.get("event"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let naming_mismatch = match trace_event.as_deref() {
        Some(event) => Some(!trace_spec_mentions_event(&spec.trace_spec, event)?),
        None => None,
    };

    let trace_spec_excerpt = match trace_event.as_deref() {
        Some(event) => excerpt_around_match(&spec.trace_spec, |line| {
            line.contains(&format!("IsEvent(\"{event}\")"))
        })?,
        None => None,
    };
    let base_spec_excerpt = match trace_event.as_deref() {
        Some(event) => excerpt_around_match(&spec.base_spec, |line| {
            let trimmed = line.trim_start();
            trimmed.starts_with(event) && trimmed.contains("==")
        })?,
        None => None,
    };

    Ok(DiagnoseReport {
        spec: report.spec,
        status: report.status,
        trace_length: report.trace_length,
        tlc_diameter: report.tlc_diameter,
        failing_entry,
        context: report.context,
        source_file,
        source_line,
        trace_event,
        naming_mismatch,
        trace_spec_excerpt,
        base_spec_excerpt,
        tlc_raw_output: report.tlc_raw_output,
        message: report.message,
    })
}

fn trace_spec_mentions_event(path: &Path, event_name: &str) -> Result<bool, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    Ok(contents.contains(&format!("IsEvent(\"{event_name}\")")))
}

fn excerpt_around_match(
    path: &Path,
    predicate: impl Fn(&str) -> bool,
) -> Result<Option<SpecExcerpt>, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let lines = contents.lines().collect::<Vec<_>>();
    for (index, line) in lines.iter().enumerate() {
        if !predicate(line) {
            continue;
        }

        let start = index.saturating_sub(DEFAULT_FAILURE_CONTEXT);
        let end = (index + DEFAULT_FAILURE_CONTEXT + 1).min(lines.len());
        return Ok(Some(SpecExcerpt {
            file: path_string(path),
            line: index + 1,
            snippet: lines[start..end].join("\n"),
        }));
    }
    Ok(None)
}

fn parse_only_filter(only: Option<&str>) -> Result<Option<BTreeSet<String>>, String> {
    let Some(only) = only else {
        return Ok(None);
    };

    let parsed = only
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>();
    if parsed.is_empty() {
        return Err(String::from("--only must contain at least one spec name"));
    }
    Ok(Some(parsed))
}

fn parse_tlc_diameter(output: &str) -> Option<usize> {
    for line in output.lines().rev() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("diameter") || lower.contains("depth of the complete state graph search") {
            if let Some(value) = extract_last_usize(line) {
                return Some(value);
            }
        }
    }
    None
}

fn extract_last_usize(line: &str) -> Option<usize> {
    line.split(|character: char| !character.is_ascii_digit())
        .filter(|segment| !segment.is_empty())
        .next_back()
        .and_then(|segment| segment.parse::<usize>().ok())
}

fn combine_output(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout.into_owned(),
        (true, false) => stderr.into_owned(),
        (false, false) => format!("{stdout}\n{stderr}"),
    }
}

fn resolve_process_path(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    std::env::current_dir()
        .map(|current_dir| current_dir.join(path))
        .map_err(|error| format!("failed to resolve current directory: {error}"))
}

fn failure_message(trace_length: usize, diameter: Option<usize>, tlc_success: bool) -> String {
    match (diameter, tlc_success) {
        (Some(diameter), true) => format!(
            "TLC accepted a prefix of length {diameter}, but the trace has {trace_length} entries"
        ),
        (Some(diameter), false) => format!(
            "TLC exited unsuccessfully after reaching a diameter of {diameter} for a trace of length {trace_length}"
        ),
        (None, true) => format!(
            "TLC did not report a state-graph diameter for a trace of length {trace_length}"
        ),
        (None, false) => String::from("TLC exited unsuccessfully and did not report a state-graph diameter"),
    }
}

fn print_human_report(results: &[SpecReport]) {
    for result in results {
        match result.status {
            SpecStatus::Skip => {
                println!("SKIP {} {}", result.spec, result.message.as_deref().unwrap_or(""));
            }
            SpecStatus::None => {
                println!("NONE {} {}", result.spec, result.message.as_deref().unwrap_or(""));
            }
            SpecStatus::Ok => {
                println!("CHECK {} ... OK", result.spec);
            }
            SpecStatus::Fail => {
                println!("CHECK {} ... FAIL", result.spec);
                if let Some(message) = result.message.as_deref() {
                    println!("  {message}");
                }
                if let Some(failing_entry) = result.failing_entry.as_ref() {
                    let source_file = failing_entry
                        .get("source_file")
                        .and_then(Value::as_str)
                        .unwrap_or("<unknown>");
                    let source_line = failing_entry
                        .get("source_line")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    println!("  failing entry source: {source_file}:{source_line}");
                }
            }
        }
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}