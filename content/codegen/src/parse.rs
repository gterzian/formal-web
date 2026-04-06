use std::{fs, path::Path};

use regex::Regex;

#[derive(Debug, Clone)]
pub struct InterfaceDef {
    pub name: String,
    pub inherits: Option<String>,
    pub members: Vec<MemberDef>,
    pub source_file: String,
}

#[derive(Debug, Clone)]
pub enum MemberDef {
    Attribute {
        name: String,
        readonly: bool,
    },
    Operation {
        name: String,
        argc: usize,
    },
}

pub fn parse_interface(path: &Path) -> Result<InterfaceDef, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    weedle::parse(&source)
        .map_err(|error| format!("failed to parse {} with weedle2: {error:?}", path.display()))?;

    let interface_re = Regex::new(
        r"(?s)interface\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)(?:\s*:\s*(?P<inherits>[A-Za-z_][A-Za-z0-9_]*))?\s*\{(?P<body>.*?)\};",
    )
    .map_err(|error| format!("invalid interface regex: {error}"))?;
    let attribute_re = Regex::new(
        r"^(?P<readonly>readonly\s+)?attribute\s+[A-Za-z0-9_?<>\s]+\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)$",
    )
    .map_err(|error| format!("invalid attribute regex: {error}"))?;
    let operation_re = Regex::new(
        r"^(?P<ret>[A-Za-z0-9_?<>\s]+)\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)\((?P<args>.*)\)$",
    )
    .map_err(|error| format!("invalid operation regex: {error}"))?;

    let captures = interface_re.captures(&source).ok_or_else(|| {
        format!("no interface definition found in {}", path.display())
    })?;

    let body = captures.name("body").map(|match_| match_.as_str()).unwrap_or("");
    let mut members = Vec::new();
    for raw_member in body.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let member = raw_member.trim_end_matches(';').trim();
        if let Some(captures) = attribute_re.captures(member) {
            members.push(MemberDef::Attribute {
                name: captures["name"].to_owned(),
                readonly: captures.name("readonly").is_some(),
            });
            continue;
        }
        if let Some(captures) = operation_re.captures(member) {
            let args = captures.name("args").map(|match_| match_.as_str()).unwrap_or("");
            let argc = args
                .split(',')
                .map(str::trim)
                .filter(|arg| !arg.is_empty())
                .count();
            members.push(MemberDef::Operation {
                name: captures["name"].to_owned(),
                argc,
            });
        }
    }

    Ok(InterfaceDef {
        name: captures["name"].to_owned(),
        inherits: captures.name("inherits").map(|match_| match_.as_str().to_owned()),
        members,
        source_file: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned(),
    })
}