//! Configuration from the environment (docs/spec/foundations.md#configuration).
//! Only what's needed before anyone can log in, or what touches the host; the rest is in-app Settings.

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::http::HeaderName;
use ipnet::IpNet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub data_dir: PathBuf,
    pub listen: SocketAddr,
    pub base_url: Option<String>,
    pub auth: Auth,
    pub backup_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Auth {
    /// Built-in login: passwords and sessions.
    Builtin,
    /// Trust `header` for the username, but only on requests from `trusted_proxy`.
    Header {
        trusted_proxy: IpNet,
        header: HeaderName,
    },
}

/// A variable that's missing or invalid. The message names it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{name} {problem}")]
pub struct ConfigError {
    pub name: &'static str,
    pub problem: String,
}

fn invalid(name: &'static str, value: &str, expected: &str) -> ConfigError {
    ConfigError {
        name,
        problem: format!("is {value:?}; expected {expected}"),
    }
}

impl Config {
    /// Reads the `BUDGET_*` variables through `var`. An empty value counts as unset.
    ///
    /// # Errors
    /// The first variable that's missing or invalid.
    pub fn from_env(var: impl Fn(&str) -> Option<String>) -> Result<Config, ConfigError> {
        let get = |name: &str| var(name).filter(|value| !value.is_empty());

        let data_dir = PathBuf::from(get("BUDGET_DATA_DIR").unwrap_or_else(|| "/data".to_owned()));

        let listen = match get("BUDGET_LISTEN") {
            None => SocketAddr::from(([0, 0, 0, 0], 8080)),
            Some(value) => value.parse().map_err(|_| {
                invalid(
                    "BUDGET_LISTEN",
                    &value,
                    "an address and port, like 0.0.0.0:8080",
                )
            })?,
        };

        let base_url = match get("BUDGET_BASE_URL") {
            None => None,
            Some(value) if value.starts_with("http://") || value.starts_with("https://") => {
                Some(value.trim_end_matches('/').to_owned())
            }
            Some(value) => {
                return Err(invalid(
                    "BUDGET_BASE_URL",
                    &value,
                    "a URL starting with https:// or http://",
                ));
            }
        };

        let auth = match get("BUDGET_AUTH_MODE").as_deref() {
            None | Some("builtin") => Auth::Builtin,
            Some("header") => {
                let trusted_proxy = match get("BUDGET_TRUSTED_PROXY") {
                    None => {
                        return Err(ConfigError {
                            name: "BUDGET_TRUSTED_PROXY",
                            problem: "is required when BUDGET_AUTH_MODE is header".to_owned(),
                        });
                    }
                    Some(value) => parse_proxy(&value).ok_or_else(|| {
                        invalid(
                            "BUDGET_TRUSTED_PROXY",
                            &value,
                            "an IP address or CIDR range, like 172.18.0.2 or 172.18.0.0/16",
                        )
                    })?,
                };
                let header = match get("BUDGET_AUTH_HEADER") {
                    None => HeaderName::from_static("remote-user"),
                    Some(value) => HeaderName::try_from(value.as_str()).map_err(|_| {
                        invalid("BUDGET_AUTH_HEADER", &value, "an HTTP header name")
                    })?,
                };
                Auth::Header {
                    trusted_proxy,
                    header,
                }
            }
            Some(value) => return Err(invalid("BUDGET_AUTH_MODE", value, "builtin or header")),
        };

        let backup_dir =
            get("BUDGET_BACKUP_DIR").map_or_else(|| data_dir.join("backups"), PathBuf::from);

        Ok(Config {
            data_dir,
            listen,
            base_url,
            auth,
            backup_dir,
        })
    }
}

/// An IP address is a range of one.
fn parse_proxy(value: &str) -> Option<IpNet> {
    value
        .parse::<IpNet>()
        .ok()
        .or_else(|| value.parse::<std::net::IpAddr>().ok().map(IpNet::from))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn config(vars: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        Config::from_env(|name| vars.get(name).cloned())
    }

    fn error_names(vars: &[(&str, &str)], name: &str) {
        let error = config(vars).unwrap_err();
        assert_eq!(error.name, name);
        assert!(error.to_string().starts_with(name), "{error}");
    }

    #[test]
    fn defaults() {
        assert_eq!(
            config(&[]).unwrap(),
            Config {
                data_dir: PathBuf::from("/data"),
                listen: "0.0.0.0:8080".parse().unwrap(),
                base_url: None,
                auth: Auth::Builtin,
                backup_dir: PathBuf::from("/data/backups"),
            }
        );
    }

    #[test]
    fn empty_counts_as_unset() {
        assert_eq!(
            config(&[("BUDGET_LISTEN", ""), ("BUDGET_AUTH_MODE", "")]),
            config(&[])
        );
    }

    #[test]
    fn reads_every_variable() {
        let c = config(&[
            ("BUDGET_DATA_DIR", "/srv/budget"),
            ("BUDGET_LISTEN", "127.0.0.1:9000"),
            ("BUDGET_BASE_URL", "https://budget.example.test/"),
            ("BUDGET_AUTH_MODE", "header"),
            ("BUDGET_TRUSTED_PROXY", "172.18.0.0/16"),
            ("BUDGET_AUTH_HEADER", "X-Forwarded-User"),
            ("BUDGET_BACKUP_DIR", "/backups"),
        ])
        .unwrap();
        assert_eq!(c.data_dir, PathBuf::from("/srv/budget"));
        assert_eq!(c.listen, "127.0.0.1:9000".parse().unwrap());
        assert_eq!(c.base_url.as_deref(), Some("https://budget.example.test"));
        assert_eq!(
            c.auth,
            Auth::Header {
                trusted_proxy: "172.18.0.0/16".parse().unwrap(),
                header: HeaderName::from_static("x-forwarded-user"),
            }
        );
        assert_eq!(c.backup_dir, PathBuf::from("/backups"));
    }

    #[test]
    fn backups_follow_the_data_dir() {
        assert_eq!(
            config(&[("BUDGET_DATA_DIR", "/srv")]).unwrap().backup_dir,
            PathBuf::from("/srv/backups")
        );
    }

    #[test]
    fn header_mode_defaults_the_header_and_takes_a_single_ip() {
        let c = config(&[
            ("BUDGET_AUTH_MODE", "header"),
            ("BUDGET_TRUSTED_PROXY", "10.0.0.5"),
        ])
        .unwrap();
        assert_eq!(
            c.auth,
            Auth::Header {
                trusted_proxy: "10.0.0.5/32".parse().unwrap(),
                header: HeaderName::from_static("remote-user"),
            }
        );
    }

    #[test]
    fn invalid_values_name_the_variable() {
        error_names(&[("BUDGET_AUTH_MODE", "nonsense")], "BUDGET_AUTH_MODE");
        error_names(&[("BUDGET_LISTEN", "8080")], "BUDGET_LISTEN");
        error_names(
            &[("BUDGET_BASE_URL", "budget.example.test")],
            "BUDGET_BASE_URL",
        );
        error_names(&[("BUDGET_AUTH_MODE", "header")], "BUDGET_TRUSTED_PROXY");
        error_names(
            &[
                ("BUDGET_AUTH_MODE", "header"),
                ("BUDGET_TRUSTED_PROXY", "proxy"),
            ],
            "BUDGET_TRUSTED_PROXY",
        );
        error_names(
            &[
                ("BUDGET_AUTH_MODE", "header"),
                ("BUDGET_TRUSTED_PROXY", "10.0.0.5"),
                ("BUDGET_AUTH_HEADER", "bad header"),
            ],
            "BUDGET_AUTH_HEADER",
        );
    }

    #[test]
    fn the_message_says_what_was_expected() {
        assert_eq!(
            config(&[("BUDGET_AUTH_MODE", "nonsense")])
                .unwrap_err()
                .to_string(),
            "BUDGET_AUTH_MODE is \"nonsense\"; expected builtin or header"
        );
    }
}
