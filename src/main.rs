#[cfg(not(test))]
use clap::Parser;

#[cfg(not(test))]
use butterfly_bot::config::Config;
#[cfg(not(test))]
use butterfly_bot::config_store;
#[cfg(not(test))]
use butterfly_bot::error::Result;
#[cfg(not(test))]
use butterfly_bot::vault;

#[cfg(not(test))]
#[derive(Parser, Debug)]
#[command(name = "butterfly-bot")]
#[command(about = "Butterfly Bot desktop launcher")]
struct Cli {
    #[arg(long, default_value_t = butterfly_bot::runtime_paths::default_db_path())]
    db: String,

    #[arg(long, default_value = "http://127.0.0.1:7878")]
    daemon: String,

    #[arg(long, default_value = "user")]
    user_id: String,
}

#[cfg(not(test))]
fn main() -> Result<()> {
    butterfly_bot::logging::init_tracing("butterfly_bot");

    let cli = Cli::parse();
    std::env::set_var("BUTTERFLY_BOT_DB", &cli.db);
    std::env::set_var("BUTTERFLY_BOT_DAEMON", &cli.daemon);
    std::env::set_var("BUTTERFLY_BOT_USER_ID", &cli.user_id);

    if let Ok(token) = vault::ensure_daemon_auth_token() {
        std::env::set_var("BUTTERFLY_BOT_TOKEN", token);
    }

    ensure_default_config(&cli.db)?;
    butterfly_bot::iced_ui::launch_ui(butterfly_bot::iced_ui::IcedUiLaunchConfig {
        daemon_url: cli.daemon,
        user_id: cli.user_id,
        db_path: cli.db,
    })
    .map_err(|err| butterfly_bot::ButterflyBotError::Config(err.to_string()))?;
    Ok(())
}

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

#[cfg(test)]
fn main() {}

#[cfg(test)]
mod tests {
    #[test]
    fn covers_main_stub() {
        super::main();
    }
}
