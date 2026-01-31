use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use crate::interfaces::plugins::{Plugin, PluginManager};
use crate::plugins::registry::ToolRegistry;

pub struct DefaultPluginManager {
    config: Value,
    tool_registry: ToolRegistry,
    plugins: HashMap<String, Box<dyn Plugin>>,
    plugin_factories: HashMap<String, PluginFactory>,
}

type PluginFactory = Arc<dyn Fn(Value) -> Box<dyn Plugin> + Send + Sync>;

impl DefaultPluginManager {
    pub fn new(config: Value) -> Self {
        Self {
            config,
            tool_registry: ToolRegistry::new(),
            plugins: HashMap::new(),
            plugin_factories: HashMap::new(),
        }
    }

    pub fn register_factory<F>(&mut self, name: &str, factory: F)
    where
        F: Fn(Value) -> Box<dyn Plugin> + Send + Sync + 'static,
    {
        self.plugin_factories
            .insert(name.to_string(), Arc::new(factory));
    }

    pub fn tool_registry(&self) -> &ToolRegistry {
        &self.tool_registry
    }
}

impl PluginManager for DefaultPluginManager {
    fn register_plugin(&mut self, plugin: Box<dyn Plugin>) -> bool {
        if !plugin.initialize(&self.tool_registry) {
            return false;
        }
        self.plugins.insert(plugin.name().to_string(), plugin);
        true
    }

    fn load_plugins(&mut self) -> Vec<String> {
        let mut loaded = Vec::new();

        let plugin_entries = self
            .config
            .get("plugins")
            .and_then(|value| value.as_array())
            .cloned();

        let mut to_load: Vec<(String, Value)> = Vec::new();

        if let Some(entries) = plugin_entries {
            for entry in entries {
                match entry {
                    Value::String(name) => {
                        to_load.push((name, Value::Null));
                    }
                    Value::Object(map) => {
                        let name = map
                            .get("name")
                            .or_else(|| map.get("class"))
                            .and_then(|value| value.as_str())
                            .map(|name| name.to_string());
                        if let Some(name) = name {
                            let config = map.get("config").cloned().unwrap_or(Value::Null);
                            to_load.push((name, config));
                        }
                    }
                    _ => {}
                }
            }
        } else {
            let mut names: Vec<String> = self.plugin_factories.keys().cloned().collect();
            names.sort();
            for name in names {
                to_load.push((name, Value::Null));
            }
        }

        for (name, config) in to_load {
            if self.plugins.contains_key(&name) {
                continue;
            }
            if let Some(factory) = self.plugin_factories.get(&name) {
                let plugin = factory(config);
                if self.register_plugin(plugin) {
                    loaded.push(name);
                }
            }
        }

        loaded
    }

    fn get_plugin(&self, name: &str) -> Option<&dyn Plugin> {
        self.plugins.get(name).map(|p| p.as_ref())
    }

    fn list_plugins(&self) -> Vec<Value> {
        self.plugins
            .values()
            .map(|p| {
                serde_json::json!({
                    "name": p.name(),
                    "description": p.description(),
                })
            })
            .collect()
    }

    fn configure(&mut self, config: Value) {
        self.config = config.clone();
        let _ = futures::executor::block_on(self.tool_registry.configure_all_tools(config));
    }
}
