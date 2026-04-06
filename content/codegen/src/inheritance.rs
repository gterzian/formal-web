use std::collections::HashMap;

use serde::Deserialize;

use crate::parse::InterfaceDef;

#[derive(Debug, Default, Deserialize)]
pub struct InterfaceConfig {
    #[serde(default)]
    pub concrete: Vec<String>,
    #[serde(default)]
    pub abstract_interfaces: Vec<String>,
}

impl InterfaceConfig {
    pub fn is_concrete(&self, interface_name: &str) -> bool {
        self.concrete.iter().any(|name| name == interface_name)
    }
}

pub fn descendant_map(interfaces: &[InterfaceDef], config: &InterfaceConfig) -> HashMap<String, Vec<String>> {
    let mut descendants = HashMap::<String, Vec<String>>::new();
    for interface in interfaces {
        let Some(parent) = &interface.inherits else {
            continue;
        };
        if !config.is_concrete(&interface.name) {
            continue;
        }
        descendants.entry(parent.clone()).or_default().push(interface.name.clone());
    }
    descendants
}