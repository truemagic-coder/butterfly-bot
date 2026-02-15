use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "butterfly-bot-ui")]
struct UiCli {
    /// Optional config JSON to import into the app database before launch.
    #[arg(long, env = "BUTTERFLY_BOT_CONFIG")]
    config: Option<String>,

    /// Database path for settings/config storage.
    #[arg(
        long,
        env = "BUTTERFLY_BOT_DB",
        default_value = "./data/butterfly-bot.db"
    )]
    db: String,

    /// Daemon address (e.g. http://127.0.0.1:7878).
    #[arg(long, env = "BUTTERFLY_BOT_DAEMON")]
    daemon: Option<String>,

    /// User id for chat context.
    #[arg(long, env = "BUTTERFLY_BOT_USER_ID")]
    user_id: Option<String>,
}

fn main() -> butterfly_bot::Result<()> {
    let cli = UiCli::parse();

    if let Some(config_path) = cli.config.as_ref() {
        let config = butterfly_bot::config::Config::from_file(config_path)?;
        butterfly_bot::config_store::save_config(&cli.db, &config)?;
    }

    if butterfly_bot::config::Config::from_store(&cli.db).is_err() {
        let defaults = butterfly_bot::config::Config::convention_defaults(&cli.db);
        butterfly_bot::config_store::save_config(&cli.db, &defaults)?;
    }

    std::env::set_var("BUTTERFLY_BOT_DB", &cli.db);
    if let Some(daemon) = cli.daemon.as_ref() {
        std::env::set_var("BUTTERFLY_BOT_DAEMON", daemon);
    }
    if let Ok(token) = butterfly_bot::vault::ensure_daemon_auth_token() {
        std::env::set_var("BUTTERFLY_BOT_TOKEN", token);
    }
    if let Some(user_id) = cli.user_id.as_ref() {
        std::env::set_var("BUTTERFLY_BOT_USER_ID", user_id);
    }

    butterfly_bot::ui::launch_ui();
    Ok(())
}
