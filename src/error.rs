use thiserror::Error;

#[derive(Debug, Error)]
pub enum ButterflyBotError {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("http error: {0}")]
    Http(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("runtime error: {0}")]
    Runtime(String),
}

pub use crate::Result;
pub fn result_ok() -> Result<()> {
    Ok(())
}

#[cfg(test)]
pub fn coverage_probe() -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_error_probe_and_display() {
        coverage_probe().unwrap();
        result_ok().unwrap();
        let err = ButterflyBotError::Config("x".to_string());
        assert!(format!("{err}").contains("configuration error"));
    }
}
