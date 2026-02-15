use butterfly_bot::config::Config;
use butterfly_bot::config_store;
use butterfly_bot::daemon;
use butterfly_bot::error::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "butterfly-botd")]
#[command(about = "ButterFly Bot local daemon")]
struct Cli {
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    #[arg(long, default_value_t = 7878)]
    port: u16,

    #[arg(long, default_value = "./data/butterfly-bot.db")]
    db: String,

    #[arg(long, env = "BUTTERFLY_BOT_TOKEN", default_value = "")]
    token: String,

    /// Path to a JSON config file to import into the store on startup.
    /// This ensures the daemon always uses the latest config.
    #[arg(long)]
    config: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,butterfly_bot=info,lance=warn,lancedb=warn"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
    let cli = Cli::parse();

    // If --config is given, import it into the store before starting.
    if let Some(config_path) = &cli.config {
        tracing::info!("Importing config from {} into store", config_path);
        let config = Config::from_file(config_path)?;
        config_store::save_config(&cli.db, &config)?;
        tracing::info!(
            "Config imported successfully (prompt_source={:?})",
            config.prompt_source
        );
    }

    daemon::run(&cli.host, cli.port, &cli.db, &cli.token).await
}
