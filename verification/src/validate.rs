use clap::{Args, Parser, Subcommand};
use log::error;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, ErrorKind};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Output};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_SPECS_DIR: &str = "verification/tla_specs";
const DEFAULT_TLA2TOOLS_JAR: &str = "/Applications/TLA+ Toolbox.app/Contents/Eclipse/tla2tools.jar";
const DEFAULT_TLC_WORKERS: usize = 8;
const DEFAULT_FAILURE_CONTEXT: usize = 3;

#[derive(Debug, Clone)]
pub struct ValidationOptions {
    pub logs: PathBuf,
    pub specs: PathBuf,
    pub tla2tools: PathBuf,
    pub tlc_workers: usize,
    pub only: Option<String>,
    pub json: bool,
    pub workspace_root: Option<PathBuf>,
}

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
    #[arg(long, alias = "traces", value_name = "DIR")]
    logs: PathBuf,

    #[arg(long, default_value = DEFAULT_SPECS_DIR, value_name = "DIR")]
    specs: PathBuf,

    #[arg(long, value_name = "SPEC_A,SPEC_B")]
    only: Option<String>,

    #[arg(long, default_value = DEFAULT_TLA2TOOLS_JAR, value_name = "JAR")]
    tla2tools: PathBuf,

    #[arg(long, default_value_t = DEFAULT_TLC_WORKERS, value_name = "COUNT")]
    tlc_workers: usize,

    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct DiagnoseArgs {
    spec: String,

    #[arg(long, alias = "traces", value_name = "DIR")]
    logs: PathBuf,

    #[arg(long, default_value = DEFAULT_SPECS_DIR, value_name = "DIR")]
    specs: PathBuf,

    #[arg(long, default_value = DEFAULT_TLA2TOOLS_JAR, value_name = "JAR")]
    tla2tools: PathBuf,

    #[arg(long, default_value_t = DEFAULT_TLC_WORKERS, value_name = "COUNT")]
    tlc_workers: usize,

    #[arg(long, default_value_t = DEFAULT_FAILURE_CONTEXT)]
    context: usize,
}

#[derive(Debug, Clone)]
struct SpecLayout {
    name: String,
    source_dir: PathBuf,
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

#[derive(Debug, Clone)]
struct TraceEvent {
    event: String,
    event_args: Vec<String>,
}

const TRACE_METADATA_FIELDS: &[&str] = &[
    "clock",
    "spec",
    "producer",
    "event",
    "event_args",
    "source_file",
    "source_line",
];

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

#[derive(Debug, Serialize, Clone)]
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
    /// Human-readable explanation of what the trace model was trying
    /// to do with the failing entry and why it failed.
    event_context: Option<String>,
}

#[derive(Debug, Serialize)]
struct SpecExcerpt {
    file: String,
    line: usize,
    snippet: String,
}

pub fn run_validation_from_iter<I, T>(args: I) -> Result<ExitCode, String>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::parse_from(args);
    run(cli)
}

pub fn validate_and_print(options: &ValidationOptions) -> Result<bool, String> {
    let reports = validate_specs(options, DEFAULT_FAILURE_CONTEXT)?;
    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ValidationReport {
                results: reports.clone(),
            })
            .map_err(|error| format!("failed to encode validation report: {error}"))?
        );
    } else {
        print_human_report(&reports);
    }
    Ok(!reports
        .iter()
        .any(|report| report.status == SpecStatus::Fail))
}

fn run(cli: Cli) -> Result<ExitCode, String> {
    match cli.command {
        Some(CommandKind::Diagnose(args)) => run_diagnose(args),
        None => run_validate(cli.options),
    }
}

fn run_validate(options: ValidateArgs) -> Result<ExitCode, String> {
    let valid = validate_and_print(&ValidationOptions {
        logs: options.logs,
        specs: options.specs,
        tla2tools: options.tla2tools,
        tlc_workers: options.tlc_workers,
        only: options.only,
        json: options.json,
        workspace_root: None,
    })?;
    Ok(ExitCode::from(if valid { 0 } else { 1 }))
}

fn run_diagnose(args: DiagnoseArgs) -> Result<ExitCode, String> {
    let specs = discover_specs(&args.specs)?;
    let spec = specs
        .into_iter()
        .find(|candidate| candidate.name == args.spec)
        .ok_or_else(|| format!("unknown spec {}", args.spec))?;
    let tla2tools = resolve_process_path(&args.tla2tools)?;
    let report = with_validation_workspace(None, |workspace_root| {
        validate_spec(
            &spec,
            &args.logs,
            &tla2tools,
            args.tlc_workers,
            args.context,
            workspace_root,
        )
    })?;
    let diagnosis = build_diagnose_report(&spec, report)?;
    let failed = diagnosis.status == SpecStatus::Fail;
    println!(
        "{}",
        serde_json::to_string_pretty(&diagnosis)
            .map_err(|error| format!("failed to encode diagnose report: {error}"))?
    );
    Ok(ExitCode::from(if failed { 1 } else { 0 }))
}

fn validate_specs(
    options: &ValidationOptions,
    failure_context: usize,
) -> Result<Vec<SpecReport>, String> {
    let selected_specs = select_specs(&options.specs, options.only.as_deref())?;
    let tla2tools = resolve_process_path(&options.tla2tools)?;

    with_validation_workspace(options.workspace_root.clone(), |workspace_root| {
        let mut reports = Vec::with_capacity(selected_specs.len());
        for spec in &selected_specs {
            reports.push(validate_spec(
                spec,
                &options.logs,
                &tla2tools,
                options.tlc_workers,
                failure_context,
                workspace_root,
            )?);
        }
        Ok(reports)
    })
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
    let missing = filter.difference(&known).cloned().collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!("unknown spec filter(s): {}", missing.join(", ")));
    }

    Ok(selected)
}

fn discover_specs(spec_root: &Path) -> Result<Vec<SpecLayout>, String> {
    let entries = fs::read_dir(spec_root).map_err(|error| {
        format!(
            "failed to read spec directory {}: {error}",
            spec_root.display()
        )
    })?;
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
            let layout = SpecLayout::nested(spec_root, &name);
            if layout.base_spec.exists() {
                specs.entry(name).or_insert(layout);
            }
            continue;
        }

        if !file_type.is_file() || path.extension() != Some(OsStr::new("tla")) {
            continue;
        }

        let Some(stem) = path.file_stem().and_then(OsStr::to_str) else {
            continue;
        };
        if stem.ends_with("Trace") || stem.contains("_TTrace_") {
            continue;
        }

        let name = stem.to_owned();
        specs
            .entry(name.clone())
            .or_insert_with(|| SpecLayout::flat(spec_root, &name));
    }

    Ok(specs.into_values().collect())
}

impl SpecLayout {
    fn nested(spec_root: &Path, name: &str) -> Self {
        let source_dir = spec_root.join(name);
        let config = source_dir.join(format!("{name}.cfg"));
        Self {
            name: name.to_owned(),
            source_dir: source_dir.clone(),
            base_spec: source_dir.join(format!("{name}.tla")),
            trace_spec: source_dir.join(format!("{name}Trace.tla")),
            config: config.exists().then_some(config),
        }
    }

    fn flat(spec_root: &Path, name: &str) -> Self {
        let config = spec_root.join(format!("{name}.cfg"));
        Self {
            name: name.to_owned(),
            source_dir: spec_root.to_path_buf(),
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

    fn trace_config_path(&self) -> Option<PathBuf> {
        let path = self.source_dir.join(format!("{}Trace.cfg", self.name));
        path.exists().then_some(path)
    }

    fn trace_data_module_name(&self) -> String {
        format!("{}TraceData", self.name)
    }

    fn trace_data_module_path(&self, run_dir: &Path) -> PathBuf {
        run_dir.join(format!("{}.tla", self.trace_data_module_name()))
    }

    fn trace_file_path(&self, logs_root: &Path) -> PathBuf {
        logs_root.join(format!("{}.ndjson", self.name))
    }
}

fn validate_spec(
    spec: &SpecLayout,
    logs_root: &Path,
    tla2tools: &Path,
    tlc_workers: usize,
    failure_context: usize,
    workspace_root: &Path,
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

    let trace_file = spec.trace_file_path(logs_root);
    if !trace_file.exists() {
        return Ok(SpecReport {
            spec: spec.name.clone(),
            status: SpecStatus::None,
            message: Some(String::from(
                "no trace file for this spec in the current run",
            )),
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
    let tlc_run = run_tlc(spec, &trace_file, tla2tools, tlc_workers, workspace_root)?;
    let matches_trace = tlc_run.success && tlc_accepts_full_trace(trace_length, tlc_run.diameter);
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
            Some(failure_message(
                trace_length,
                tlc_run.diameter,
                tlc_run.success,
            ))
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
        // The TLC diameter is the number of states explored (initial state + accepted events).
        // The first unaccepted event is at (diameter - 1), i.e. the first trace entry that
        // the model could not transition from.  Map to 0-indexed line position directly.
        let failing_index = report
            .tlc_diameter
            .map(|diameter| diameter.saturating_sub(1))
            .unwrap_or(trace_length);
        let window = read_trace_window(&trace_file, failing_index, failure_context)?;
        report.failing_entry = window.failing_entry;
        report.context = window.context;
    }

    Ok(report)
}

fn run_tlc(
    spec: &SpecLayout,
    trace_file: &Path,
    tla2tools: &Path,
    tlc_workers: usize,
    workspace_root: &Path,
) -> Result<TlcRun, String> {
    let spec_run_dir = prepare_spec_run_dir(spec, workspace_root)?;
    let generated_trace_data_path = write_trace_data_module(spec, trace_file, &spec_run_dir)?;
    let mut command = Command::new("java");
    command
        .arg("-XX:+UseParallelGC")
        .arg("-jar")
        .arg(tla2tools)
        .arg("-deadlock")
        .arg("-workers")
        .arg(tlc_workers.to_string());
    if let Some(config_path) = spec.trace_config_path().as_ref().or(spec.config.as_ref()) {
        let Some(config_name) = config_path.file_name() else {
            if let Err(error) = fs::remove_file(&generated_trace_data_path) {
                error!("[validate] failed to remove generated trace data: {error}");
            }
            return Err(format!("invalid config path {}", config_path.display()));
        };
        command.arg("-config").arg(config_name);
    }
    command
        .arg(spec.trace_module_name()?)
        .current_dir(&spec_run_dir);

    let output = command.output().map_err(|error| {
        format!(
            "failed to run TLC for spec {} from {}: {error}",
            spec.name,
            spec_run_dir.display()
        )
    });
    let cleanup_result = fs::remove_file(&generated_trace_data_path);
    let output = output?;
    if let Err(error) = cleanup_result {
        return Err(format!(
            "failed to remove generated trace module {}: {error}",
            generated_trace_data_path.display()
        ));
    }

    let raw_output = combine_output(&output);
    Ok(TlcRun {
        success: output.status.success(),
        diameter: parse_tlc_diameter(&raw_output),
        raw_output,
    })
}

fn prepare_spec_run_dir(spec: &SpecLayout, workspace_root: &Path) -> Result<PathBuf, String> {
    let run_dir = workspace_root.join(&spec.name);
    recreate_dir(&run_dir)?;
    copy_spec_sources(&spec.source_dir, &run_dir)?;
    Ok(run_dir)
}

fn copy_spec_sources(source_dir: &Path, run_dir: &Path) -> Result<(), String> {
    let entries = fs::read_dir(source_dir).map_err(|error| {
        format!(
            "failed to read spec source directory {}: {error}",
            source_dir.display()
        )
    })?;
    for entry_result in entries {
        let entry = entry_result.map_err(|error| {
            format!(
                "failed to read entry in spec source directory {}: {error}",
                source_dir.display()
            )
        })?;
        let file_type = entry.file_type().map_err(|error| {
            format!(
                "failed to determine file type for {}: {error}",
                entry.path().display()
            )
        })?;
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        if !matches!(
            path.extension().and_then(OsStr::to_str),
            Some("tla") | Some("cfg")
        ) {
            continue;
        }
        fs::copy(&path, run_dir.join(entry.file_name())).map_err(|error| {
            format!(
                "failed to copy {} into {}: {error}",
                path.display(),
                run_dir.display()
            )
        })?;
    }
    Ok(())
}

fn write_trace_data_module(
    spec: &SpecLayout,
    trace_file: &Path,
    run_dir: &Path,
) -> Result<PathBuf, String> {
    let trace_events = read_trace_events(trace_file)?;
    let (nav_ids, nk_ids) = collect_trace_ids(&trace_events);
    let module_path = spec.trace_data_module_path(run_dir);
    let module_contents = render_trace_data_module(
        &spec.trace_data_module_name(),
        &trace_events,
        &nav_ids,
        &nk_ids,
    );
    fs::write(&module_path, module_contents).map_err(|error| {
        format!(
            "failed to write generated trace module {}: {error}",
            module_path.display()
        )
    })?;
    Ok(module_path)
}

fn read_trace_events(path: &Path) -> Result<Vec<TraceEvent>, String> {
    let file = File::open(path)
        .map_err(|error| format!("failed to open trace file {}: {error}", path.display()))?;
    let reader = BufReader::new(file);
    let mut events = Vec::new();

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
        let unsupported_fields = trace_update_fields(&entry)?;
        if !unsupported_fields.is_empty() {
            return Err(format!(
                "trace entry {} from {} contains abstract-state update fields ({}) that the current validator does not consume; current in-tree validation is event-based and must be extended before these updates can be treated as checked",
                line_index,
                path.display(),
                unsupported_fields.join(", ")
            ));
        }
        let event = entry
            .get("event")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let event_args = entry
            .get("event_args")
            .and_then(Value::as_array)
            .map(|args| {
                args.iter()
                    .map(|arg| {
                        arg.as_str()
                            .map(ToOwned::to_owned)
                            .unwrap_or_else(|| arg.to_string())
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        events.push(TraceEvent { event, event_args });
    }

    Ok(events)
}

fn trace_update_fields(entry: &Value) -> Result<Vec<String>, String> {
    let object = entry
        .as_object()
        .ok_or_else(|| String::from("trace entry must be a JSON object"))?;
    let mut fields = object
        .keys()
        .filter(|key| !TRACE_METADATA_FIELDS.contains(&key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    fields.sort();
    Ok(fields)
}

fn collect_trace_ids(trace_events: &[TraceEvent]) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut nav_ids = BTreeSet::new();
    let mut nk_ids = BTreeSet::new();

    for event in trace_events {
        match event.event.as_str() {
            "CreateNavigable" => {
                if let Some(navigable_id) = event.event_args.first() {
                    nav_ids.insert(navigable_id.clone());
                }
            }
            "CreateChildNavigable" => {
                if let Some(navigable_id) = event.event_args.first() {
                    nav_ids.insert(navigable_id.clone());
                }
                if let Some(parent_id) = event.event_args.get(1) {
                    nav_ids.insert(parent_id.clone());
                }
            }
            "CreateNavigation" => {
                if let Some(navigation_id) = event.event_args.first() {
                    nk_ids.insert(navigation_id.clone());
                }
                if let Some(navigable_id) = event.event_args.get(1) {
                    nav_ids.insert(navigable_id.clone());
                }
            }
            "StartNavigating" => {
                if let Some(navigation_id) = event.event_args.first() {
                    nk_ids.insert(navigation_id.clone());
                }
            }
            "RunBeforeUnload" => {
                if let Some(navigable_id) = event.event_args.first() {
                    nav_ids.insert(navigable_id.clone());
                }
                if let Some(navigation_id) = event.event_args.get(1) {
                    nk_ids.insert(navigation_id.clone());
                }
            }
            "ContinueNavigation" => {
                if let Some(navigation_id) = event.event_args.first() {
                    nk_ids.insert(navigation_id.clone());
                }
            }
            _ => {}
        }
    }

    (nav_ids, nk_ids)
}

fn render_trace_data_module(
    module_name: &str,
    trace_events: &[TraceEvent],
    nav_ids: &BTreeSet<String>,
    nk_ids: &BTreeSet<String>,
) -> String {
    let trace_entries = if trace_events.is_empty() {
        String::from("<<>>")
    } else {
        let entries = trace_events
            .iter()
            .map(render_trace_event)
            .collect::<Vec<_>>()
            .join(",\n    ");
        format!("<<\n    {entries}\n>>")
    };

    format!(
        "------------------------- MODULE {module_name} -------------------------\n\\* Generated by verification from the current NDJSON trace.\nTrace == {trace_entries}\n\nTraceNavIDs == {nav_ids}\n\nTraceNkIDs == {nk_ids}\n\n=============================================================================\n",
        nav_ids = render_string_set(nav_ids),
        nk_ids = render_string_set(nk_ids),
    )
}

fn render_trace_event(event: &TraceEvent) -> String {
    format!(
        "[event |-> {}, event_args |-> {}]",
        render_tla_string(&event.event),
        render_string_sequence(&event.event_args)
    )
}

fn render_string_set(values: &BTreeSet<String>) -> String {
    if values.is_empty() {
        return String::from("{}");
    }

    let rendered = values
        .iter()
        .map(|value| render_tla_string(value))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{{rendered}}}")
}

fn render_string_sequence(values: &[String]) -> String {
    if values.is_empty() {
        return String::from("<<>>");
    }

    let rendered = values
        .iter()
        .map(|value| render_tla_string(value))
        .collect::<Vec<_>>()
        .join(", ");
    format!("<<{rendered}>>")
}

fn render_tla_string(value: &str) -> String {
    let mut rendered = String::with_capacity(value.len() + 2);
    rendered.push('"');
    for character in value.chars() {
        match character {
            '\\' => rendered.push_str("\\\\"),
            '"' => rendered.push_str("\\\""),
            '\n' => rendered.push_str("\\n"),
            '\r' => rendered.push_str("\\r"),
            '\t' => rendered.push_str("\\t"),
            _ => rendered.push(character),
        }
    }
    rendered.push('"');
    rendered
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

fn read_trace_window(
    path: &Path,
    index: usize,
    context_size: usize,
) -> Result<TraceWindow, String> {
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

    let event_context = trace_event
        .as_deref()
        .map(event_failure_context)
        .map(|ctx| {
            let cleaned = ctx.trim();
            // The event_failure_context strings end with a space; clean it.
            cleaned.to_owned()
        });

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
        event_context,
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
        if lower.contains("diameter") || lower.contains("depth of the complete state graph search")
        {
            if let Some(value) = extract_last_usize(line) {
                return Some(value);
            }
        }
    }
    None
}

fn tlc_accepts_full_trace(trace_length: usize, diameter: Option<usize>) -> bool {
    matches!(diameter, Some(value) if value == trace_length || value == trace_length + 1)
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

/// Explain what the TLA+ model was trying to do when processing a trace event that failed.
/// This gives the user actionable context about why the model might have gotten stuck.
fn event_failure_context(event: &str) -> &'static str {
    match event {
        "CreateNavigable" => {
            "The model tried to create a new navigable but no free IDs were available. "
        }
        "CreateChildNavigable" => {
            "The model tried to create a child navigable but no free IDs were available. "
        }
        "CreateNavigation" => {
            "The model tried to create a new navigation but no free navigation IDs were \
available. This is unlikely to be the root cause \u{2014} a preceding event probably failed first, leaving the model stuck before reaching this entry. "
        }
        "StartNavigating" => {
            "The model tried to start a navigation by queueing beforeunload for all \
affected navigables. The navigation start queue must be non-empty with this \
navigation at the head. A missing or out-of-order CreateNavigation event is \
the most common cause. "
        }
        "RunBeforeUnload" => {
            "The model tried to record a beforeunload outcome for a navigable \
(Approved or Aborted). The navigable's beforeunload status must be set to \
'Queued' for this navigation ID first (by the preceding StartNavigating). \
A missing StartNavigating event, or a navigable that was not included in \
the affected set, prevents this step. "
        }
        "ContinueNavigation" => {
            "The model tried to finalize or abort a navigation, but not all affected \
navigables have resolved their beforeunload status. Every navigable in the \
affected set must have a RunBeforeUnload event (outcome \"Approved\" or \
\"Aborted\") before ContinueNavigation can proceed. A missing \
RunBeforeUnload event for one or more affected navigables is the most \
common cause. "
        }
        _ => {
            "The model could not apply this event. Check the trace model \
preconditions for this action in the TLA+ spec. "
        }
    }
}

fn failure_message(trace_length: usize, diameter: Option<usize>, tlc_success: bool) -> String {
    match (diameter, tlc_success) {
        (Some(diameter), true) => format!(
            "TLC accepted a prefix of length {}, but the trace has {trace_length} entries",
            diameter.saturating_sub(1)
        ),
        (Some(diameter), false) => format!(
            "TLC exited unsuccessfully after reaching a diameter of {diameter} for a trace of length {trace_length}"
        ),
        (None, true) => {
            format!(
                "TLC did not report a state-graph diameter for a trace of length {trace_length}"
            )
        }
        (None, false) => {
            String::from("TLC exited unsuccessfully and did not report a state-graph diameter")
        }
    }
}

fn print_human_report(results: &[SpecReport]) {
    for result in results {
        match result.status {
            SpecStatus::Skip => {
                println!(
                    "SKIP {} {}",
                    result.spec,
                    result.message.as_deref().unwrap_or("")
                );
            }
            SpecStatus::None => {
                println!(
                    "NONE {} {}",
                    result.spec,
                    result.message.as_deref().unwrap_or("")
                );
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
                    let event = failing_entry
                        .get("event")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    println!("  failing entry source: {source_file}:{source_line}");
                    println!("    {:?}", event_failure_context(event));
                }
                if let Some(failing_entry) = result.failing_entry.as_ref() {
                    println!("  failing NDJSON entry:");
                    println!(
                        "    {}",
                        serde_json::to_string(failing_entry).unwrap_or_default()
                    );
                }
                if !result.context.is_empty() {
                    println!("  preceding context entries:");
                    for entry in &result.context {
                        println!("    {}", serde_json::to_string(entry).unwrap_or_default());
                    }
                }
                if let Some(tlc_output) = result.tlc_raw_output.as_ref() {
                    println!("  TLC output (counterexample trace):");
                    for line in tlc_output.lines() {
                        println!("    | {line}");
                    }
                }
            }
        }
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn with_validation_workspace<T>(
    workspace_root: Option<PathBuf>,
    action: impl FnOnce(&Path) -> Result<T, String>,
) -> Result<T, String> {
    let workspace_root = workspace_root.unwrap_or_else(default_validation_workspace_root);
    recreate_dir(&workspace_root)?;
    let result = action(&workspace_root);
    let cleanup_result = remove_dir_all_if_exists(&workspace_root);
    match (result, cleanup_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(cleanup_error)) => Err(cleanup_error),
        (Err(error), Err(cleanup_error)) => Err(format!("{error}; {cleanup_error}")),
    }
}

fn default_validation_workspace_root() -> PathBuf {
    let base_stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "formal-web-validation-{}-{}",
        std::process::id(),
        base_stamp
    ))
}

fn recreate_dir(path: &Path) -> Result<(), String> {
    remove_dir_all_if_exists(path)?;
    fs::create_dir_all(path)
        .map_err(|error| format!("failed to create directory {}: {error}", path.display()))
}

fn remove_dir_all_if_exists(path: &Path) -> Result<(), String> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "failed to remove directory {}: {error}",
            path.display()
        )),
    }
}
