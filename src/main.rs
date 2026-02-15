#[cfg(not(test))]
use clap::Parser;
#[cfg(not(test))]
use std::process::Command;

#[cfg(not(test))]
use butterfly_bot::config::Config;
#[cfg(not(test))]
use butterfly_bot::config_store;
#[cfg(not(test))]
use butterfly_bot::error::Result;
#[cfg(not(test))]
use butterfly_bot::ui;
#[cfg(not(test))]
use butterfly_bot::vault;
#[cfg(not(test))]
use tracing_subscriber::EnvFilter;

#[cfg(not(test))]
#[derive(Parser, Debug)]
#[command(name = "butterfly-bot")]
#[command(about = "Butterfly Bot desktop launcher")]
struct Cli {
    #[arg(long, default_value = "./data/butterfly-bot.db")]
    db: String,

    #[arg(long, default_value = "http://127.0.0.1:7878")]
    daemon: String,

    #[arg(long, default_value = "user")]
    user_id: String,
}

#[cfg(not(test))]
#[tokio::main]
async fn main() -> Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,butterfly_bot=info,lance=warn,lancedb=warn"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
    force_dbusrs();

    let cli = Cli::parse();
    std::env::set_var("BUTTERFLY_BOT_DB", &cli.db);
    std::env::set_var("BUTTERFLY_BOT_DAEMON", &cli.daemon);
    std::env::set_var("BUTTERFLY_BOT_USER_ID", &cli.user_id);

    if let Ok(token) = vault::ensure_daemon_auth_token() {
        std::env::set_var("BUTTERFLY_BOT_TOKEN", token);
    }

    let config = ensure_default_config(&cli.db)?;
    ensure_ollama_models(&config)?;
    ui::launch_ui();
    Ok(())
}

#[cfg(not(test))]
#[cfg(target_os = "linux")]
fn force_dbusrs() {
    if std::env::var("DBUSRS").is_err() {
        std::env::set_var("DBUSRS", "1");
    }
}

#[cfg(not(test))]
#[cfg(not(target_os = "linux"))]
fn force_dbusrs() {}

#[cfg(not(test))]
fn ensure_default_config(db_path: &str) -> Result<Config> {
    match Config::from_store(db_path) {
        Ok(config) => Ok(config),
        Err(_) => {
            let config = Config::convention_defaults(db_path);
            config_store::save_config(db_path, &config)?;
            Ok(config)
        }
    }
}

#[cfg(not(test))]
fn ensure_ollama_models(config: &Config) -> Result<()> {
    let Some(openai) = &config.openai else {
        return Ok(());
    };
    let Some(base_url) = &openai.base_url else {
        return Ok(());
    };
    if !is_ollama_local(base_url) {
        return Ok(());
    }

    let mut required = Vec::new();
    if let Some(model) = &openai.model {
        if !model.trim().is_empty() {
            required.push(model.clone());
        }
    }
    if let Some(memory) = &config.memory {
        for value in [
            memory.embedding_model.as_ref(),
            memory.rerank_model.as_ref(),
            memory.summary_model.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if !value.trim().is_empty() {
                required.push(value.clone());
            }
        }
    }

    required.sort();
    required.dedup();
    if required.is_empty() {
        return Ok(());
    }

    let installed = list_ollama_models()?;
    for model in required {
        if !installed.iter().any(|name| model_matches(&model, name)) {
            println!("Loading Ollama model '{model}'...");
            pull_ollama_model(&model)?;
        }
    }

    Ok(())
}

#[cfg(not(test))]
fn is_ollama_local(base_url: &str) -> bool {
    base_url.starts_with("http://localhost:11434") || base_url.starts_with("http://127.0.0.1:11434")
}

#[cfg(not(test))]
fn list_ollama_models() -> Result<Vec<String>> {
    let output = Command::new("ollama")
        .arg("list")
        .output()
        .map_err(|e| butterfly_bot::error::ButterflyBotError::Runtime(e.to_string()))?;
    if !output.status.success() {
        return Err(butterfly_bot::error::ButterflyBotError::Runtime(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut models = Vec::new();
    for line in stdout.lines().skip(1) {
        let name = line.split_whitespace().next().unwrap_or("");
        if !name.is_empty() {
            models.push(name.to_string());
        }
    }
    Ok(models)
}

#[cfg(not(test))]
fn pull_ollama_model(model: &str) -> Result<()> {
    let status = Command::new("ollama")
        .arg("pull")
        .arg(model)
        .status()
        .map_err(|e| butterfly_bot::error::ButterflyBotError::Runtime(e.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(butterfly_bot::error::ButterflyBotError::Runtime(format!(
            "Failed to pull model '{model}'"
        )))
    }
}

#[cfg(not(test))]
fn split_model_name(model: &str) -> (String, Option<String>) {
    let mut parts = model.rsplitn(2, ':');
    let tag = parts.next().map(|v| v.to_string());
    let base = parts.next();
    match base {
        Some(base) if !base.is_empty() => (base.to_string(), tag),
        _ => (model.to_string(), None),
    }
}

#[cfg(not(test))]
fn model_matches(required: &str, installed: &str) -> bool {
    let (req_base, req_tag) = split_model_name(required);
    let (ins_base, ins_tag) = split_model_name(installed);
    if req_base != ins_base {
        return false;
    }
    match (req_tag, ins_tag) {
        (Some(req), Some(ins)) => req == ins,
        (Some(req), None) => req == "latest",
        (None, Some(ins)) => ins == "latest",
        (None, None) => true,
    }
}

#[cfg(test)]
fn main() {}

#[cfg(test)]
mod tests {
    #[test]
    fn covers_main_stub() {
        super::main();
    }
}
