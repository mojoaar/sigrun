//! Build the `ssh` command line for a connection and open it in the user's terminal.

use std::path::Path;
use std::process::Command;

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
            let cmd = applescript_escape(&cmd);
            let (app, script) = if configured == Some("iterm") {
                (
                    "iTerm",
                    format!(
                        "tell application \"iTerm\" to create window with default profile command \"{}\"",
                        cmd
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
