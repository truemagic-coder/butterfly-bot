#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretResolutionMode {
    Strict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretResolutionPolicy {
    mode: SecretResolutionMode,
}

impl SecretResolutionPolicy {
    pub fn mode(self) -> SecretResolutionMode {
        self.mode
    }

    pub fn is_strict(self) -> bool {
        matches!(self.mode, SecretResolutionMode::Strict)
    }
}

pub fn current_secret_resolution_policy() -> SecretResolutionPolicy {
    SecretResolutionPolicy {
        mode: SecretResolutionMode::Strict,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_strict_policy() {
        let policy = current_secret_resolution_policy();
        assert_eq!(policy.mode(), SecretResolutionMode::Strict);
        assert!(policy.is_strict());
    }

    #[test]
    fn repeated_calls_remain_strict() {
        let first = current_secret_resolution_policy();
        let policy = current_secret_resolution_policy();
        assert_eq!(first.mode(), SecretResolutionMode::Strict);
        assert_eq!(policy.mode(), SecretResolutionMode::Strict);
        assert!(policy.is_strict());
    }

    #[test]
    fn mode_accessor_matches_strict() {
        let policy = current_secret_resolution_policy();
        assert_eq!(policy.mode(), SecretResolutionMode::Strict);
        assert!(policy.is_strict());
    }
}
