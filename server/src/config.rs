use std::{env, net::SocketAddr};

/// Runtime configuration for the HTTP server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub bind_address: SocketAddr,
    pub database_url: String,
    pub seed_on_startup: bool,
}

impl Config {
    /// Loads configuration from environment variables.
    pub fn from_env() -> Self {
        let port = read_port();
        let database_url = read_database_url();
        let seed_on_startup = read_seed_on_startup();

        Self {
            bind_address: SocketAddr::from(([0, 0, 0, 0], port)),
            database_url,
            seed_on_startup,
        }
    }
}

fn read_port() -> u16 {
    match env::var("PORT") {
        Ok(value) => match value.parse::<u16>() {
            Ok(port) => port,
            Err(error) => panic!("PORT must be a valid u16 integer, got {value}: {error}"),
        },
        Err(env::VarError::NotPresent) => 8080,
        Err(error) => panic!("failed to read PORT from environment: {error}"),
    }
}

fn read_database_url() -> String {
    match env::var("DATABASE_URL") {
        Ok(value) if !value.trim().is_empty() => value,
        Ok(_) => panic!("DATABASE_URL must not be empty"),
        Err(env::VarError::NotPresent) => panic!("DATABASE_URL is required"),
        Err(error) => panic!("failed to read DATABASE_URL from environment: {error}"),
    }
}

fn read_seed_on_startup() -> bool {
    match env::var("SEED_ON_STARTUP") {
        Ok(value) => parse_bool_env("SEED_ON_STARTUP", &value),
        Err(env::VarError::NotPresent) => false,
        Err(error) => panic!("failed to read SEED_ON_STARTUP from environment: {error}"),
    }
}

fn parse_bool_env(name: &str, value: &str) -> bool {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => panic!("{name} must be a boolean value, got {value}"),
    }
}

#[cfg(test)]
mod tests {
    use super::Config;
    use std::sync::{Mutex, OnceLock};

    static ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn defaults_to_port_8080_and_seed_flag_false() {
        let _guard = ENV_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        unsafe {
            std::env::remove_var("PORT");
            std::env::remove_var("SEED_ON_STARTUP");
            std::env::set_var(
                "DATABASE_URL",
                "postgres://postgres:postgres@localhost/test",
            );
        }

        let config = Config::from_env();

        assert_eq!(config.bind_address.port(), 8080);
        assert!(!config.seed_on_startup);
        assert_eq!(
            config.database_url,
            "postgres://postgres:postgres@localhost/test"
        );
    }

    #[test]
    fn parses_seed_flag_when_present() {
        let _guard = ENV_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        unsafe {
            std::env::set_var(
                "DATABASE_URL",
                "postgres://postgres:postgres@localhost/test",
            );
            std::env::set_var("SEED_ON_STARTUP", "true");
        }

        let config = Config::from_env();

        assert!(config.seed_on_startup);
    }
}
