use crate::iced_ui::{self, IcedUiLaunchConfig};

#[derive(Clone)]
pub struct UiLaunchConfig {
    pub db_path: String,
    pub daemon_url: String,
    pub user_id: String,
}

impl Default for UiLaunchConfig {
    fn default() -> Self {
        Self {
            db_path: crate::runtime_paths::default_db_path(),
            daemon_url: "http://127.0.0.1:7878".to_string(),
            user_id: "user".to_string(),
        }
    }
}

pub fn launch_ui() {
    launch_ui_with_config(UiLaunchConfig::default());
}

pub fn launch_ui_with_config(config: UiLaunchConfig) {
    let result = iced_ui::launch_ui(IcedUiLaunchConfig {
        daemon_url: config.daemon_url,
        user_id: config.user_id,
        db_path: config.db_path,
    });

    if let Err(err) = result {
        tracing::error!(error = %err, "failed to launch iced UI");
    }
}
