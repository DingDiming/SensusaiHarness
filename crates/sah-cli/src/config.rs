use anyhow::{Context, Result, bail};
use sah_domain::{ApprovalMode, ProviderKind};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const ENV_CONFIG_PATH: &str = "SAH_CONFIG";
const ENV_PROVIDER: &str = "SAH_PROVIDER";
const ENV_APPROVAL: &str = "SAH_APPROVAL";
const ENV_HOME: &str = "SAH_HOME";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CliConfigFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_provider: Option<ProviderKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_approval: Option<ApprovalMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sah_home: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ResolvedDefaults {
    pub config_path: PathBuf,
    pub config_exists: bool,
    pub file: CliConfigFile,
    pub default_provider: ProviderKind,
    pub default_provider_source: String,
    pub default_approval: ApprovalMode,
    pub default_approval_source: String,
    pub sah_home: PathBuf,
    pub sah_home_source: String,
}

pub fn resolve_config_path(cli_override: Option<PathBuf>) -> PathBuf {
    cli_override
        .or_else(|| env::var_os(ENV_CONFIG_PATH).map(PathBuf::from))
        .unwrap_or_else(default_config_path)
}

pub fn load_config(path: &Path) -> Result<CliConfigFile> {
    if !path.exists() {
        return Ok(CliConfigFile::default());
    }

    let bytes = fs::read(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn save_config(path: &Path, file: &CliConfigFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }

    let bytes = serde_json::to_vec_pretty(file)?;
    fs::write(path, bytes).with_context(|| format!("failed to write config file {}", path.display()))
}

pub fn resolve_defaults(
    config_path: &Path,
    file: &CliConfigFile,
    cli_sah_home: Option<PathBuf>,
) -> Result<ResolvedDefaults> {
    let (default_provider, default_provider_source) = resolve_provider_default(file)?;
    let (default_approval, default_approval_source) = resolve_approval_default(file)?;
    let (sah_home, sah_home_source) = resolve_sah_home(config_path, file, cli_sah_home)?;

    Ok(ResolvedDefaults {
        config_path: config_path.to_path_buf(),
        config_exists: config_path.exists(),
        file: file.clone(),
        default_provider,
        default_provider_source,
        default_approval,
        default_approval_source,
        sah_home,
        sah_home_source,
    })
}

pub fn update_config_file(
    mut file: CliConfigFile,
    provider: Option<ProviderKind>,
    approval: Option<ApprovalMode>,
    sah_home: Option<PathBuf>,
    clear_provider: bool,
    clear_approval: bool,
    clear_sah_home: bool,
) -> Result<CliConfigFile> {
    if provider.is_some() && clear_provider {
        bail!("cannot set and clear default provider in the same command");
    }
    if approval.is_some() && clear_approval {
        bail!("cannot set and clear default approval in the same command");
    }
    if sah_home.is_some() && clear_sah_home {
        bail!("cannot set and clear sah_home in the same command");
    }
    if provider.is_none()
        && approval.is_none()
        && sah_home.is_none()
        && !clear_provider
        && !clear_approval
        && !clear_sah_home
    {
        bail!("config set requires at least one change");
    }

    if clear_provider {
        file.default_provider = None;
    } else if let Some(provider) = provider {
        file.default_provider = Some(provider);
    }

    if clear_approval {
        file.default_approval = None;
    } else if let Some(approval) = approval {
        file.default_approval = Some(approval);
    }

    if clear_sah_home {
        file.sah_home = None;
    } else if let Some(sah_home) = sah_home {
        file.sah_home = Some(normalize_store_home(sah_home)?);
    }

    Ok(file)
}

fn resolve_provider_default(file: &CliConfigFile) -> Result<(ProviderKind, String)> {
    if let Some(value) = env::var_os(ENV_PROVIDER) {
        let value = value.to_string_lossy().to_string();
        let provider = value
            .parse()
            .map_err(|error: String| anyhow::anyhow!("invalid {ENV_PROVIDER}: {error}"))?;
        return Ok((provider, format!("env:{ENV_PROVIDER}")));
    }

    if let Some(provider) = file.default_provider {
        return Ok((provider, "config".to_owned()));
    }

    Ok((ProviderKind::Codex, "default".to_owned()))
}

fn resolve_approval_default(file: &CliConfigFile) -> Result<(ApprovalMode, String)> {
    if let Some(value) = env::var_os(ENV_APPROVAL) {
        let value = value.to_string_lossy().to_string();
        let approval = value
            .parse()
            .map_err(|error: String| anyhow::anyhow!("invalid {ENV_APPROVAL}: {error}"))?;
        return Ok((approval, format!("env:{ENV_APPROVAL}")));
    }

    if let Some(approval) = file.default_approval {
        return Ok((approval, "config".to_owned()));
    }

    Ok((ApprovalMode::Auto, "default".to_owned()))
}

fn resolve_sah_home(
    config_path: &Path,
    file: &CliConfigFile,
    cli_sah_home: Option<PathBuf>,
) -> Result<(PathBuf, String)> {
    if let Some(path) = cli_sah_home {
        return Ok((normalize_store_home(path)?, "cli".to_owned()));
    }

    if let Some(path) = env::var_os(ENV_HOME) {
        return Ok((normalize_store_home(PathBuf::from(path))?, format!("env:{ENV_HOME}")));
    }

    if let Some(path) = &file.sah_home {
        let resolved = if path.is_absolute() {
            path.clone()
        } else if let Some(parent) = config_path.parent() {
            parent.join(path)
        } else {
            env::current_dir()
                .context("failed to resolve current directory for relative sah_home")?
                .join(path)
        };
        return Ok((resolved, "config".to_owned()));
    }

    Ok((default_store_home(), "default".to_owned()))
}

fn default_config_path() -> PathBuf {
    if let Some(config_dir) = dirs::config_dir() {
        return config_dir.join("sah").join("config.json");
    }

    PathBuf::from(".sah-config.json")
}

fn default_store_home() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        return home.join(".sah");
    }

    PathBuf::from(".sah")
}

fn normalize_store_home(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path);
    }

    Ok(env::current_dir()
        .context("failed to resolve current directory for relative sah_home")?
        .join(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_defaults_from_config_when_env_is_missing() {
        let path = PathBuf::from("/tmp/sah-config.json");
        let file = CliConfigFile {
            default_provider: Some(ProviderKind::Claude),
            default_approval: Some(ApprovalMode::Confirm),
            sah_home: Some(PathBuf::from("/tmp/sah-store")),
        };

        let resolved = resolve_defaults(&path, &file, None).expect("resolve defaults");
        assert_eq!(resolved.default_provider, ProviderKind::Claude);
        assert_eq!(resolved.default_provider_source, "config");
        assert_eq!(resolved.default_approval, ApprovalMode::Confirm);
        assert_eq!(resolved.default_approval_source, "config");
        assert_eq!(resolved.sah_home, PathBuf::from("/tmp/sah-store"));
        assert_eq!(resolved.sah_home_source, "config");
    }

    #[test]
    fn cli_sah_home_overrides_config() {
        let path = PathBuf::from("/tmp/sah-config.json");
        let file = CliConfigFile {
            sah_home: Some(PathBuf::from("/tmp/from-config")),
            ..CliConfigFile::default()
        };

        let resolved =
            resolve_defaults(&path, &file, Some(PathBuf::from("/tmp/from-cli"))).expect("resolve");
        assert_eq!(resolved.sah_home, PathBuf::from("/tmp/from-cli"));
        assert_eq!(resolved.sah_home_source, "cli");
    }

    #[test]
    fn update_config_file_can_set_and_clear_values() {
        let file = CliConfigFile {
            default_provider: Some(ProviderKind::Codex),
            default_approval: Some(ApprovalMode::Auto),
            sah_home: Some(PathBuf::from("/tmp/original")),
        };

        let file = update_config_file(
            file,
            Some(ProviderKind::Claude),
            None,
            None,
            false,
            true,
            true,
        )
        .expect("update config");

        assert_eq!(file.default_provider, Some(ProviderKind::Claude));
        assert_eq!(file.default_approval, None);
        assert_eq!(file.sah_home, None);
    }
}
