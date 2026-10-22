//! Read-only parsing of `~/.ssh/config` hosts. Sigrun never writes to it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::error::Result;

/// One concrete host alias extracted from an `~/.ssh/config` file.
#[derive(Debug, Clone, PartialEq)]
pub struct SshHost {
    /// The user-facing alias this block was matched under.
    pub alias: String,
    /// Resolved target; defaults to the alias when no `HostName` is present.
    pub host_name: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    /// `IdentityFile` with a leading `~` expanded; shown in the UI, never adopted.
    pub identity_file: Option<PathBuf>,
}

/// Parse `~/.ssh/config` text into concrete host entries.
///
// shortcut: Include is not followed in v1; add when a user relies on it.
pub fn parse_ssh_config(text: &str) -> Vec<SshHost> {
    // First-appearance order, with first-seen values winning on repeated
    // aliases (OpenSSH applies every block for an alias, first value wins).
    let mut hosts: Vec<SshHost> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut current: Vec<usize> = Vec::new();
    let mut after_match = false;
    let mut explicit_hostname: HashSet<String> = HashSet::new();

    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        // OpenSSH accepts both `Key Value` and `Key=Value` (with optional
        // spaces around the `=`); only the first `=` splits key from value.
        let mut tokens = line.split_whitespace();
        let first = tokens.next().unwrap_or_default();
        let rest = tokens.collect::<Vec<_>>().join(" ");
        let (key, value) = match first.split_once('=') {
            // `Key= Value` leaves an empty v; the value sits in rest.
            Some((k, v)) if v.is_empty() && !rest.is_empty() => (k.to_ascii_lowercase(), rest),
            Some((k, v)) => (k.to_ascii_lowercase(), v.to_string()),
            // `Key = Value` leaves rest starting with a bare `=` token.
            None if rest.starts_with('=') => (
                first.to_ascii_lowercase(),
                rest.trim_start_matches('=').trim_start().to_string(),
            ),
            None => (first.to_ascii_lowercase(), rest),
        };

        match key.as_str() {
            // A Match line's options apply at ssh's discretion; never attach
            // them to the previous Host block.
            "match" => {
                after_match = true;
                current.clear();
            }
            "host" => {
                after_match = false;
                current.clear();
                for pattern in value.split_whitespace() {
                    // shortcut: `!negated` and wildcards are dropped entirely;
                    // add full pattern matching if a user's config needs it.
                    if pattern.contains(['*', '?', '!']) {
                        continue;
                    }
                    let i = match index.get(pattern) {
                        Some(&i) => i,
                        None => {
                            let i = hosts.len();
                            hosts.push(SshHost {
                                alias: pattern.to_string(),
                                host_name: pattern.to_string(),
                                user: None,
                                port: None,
                                identity_file: None,
                            });
                            index.insert(pattern.to_string(), i);
                            i
                        }
                    };
                    if !current.contains(&i) {
                        current.push(i);
                    }
                }
            }
            _ if after_match || current.is_empty() => {}
            _ => {
                for &i in &current {
                    apply_option(&mut hosts[i], &key, &value, &mut explicit_hostname);
                }
            }
        }
    }
    hosts
}

/// Record an option into a host slot, keeping only the first value per field.
fn apply_option(
    host: &mut SshHost,
    key: &str,
    value: &str,
    explicit_hostname: &mut HashSet<String>,
) {
    if value.is_empty() {
        return;
    }
    match key {
        // Explicit `HostName` wins only once; the alias default must not make
        // a later block think the first block's value was unset.
        "hostname" if !explicit_hostname.contains(&host.alias) => {
            explicit_hostname.insert(host.alias.clone());
            host.host_name = value.to_string();
        }
        "user" if host.user.is_none() => host.user = Some(value.to_string()),
        "port" if host.port.is_none() => host.port = value.parse().ok(),
        "identityfile" if host.identity_file.is_none() => {
            host.identity_file = Some(expand_tilde(value));
        }
        _ => {}
    }
}

/// Expand a leading `~/` or bare `~` using the user's home directory.
fn expand_tilde(value: &str) -> PathBuf {
    let Some(home) = dirs::home_dir() else {
        return PathBuf::from(value);
    };
    match value {
        "~" => home,
        v if v.starts_with("~/") => home.join(&v[2..]),
        _ => PathBuf::from(value),
    }
}

/// Parse the config at `config_path` (`~/.ssh/config` itself, not a directory).
/// A missing file yields no hosts; any other read failure is an `Error::Io`.
pub fn discover_ssh_config(config_path: &Path) -> Result<Vec<SshHost>> {
    match std::fs::read_to_string(config_path) {
        Ok(text) => Ok(parse_ssh_config(&text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Connection, ConnectionSource};

    #[test]
    fn parses_blocks_and_first_value_wins() {
        let text = r#"
Host web prod
    HostName 10.0.0.5
    HostName ignored.example
    User deploy
    Port 2222
    IdentityFile ~/.ssh/id_work

Host db
    HostName db.internal
"#;
        let hosts = parse_ssh_config(text);
        assert_eq!(hosts.len(), 3);
        let web = hosts.iter().find(|h| h.alias == "web").unwrap();
        assert_eq!(web.host_name, "10.0.0.5");
        assert_eq!(web.user.as_deref(), Some("deploy"));
        assert_eq!(web.port, Some(2222));
    }

    #[test]
    fn wildcard_patterns_are_never_connections() {
        let text = "Host *\n  User root\n\nHost app?\n  User x\n\nHost !bad good\n  User ok\n";
        let hosts = parse_ssh_config(text);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].alias, "good");
    }

    #[test]
    fn comments_and_global_defaults_are_ignored() {
        let text = "# global\nUser global\n\nHost box # trailing\n  HostName box.local\n";
        let hosts = parse_ssh_config(text);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].host_name, "box.local");
    }

    #[test]
    fn tilde_in_identity_file_is_expanded() {
        let home = dirs::home_dir().unwrap();
        let text = "Host k\n  IdentityFile ~/.ssh/id_work\n";
        let hosts = parse_ssh_config(text);
        assert_eq!(hosts[0].identity_file, Some(home.join(".ssh/id_work")));
    }

    #[test]
    fn adopt_copies_fields() {
        let h = SshHost {
            alias: "web".into(),
            host_name: "10.0.0.5".into(),
            user: Some("deploy".into()),
            port: Some(2222),
            identity_file: None,
        };
        let c = Connection::from_ssh_host(&h, "id1".into());
        assert_eq!(c.label, "web");
        assert_eq!(c.host, "10.0.0.5");
        assert_eq!(c.user, "deploy");
        assert_eq!(c.port, 2222);
        assert_eq!(c.source, ConnectionSource::Discovered);
    }

    #[test]
    fn match_line_ends_the_block() {
        let text = "Host a\n  HostName a.local\nMatch all\n  User ignored\nHost b\n  User bob\n";
        let hosts = parse_ssh_config(text);
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].user, None);
        assert_eq!(hosts[1].user.as_deref(), Some("bob"));
    }

    #[test]
    fn explicit_hostname_equal_to_alias_beats_later_block() {
        // First block's explicit HostName equals its alias; the alias
        // default must not make the second block overwrite it.
        let text =
            "Host github.com\n  HostName github.com\nHost github.com\n  HostName 192.168.1.5\n";
        let hosts = parse_ssh_config(text);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].host_name, "github.com");
    }

    #[test]
    fn repeated_aliases_merge_first_value_wins() {
        let text = "Host srv\n  HostName a.example\n  User al\nHost srv\n  User bob\n";
        let hosts = parse_ssh_config(text);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].host_name, "a.example");
        assert_eq!(hosts[0].user.as_deref(), Some("al"));
    }

    #[test]
    fn host_name_defaults_to_alias_and_invalid_port_is_ignored() {
        let text = "Host s\n  Port 99999\n";
        let hosts = parse_ssh_config(text);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].host_name, "s");
        assert_eq!(hosts[0].port, None);
    }

    #[test]
    fn parses_key_value_syntax() {
        let text = "Host=example.com\n  Port=2222\n  User=admin\n  IdentityFile=~/.ssh/id_rsa\n";
        let hosts = parse_ssh_config(text);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].alias, "example.com");
        assert_eq!(hosts[0].port, Some(2222));
        assert_eq!(hosts[0].user.as_deref(), Some("admin"));
        let home = dirs::home_dir().unwrap();
        assert_eq!(hosts[0].identity_file, Some(home.join(".ssh/id_rsa")));
    }

    #[test]
    fn parses_mixed_syntax() {
        let text = "Host example.com\n  Port=2222\n  User admin\n  IdentityFile ~/.ssh/id_rsa\n";
        let hosts = parse_ssh_config(text);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].alias, "example.com");
        assert_eq!(hosts[0].port, Some(2222));
        assert_eq!(hosts[0].user.as_deref(), Some("admin"));
        let home = dirs::home_dir().unwrap();
        assert_eq!(hosts[0].identity_file, Some(home.join(".ssh/id_rsa")));
    }

    #[test]
    fn handles_equals_in_value() {
        // ProxyCommand is not a field SshHost tracks; use HostName to prove
        // only the first `=` splits key from value.
        let text = "Host=example.com\n  HostName=foo=bar.example\n";
        let hosts = parse_ssh_config(text);
        assert_eq!(hosts[0].host_name, "foo=bar.example");
    }

    #[test]
    fn handles_spaces_around_equals() {
        let text = "Host example.com\n  Port = 2222\n  User = admin\n";
        let hosts = parse_ssh_config(text);
        assert_eq!(hosts[0].port, Some(2222));
        assert_eq!(hosts[0].user.as_deref(), Some("admin"));
    }

    #[test]
    fn adopt_defaults_match_managed_convention() {
        let h = SshHost {
            alias: "x".into(),
            host_name: "x".into(),
            user: None,
            port: None,
            identity_file: None,
        };
        let c = Connection::from_ssh_host(&h, "id2".into());
        assert_eq!(c.user, "");
        assert_eq!(c.port, 22);
        assert_eq!(c.key_id, None);
        assert_eq!(c.group, None);
        assert!(c.tags.is_empty());
    }
}
