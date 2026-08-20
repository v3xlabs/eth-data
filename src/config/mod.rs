//! One configured field per network: an RPC URL.
//!
//! Naming a network is what enables it. Chain ID, explorer, trace support, and
//! price anchors are all derived from the endpoint at startup, so there is no
//! second field for a user to get wrong.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use figment::{
    Figment,
    providers::{Env, Format, Toml},
};
use serde::{Deserialize, Serialize};

pub mod derived;
pub mod network;

pub use network::Networks;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    /// Network name to JSON-RPC endpoint. The only configured field there is.
    #[serde(default)]
    pub networks: BTreeMap<String, String>,
}

/// Shipped so a bare `nix run` works before any config exists. They are a
/// fallback, not a base to merge onto: naming any network of your own replaces
/// all three, which is the only way "I want mainnet and nothing else" can be
/// expressed.
pub fn default_networks() -> BTreeMap<String, String> {
    [
        ("mainnet", "https://ethereum.reth.rs/rpc"),
        ("sepolia", "https://ethereum-sepolia-rpc.publicnode.com"),
        ("hoodi", "https://ethereum-hoodi-rpc.publicnode.com"),
    ]
    .into_iter()
    .map(|(name, url)| (name.to_string(), url.to_string()))
    .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read configuration: {0}")]
    Read(Box<figment::Error>),
    #[error("network {name:?}: {reason}")]
    Network { name: String, reason: String },
    #[error("no networks are configured")]
    Empty,
}

impl Config {
    /// Reads the file, then the environment. A file that is not there is not an
    /// error; a file that is there and malformed is. Configuring no networks at
    /// all falls back to the shipped three.
    pub fn load(explicit_path: Option<&Path>) -> Result<Self, ConfigError> {
        let mut figment = Figment::new();

        match explicit_path {
            Some(path) => figment = figment.merge(Toml::file_exact(path)),
            None => {
                if let Some(path) = default_path() {
                    figment = figment.merge(Toml::file(path));
                }
            }
        }

        let mut config: Self = figment
            .merge(Env::prefixed("ETH_DATA_").split("__"))
            .extract()
            .map_err(|error| ConfigError::Read(Box::new(error)))?;

        if config.networks.is_empty() {
            config.networks = default_networks();
        }

        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.networks.is_empty() {
            return Err(ConfigError::Empty);
        }
        for (name, url) in &self.networks {
            check_name(name)?;
            check_url(name, url)?;
        }
        Ok(())
    }
}

fn check_name(name: &str) -> Result<(), ConfigError> {
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if valid {
        return Ok(());
    }
    Err(ConfigError::Network {
        name: name.to_string(),
        reason: "names may only contain lowercase letters, digits, and hyphens".to_string(),
    })
}

fn check_url(name: &str, url: &str) -> Result<(), ConfigError> {
    let parsed = url
        .parse::<reqwest::Url>()
        .map_err(|error| ConfigError::Network {
            name: name.to_string(),
            reason: format!("{url:?} is not a URL: {error}"),
        })?;
    // http is allowed so a local node on loopback works without a certificate.
    // Anything else would let a config file point the server at a file or a
    // socket, which is not what an RPC URL is.
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(ConfigError::Network {
            name: name.to_string(),
            reason: format!(
                "expected an http or https URL, received {:?}",
                parsed.scheme()
            ),
        });
    }
    Ok(())
}

fn default_path() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("eth-data").join("config.toml"))
}

#[cfg(test)]
// figment::Jail::expect_with hands the closure figment::Error, which is large.
// That is the test harness's type, not ours.
#[allow(clippy::result_large_err)]
mod tests {
    use super::*;

    #[test]
    fn defaults_cover_the_three_shipped_networks() {
        let config = Config {
            networks: default_networks(),
        };
        assert_eq!(config.networks.len(), 3);
        for name in ["mainnet", "sepolia", "hoodi"] {
            assert!(config.networks.contains_key(name));
        }
        config.validate().unwrap();
    }

    #[test]
    fn rejects_a_non_http_scheme() {
        let config = Config {
            networks: [("local".to_string(), "file:///etc/passwd".to_string())].into(),
        };
        let error = config.validate().unwrap_err();
        assert!(matches!(error, ConfigError::Network { .. }));
    }

    #[test]
    fn allows_loopback_http() {
        let config = Config {
            networks: [("local".to_string(), "http://127.0.0.1:8545".to_string())].into(),
        };
        config.validate().unwrap();
    }

    #[test]
    fn rejects_an_uppercase_network_name() {
        let config = Config {
            networks: [("Mainnet".to_string(), "https://example.invalid".to_string())].into(),
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_an_empty_network_set() {
        let config = Config {
            networks: BTreeMap::new(),
        };
        assert!(matches!(config.validate(), Err(ConfigError::Empty)));
    }

    #[test]
    fn a_config_file_replaces_the_defaults_rather_than_merging() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "config.toml",
                r#"
                [networks]
                mainnet = "https://my-node.internal/rpc"
                "#,
            )?;
            let config = Config::load(Some(Path::new("config.toml"))).unwrap();
            assert_eq!(
                config.networks.get("mainnet").map(String::as_str),
                Some("https://my-node.internal/rpc")
            );
            // Naming one network is how you say "only this one".
            assert_eq!(config.networks.len(), 1);
            Ok(())
        });
    }

    #[test]
    fn the_environment_names_networks_too() {
        figment::Jail::expect_with(|jail| {
            jail.set_env("ETH_DATA_NETWORKS__LOCAL", "http://127.0.0.1:8545");
            let config = Config::load(None).unwrap();
            assert_eq!(
                config.networks.get("local").map(String::as_str),
                Some("http://127.0.0.1:8545")
            );
            assert_eq!(config.networks.len(), 1);
            Ok(())
        });
    }

    #[test]
    fn no_configuration_falls_back_to_the_shipped_three() {
        figment::Jail::expect_with(|_| {
            let config = Config::load(None).unwrap();
            assert_eq!(config.networks, default_networks());
            Ok(())
        });
    }
}
