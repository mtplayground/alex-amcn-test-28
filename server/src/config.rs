use std::{env, net::SocketAddr};

/// Runtime configuration for the HTTP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    pub bind_address: SocketAddr,
}

impl Config {
    /// Loads configuration from environment variables.
    pub fn from_env() -> Result<Self, ConfigError> {
        let port = match env::var("PORT") {
            Ok(value) => value
                .parse::<u16>()
                .map_err(|source| ConfigError::InvalidPort { value, source })?,
            Err(env::VarError::NotPresent) => 8080,
            Err(source) => return Err(ConfigError::ReadPort(source)),
        };

        Ok(Self {
            bind_address: SocketAddr::from(([0, 0, 0, 0], port)),
        })
    }
}

#[derive(Debug)]
pub enum ConfigError {
    ReadPort(env::VarError),
    InvalidPort {
        value: String,
        source: std::num::ParseIntError,
    },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadPort(_) => write!(f, "failed to read PORT from environment"),
            Self::InvalidPort { value, .. } => {
                write!(f, "PORT must be a valid u16 integer, got {value}")
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ReadPort(source) => Some(source),
            Self::InvalidPort { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn defaults_to_port_8080() {
        unsafe {
            std::env::remove_var("PORT");
        }

        let config_result = Config::from_env();
        assert!(config_result.is_ok());

        let config = match config_result {
            Ok(config) => config,
            Err(error) => panic!("config should load: {error}"),
        };

        assert_eq!(config.bind_address.port(), 8080);
    }
}
