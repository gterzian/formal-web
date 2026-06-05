use std::ffi::OsStr;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{TraceMonitor, TraceSender, ValidationOptions, validate_and_print};

const DEFAULT_TLA2TOOLS_JAR: &str = "/Applications/TLA+ Toolbox.app/Contents/Eclipse/tla2tools.jar";
const DEFAULT_TLC_WORKERS: usize = 8;
const TLA2TOOLS_JAR_ENV: &str = "FORMAL_WEB_TLA2TOOLS_JAR";
const TLC_WORKERS_ENV: &str = "FORMAL_WEB_TLC_WORKERS";
const VERIFICATION_TEMP_ROOT_NAME: &str = "formal-web-verification";

pub struct VerificationRun {
    repo_root: PathBuf,
    session_dir: PathBuf,
    log_dir: PathBuf,
    monitor: TraceMonitor,
    specs_dir: PathBuf,
    tla2tools: PathBuf,
    tlc_workers: usize,
}

impl VerificationRun {
    pub fn start() -> Result<Self, String> {
        let repo_root = std::env::current_dir()
            .map_err(|error| format!("failed to resolve repository root: {error}"))?;
        cleanup_repo_verification_artifacts(&repo_root)?;
        cleanup_temp_verification_artifacts()?;
        let session_dir = create_session_dir()?;
        let log_dir = session_dir.join("logs");
        let monitor = TraceMonitor::start(&log_dir)?;
        Ok(Self {
            repo_root: repo_root.clone(),
            session_dir,
            log_dir,
            monitor,
            specs_dir: repo_root.join("verification").join("tla_specs"),
            tla2tools: std::env::var_os(TLA2TOOLS_JAR_ENV)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_TLA2TOOLS_JAR)),
            tlc_workers: std::env::var(TLC_WORKERS_ENV)
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|value| *value > 0)
                .unwrap_or(DEFAULT_TLC_WORKERS),
        })
    }

    pub fn sender_clone(&self) -> TraceSender {
        self.monitor.sender_clone()
    }

    pub fn finish(self) -> Result<(), String> {
        let Self {
            repo_root,
            session_dir,
            log_dir,
            monitor,
            specs_dir,
            tla2tools,
            tlc_workers,
        } = self;

        let mut errors = Vec::new();
        if let Err(error) = monitor.shutdown() {
            errors.push(error);
        }

        match validate_and_print(&ValidationOptions {
            logs: log_dir,
            specs: specs_dir,
            tla2tools,
            tlc_workers,
            only: None,
            json: false,
            workspace_root: Some(session_dir.join("validation")),
        }) {
            Ok(true) => {}
            Ok(false) => errors.push(String::from("verification failed")),
            Err(error) => errors.push(error),
        }

        if let Err(error) = remove_dir_all_if_exists(&session_dir) {
            errors.push(error);
        }

        if let Err(error) = cleanup_temp_verification_artifacts() {
            errors.push(error);
        }

        if let Err(error) = cleanup_repo_verification_artifacts(&repo_root) {
            errors.push(error);
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

fn cleanup_temp_verification_artifacts() -> Result<(), String> {
    remove_dir_all_if_exists(&verification_temp_root())
}

fn verification_temp_root() -> PathBuf {
    std::env::temp_dir().join(VERIFICATION_TEMP_ROOT_NAME)
}

fn cleanup_repo_verification_artifacts(repo_root: &Path) -> Result<(), String> {
    let mut errors = Vec::new();
    for path in [
        repo_root.join("tla-traces"),
        repo_root.join("states"),
        repo_root.join("tla_specs").join("states"),
        repo_root
            .join("verification")
            .join("tla_specs")
            .join("states"),
    ] {
        if let Err(error) = remove_dir_all_if_exists(&path) {
            errors.push(error);
        }
    }

    for spec_root in [
        repo_root.join("tla_specs"),
        repo_root.join("verification").join("tla_specs"),
    ] {
        if let Err(error) = remove_ignored_tlc_outputs(&spec_root) {
            errors.push(error);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn remove_ignored_tlc_outputs(spec_root: &Path) -> Result<(), String> {
    let entries = match fs::read_dir(spec_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "failed to read TLA spec directory {}: {error}",
                spec_root.display()
            ));
        }
    };

    let mut errors = Vec::new();
    for entry_result in entries {
        let entry = match entry_result {
            Ok(entry) => entry,
            Err(error) => {
                errors.push(format!(
                    "failed to read entry in {}: {error}",
                    spec_root.display()
                ));
                continue;
            }
        };

        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                errors.push(format!(
                    "failed to determine file type for {}: {error}",
                    path.display()
                ));
                continue;
            }
        };

        if file_type.is_dir() {
            if let Err(error) = remove_ignored_tlc_outputs(&path) {
                errors.push(error);
            }
            continue;
        }

        if file_type.is_file() && path.extension() == Some(OsStr::new("out")) {
            if let Err(error) = fs::remove_file(&path) {
                errors.push(format!("failed to remove file {}: {error}", path.display()));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn create_session_dir() -> Result<PathBuf, String> {
    let base_dir = verification_temp_root();
    fs::create_dir_all(&base_dir).map_err(|error| {
        format!(
            "failed to create verification temp root {}: {error}",
            base_dir.display()
        )
    })?;

    let base_stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);

    for attempt in 0..1024u32 {
        let session_dir =
            base_dir.join(format!("pid-{}-{}-{}", process::id(), base_stamp, attempt));
        if session_dir.exists() {
            continue;
        }
        fs::create_dir_all(&session_dir).map_err(|error| {
            format!(
                "failed to create verification session directory {}: {error}",
                session_dir.display()
            )
        })?;
        return Ok(session_dir);
    }

    Err(format!(
        "failed to allocate a verification session directory under {}",
        base_dir.display()
    ))
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
