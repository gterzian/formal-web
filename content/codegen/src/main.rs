mod emit;
mod inheritance;
mod parse;

use std::{fs, path::{Path, PathBuf}};

use inheritance::{InterfaceConfig, descendant_map};
use parse::parse_interface;

fn main() -> Result<(), String> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bindings_dir = manifest_dir
        .parent()
        .ok_or_else(|| String::from("codegen manifest must live under content/"))?
        .join("src/boa/bindings");
    let config_path = manifest_dir.join("interfaces.toml");
    let config = load_config(&config_path)?;

    let mut interfaces = bindings_dir
        .read_dir()
        .map_err(|error| format!("failed to list {}: {error}", bindings_dir.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("webidl"))
        .map(|path| parse_interface(&path))
        .collect::<Result<Vec<_>, _>>()?;

    interfaces.sort_by(|left, right| left.name.cmp(&right.name));
    let descendants = descendant_map(&interfaces, &config);

    for interface in &interfaces {
        let generated_path = bindings_dir.join(format!(
            "{}_generated.rs",
            interface.name.to_ascii_lowercase()
        ));
        fs::write(&generated_path, emit::emit(interface, &descendants)).map_err(|error| {
            format!("failed to write {}: {error}", generated_path.display())
        })?;
    }

    Ok(())
}

fn load_config(path: &Path) -> Result<InterfaceConfig, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    toml::from_str(&source)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))
}