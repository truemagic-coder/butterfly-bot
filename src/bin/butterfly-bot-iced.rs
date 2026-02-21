use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "butterfly-bot-iced")]
struct UiCli {
    /// Database path for settings/config storage.
    #[arg(
        long,
        env = "BUTTERFLY_BOT_DB",
        default_value_t = butterfly_bot::runtime_paths::default_db_path()
    )]
    db: String,

    /// Daemon address (e.g. http://127.0.0.1:7878).
    #[arg(long, env = "BUTTERFLY_BOT_DAEMON", default_value = "http://127.0.0.1:7878")]
    daemon: String,

    /// User id for chat context.
    #[arg(long, env = "BUTTERFLY_BOT_USER_ID", default_value = "user")]
    user_id: String,
}

fn main() -> butterfly_bot::Result<()> {
    butterfly_bot::logging::init_tracing("butterfly_bot_iced_ui");
    let cli = UiCli::parse();

    if butterfly_bot::config::Config::from_store(&cli.db).is_err() {
        let defaults = butterfly_bot::config::Config::convention_defaults(&cli.db);
        butterfly_bot::config_store::save_config(&cli.db, &defaults)?;
    }

    std::env::set_var("BUTTERFLY_BOT_DB", &cli.db);
    std::env::set_var("BUTTERFLY_BOT_DAEMON", &cli.daemon);
    std::env::set_var("BUTTERFLY_BOT_USER_ID", &cli.user_id);
    if let Ok(token) = butterfly_bot::vault::ensure_daemon_auth_token() {
        std::env::set_var("BUTTERFLY_BOT_TOKEN", token);
    }

    butterfly_bot::iced_ui::launch_ui(butterfly_bot::iced_ui::IcedUiLaunchConfig {
        daemon_url: cli.daemon,
        user_id: cli.user_id,
        db_path: cli.db,
    })
    .map_err(|err| butterfly_bot::ButterflyBotError::Config(err.to_string()))
}
