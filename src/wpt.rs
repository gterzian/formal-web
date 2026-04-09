use clap::Args;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_CONFIG_PATH: &str = "tests/wpt/config.ini";
const DEFAULT_INCLUDE_PATH: &str = "tests/wpt/include.ini";
const DEFAULT_RUNNER_MANIFEST: &str = "vendor/blitz/wpt/runner/Cargo.toml";

#[derive(Args, Debug)]
pub struct TestWptArgs {
    #[arg(value_name = "PATH")]
    paths: Vec<String>,

    #[arg(long, default_value = DEFAULT_CONFIG_PATH)]
    config: PathBuf,

    #[arg(long)]
    include: Option<PathBuf>,

    #[arg(long)]
    meta: Option<PathBuf>,

    #[arg(long, default_value = DEFAULT_RUNNER_MANIFEST)]
    runner_manifest: PathBuf,

    #[arg(long)]
    list: bool,

    #[arg(long)]
    release_runner: bool,
}

#[derive(Debug)]
struct WptConfig {
    tests_dir: PathBuf,
    meta_dir: PathBuf,
    include_path: PathBuf,
}

#[derive(Debug, Default)]
struct IncludeRules {
    default_skip: bool,
    explicit_skip: BTreeMap<String, bool>,
}

#[derive(Debug, Default)]
struct ExpectedOutcome {
    test: Option<ExpectationStatus>,
    subtests: BTreeMap<String, ExpectationStatus>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExpectationStatus {
    Pass,
    Fail,
    Skip,
    Crash,
}

#[derive(Debug, Deserialize)]
struct WptReport {
    results: Vec<WptTestResult>,
}

#[derive(Debug, Deserialize)]
struct WptTestResult {
    test: String,
    status: String,
    #[serde(default)]
    subtests: Vec<WptSubtestResult>,
}

#[derive(Debug, Deserialize)]
struct WptSubtestResult {
    name: String,
    status: String,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn normalize_rel_path(value: &str) -> String {
    value.trim().trim_matches('/').replace('\\', "/")
}

fn path_is_ancestor(ancestor: &str, candidate: &str) -> bool {
    ancestor == candidate
        || candidate
            .strip_prefix(ancestor)
            .is_some_and(|rest| rest.starts_with('/'))
}

impl WptConfig {
    fn load(config_path: &Path) -> Result<Self, String> {
        let config_path = if config_path.is_absolute() {
            config_path.to_path_buf()
        } else {
            repo_root().join(config_path)
        };
        let config_dir = config_path
            .parent()
            .ok_or_else(|| format!("config path has no parent: {}", config_path.display()))?;
        let contents = fs::read_to_string(&config_path)
            .map_err(|error| format!("failed to read {}: {error}", config_path.display()))?;

        let mut current_section = String::new();
        let mut tests_dir = None;
        let mut meta_dir = None;

        for raw_line in contents.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                current_section = line[1..line.len() - 1].trim().to_string();
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            if current_section == "manifest:upstream" && key == "tests" {
                tests_dir = Some(config_dir.join(value));
            }
            if current_section == "manifest:upstream" && key == "metadata" {
                meta_dir = Some(config_dir.join(value));
            }
        }

        let tests_dir = tests_dir.ok_or_else(|| {
            format!(
                "{} is missing [manifest:upstream] tests = ...",
                config_path.display()
            )
        })?;
        let meta_dir = meta_dir.ok_or_else(|| {
            format!(
                "{} is missing [manifest:upstream] metadata = ...",
                config_path.display()
            )
        })?;

        Ok(Self {
            tests_dir,
            meta_dir,
            include_path: config_dir.join(DEFAULT_INCLUDE_PATH.rsplit('/').next().unwrap()),
        })
    }
}

impl IncludeRules {
    fn load(path: &Path) -> Result<Self, String> {
        let contents = fs::read_to_string(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let mut rules = Self::default();
        let mut sections: Vec<String> = Vec::new();

        for raw_line in contents.lines() {
            let trimmed = raw_line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
                continue;
            }

            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                let indent = raw_line.chars().take_while(|char| *char == ' ').count();
                if indent % 2 != 0 {
                    return Err(format!(
                        "{} has a non-two-space indentation level in line: {trimmed}",
                        path.display()
                    ));
                }
                let depth = indent / 2;
                if depth > sections.len() {
                    return Err(format!(
                        "{} skips a nesting level before section {trimmed}",
                        path.display()
                    ));
                }
                sections.truncate(depth);
                sections.push(trimmed[1..trimmed.len() - 1].trim().to_string());
                continue;
            }

            let Some((key, value)) = trimmed.split_once(':') else {
                continue;
            };
            if key.trim() != "skip" {
                continue;
            }
            let skip = match value.trim() {
                "true" => true,
                "false" => false,
                other => {
                    return Err(format!(
                        "{} has an invalid skip value `{other}`",
                        path.display()
                    ));
                }
            };

            if sections.is_empty() {
                rules.default_skip = skip;
            } else {
                rules
                    .explicit_skip
                    .insert(sections.join("/"), skip);
            }
        }

        Ok(rules)
    }

    fn enabled_paths(&self) -> Result<Vec<String>, String> {
        if !self.default_skip {
            return Err(String::from(
                "the simplified WPT wrapper expects `skip: true` at the root and explicit `skip: false` opt-ins",
            ));
        }

        let mut enabled = self
            .explicit_skip
            .iter()
            .filter_map(|(path, skip)| (!*skip).then_some(path.clone()))
            .collect::<Vec<_>>();
        enabled.sort();

        let unsupported_exclusions = self
            .explicit_skip
            .iter()
            .filter_map(|(path, skip)| {
                if *skip
                    && enabled
                        .iter()
                        .any(|enabled_path| path_is_ancestor(enabled_path, path))
                {
                    Some(path.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        if !unsupported_exclusions.is_empty() {
            return Err(format!(
                "nested `skip: true` exclusions are not supported by the simplified wrapper because the vendored Blitz runner only accepts positive path filters: {}",
                unsupported_exclusions.join(", ")
            ));
        }

        let enabled_snapshot = enabled.clone();
        enabled.retain(|candidate| {
            !enabled_snapshot
                .iter()
                .any(|other| other != candidate && path_is_ancestor(other, candidate))
        });

        Ok(enabled)
    }
}

impl ExpectedOutcome {
    fn load(meta_dir: &Path, test_path: &str) -> Result<Self, String> {
        let meta_path = meta_dir.join(format!("{test_path}.ini"));
        if !meta_path.exists() {
            return Ok(Self::default());
        }

        let contents = fs::read_to_string(&meta_path)
            .map_err(|error| format!("failed to read {}: {error}", meta_path.display()))?;
        let mut result = Self::default();
        let mut current_test = None::<String>;
        let mut current_subtest = None::<String>;
        let file_name = Path::new(test_path)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("invalid WPT test path: {test_path}"))?
            .to_string();

        for raw_line in contents.lines() {
            let trimmed = raw_line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
                continue;
            }

            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                let indent = raw_line.chars().take_while(|char| *char == ' ').count();
                if indent == 0 {
                    current_test = Some(trimmed[1..trimmed.len() - 1].trim().to_string());
                    current_subtest = None;
                } else {
                    current_subtest = Some(trimmed[1..trimmed.len() - 1].trim().to_string());
                }
                continue;
            }

            let Some((key, value)) = trimmed.split_once(':') else {
                continue;
            };
            if key.trim() != "expected" {
                continue;
            }

            let Some(status) = ExpectationStatus::parse(value.trim()) else {
                continue;
            };

            if current_test.as_deref() != Some(file_name.as_str()) {
                continue;
            }

            if let Some(subtest_name) = &current_subtest {
                result.subtests.insert(subtest_name.clone(), status);
            } else {
                result.test = Some(status);
            }
        }

        Ok(result)
    }
}

impl ExpectationStatus {
    fn parse(raw: &str) -> Option<Self> {
        raw.split(|char: char| !char.is_ascii_alphabetic())
            .find_map(|token| match token {
                "PASS" => Some(Self::Pass),
                "FAIL" | "ERROR" | "TIMEOUT" => Some(Self::Fail),
                "SKIP" => Some(Self::Skip),
                "CRASH" => Some(Self::Crash),
                _ => None,
            })
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Skip => "SKIP",
            Self::Crash => "CRASH",
        }
    }
}

fn read_report(report_path: &Path) -> Result<WptReport, String> {
    let contents = fs::read_to_string(report_path)
        .map_err(|error| format!("failed to read {}: {error}", report_path.display()))?;
    serde_json::from_str(&contents)
        .map_err(|error| format!("failed to parse {}: {error}", report_path.display()))
}

fn compare_expectations(report: &WptReport, meta_dir: &Path) -> Result<Vec<String>, String> {
    let mut unexpected = Vec::new();

    for test in &report.results {
        let actual_test_status = ExpectationStatus::parse(&test.status).ok_or_else(|| {
            format!("unsupported WPT status `{}` for {}", test.status, test.test)
        })?;
        let expected = ExpectedOutcome::load(meta_dir, &test.test)?;
        let expected_test_status = expected.test.unwrap_or(ExpectationStatus::Pass);

        if actual_test_status != expected_test_status {
            unexpected.push(format!(
                "{} expected {} but got {}",
                test.test,
                expected_test_status.as_str(),
                actual_test_status.as_str()
            ));
        }

        for subtest in &test.subtests {
            let actual_subtest_status = ExpectationStatus::parse(&subtest.status).ok_or_else(|| {
                format!(
                    "unsupported WPT subtest status `{}` for {} / {}",
                    subtest.status, test.test, subtest.name
                )
            })?;
            let expected_subtest_status = expected
                .subtests
                .get(&subtest.name)
                .copied()
                .unwrap_or(ExpectationStatus::Pass);
            if actual_subtest_status != expected_subtest_status {
                unexpected.push(format!(
                    "{} / {} expected {} but got {}",
                    test.test,
                    subtest.name,
                    expected_subtest_status.as_str(),
                    actual_subtest_status.as_str()
                ));
            }
        }
    }

    Ok(unexpected)
}

pub fn run(args: TestWptArgs) -> Result<(), String> {
    let repo_root = repo_root();
    let config = WptConfig::load(&args.config)?;
    let include_path = args
        .include
        .map(|path| repo_root.join(path))
        .unwrap_or(config.include_path.clone());
    let meta_dir = args
        .meta
        .map(|path| repo_root.join(path))
        .unwrap_or(config.meta_dir.clone());
    let runner_manifest = if args.runner_manifest.is_absolute() {
        args.runner_manifest.clone()
    } else {
        repo_root.join(&args.runner_manifest)
    };

    let selected_paths = if args.paths.is_empty() {
        let rules = IncludeRules::load(&include_path)?;
        rules.enabled_paths()?
    } else {
        args.paths
            .iter()
            .map(|path| normalize_rel_path(path))
            .collect::<Vec<_>>()
    };

    if selected_paths.is_empty() {
        return Err(format!(
            "no WPT paths selected; pass explicit paths or set `skip: false` branches in {}",
            include_path.display()
        ));
    }

    println!("WPT root: {}", config.tests_dir.display());
    println!("Selection source: {}", include_path.display());
    println!("Expectations: {}", meta_dir.display());
    for path in &selected_paths {
        println!("  {path}");
    }

    if args.list {
        return Ok(());
    }

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| String::from("cargo"));
    let mut command = Command::new(cargo);
    command
        .arg("run")
        .arg("--manifest-path")
        .arg(&runner_manifest)
        .current_dir(&repo_root)
        .env("WPT_DIR", &config.tests_dir);
    if args.release_runner {
        command.arg("--release");
    }
    command.arg("--");
    command.args(&selected_paths);

    let status = command
        .status()
        .map_err(|error| format!("failed to run the vendored Blitz WPT runner: {error}"))?;

    let runner_output_dir = runner_manifest
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| format!("invalid runner manifest path: {}", runner_manifest.display()))?
        .join("output");
    let report_path = runner_output_dir.join("wptreport.json");
    if !report_path.exists() {
        return Err(format!(
            "the vendored Blitz WPT runner did not produce {} (runner status: {status})",
            report_path.display()
        ));
    }

    let report = read_report(&report_path)?;
    let unexpected = compare_expectations(&report, &meta_dir)?;

    if unexpected.is_empty() {
        if !status.success() {
            println!(
                "runner exited with {status}, but the generated report matched the configured expectations"
            );
        }
        println!("all observed WPT results matched the configured expectations");
        return Ok(());
    }

    eprintln!("unexpected WPT results:");
    for entry in &unexpected {
        eprintln!("  {entry}");
    }

    Err(format!(
        "{} WPT result(s) did not match {}",
        unexpected.len(),
        meta_dir.display()
    ))
}
