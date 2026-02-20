use butterfly_bot::daemon;
use butterfly_bot::error::Result;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "butterfly-botd")]
#[command(about = "ButterFly Bot local daemon")]
struct Cli {
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    #[arg(long, default_value_t = 7878)]
    port: u16,

    #[arg(long, default_value_t = butterfly_bot::runtime_paths::default_db_path())]
    db: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    butterfly_bot::logging::init_tracing("butterfly_botd");
    let cli = Cli::parse();

    if std::env::var("BUTTERFLY_TPM_MODE").is_err() {
        if let Ok(config) = butterfly_bot::config::Config::from_store(&cli.db) {
            let tpm_mode = config
                .tools
                .as_ref()
                .and_then(|tools| tools.get("settings"))
                .and_then(|settings| settings.get("security"))
                .and_then(|security| security.get("tpm_mode"))
                .and_then(|value| value.as_str())
                .unwrap_or("auto")
                .to_string();
            std::env::set_var("BUTTERFLY_TPM_MODE", tpm_mode);
        }
    }

    let token = butterfly_bot::vault::ensure_daemon_auth_token()?;

    daemon::run(&cli.host, cli.port, &cli.db, &token).await
}
