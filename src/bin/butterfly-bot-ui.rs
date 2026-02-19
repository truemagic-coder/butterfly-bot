use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "butterfly-bot-ui")]
struct UiCli {
    /// Database path for settings/config storage.
    #[arg(long, default_value_t = butterfly_bot::runtime_paths::default_db_path())]
    db: String,

    /// Daemon address (e.g. http://127.0.0.1:7878).
    #[arg(long)]
    daemon: Option<String>,

    /// User id for chat context.
    #[arg(long, default_value = "user")]
    user_id: String,
}

fn main() -> butterfly_bot::Result<()> {
    let cli = UiCli::parse();

    if butterfly_bot::config::Config::from_store(&cli.db).is_err() {
        let defaults = butterfly_bot::config::Config::convention_defaults(&cli.db);
        butterfly_bot::config_store::save_config(&cli.db, &defaults)?;
    }

    let _token = butterfly_bot::vault::ensure_daemon_auth_token()?;

    butterfly_bot::ui::launch_ui_with_config(butterfly_bot::ui::UiLaunchConfig {
        db_path: cli.db,
        daemon_url: cli
            .daemon
            .unwrap_or_else(|| "http://127.0.0.1:7878".to_string()),
        user_id: cli.user_id,
    });
    Ok(())
}
