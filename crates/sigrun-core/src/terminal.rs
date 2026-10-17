//! Build the `ssh` command line for a connection and open it in the user's terminal.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::model::{Connection, KeyMetadata, KeySource};

/// The platform Sigrun is running on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Linux,
    Windows,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// One candidate terminal invocation, tried in order until one spawns.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchSpec {
    pub program: String,
    pub args: Vec<String>,
}

/// `[A-Za-z0-9._-]`, non-empty — the only bytes allowed in a host or user.
fn safe_host_bytes(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Build the `ssh` argv for a connection. This is the injection boundary: host
/// and user are validated here, and the terminal path re-escapes on top.
pub fn ssh_invocation(conn: &Connection, key: Option<&KeyMetadata>) -> Result<Vec<String>> {
    if !safe_host_bytes(&conn.host) {
        return Err(Error::InvalidInput(
            "host contains unsupported characters".into(),
        ));
    }
    if !conn.user.is_empty() && !safe_host_bytes(&conn.user) {
        return Err(Error::InvalidInput(
            "user contains unsupported characters".into(),
        ));
    }
    if conn.port == 0 {
        return Err(Error::InvalidInput(
            "port must be between 1 and 65535".into(),
        ));
    }

    let target = if conn.user.is_empty() {
        conn.host.clone()
    } else {
        format!("{}@{}", conn.user, conn.host)
    };

    let mut argv = vec![
        "ssh".to_string(),
        target,
        "-p".to_string(),
        conn.port.to_string(),
    ];

    // Only an on-disk key contributes `-i`; a vault key is served by the agent.
    if let Some(KeyMetadata {
        source: KeySource::Disk,
        disk_path: Some(path),
        ..
    }) = key
    {
        argv.push("-i".to_string());
        argv.push(path.clone());
        argv.push("-o".to_string());
        argv.push("IdentitiesOnly=yes".to_string());
    }

    Ok(argv)
}

/// Build the `Unreachable` error for a failed probe, keeping the OS message for
/// every failure except a plain refusal (where the concise wording is enough).
fn probe_failure(host: &str, port: u16, err: Option<&std::io::Error>) -> Error {
    Error::Unreachable(match err {
        None => format!("could not resolve {host}"),
        Some(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
            format!("connection refused on {host}:{port}")
        }
        Some(e) => format!("cannot reach {host}:{port}: {e}"),
    })
}

/// Check that a TCP connection to `host:port` can be established within
/// `timeout`. Carries no secret: the message names host and port only.
pub async fn probe_reachable(host: &str, port: u16, timeout: std::time::Duration) -> Result<()> {
    let attempt = async {
        let addrs = tokio::net::lookup_host((host, port))
            .await
            .map_err(|e| probe_failure(host, port, Some(&e)))?;
        let mut last_error = None;
        for addr in addrs {
            match tokio::net::TcpStream::connect(addr).await {
                Ok(_) => return Ok(()),
                Err(e) => last_error = Some(e),
            }
        }
        Err(probe_failure(host, port, last_error.as_ref()))
    };
    match tokio::time::timeout(timeout, attempt).await {
        Ok(result) => result,
        Err(_) => Err(Error::Unreachable(format!(
            "connection timed out on {host}:{port}"
        ))),
    }
}

fn shell_safe_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'.' | b'_' | b'/' | b':' | b'@' | b'%' | b'+' | b',' | b'-'
        )
}

/// POSIX single-quote an argv element unless it is already shell-safe.
pub fn shell_escape(value: &str) -> String {
    if value.bytes().all(shell_safe_byte) {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

/// Join shell-escaped argv elements into a single command string.
pub fn render_command(argv: &[String]) -> String {
    argv.iter()
        .map(|a| shell_escape(a))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Escape a string for embedding inside a double-quoted AppleScript string.
fn applescript_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Candidate terminal launch specs for this platform, in fallback order.
pub fn terminal_launch_specs(
    platform: Platform,
    configured: Option<&str>,
    argv: &[String],
) -> Result<Vec<LaunchSpec>> {
    let cmd = render_command(argv);
    match platform {
        Platform::MacOs => {
            // CLI terminals run the command directly; the two GUI apps use AppleScript.
            match configured {
                Some("ghostty") => {
                    let mut args = vec!["-e".to_string()];
                    args.extend(argv.iter().cloned());
                    return Ok(vec![LaunchSpec {
                        program: "ghostty".into(),
                        args,
                    }]);
                }
                Some("kitty") => {
                    return Ok(vec![LaunchSpec {
                        program: "kitty".into(),
                        args: argv.to_vec(),
                    }]);
                }
                Some("alacritty") => {
                    let mut args = vec!["-e".to_string()];
                    args.extend(argv.iter().cloned());
                    return Ok(vec![LaunchSpec {
                        program: "alacritty".into(),
                        args,
                    }]);
                }
                Some("wezterm") => {
                    let mut args = vec!["start".to_string(), "--".to_string()];
                    args.extend(argv.iter().cloned());
                    return Ok(vec![LaunchSpec {
                        program: "wezterm".into(),
                        args,
                    }]);
                }
                _ => {}
            }
            let cmd = applescript_escape(&cmd);
            let (app, script) = if configured == Some("iterm") {
                (
                    "iTerm",
                    // A tab in the current window when one is open; a window
                    // otherwise, so the first launch is not a silent no-op.
                    format!(
                        r#"tell application "iTerm"
  if (count of windows) is 0 then
    create window with default profile command "{cmd}"
  else
    tell current window to create tab with default profile command "{cmd}"
  end if
end tell"#
                    ),
                )
            } else {
                (
                    "Terminal",
                    format!("tell application \"Terminal\" to do script \"{}\"", cmd),
                )
            };
            Ok(vec![LaunchSpec {
                program: "osascript".into(),
                args: vec![
                    "-e".into(),
                    format!("tell application \"{app}\" to activate"),
                    "-e".into(),
                    script,
                    "-e".into(),
                    format!("tell application \"{app}\" to activate"),
                ],
            }])
        }
        Platform::Linux => {
            // Fallback chain, deduplicated by name, first occurrence wins.
            let mut programs: Vec<String> = Vec::new();
            let push = |program: String, programs: &mut Vec<String>| {
                if !program.is_empty() && !programs.contains(&program) {
                    programs.push(program);
                }
            };
            if let Some(c) = configured {
                push(c.to_string(), &mut programs);
            }
            if let Ok(term) = std::env::var("TERMINAL") {
                push(term, &mut programs);
            }
            for p in ["gnome-terminal", "konsole", "x-terminal-emulator", "xterm"] {
                push(p.to_string(), &mut programs);
            }
            Ok(programs
                .into_iter()
                .map(|program| {
                    let args = if program == "gnome-terminal" {
                        vec![
                            "--".to_string(),
                            "sh".to_string(),
                            "-c".to_string(),
                            cmd.clone(),
                        ]
                    } else {
                        vec![
                            "-e".to_string(),
                            "sh".to_string(),
                            "-c".to_string(),
                            cmd.clone(),
                        ]
                    };
                    LaunchSpec { program, args }
                })
                .collect())
        }
        Platform::Windows => match configured {
            Some(prog) if !prog.is_empty() => Ok(vec![LaunchSpec {
                program: prog.to_string(),
                args: vec!["/k".into(), cmd],
            }]),
            _ => Ok(vec![
                LaunchSpec {
                    program: "wt.exe".into(),
                    args: vec![
                        "new-tab".into(),
                        "--".into(),
                        "cmd.exe".into(),
                        "/k".into(),
                        cmd.clone(),
                    ],
                },
                LaunchSpec {
                    program: "cmd.exe".into(),
                    args: vec!["/k".into(), cmd],
                },
            ]),
        },
    }
}

/// A terminal the user can pick in Settings, discovered as installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalInfo {
    /// The value stored in `Settings.terminal` (program name / override id).
    pub id: String,
    /// Human-readable name for the UI.
    pub name: String,
}

/// Terminals we can launch, filtered to the ones actually installed.
/// Read-only: filesystem and `PATH` checks only, never spawns anything.
pub fn detect_terminals(platform: Platform) -> Vec<TerminalInfo> {
    let on_path = |prog: &str| -> bool { which(prog) };
    match platform {
        Platform::MacOs => {
            let mut out = vec![TerminalInfo {
                id: "terminal".into(),
                name: "Terminal".into(),
            }];
            for (id, name, present) in [
                (
                    "iterm",
                    "iTerm",
                    Path::new("/Applications/iTerm.app").is_dir(),
                ),
                (
                    "ghostty",
                    "Ghostty",
                    Path::new("/Applications/Ghostty.app").is_dir() || on_path("ghostty"),
                ),
                (
                    "kitty",
                    "kitty",
                    Path::new("/Applications/kitty.app").is_dir() || on_path("kitty"),
                ),
                (
                    "alacritty",
                    "Alacritty",
                    Path::new("/Applications/Alacritty.app").is_dir() || on_path("alacritty"),
                ),
                (
                    "wezterm",
                    "WezTerm",
                    Path::new("/Applications/WezTerm.app").is_dir() || on_path("wezterm"),
                ),
            ] {
                if present {
                    out.push(TerminalInfo {
                        id: id.into(),
                        name: name.into(),
                    });
                }
            }
            out
        }
        Platform::Linux => ["gnome-terminal", "konsole", "x-terminal-emulator", "xterm"]
            .iter()
            .filter(|p| on_path(p))
            .map(|p| TerminalInfo {
                id: (*p).into(),
                name: (*p).into(),
            })
            .collect(),
        Platform::Windows => {
            let mut out = Vec::new();
            if on_path("wt.exe") {
                out.push(TerminalInfo {
                    id: "wt.exe".into(),
                    name: "Windows Terminal".into(),
                });
            }
            out.push(TerminalInfo {
                id: "cmd.exe".into(),
                name: "Command Prompt".into(),
            });
            out
        }
    }
}

/// True when `prog` is an executable on `PATH` (best-effort, no spawn).
#[cfg(unix)]
fn which(prog: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let p = dir.join(prog);
        p.is_file()
            && p.metadata()
                .map(|m| m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
    })
}

#[cfg(windows)]
fn which(prog: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(prog).is_file())
}

/// Spawn the first spec that launches, exposing the agent socket when given.
pub fn launch_first(specs: &[LaunchSpec], agent_socket: Option<&Path>) -> Result<()> {
    let mut last_error: Option<std::io::Error> = None;
    for spec in specs {
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args);
        if let Some(sock) = agent_socket {
            cmd.env("SSH_AUTH_SOCK", sock);
        }
        match cmd.spawn() {
            Ok(_) => return Ok(()),
            Err(e) => last_error = Some(e),
        }
    }
    let err = last_error.unwrap_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "no terminal candidates")
    });
    Err(Error::Config(format!("could not launch a terminal: {err}")))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::model::KeyAlgorithm;
    use std::time::Duration;

    #[tokio::test]
    async fn probe_reports_a_closed_port() {
        let err = probe_reachable("127.0.0.1", 1, Duration::from_millis(500))
            .await
            .unwrap_err();
        let Error::Unreachable(m) = err else {
            panic!("unexpected {err:?}");
        };
        assert!(m.contains("127.0.0.1"), "message was {m:?}");
    }

    #[tokio::test]
    async fn probe_reachability_resolves_a_bad_name() {
        // `.invalid` is reserved and never resolves.
        let err = probe_reachable("sigrun.invalid", 22, Duration::from_millis(500))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Unreachable(_)));
    }

    #[test]
    fn probe_failures_keep_the_os_error() {
        let denied = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "not permitted");
        let Error::Unreachable(m) = probe_failure("h", 22, Some(&denied)) else {
            panic!("expected unreachable");
        };
        assert_eq!(m, "cannot reach h:22: not permitted");

        let refused = std::io::Error::from(std::io::ErrorKind::ConnectionRefused);
        let Error::Unreachable(m) = probe_failure("h", 22, Some(&refused)) else {
            panic!("expected unreachable");
        };
        assert_eq!(m, "connection refused on h:22");
    }

    #[test]
    fn ssh_invocation_uses_port_and_disk_key() {
        let mut c = Connection::managed("id", "web");
        c.host = "example.com".into();
        c.user = "deploy".into();
        c.port = 2222;
        let mut k = KeyMetadata::now(
            "k",
            KeyAlgorithm::Ed25519,
            "ssh-ed25519 AAAA",
            "SHA256:x",
            KeySource::Disk,
        );
        k.disk_path = Some("/home/u/.ssh/id_work".into());
        assert_eq!(
            ssh_invocation(&c, Some(&k)).unwrap(),
            vec![
                "ssh",
                "deploy@example.com",
                "-p",
                "2222",
                "-i",
                "/home/u/.ssh/id_work",
                "-o",
                "IdentitiesOnly=yes"
            ]
        );
    }

    #[test]
    fn vault_key_is_served_by_the_agent_without_dash_i() {
        let c = Connection::managed("id", "web");
        let k = KeyMetadata::now(
            "k",
            KeyAlgorithm::Ed25519,
            "ssh-ed25519 AAAA",
            "SHA256:x",
            KeySource::Vault,
        );
        let argv = ssh_invocation(&c, Some(&k)).unwrap();
        assert!(!argv.iter().any(|a| a == "-i"));
        assert!(!argv.iter().any(|a| a == "IdentitiesOnly=yes"));
    }

    #[test]
    fn shell_metacharacters_are_rejected_not_injected() {
        let mut c = Connection::managed("id", "web");
        c.host = "example.com; rm -rf /".into();
        assert!(matches!(
            ssh_invocation(&c, None),
            Err(Error::InvalidInput(_))
        ));
        let mut c = Connection::managed("id", "web");
        c.user = "deploy$(id)".into();
        assert!(matches!(
            ssh_invocation(&c, None),
            Err(Error::InvalidInput(_))
        ));
    }

    #[test]
    fn escaping_quotes_posix_metacharacters() {
        assert_eq!(shell_escape("plain-host"), "plain-host");
        assert_eq!(shell_escape("a b"), "'a b'");
        assert_eq!(shell_escape("a'b"), "'a'\\''b'");
    }

    #[test]
    fn macos_uses_terminal_app_by_default() {
        let specs =
            terminal_launch_specs(Platform::MacOs, None, &["ssh".into(), "u@h".into()]).unwrap();
        assert_eq!(specs[0].program, "osascript");
        assert!(specs[0].args.iter().any(|a| a.contains("Terminal")));
    }

    #[test]
    fn macos_iterm_reuses_a_tab_and_still_handles_no_windows() {
        let specs = terminal_launch_specs(
            Platform::MacOs,
            Some("iterm"),
            &["ssh".into(), "u@h".into(), "-p".into(), "22".into()],
        )
        .unwrap();
        assert_eq!(specs[0].program, "osascript");
        let script = &specs[0].args[3];
        assert!(
            script.contains("create tab with default profile"),
            "script was {script:?}"
        );
        assert!(
            script.contains("(count of windows) is 0"),
            "script was {script:?}"
        );
    }

    #[test]
    fn linux_prefers_override_then_env_then_known_terminals() {
        let specs =
            terminal_launch_specs(Platform::Linux, Some("konsole"), &["ssh".into()]).unwrap();
        assert_eq!(specs[0].program, "konsole");
        assert!(specs.iter().any(|s| s.program == "gnome-terminal"));
    }

    #[test]
    fn windows_prefers_windows_terminal() {
        let specs = terminal_launch_specs(Platform::Windows, None, &["ssh".into()]).unwrap();
        assert_eq!(specs[0].program, "wt.exe");
        assert!(specs.iter().any(|s| s.program == "cmd.exe"));
    }

    #[test]
    fn detect_terminals_returns_launchable_terminals() {
        // Every detected id must be one `terminal_launch_specs` can build a
        // spec for, and the platform's built-in is always present.
        let platform = Platform::current();
        let found = detect_terminals(platform);
        assert!(!found.is_empty());
        for t in &found {
            let specs = terminal_launch_specs(platform, Some(&t.id), &["ssh".into(), "u@h".into()]);
            assert!(specs.is_ok(), "{} should be launchable", t.id);
        }
        let builtin = match platform {
            Platform::MacOs => "terminal",
            Platform::Windows => "cmd.exe",
            Platform::Linux => found[0].id.as_str(), // whatever was found first
        };
        assert!(found.iter().any(|t| t.id == builtin));
    }

    #[cfg(unix)]
    #[test]
    fn launch_first_falls_back_to_next_spec() {
        let specs = vec![
            LaunchSpec {
                program: "sigrun-no-such-terminal-xyz".into(),
                args: vec![],
            },
            LaunchSpec {
                program: "true".into(),
                args: vec![],
            },
        ];
        assert!(launch_first(&specs, None).is_ok());

        let bad = vec![LaunchSpec {
            program: "sigrun-no-such-terminal-xyz".into(),
            args: vec![],
        }];
        assert!(matches!(launch_first(&bad, None), Err(Error::Config(_))));
    }
}
