//! Validation profile selection (spec §10.5).

use ctx_core::ValidationConfig;
use ctx_indexing::{BuildSystem, RepositoryProfile, ValidationCommand, ValidationKind};

/// Explicit configuration wins; otherwise the detected project commands.
pub fn validation_commands(config: &ValidationConfig, profile: &RepositoryProfile) -> Vec<ValidationCommand> {
    configured_validation_commands(config).unwrap_or_else(|| profile.validation.clone())
}

/// The commands when the configuration alone decides them (listed
/// explicitly, or auto-detection off); `None` when they come from the
/// repository profile.
pub fn configured_validation_commands(config: &ValidationConfig) -> Option<Vec<ValidationCommand>> {
    if !config.commands.is_empty() {
        return Some(
            config
                .commands
                .iter()
                .map(|c| ValidationCommand { command: c.clone(), kind: ValidationKind::Test, cwd: ".".into(), source: "configuration (validation.commands)".into(), system: BuildSystem::Make })
                .collect(),
        );
    }
    if !config.auto_detect {
        return Some(Vec::new());
    }
    None
}
