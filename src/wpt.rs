use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_WPT_ROOT: &str = "vendor/wpt";
const DEFAULT_WPT_CONFIG_PATH: &str = "tests/wpt/config.ini";
const DEFAULT_WPT_INCLUDE_PATH: &str = "tests/wpt/include.ini";
const DEFAULT_WPT_META_ROOT: &str = "tests/wpt/meta";

const DEFAULT_FORMAL_ROOT: &str = "tests/formal/tests";
const DEFAULT_FORMAL_INCLUDE_PATH: &str = "tests/formal/include.ini";
const DEFAULT_FORMAL_META_ROOT: &str = "tests/formal/meta";
const DEFAULT_FORMAL_DISPLAY_PREFIX: &str = "formal/";
const DEFAULT_FORMAL_URL_PREFIX: &str = "__formal__";

const CHILD_STDERR_LIMIT: usize = 16 * 1024;
const WPTSERVE_STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
const WPTSERVE_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
const WEBDRIVER_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const WEBDRIVER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
const HTTP_POLL_INTERVAL: Duration = Duration::from_millis(100);
const REPORT_POLL_INTERVAL: Duration = Duration::from_millis(100);
const RUNNER_ARTIFACT_ROOT: &str = "scratchpad/wpt-runner";

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Args, Debug)]
pub struct TestWptArgs {
    #[arg(value_name = "PATH")]
    path: Option<String>,

    #[arg(long, default_value_t = 10_000)]
    timeout_ms: u64,

    #[arg(long, value_name = "PATH")]
    output: Option<PathBuf>,

    #[arg(long)]
    list: bool,

    #[arg(long)]
    headed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TestKind {
    Html,
    WindowScript,
    AnyScript,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SuiteKind {
    Wpt,
    Formal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum WptStatus {
    Pass,
    Fail,
    Timeout,
    Error,
    NotRun,
    PreconditionFailed,
    Crash,
    Skip,
}

impl WptStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Timeout => "TIMEOUT",
            Self::Error => "ERROR",
            Self::NotRun => "NOTRUN",
            Self::PreconditionFailed => "PRECONDITION_FAILED",
            Self::Crash => "CRASH",
            Self::Skip => "SKIP",
        }
    }

    fn parse_expected(value: &str) -> Option<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "PASS" => Some(Self::Pass),
            "FAIL" => Some(Self::Fail),
            "TIMEOUT" => Some(Self::Timeout),
            "ERROR" => Some(Self::Error),
            "NOTRUN" | "NOT_RUN" => Some(Self::NotRun),
            "PRECONDITION_FAILED" | "OPTIONAL_FEATURE_UNSUPPORTED" => {
                Some(Self::PreconditionFailed)
            }
            "CRASH" => Some(Self::Crash),
            "SKIP" => Some(Self::Skip),
            _ => None,
        }
    }

    fn from_test_code(code: u32) -> Option<Self> {
        match code {
            0 => Some(Self::Pass),
            1 => Some(Self::Fail),
            2 => Some(Self::Timeout),
            3 => Some(Self::NotRun),
            4 => Some(Self::PreconditionFailed),
            _ => None,
        }
    }

    fn from_harness_code(code: u32) -> Option<Self> {
        match code {
            0 => Some(Self::Pass),
            1 => Some(Self::Error),
            2 => Some(Self::Timeout),
            3 => Some(Self::PreconditionFailed),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
struct SelectedTest {
    suite: SuiteKind,
    source_relative_path: String,
    display_path: String,
    served_path: String,
    kind: TestKind,
}

#[derive(Clone, Debug)]
struct SuiteDescriptor {
    kind: SuiteKind,
    root: PathBuf,
    include_path: PathBuf,
    meta_root: PathBuf,
    display_prefix: &'static str,
    url_prefix: &'static str,
}

#[derive(Clone, Debug)]
struct RunnerConfig {
    wpt: SuiteDescriptor,
    formal: SuiteDescriptor,
}

#[derive(Clone, Debug, Default)]
struct IncludeFilter {
    root_skip: bool,
    rules: HashMap<String, bool>,
}

#[derive(Clone, Debug, Default)]
struct DirectoryExpectation {
    disabled: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct SubtestExpectation {
    expected: Option<WptStatus>,
}

#[derive(Clone, Debug, Default)]
struct TestExpectation {
    disabled: Option<String>,
    expected: Option<WptStatus>,
    subtests: HashMap<String, SubtestExpectation>,
}

#[derive(Clone, Debug, Default)]
struct MetaTree {
    directories: HashMap<String, DirectoryExpectation>,
    tests: HashMap<String, TestExpectation>,
}

#[derive(Clone, Debug)]
enum ClassifiedTest {
    Supported(TestKind),
    Unsupported(String),
    Ignore,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct HarnessStatusRecord {
    status: u32,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    stack: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct HarnessSubtestRecord {
    name: String,
    status: u32,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    stack: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct HarnessCompletionReport {
    location: String,
    title: String,
    tests: Vec<HarnessSubtestRecord>,
    status: HarnessStatusRecord,
    #[serde(default)]
    asserts: Vec<Value>,
}

#[derive(Clone, Debug, Deserialize)]
struct LiveHarnessSummary {
    #[serde(default, rename = "harnessStatus")]
    harness_status: Option<String>,
    #[serde(default)]
    pass: f64,
    #[serde(default)]
    fail: f64,
    #[serde(default)]
    timeout: f64,
    #[serde(default, rename = "notRun")]
    not_run: f64,
    #[serde(default, rename = "preconditionFailed")]
    precondition_failed: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ObservedTestResult {
    path: String,
    kind: TestKind,
    actual: WptStatus,
    #[serde(default)]
    message: Option<String>,
    harness: Option<HarnessCompletionReport>,
    duration_ms: u128,
}

#[derive(Clone, Debug, Serialize)]
struct ComparedSubtestResult {
    name: String,
    actual: WptStatus,
    expected: WptStatus,
    unexpected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct ComparedTestResult {
    path: String,
    kind: TestKind,
    actual: WptStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected: Option<WptStatus>,
    unexpected: bool,
    skipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    duration_ms: u128,
    subtests: Vec<ComparedSubtestResult>,
}

#[derive(Clone, Debug, Default, Serialize)]
struct RunSummary {
    total: usize,
    executed: usize,
    skipped: usize,
    unexpected: usize,
    passed: usize,
    failed: usize,
    timed_out: usize,
    errors: usize,
    crashes: usize,
}

#[derive(Clone, Debug, Serialize)]
struct RunReport {
    summary: RunSummary,
    tests: Vec<ComparedTestResult>,
}

#[derive(Debug)]
struct HttpResponse {
    status: u16,
    body: Vec<u8>,
}

#[derive(Debug)]
struct WptServeProcess {
    child: Child,
    port: u16,
    temp_dir: PathBuf,
}

#[derive(Debug)]
struct BrowserProcess {
    child: Child,
    port: u16,
}

#[derive(Clone, Debug)]
struct WebDriverSession {
    port: u16,
    session_id: String,
}

pub fn run(args: TestWptArgs) -> Result<(), String> {
    let repo_root = repo_root();
    let config = RunnerConfig::load(&repo_root)?;

    let wpt_include = IncludeFilter::load(&config.wpt.include_path)?;
    let formal_include = IncludeFilter::load(&config.formal.include_path)?;
    let wpt_meta = MetaTree::load(&config.wpt.meta_root)?;
    let formal_meta = MetaTree::load(&config.formal.meta_root)?;

    let selected = collect_selected_tests(
        &config,
        args.path.as_deref(),
        &wpt_include,
        &formal_include,
    )?;

    if args.list {
        for test in &selected {
            println!("{}", test.display_path);
        }
        return Ok(());
    }

    let timeout = Duration::from_millis(args.timeout_ms);
    println!("WPT root: {}", config.wpt.root.display());
    println!("Mode: WebDriver + wptserve runner");
    println!("Browser mode: {}", if args.headed { "headed" } else { "headless" });
    println!("Selected tests: {}", selected.len());

    let mut summary = RunSummary::default();
    let mut results = Vec::with_capacity(selected.len());

    for test in selected {
        let suite_meta = match test.suite {
            SuiteKind::Wpt => &wpt_meta,
            SuiteKind::Formal => &formal_meta,
        };

        if let Some(reason) = suite_meta
            .disabled_reason_for(&test.source_relative_path)
            .or_else(|| suite_meta.expectation_for(&test.source_relative_path).and_then(|entry| entry.disabled.clone()))
        {
            let result = skipped_result(&test.display_path, test.kind, reason);
            print_test_result(&result);
            update_summary(&mut summary, &result);
            results.push(result);
            continue;
        }

        let server = match WptServeProcess::start(&config) {
            Ok(server) => server,
            Err(error) => {
                let result = compare_observed_result(
                    crash_result(&test, error, 0),
                    suite_meta.expectation_for(&test.source_relative_path),
                );
                print_test_result(&result);
                update_summary(&mut summary, &result);
                results.push(result);
                continue;
            }
        };
        let observed = run_single_test(&test, &server, timeout, !args.headed);
        let compared = compare_observed_result(
            observed,
            suite_meta.expectation_for(&test.source_relative_path),
        );
        print_test_result(&compared);
        update_summary(&mut summary, &compared);
        results.push(compared);
    }

    print_summary(&summary);

    if let Some(output_path) = args.output.as_deref() {
        write_run_report(
            output_path,
            &RunReport {
                summary: summary.clone(),
                tests: results.clone(),
            },
        )?;
    }

    if summary.unexpected > 0 {
        return Err(format!("{} unexpected WPT result(s)", summary.unexpected));
    }

    Ok(())
}

impl RunnerConfig {
    fn load(repo_root: &Path) -> Result<Self, String> {
        let config_path = repo_root.join(DEFAULT_WPT_CONFIG_PATH);
        let include_path = repo_root.join(DEFAULT_WPT_INCLUDE_PATH);

        let (wpt_root, wpt_meta_root) = if config_path.exists() {
            let contents = fs::read_to_string(&config_path)
                .map_err(|error| format!("failed to read {}: {error}", config_path.display()))?;
            let config_dir = config_path
                .parent()
                .ok_or_else(|| String::from("WPT config path has no parent directory"))?;

            let mut section = String::new();
            let mut tests_path = None;
            let mut metadata_path = None;

            for raw_line in contents.lines() {
                let line = raw_line.trim();
                if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                    continue;
                }
                if line.starts_with('[') && line.ends_with(']') {
                    section = line[1..line.len() - 1].trim().to_owned();
                    continue;
                }
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                if section == "manifest:upstream" {
                    match key.trim() {
                        "tests" => tests_path = Some(value.trim().to_owned()),
                        "metadata" => metadata_path = Some(value.trim().to_owned()),
                        _ => {}
                    }
                }
            }

            let tests_path = tests_path.unwrap_or_else(|| String::from("../../vendor/wpt"));
            let metadata_path = metadata_path.unwrap_or_else(|| String::from("meta"));
            let wpt_root = config_dir
                .join(tests_path)
                .canonicalize()
                .map_err(|error| format!("failed to resolve configured WPT root: {error}"))?;
            let meta_root = config_dir.join(metadata_path);
            (wpt_root, meta_root)
        } else {
            (
                repo_root
                    .join(DEFAULT_WPT_ROOT)
                    .canonicalize()
                    .map_err(|error| format!("failed to resolve {}: {error}", DEFAULT_WPT_ROOT))?,
                repo_root.join(DEFAULT_WPT_META_ROOT),
            )
        };

        Ok(Self {
            wpt: SuiteDescriptor {
                kind: SuiteKind::Wpt,
                root: wpt_root,
                include_path,
                meta_root: wpt_meta_root,
                display_prefix: "",
                url_prefix: "",
            },
            formal: SuiteDescriptor {
                kind: SuiteKind::Formal,
                root: repo_root.join(DEFAULT_FORMAL_ROOT),
                include_path: repo_root.join(DEFAULT_FORMAL_INCLUDE_PATH),
                meta_root: repo_root.join(DEFAULT_FORMAL_META_ROOT),
                display_prefix: DEFAULT_FORMAL_DISPLAY_PREFIX,
                url_prefix: DEFAULT_FORMAL_URL_PREFIX,
            },
        })
    }
}

impl IncludeFilter {
    fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let contents = fs::read_to_string(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let mut filter = Self::default();
        let mut sections = Vec::<String>::new();

        for raw_line in contents.lines() {
            let trimmed = raw_line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
                continue;
            }

            let indent = raw_line
                .chars()
                .take_while(|character| character.is_ascii_whitespace())
                .count();
            let depth = indent / 2;

            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                sections.truncate(depth);
                sections.push(normalize_rel_path(&trimmed[1..trimmed.len() - 1]));
                continue;
            }

            let Some((key, value)) = trimmed.split_once(':') else {
                continue;
            };
            if key.trim() != "skip" {
                continue;
            }
            let skip = parse_bool(value)?;
            let section_path = sections.join("/");
            if section_path.is_empty() {
                filter.root_skip = skip;
            } else {
                filter.rules.insert(section_path, skip);
            }
        }

        Ok(filter)
    }

    fn explicit_includes(&self) -> Vec<String> {
        let mut includes = self
            .rules
            .iter()
            .filter_map(|(path, skip)| (!*skip).then_some(path.clone()))
            .collect::<Vec<_>>();
        includes.sort();
        includes
    }
}

impl MetaTree {
    fn load(meta_root: &Path) -> Result<Self, String> {
        if !meta_root.exists() {
            return Ok(Self::default());
        }

        let mut tree = Self::default();
        let mut stack = vec![meta_root.to_path_buf()];

        while let Some(path) = stack.pop() {
            let entries = fs::read_dir(&path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
            for entry in entries {
                let entry = entry.map_err(|error| format!("failed to read metadata entry: {error}"))?;
                let entry_path = entry.path();
                if entry_path.is_dir() {
                    stack.push(entry_path);
                    continue;
                }

                let Some(file_name) = entry_path.file_name().and_then(OsStr::to_str) else {
                    continue;
                };
                if file_name == "__dir__.ini" {
                    let relative_dir = entry_path
                        .parent()
                        .and_then(|parent| parent.strip_prefix(meta_root).ok())
                        .map(path_buf_to_string)
                        .unwrap_or_default();
                    let expectation = parse_directory_expectation_file(&entry_path)?;
                    if expectation.disabled.is_some() {
                        tree.directories.insert(relative_dir, expectation);
                    }
                    continue;
                }

                if !file_name.ends_with(".ini") {
                    continue;
                }

                let relative_meta_path = entry_path
                    .strip_prefix(meta_root)
                    .map_err(|error| format!("failed to resolve metadata path: {error}"))?;
                let relative_meta_string = relative_meta_path.to_string_lossy().into_owned();
                let relative_test_path = normalize_rel_path(
                    relative_meta_string
                        .strip_suffix(".ini")
                        .unwrap_or(&relative_meta_string),
                );
                let expectation = parse_test_expectation_file(&entry_path, &relative_test_path)?;
                if expectation.disabled.is_some()
                    || expectation.expected.is_some()
                    || !expectation.subtests.is_empty()
                {
                    tree.tests.insert(relative_test_path, expectation);
                }
            }
        }

        Ok(tree)
    }

    fn expectation_for(&self, relative_path: &str) -> Option<&TestExpectation> {
        self.tests.get(relative_path)
    }

    fn disabled_reason_for(&self, relative_path: &str) -> Option<String> {
        let mut current = Path::new(relative_path).parent();
        while let Some(parent) = current {
            let key = path_buf_to_string(parent);
            if let Some(entry) = self
                .directories
                .get(&key)
                .and_then(|entry| entry.disabled.as_deref())
            {
                return Some(entry.to_owned());
            }
            current = parent.parent();
        }

        None
    }
}

impl WptServeProcess {
    fn start(config: &RunnerConfig) -> Result<Self, String> {
        let temp_dir = temp_dir_path("wptserve");
        fs::create_dir_all(&temp_dir)
            .map_err(|error| format!("failed to create {}: {error}", temp_dir.display()))?;

        let port = pick_unused_port()?;
        let config_path = temp_dir.join("serve-config.json");
        let alias_file = temp_dir.join("aliases.txt");
        let inject_script = temp_dir.join("formal-web-inject.js");

        let config_json = json!({
            "browser_host": "localhost",
            "alternate_hosts": { "alt": "127.0.0.1" },
            "check_subdomains": false,
            "doc_root": config.wpt.root,
            "ports": {
                "http": [port, "auto"]
            }
        });
        fs::write(
            &config_path,
            serde_json::to_vec_pretty(&config_json)
                .map_err(|error| format!("failed to encode wptserve config: {error}"))?,
        )
        .map_err(|error| format!("failed to write {}: {error}", config_path.display()))?;

        fs::write(
            &alias_file,
            format!("/{}/, {}\n", DEFAULT_FORMAL_URL_PREFIX, config.formal.root.display()),
        )
        .map_err(|error| format!("failed to write {}: {error}", alias_file.display()))?;
        fs::write(&inject_script, reporter_inject_script())
            .map_err(|error| format!("failed to write {}: {error}", inject_script.display()))?;

        let mut command = Command::new("python3");
        command
            .arg("./wpt")
            .arg("serve")
            .arg("--config")
            .arg(&config_path)
            .arg("--no-h2")
            .arg("--alias_file")
            .arg(&alias_file)
            .arg("--inject-script")
            .arg(&inject_script)
            .current_dir(&config.wpt.root)
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        configure_wptserve_command(&mut command);

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = fs::remove_dir_all(&temp_dir);
                return Err(format!("failed to start wptserve: {error}"));
            }
        };

        if let Err(error) = wait_for_wptserve_ready(port, &mut child, WPTSERVE_STARTUP_TIMEOUT) {
            let _ = shutdown_wptserve_child(&mut child);
            let _ = fs::remove_dir_all(&temp_dir);
            return Err(error);
        }

        Ok(Self { child, port, temp_dir })
    }

    fn base_url(&self) -> String {
        format!("http://localhost:{}", self.port)
    }
}

impl Drop for WptServeProcess {
    fn drop(&mut self) {
        let _ = shutdown_wptserve_child(&mut self.child);
        let _ = fs::remove_dir_all(&self.temp_dir);
    }
}

impl BrowserProcess {
    fn start(port: u16, startup_url: Option<&str>, headless: bool) -> Result<Self, String> {
        let executable = std::env::current_exe()
            .map_err(|error| format!("failed to resolve current executable: {error}"))?;
        let mut command = Command::new(executable);
        command
            .arg("webdriver")
            .arg("--port")
            .arg(port.to_string());
        if headless {
            command.arg("--headless");
        }
        if let Some(startup_url) = startup_url {
            command.arg("--startup-url").arg(startup_url);
        }
        let mut child = command
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("failed to start formal-web WebDriver child: {error}"))?;

        wait_for_webdriver_ready(port, &mut child, WEBDRIVER_STARTUP_TIMEOUT)?;
        Ok(Self { child, port })
    }

    fn wait_for_exit(&mut self, timeout: Duration) -> Result<String, String> {
        let status = wait_for_child(&mut self.child, timeout)?;
        let stderr = read_child_stderr(&mut self.child);
        if status.success() {
            Ok(stderr)
        } else {
            Err(child_failure_message(Some(&status), &stderr))
        }
    }
}

impl Drop for BrowserProcess {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

impl WebDriverSession {
    fn create(port: u16) -> Result<Self, String> {
        let response = webdriver_request(
            port,
            "POST",
            "/session",
            Some(&json!({ "capabilities": { "alwaysMatch": {} } })),
        )?;
        let session_id = response
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| String::from("WebDriver session response omitted sessionId"))?;
        Ok(Self {
            port,
            session_id: session_id.to_owned(),
        })
    }

    fn delete(&self) -> Result<(), String> {
        let path = format!("/session/{}", self.session_id);
        let _ = webdriver_request(self.port, "DELETE", &path, None)?;
        Ok(())
    }

    fn execute_script(&self, script: &str, args: &[Value]) -> Result<Value, String> {
        let path = format!("/session/{}/execute/sync", self.session_id);
        webdriver_request(
            self.port,
            "POST",
            &path,
            Some(&json!({
                "script": script,
                "args": args,
            })),
        )
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn normalize_rel_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn path_buf_to_string(path: &Path) -> String {
    normalize_rel_path(&path.to_string_lossy())
}

fn parse_bool(value: &str) -> Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!("expected `true` or `false`, got `{other}`")),
    }
}

fn path_is_html_file(path: &str) -> bool {
    matches!(
        Path::new(path).extension().and_then(|extension| extension.to_str()),
        Some("html" | "htm" | "xhtml" | "svg")
    )
}

fn parse_directory_expectation_file(path: &Path) -> Result<DirectoryExpectation, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let mut expectation = DirectoryExpectation::default();

    for raw_line in contents.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        match key.trim() {
            "disabled" => expectation.disabled = Some(value.trim().to_owned()),
            "skip" if parse_bool(value)? => {
                expectation.disabled = Some(String::from("disabled by __dir__.ini"))
            }
            _ => {}
        }
    }

    Ok(expectation)
}

fn parse_test_expectation_file(path: &Path, relative_test_path: &str) -> Result<TestExpectation, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let expected_section = Path::new(relative_test_path)
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| format!("failed to determine metadata section name for {relative_test_path}"))?;

    let mut expectation = TestExpectation::default();
    let mut sections = Vec::<String>::new();

    for raw_line in contents.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }

        let indent = raw_line
            .chars()
            .take_while(|character| character.is_ascii_whitespace())
            .count();
        let depth = indent / 2;

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            sections.truncate(depth);
            sections.push(trimmed[1..trimmed.len() - 1].to_owned());
            continue;
        }

        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();

        match sections.as_slice() {
            [section] if section == expected_section => match key {
                "disabled" => expectation.disabled = Some(value.to_owned()),
                "expected" => expectation.expected = WptStatus::parse_expected(value),
                _ => {}
            },
            [section, subtest] if section == expected_section => {
                if key == "expected" {
                    expectation
                        .subtests
                        .entry(subtest.clone())
                        .or_default()
                        .expected = WptStatus::parse_expected(value);
                }
            }
            _ => {}
        }
    }

    Ok(expectation)
}

fn collect_selected_tests(
    config: &RunnerConfig,
    selection: Option<&str>,
    wpt_include: &IncludeFilter,
    formal_include: &IncludeFilter,
) -> Result<Vec<SelectedTest>, String> {
    let mut selected = Vec::new();
    let mut seen = HashSet::new();

    match selection {
        Some(raw_path) => {
            let (suite, relative_path, absolute_path) = resolve_explicit_selection(raw_path, config)?;
            if absolute_path.is_file() {
                add_explicit_test(suite, &relative_path, &absolute_path, &mut seen, &mut selected)?;
            } else if absolute_path.is_dir() {
                collect_tests_under(suite, &absolute_path, &mut seen, &mut selected)?;
            } else {
                return Err(format!("{relative_path} is not a file or directory"));
            }
        }
        None => {
            collect_default_suite_tests(&config.wpt, wpt_include, &mut seen, &mut selected)?;
            collect_default_suite_tests(&config.formal, formal_include, &mut seen, &mut selected)?;
        }
    }

    selected.sort_by(|left, right| left.display_path.cmp(&right.display_path));
    if selected.is_empty() {
        return Err(String::from("no supported tests matched the requested selection"));
    }
    Ok(selected)
}

fn collect_default_suite_tests(
    suite: &SuiteDescriptor,
    include_filter: &IncludeFilter,
    seen: &mut HashSet<String>,
    selected: &mut Vec<SelectedTest>,
) -> Result<(), String> {
    if !suite.root.exists() {
        return Ok(());
    }

    let includes = include_filter.explicit_includes();
    if includes.is_empty() {
        if include_filter.root_skip {
            return Ok(());
        }
        collect_tests_under(suite, &suite.root, seen, selected)?;
        return Ok(());
    }

    for include in includes {
        let absolute_path = suite.root.join(&include);
        if absolute_path.is_file() {
            add_explicit_test(suite, &include, &absolute_path, seen, selected)?;
        } else if absolute_path.is_dir() {
            collect_tests_under(suite, &absolute_path, seen, selected)?;
        } else {
            return Err(format!(
                "{} selects `{include}`, but that path does not exist under {}",
                suite.include_path.display(),
                suite.root.display()
            ));
        }
    }

    Ok(())
}

fn resolve_explicit_selection<'a>(
    raw_path: &str,
    config: &'a RunnerConfig,
) -> Result<(&'a SuiteDescriptor, String, PathBuf), String> {
    let normalized = normalize_rel_path(raw_path.trim());
    if normalized.is_empty() {
        return Err(String::from("test path must not be empty"));
    }

    if let Some(relative) = normalized.strip_prefix("vendor/wpt/") {
        let relative = relative.to_owned();
        let absolute = config.wpt.root.join(&relative);
        if absolute.exists() {
            return Ok((&config.wpt, relative, absolute));
        }
    }

    if let Some(relative) = normalized.strip_prefix(DEFAULT_FORMAL_DISPLAY_PREFIX) {
        let relative = relative.to_owned();
        let absolute = config.formal.root.join(&relative);
        if absolute.exists() {
            return Ok((&config.formal, relative, absolute));
        }
    }

    if let Some(relative) = normalized.strip_prefix("tests/formal/tests/") {
        let relative = relative.to_owned();
        let absolute = config.formal.root.join(&relative);
        if absolute.exists() {
            return Ok((&config.formal, relative, absolute));
        }
    }

    let wpt_absolute = config.wpt.root.join(&normalized);
    let formal_absolute = config.formal.root.join(&normalized);

    match (wpt_absolute.exists(), formal_absolute.exists()) {
        (true, false) => Ok((&config.wpt, normalized.clone(), wpt_absolute)),
        (false, true) => Ok((&config.formal, normalized.clone(), formal_absolute)),
        (true, true) => Err(format!(
            "{normalized} matches both the WPT tree and the formal test tree; prefix the path with `vendor/wpt/` or `formal/`"
        )),
        (false, false) => Err(format!(
            "{normalized} does not exist under {} or {}",
            config.wpt.root.display(),
            config.formal.root.display()
        )),
    }
}

fn collect_tests_under(
    suite: &SuiteDescriptor,
    root: &Path,
    seen: &mut HashSet<String>,
    selected: &mut Vec<SelectedTest>,
) -> Result<(), String> {
    let entries = fs::read_dir(root)
        .map_err(|error| format!("failed to read {}: {error}", root.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("failed to read test directory entry: {error}"))?;
        let path = entry.path();
        if path.is_dir() {
            if should_skip_directory(&path) {
                continue;
            }
            collect_tests_under(suite, &path, seen, selected)?;
            continue;
        }

        let relative_path = path
            .strip_prefix(&suite.root)
            .map(path_buf_to_string)
            .map_err(|error| format!("failed to determine test-relative path: {error}"))?;

        if let ClassifiedTest::Supported(kind) = classify_test(suite.kind, &relative_path, &path)? {
            let display_path = format!("{}{}", suite.display_prefix, relative_path);
            if seen.insert(display_path.clone()) {
                selected.push(SelectedTest {
                    suite: suite.kind,
                    source_relative_path: relative_path.clone(),
                    display_path,
                    served_path: served_path_for_test(suite.url_prefix, &relative_path, kind),
                    kind,
                });
            }
        }
    }

    Ok(())
}

fn add_explicit_test(
    suite: &SuiteDescriptor,
    relative_path: &str,
    absolute_path: &Path,
    seen: &mut HashSet<String>,
    selected: &mut Vec<SelectedTest>,
) -> Result<(), String> {
    match classify_test(suite.kind, relative_path, absolute_path)? {
        ClassifiedTest::Supported(kind) => {
            let display_path = format!("{}{}", suite.display_prefix, relative_path);
            if seen.insert(display_path.clone()) {
                selected.push(SelectedTest {
                    suite: suite.kind,
                    source_relative_path: relative_path.to_owned(),
                    display_path,
                    served_path: served_path_for_test(suite.url_prefix, relative_path, kind),
                    kind,
                });
            }
            Ok(())
        }
        ClassifiedTest::Unsupported(reason) => {
            Err(format!("{}{} is not runnable yet: {reason}", suite.display_prefix, relative_path))
        }
        ClassifiedTest::Ignore => Err(format!(
            "{}{} is not a supported testharness test",
            suite.display_prefix, relative_path
        )),
    }
}

fn should_skip_directory(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| matches!(name, ".git" | ".github" | "resources" | "support" | "meta"))
}

fn classify_test(
    suite_kind: SuiteKind,
    relative_path: &str,
    absolute_path: &Path,
) -> Result<ClassifiedTest, String> {
    if relative_path.contains("/resources/") || relative_path.starts_with("resources/") {
        return Ok(ClassifiedTest::Ignore);
    }
    if relative_path.contains("/support/") || relative_path.starts_with("support/") {
        return Ok(ClassifiedTest::Ignore);
    }

    let file_name = Path::new(relative_path)
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or(relative_path);

    if is_manual_test_name(file_name) {
        return Ok(ClassifiedTest::Unsupported(String::from(
            "manual tests require human input",
        )));
    }

    if path_is_html_file(relative_path) {
        let source = match fs::read_to_string(absolute_path) {
            Ok(source) => source,
            Err(_) => return Ok(ClassifiedTest::Ignore),
        };
        if is_reftest_source(&source) {
            return Ok(ClassifiedTest::Unsupported(String::from(
                "reftests are not implemented yet",
            )));
        }
        if source.contains("testharness.js") {
            return Ok(ClassifiedTest::Supported(TestKind::Html));
        }
        if suite_kind == SuiteKind::Formal && source.contains("__formalWebTestResult") {
            return Ok(ClassifiedTest::Supported(TestKind::Html));
        }
        return Ok(ClassifiedTest::Ignore);
    }

    if is_window_script_path(file_name) {
        let source = match fs::read_to_string(absolute_path) {
            Ok(source) => source,
            Err(_) => return Ok(ClassifiedTest::Ignore),
        };
        if source.contains("importScripts(") {
            return Ok(ClassifiedTest::Unsupported(String::from(
                "worker JavaScript tests are not implemented yet",
            )));
        }
        if source.contains("testharness.js")
            || source.contains("promise_test(")
            || source.contains("async_test(")
            || source.contains("test(")
        {
            return Ok(ClassifiedTest::Supported(TestKind::WindowScript));
        }
        return Ok(ClassifiedTest::Ignore);
    }

    if is_any_script_path(file_name) {
        let source = match fs::read_to_string(absolute_path) {
            Ok(source) => source,
            Err(_) => return Ok(ClassifiedTest::Ignore),
        };
        if !any_script_supports_window(&source) {
            return Ok(ClassifiedTest::Unsupported(String::from(
                ".any.js tests without a window global are not implemented yet",
            )));
        }
        if source.contains("importScripts(") {
            return Ok(ClassifiedTest::Unsupported(String::from(
                "worker JavaScript tests are not implemented yet",
            )));
        }
        return Ok(ClassifiedTest::Supported(TestKind::AnyScript));
    }

    Ok(ClassifiedTest::Ignore)
}

fn is_manual_test_name(file_name: &str) -> bool {
    file_name.contains(".manual.") || file_name.contains("-manual.")
}

fn is_window_script_path(file_name: &str) -> bool {
    if !file_name.ends_with(".js") {
        return false;
    }

    file_name.contains(".window.")
        && !file_name.contains(".any.")
        && !file_name.contains(".worker.")
        && !file_name.contains(".sharedworker.")
        && !file_name.contains(".serviceworker.")
        && !file_name.contains(".worklet.")
}

fn is_any_script_path(file_name: &str) -> bool {
    file_name.ends_with(".any.js")
}

fn any_script_supports_window(source: &str) -> bool {
    let mut saw_global_meta = false;

    for line in source.lines() {
        let trimmed = line.trim();
        let Some(globals) = trimmed.strip_prefix("// META: global=") else {
            continue;
        };
        saw_global_meta = true;
        if globals
            .split(',')
            .map(str::trim)
            .any(|global| global == "window")
        {
            return true;
        }
    }

    !saw_global_meta
}

fn is_reftest_source(source: &str) -> bool {
    source.contains("rel=\"match\"")
        || source.contains("rel='match'")
        || source.contains("rel=match")
        || source.contains("rel=\"mismatch\"")
        || source.contains("rel='mismatch'")
        || source.contains("rel=mismatch")
}

fn served_path_for_test(url_prefix: &str, relative_path: &str, kind: TestKind) -> String {
    let relative = match kind {
        TestKind::Html => relative_path.to_owned(),
        TestKind::WindowScript => relative_path
            .strip_suffix(".window.js")
            .map(|prefix| format!("{prefix}.window.html"))
            .unwrap_or_else(|| relative_path.to_owned()),
        TestKind::AnyScript => relative_path
            .strip_suffix(".any.js")
            .map(|prefix| format!("{prefix}.any.html"))
            .unwrap_or_else(|| relative_path.to_owned()),
    };

    if url_prefix.is_empty() {
        relative
    } else {
        format!("{url_prefix}/{relative}")
    }
}

fn report_status_from_payload(report: &HarnessCompletionReport) -> WptStatus {
    let harness_status = WptStatus::from_harness_code(report.status.status).unwrap_or(WptStatus::Error);
    if harness_status != WptStatus::Pass {
        return harness_status;
    }

    let mut aggregate = WptStatus::Pass;
    for subtest in &report.tests {
        let status = WptStatus::from_test_code(subtest.status).unwrap_or(WptStatus::Error);
        aggregate = match status {
            WptStatus::Error | WptStatus::Crash => WptStatus::Error,
            WptStatus::Timeout if aggregate != WptStatus::Error => WptStatus::Timeout,
            WptStatus::Fail if !matches!(aggregate, WptStatus::Error | WptStatus::Timeout) => {
                WptStatus::Fail
            }
            WptStatus::PreconditionFailed if matches!(aggregate, WptStatus::Pass | WptStatus::NotRun) => {
                WptStatus::PreconditionFailed
            }
            WptStatus::NotRun if aggregate == WptStatus::Pass => WptStatus::NotRun,
            _ => aggregate,
        };
    }

    aggregate
}

fn compare_observed_result(
    observed: ObservedTestResult,
    expectation: Option<&TestExpectation>,
) -> ComparedTestResult {
    let expected = expectation
        .and_then(|entry| entry.expected)
        .unwrap_or(WptStatus::Pass);
    let mut unexpected = observed.actual != expected;
    let mut subtests = Vec::new();

    if let Some(report) = &observed.harness {
        for subtest in &report.tests {
            let actual = WptStatus::from_test_code(subtest.status).unwrap_or(WptStatus::Error);
            let expected = expectation
                .and_then(|entry| entry.subtests.get(&subtest.name))
                .and_then(|entry| entry.expected)
                .unwrap_or(WptStatus::Pass);
            let is_unexpected = actual != expected;
            unexpected |= is_unexpected;
            subtests.push(ComparedSubtestResult {
                name: subtest.name.clone(),
                actual,
                expected,
                unexpected: is_unexpected,
                message: subtest.message.clone(),
            });
        }
    }

    ComparedTestResult {
        path: observed.path,
        kind: observed.kind,
        actual: observed.actual,
        expected: Some(expected),
        unexpected,
        skipped: false,
        reason: None,
        message: observed.message,
        duration_ms: observed.duration_ms,
        subtests,
    }
}

fn skipped_result(path: &str, kind: TestKind, reason: String) -> ComparedTestResult {
    ComparedTestResult {
        path: path.to_owned(),
        kind,
        actual: WptStatus::Skip,
        expected: None,
        unexpected: false,
        skipped: true,
        reason: Some(reason),
        message: None,
        duration_ms: 0,
        subtests: Vec::new(),
    }
}

fn update_summary(summary: &mut RunSummary, result: &ComparedTestResult) {
    summary.total += 1;
    if result.skipped {
        summary.skipped += 1;
        return;
    }

    summary.executed += 1;
    if result.unexpected {
        summary.unexpected += 1;
    }

    match result.actual {
        WptStatus::Pass => summary.passed += 1,
        WptStatus::Fail | WptStatus::PreconditionFailed | WptStatus::NotRun => summary.failed += 1,
        WptStatus::Timeout => summary.timed_out += 1,
        WptStatus::Error => summary.errors += 1,
        WptStatus::Crash => summary.crashes += 1,
        WptStatus::Skip => {}
    }
}

fn write_run_report(path: &Path, report: &RunReport) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(report)
        .map_err(|error| format!("failed to encode report JSON: {error}"))?;
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    fs::write(path, bytes).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn print_test_result(result: &ComparedTestResult) {
    if result.skipped {
        let reason = result.reason.as_deref().unwrap_or("disabled");
        println!("SKIP   {} ({reason})", result.path);
        return;
    }

    match result.expected {
        Some(expected) if result.unexpected || expected != WptStatus::Pass => {
            println!(
                "{:<6} {} (expected {})",
                result.actual.as_str(),
                result.path,
                expected.as_str()
            );
        }
        _ => println!("{:<6} {}", result.actual.as_str(), result.path),
    }

    if let Some(message) = result.message.as_deref() {
        if result.unexpected || matches!(result.actual, WptStatus::Error | WptStatus::Crash) {
            println!("  message: {message}");
        }
    }

    for subtest in &result.subtests {
        if !subtest.unexpected {
            continue;
        }
        println!(
            "  {:<6} {} (expected {})",
            subtest.actual.as_str(),
            subtest.name,
            subtest.expected.as_str()
        );
    }
}

fn print_summary(summary: &RunSummary) {
    println!(
        "Summary: total={} executed={} skipped={} unexpected={} pass={} fail={} timeout={} error={} crash={}",
        summary.total,
        summary.executed,
        summary.skipped,
        summary.unexpected,
        summary.passed,
        summary.failed,
        summary.timed_out,
        summary.errors,
        summary.crashes
    );
}

fn run_single_test(
    test: &SelectedTest,
    server: &WptServeProcess,
    timeout: Duration,
    headless: bool,
) -> ObservedTestResult {
    let started = Instant::now();
    let port = match pick_unused_port() {
        Ok(port) => port,
        Err(error) => return crash_result(test, error, started.elapsed().as_millis()),
    };

    let test_url = format!("{}/{}", server.base_url(), test.served_path);

    let mut browser = match BrowserProcess::start(port, Some(&test_url), headless) {
        Ok(browser) => browser,
        Err(error) => return crash_result(test, error, started.elapsed().as_millis()),
    };

    let session = match WebDriverSession::create(browser.port) {
        Ok(session) => session,
        Err(error) => return crash_result(test, error, started.elapsed().as_millis()),
    };

    let mut observed = wait_for_test_report(&session, test, timeout, started);

    let mut cleanup_errors = Vec::new();
    if let Err(error) = session.delete() {
        cleanup_errors.push(format!("failed to delete WebDriver session: {error}"));
    }
    match browser.wait_for_exit(WEBDRIVER_SHUTDOWN_TIMEOUT) {
        Ok(stderr) => {
            if !stderr.is_empty() && observed.actual != WptStatus::Pass {
                cleanup_errors.push(format!("browser stderr: {stderr}"));
            }
        }
        Err(error) => cleanup_errors.push(error),
    }
    if !cleanup_errors.is_empty() {
        if observed.actual == WptStatus::Pass {
            observed.actual = WptStatus::Crash;
            observed.harness = None;
        }
        observed.message = Some(match observed.message.take() {
            Some(message) => format!("{message}; {}", cleanup_errors.join("; ")),
            None => cleanup_errors.join("; "),
        });
    }

    observed
}

fn wait_for_test_report(
    session: &WebDriverSession,
    test: &SelectedTest,
    timeout: Duration,
    started: Instant,
) -> ObservedTestResult {
    let deadline = Instant::now() + timeout;
    loop {
        match session.execute_script(
            r##"return (function () {
                function parseLeadingNumber(text) {
                    var match = String(text || "").match(/(\d+)/);
                    return match ? Number(match[1]) : 0;
                }

                function countFor(summary, labelName) {
                    var labels = document.querySelectorAll("#summary label");
                    for (var i = 0; i < labels.length; i += 1) {
                        var text = String(labels[i].textContent || "");
                        if (text.toLowerCase().indexOf(labelName) !== -1) {
                            return parseLeadingNumber(text);
                        }
                    }
                    return 0;
                }

                var summary = document.getElementById("summary");
                return {
                    result: window.__formalWebTestResult || null,
                    summary: summary ? {
                        harnessStatus: (function () {
                            var status = document.querySelector("#summary p span");
                            return status ? String(status.textContent || "") : null;
                        }()),
                        pass: countFor(summary, "pass"),
                        fail: countFor(summary, "fail"),
                        timeout: countFor(summary, "timeout"),
                        notRun: countFor(summary, "not run"),
                        preconditionFailed: countFor(summary, "precondition failed")
                    } : null
                };
            }());"##,
            &[],
        ) {
            Ok(value) => {
                let result_value = value.get("result").cloned().unwrap_or(Value::Null);
                if result_value != Value::Null {
                    let duration_ms = started.elapsed().as_millis();
                    return match serde_json::from_value::<HarnessCompletionReport>(result_value) {
                        Ok(report) => ObservedTestResult {
                            path: test.display_path.clone(),
                            kind: test.kind,
                            actual: report_status_from_payload(&report),
                            message: report.status.message.clone(),
                            harness: Some(report),
                            duration_ms,
                        },
                        Err(error) => error_result(
                            test,
                            format!("failed to decode harness report: {error}"),
                            duration_ms,
                        ),
                    };
                }

                let Some(summary_value) = value.get("summary").cloned() else {
                    // Keep polling until either the injected completion callback or the DOM summary appears.
                    // The summary is produced by testharnessreport.js once the page is complete.
                    if Instant::now() >= deadline {
                        return timeout_result(
                            test,
                            format!(
                                "test did not report completion within {} ms",
                                timeout.as_millis()
                            ),
                            started.elapsed().as_millis(),
                        );
                    }
                    thread::sleep(REPORT_POLL_INTERVAL);
                    continue;
                };

                if summary_value != Value::Null {
                    if let Ok(summary) = serde_json::from_value::<LiveHarnessSummary>(summary_value) {
                        if summary.harness_status.is_some() {
                            return observed_result_from_summary(test, summary, started.elapsed().as_millis());
                        }
                    }
                }
            }
            Err(error) => {
                return error_result(test, error, started.elapsed().as_millis());
            }
        }

        if Instant::now() >= deadline {
            return timeout_result(
                test,
                format!("test did not report completion within {} ms", timeout.as_millis()),
                started.elapsed().as_millis(),
            );
        }

        thread::sleep(REPORT_POLL_INTERVAL);
    }
}

fn observed_result_from_summary(
    test: &SelectedTest,
    summary: LiveHarnessSummary,
    duration_ms: u128,
) -> ObservedTestResult {
    let harness_status = summary.harness_status.as_deref().unwrap_or("");
    let actual = if summary.timeout > 0.0 {
        WptStatus::Timeout
    } else if summary.fail > 0.0 {
        WptStatus::Fail
    } else if summary.precondition_failed > 0.0 && summary.pass == 0.0 {
        WptStatus::PreconditionFailed
    } else if summary.not_run > 0.0 && summary.pass == 0.0 {
        WptStatus::NotRun
    } else if harness_status.eq_ignore_ascii_case("OK") {
        WptStatus::Pass
    } else if harness_status.eq_ignore_ascii_case("TIMEOUT") {
        WptStatus::Timeout
    } else if harness_status.eq_ignore_ascii_case("PRECONDITION_FAILED")
        || harness_status.eq_ignore_ascii_case("PRECONDITION FAILED")
    {
        WptStatus::PreconditionFailed
    } else {
        WptStatus::Error
    };

    let message = if actual == WptStatus::Pass {
        None
    } else {
        Some(format!(
            "rendered summary reported harness status `{}` with pass={}, fail={}, timeout={}, notRun={}, preconditionFailed={}",
            harness_status,
            summary.pass,
            summary.fail,
            summary.timeout,
            summary.not_run,
            summary.precondition_failed,
        ))
    };

    ObservedTestResult {
        path: test.display_path.clone(),
        kind: test.kind,
        actual,
        message,
        harness: None,
        duration_ms,
    }
}

fn crash_result(test: &SelectedTest, message: String, duration_ms: u128) -> ObservedTestResult {
    ObservedTestResult {
        path: test.display_path.clone(),
        kind: test.kind,
        actual: WptStatus::Crash,
        message: Some(message),
        harness: None,
        duration_ms,
    }
}

fn error_result(test: &SelectedTest, message: String, duration_ms: u128) -> ObservedTestResult {
    ObservedTestResult {
        path: test.display_path.clone(),
        kind: test.kind,
        actual: WptStatus::Error,
        message: Some(message),
        harness: None,
        duration_ms,
    }
}

fn timeout_result(test: &SelectedTest, message: String, duration_ms: u128) -> ObservedTestResult {
    ObservedTestResult {
        path: test.display_path.clone(),
        kind: test.kind,
        actual: WptStatus::Timeout,
        message: Some(message),
        harness: None,
        duration_ms,
    }
}

fn temp_dir_path(prefix: &str) -> PathBuf {
    let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    repo_root()
        .join(RUNNER_ARTIFACT_ROOT)
        .join("runtime")
        .join(format!("{prefix}-{}-{id}", std::process::id()))
}

fn configure_wptserve_command(command: &mut Command) {
    #[cfg(unix)]
    {
        command.process_group(0);
    }
}

fn shutdown_wptserve_child(child: &mut Child) -> Result<(), String> {
    if child.try_wait().ok().flatten().is_some() {
        return Ok(());
    }

    request_wptserve_shutdown(child)?;
    if wait_for_child_exit(child, WPTSERVE_SHUTDOWN_TIMEOUT)?.is_some() {
        return Ok(());
    }

    force_kill_wptserve(child)?;
    let _ = wait_for_child_exit(child, Duration::from_secs(1))?;
    Ok(())
}

fn wait_for_child_exit(
    child: &mut Child,
    timeout: Duration,
) -> Result<Option<ExitStatus>, String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("failed to wait for child process: {error}"))?
        {
            return Ok(Some(status));
        }

        if Instant::now() >= deadline {
            return Ok(None);
        }

        thread::sleep(Duration::from_millis(100));
    }
}

fn request_wptserve_shutdown(child: &mut Child) -> Result<(), String> {
    #[cfg(unix)]
    {
        return send_signal_to_wptserve_group(child.id(), libc::SIGINT);
    }

    #[cfg(not(unix))]
    {
        if child.try_wait().ok().flatten().is_none() {
            child
                .kill()
                .map_err(|error| format!("failed to stop wptserve child: {error}"))?;
        }
        Ok(())
    }
}

fn force_kill_wptserve(child: &mut Child) -> Result<(), String> {
    #[cfg(unix)]
    {
        if send_signal_to_wptserve_group(child.id(), libc::SIGKILL).is_ok() {
            return Ok(());
        }
    }

    if child.try_wait().ok().flatten().is_none() {
        child
            .kill()
            .map_err(|error| format!("failed to kill wptserve child: {error}"))?;
    }
    Ok(())
}

#[cfg(unix)]
fn send_signal_to_wptserve_group(pid: u32, signal: libc::c_int) -> Result<(), String> {
    let process_group = -(pid as libc::pid_t);
    let result = unsafe { libc::kill(process_group, signal) };
    if result == 0 {
        return Ok(());
    }

    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        return Ok(());
    }

    Err(format!(
        "failed to signal wptserve process group {pid} with signal {signal}: {error}"
    ))
}

fn pick_unused_port() -> Result<u16, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| format!("failed to allocate local port: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("failed to read allocated local port: {error}"))?
        .port();
    drop(listener);
    Ok(port)
}

fn wait_for_wptserve_ready(port: u16, child: &mut Child, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("failed to wait for wptserve: {error}"))?
        {
            let stderr = read_child_stderr(child);
            return Err(child_failure_message(Some(&status), &stderr));
        }

        if http_request_raw(port, "GET", "/resources/testharness.js", None, None)
            .map(|response| response.status == 200)
            .unwrap_or(false)
        {
            return Ok(());
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let stderr = read_child_stderr(child);
            return Err(format!(
                "wptserve did not become ready within {} ms{}",
                timeout.as_millis(),
                if stderr.is_empty() {
                    String::new()
                } else {
                    format!(": {stderr}")
                }
            ));
        }

        thread::sleep(HTTP_POLL_INTERVAL);
    }
}

fn wait_for_webdriver_ready(port: u16, child: &mut Child, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("failed to wait for WebDriver child: {error}"))?
        {
            let stderr = read_child_stderr(child);
            return Err(child_failure_message(Some(&status), &stderr));
        }

        if let Ok(value) = webdriver_request(port, "GET", "/status", None) {
            if value.get("ready").and_then(Value::as_bool).unwrap_or(false) {
                return Ok(());
            }
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let stderr = read_child_stderr(child);
            return Err(format!(
                "WebDriver child did not become ready within {} ms{}",
                timeout.as_millis(),
                if stderr.is_empty() {
                    String::new()
                } else {
                    format!(": {stderr}")
                }
            ));
        }

        thread::sleep(HTTP_POLL_INTERVAL);
    }
}

fn wait_for_child(child: &mut Child, timeout: Duration) -> Result<ExitStatus, String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("failed to wait for child process: {error}"))?
        {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "child process did not exit within {} ms",
                timeout.as_millis()
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn read_child_stderr(child: &mut Child) -> String {
    let mut stderr = String::new();
    if let Some(mut handle) = child.stderr.take() {
        let _ = handle.read_to_string(&mut stderr);
    }
    if stderr.len() > CHILD_STDERR_LIMIT {
        stderr.truncate(CHILD_STDERR_LIMIT);
    }
    stderr.trim().to_owned()
}

fn child_failure_message(status: Option<&ExitStatus>, stderr: &str) -> String {
    let mut message = match status {
        Some(status) => format!("child exited with status {status}"),
        None => String::from("child process failed"),
    };
    if !stderr.is_empty() {
        message.push_str(": ");
        message.push_str(stderr);
    }
    message
}

fn webdriver_request(port: u16, method: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
    let body_bytes = body
        .map(|value| serde_json::to_vec(value).map_err(|error| format!("failed to encode WebDriver request JSON: {error}")))
        .transpose()?;
    let response = http_request_raw(
        port,
        method,
        path,
        body_bytes.as_deref(),
        body.is_some().then_some("application/json; charset=utf-8"),
    )?;
    let value: Value = serde_json::from_slice(&response.body)
        .map_err(|error| format!("failed to decode WebDriver response JSON: {error}"))?;
    if response.status >= 400 {
        let message = value
            .get("value")
            .and_then(|inner| inner.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("WebDriver request failed");
        return Err(message.to_owned());
    }
    value
        .get("value")
        .cloned()
        .ok_or_else(|| String::from("WebDriver response omitted `value`"))
}

fn http_request_raw(
    port: u16,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
    content_type: Option<&str>,
) -> Result<HttpResponse, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .map_err(|error| format!("failed to connect to localhost:{port}: {error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|error| format!("failed to set read timeout: {error}"))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|error| format!("failed to set write timeout: {error}"))?;

    let body = body.unwrap_or(&[]);
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    if let Some(content_type) = content_type {
        request.push_str(&format!("Content-Type: {content_type}\r\n"));
    }
    request.push_str("\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("failed to write HTTP request: {error}"))?;
    if !body.is_empty() {
        stream
            .write_all(body)
            .map_err(|error| format!("failed to write HTTP request body: {error}"))?;
    }
    stream
        .flush()
        .map_err(|error| format!("failed to flush HTTP request: {error}"))?;

    let mut bytes = Vec::new();
    stream
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to read HTTP response: {error}"))?;

    parse_http_response(&bytes)
}

fn parse_http_response(bytes: &[u8]) -> Result<HttpResponse, String> {
    let header_end = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
        .ok_or_else(|| String::from("HTTP response was missing a header terminator"))?;
    let header_text = String::from_utf8_lossy(&bytes[..header_end]);
    let mut lines = header_text.lines();
    let status_line = lines
        .next()
        .ok_or_else(|| String::from("HTTP response was missing a status line"))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| format!("invalid HTTP status line: {status_line}"))?
        .parse::<u16>()
        .map_err(|error| format!("invalid HTTP status code: {error}"))?;
    Ok(HttpResponse {
        status,
        body: bytes[header_end..].to_vec(),
    })
}

fn reporter_inject_script() -> &'static str {
    r#"(function () {
  if (!document.currentScript) {
    document.currentScript = { remove: function () {} };
  }
}());
"#
}

#[cfg(test)]
mod tests {
    use super::{
        RunReport, RunSummary, TestKind, WptStatus, any_script_supports_window,
        parse_test_expectation_file, reporter_inject_script, served_path_for_test,
        temp_dir_path, write_run_report,
    };
    use std::{fs, path::Path};

    #[test]
    fn reporter_script_records_completion_result() {
        let script = reporter_inject_script();
        assert!(script.contains("document.currentScript"));
        assert!(script.contains("remove: function () {}"));
    }

    #[test]
    fn served_window_script_uses_generated_html_path() {
        assert_eq!(
            served_path_for_test("", "dom/example.window.js", TestKind::WindowScript),
            String::from("dom/example.window.html")
        );
        assert_eq!(
            served_path_for_test("__formal__", "load.window.js", TestKind::WindowScript),
            String::from("__formal__/load.window.html")
        );
    }

    #[test]
    fn served_any_script_uses_generated_html_path() {
        assert_eq!(
            served_path_for_test("", "dom/example.any.js", TestKind::AnyScript),
            String::from("dom/example.any.html")
        );
        assert_eq!(
            served_path_for_test("__formal__", "load.https.any.js", TestKind::AnyScript),
            String::from("__formal__/load.https.any.html")
        );
    }

    #[test]
    fn any_script_global_meta_controls_window_support() {
        assert!(any_script_supports_window("test(() => {});"));
        assert!(any_script_supports_window(
            "// META: global=window,dedicatedworker\ntest(() => {});"
        ));
        assert!(!any_script_supports_window(
            "// META: global=dedicatedworker,shadowrealm\ntest(() => {});"
        ));
    }

    #[test]
    fn parses_test_expectation_file() {
        let path = std::env::temp_dir().join("formal-web-wpt-meta-test.ini");
        fs::write(
            &path,
            "[example.html]\n  expected: FAIL\n\n  [subtest]\n    expected: TIMEOUT\n",
        )
        .unwrap();

        let expectation = parse_test_expectation_file(&path, "dom/example.html").unwrap();
        fs::remove_file(path).unwrap();

        assert_eq!(expectation.expected, Some(WptStatus::Fail));
        assert_eq!(
            expectation.subtests.get("subtest").and_then(|entry| entry.expected),
            Some(WptStatus::Timeout)
        );
    }

    #[test]
    fn runner_artifacts_live_under_scratchpad() {
        let path = temp_dir_path("wptserve");
        let relative = path.strip_prefix(super::repo_root()).unwrap();
        assert!(relative.starts_with(Path::new("scratchpad/wpt-runner/runtime")));
    }

    #[test]
    fn write_run_report_creates_parent_directories() {
        let base_dir = temp_dir_path("report-test");
        let report_path = base_dir.join("reports/result.json");
        let report = RunReport {
            summary: RunSummary::default(),
            tests: Vec::new(),
        };

        write_run_report(&report_path, &report).unwrap();
        assert!(report_path.exists());

        fs::remove_dir_all(base_dir).unwrap();
    }
}