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

    // A stored password is served by the askpass helper. Never let ssh try it
    // three times: one wrong attempt is enough to report the failure. Windows
    // has no helper yet, so it keeps ssh's normal retry count instead of
    // gaining a restriction that could only lock the user out sooner.
    if !cfg!(windows) && conn.has_password {
        argv.push("-o".to_string());
        argv.push("NumberOfPasswordPrompts=1".to_string());
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

/// The path to this running binary. A launched shell calls back into it for the
/// askpass helper and for the session reports; the bare name is a `PATH`
/// fallback for the rare case where the executable path cannot be read.
fn helper_exe() -> String {
    std::env::current_exe()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "sigrun".to_string())
}

/// The environment assignments that point ssh at the askpass helper. The helper
/// receives the connection id and socket; the password itself never appears.
/// `SIGRUN_ASKPASS=1` marks the call explicitly so a mistyped command in this
/// environment is not mistaken for the helper. One trailing space, so it can be
/// prepended to the command verbatim.
pub fn env_prefix(socket: &Path, connection_id: &str) -> String {
    format!(
        "SSH_ASKPASS={} SSH_ASKPASS_REQUIRE=force SIGRUN_ASKPASS=1 SIGRUN_CONNECTION_ID={} SIGRUN_SOCKET={} ",
        shell_escape(&helper_exe()),
        shell_escape(connection_id),
        shell_escape(&socket.to_string_lossy()),
    )
}

/// The `<exe> --socket <socket>` prefix both session reports share. Carries no
/// secret — only the running binary and the socket path.
fn report_prefix(socket: &Path) -> String {
    format!(
        "{} --socket {}",
        shell_escape(&helper_exe()),
        shell_escape(&socket.to_string_lossy()),
    )
}

/// The two fragments a launched line hands back to this binary: the shell's own
/// pid, reported before `ssh` runs, and the session's end. Carries no secret —
/// only the executable, the socket, the connection id, and the pid.
#[derive(Debug)]
pub struct SessionReports {
    /// `<exe> --socket <socket> session-started <id> $$`. The `$$` is expanded
    /// by the launched shell, so it stays outside any quoting.
    pub started: String,
    /// `<exe> --socket <socket> connection-ended <id>`.
    pub ended: String,
}

/// The reports to embed in a launch line, or `None` on Windows: `cmd.exe /k`
/// gives no way to chain one, so the active flag is only set where the launched
/// line can clear it again.
pub fn launch_reports(
    platform: Platform,
    socket: &Path,
    connection_id: &str,
) -> Option<SessionReports> {
    let prefix = report_prefix(socket);
    let id = shell_escape(connection_id);
    (platform != Platform::Windows).then(|| SessionReports {
        started: format!("{prefix} session-started {id} $$"),
        ended: format!("{prefix} connection-ended {id}"),
    })
}

/// Candidate terminal launch specs for this platform, in fallback order.
/// `open_in` is the user's "window" | "tab" preference; only the terminals that
/// expose a tab API act on it, and anything that is not `"tab"` means a window.
pub fn terminal_launch_specs(
    platform: Platform,
    configured: Option<&str>,
    argv: &[String],
    env_prefix: &str,
    reports: Option<&SessionReports>,
    open_in: &str,
) -> Result<Vec<LaunchSpec>> {
    // The prefix must come first so it applies to the `ssh` command, which is
    // re-escaped here; it ends with a space already.
    let cmd = format!("{env_prefix}{}", render_command(argv));
    // A failed connection must not take the window with it: Ghostty shows a
    // "failed to launch the requested command" screen when ssh exits non-zero
    // within a few frames, and other terminals simply close, hiding the error.
    // Drop into a shell instead so the reason stays on screen.
    let keep_open = match reports {
        // The end report must run before the shell takes over, and `rc` keeps
        // the failure path exactly as it was: drop into a shell so the error
        // stays on screen instead of the window closing.
        //
        // The trap is what makes the dot go out when the window is closed: that
        // kills this shell with SIGHUP, and without a trap it dies before the
        // end report below ever runs.
        //
        // The pid report runs before `ssh`, so a session killed the instant it
        // starts is still watched.
        Some(reports) => format!(
            "trap {} HUP TERM INT; {}; {cmd}; rc=$?; {}; [ $rc -ne 0 ] && exec ${{SHELL:-/bin/sh}}; exit $rc",
            shell_escape(&format!("{}; exit", reports.ended)),
            reports.started,
            reports.ended,
        ),
        None => format!("{cmd} || exec ${{SHELL:-/bin/sh}}"),
    };
    match platform {
        Platform::MacOs => {
            // CLI terminals run the command directly; the two GUI apps use AppleScript.
            match configured {
                Some("ghostty") => {
                    if open_in != "tab" {
                        return Ok(vec![LaunchSpec {
                            program: "ghostty".into(),
                            args: vec![
                                "-e".into(),
                                "/bin/sh".into(),
                                "-c".into(),
                                keep_open.clone(),
                            ],
                        }]);
                    }
                    // Ghostty's AppleScript API. `input text` does not execute in
                    // a tab this script creates (verified against Ghostty 1.3.1),
                    // so the tab is created with the line as its process
                    // instead, and `wait after command` is Ghostty's own
                    // keep-open, the equivalent of the shell chain elsewhere.
                    // The line goes through `/bin/sh -c` for the same
                    // single-parse reason as the other terminals, and it must
                    // arrive there as one single-quoted literal: Ghostty parses
                    // this `command` with its own shell first, so a `"…"`
                    // wrapper is expanded *there* and `$?`, `$rc` and
                    // `${SHELL}` never reach our shell intact (verified against
                    // Ghostty 1.3.1).
                    let command =
                        applescript_escape(&format!("/bin/sh -c {}", shell_escape(&keep_open)));
                    let script = format!(
                        r#"tell application "Ghostty"
  if (count of windows) is 0 then
    new window with configuration {{command:"{command}", wait after command:true}}
  else
    new tab in front window with configuration {{command:"{command}", wait after command:true}}
  end if
end tell"#
                    );
                    return Ok(vec![LaunchSpec {
                        program: "osascript".into(),
                        args: vec!["-e".into(), script],
                    }]);
                }
                Some("kitty") => {
                    return Ok(vec![LaunchSpec {
                        program: "kitty".into(),
                        args: vec!["/bin/sh".into(), "-c".into(), keep_open.clone()],
                    }]);
                }
                Some("alacritty") => {
                    return Ok(vec![LaunchSpec {
                        program: "alacritty".into(),
                        args: vec![
                            "-e".into(),
                            "/bin/sh".into(),
                            "-c".into(),
                            keep_open.clone(),
                        ],
                    }]);
                }
                Some("wezterm") => {
                    return Ok(vec![LaunchSpec {
                        program: "wezterm".into(),
                        args: vec![
                            "start".into(),
                            "--".into(),
                            "/bin/sh".into(),
                            "-c".into(),
                            keep_open.clone(),
                        ],
                    }]);
                }
                _ => {}
            }
            let cmd = applescript_escape(&keep_open);
            let (app, script) = if configured == Some("iterm") {
                // A window when asked; otherwise a tab in the current window and
                // a window when none is open, so the first launch is not a
                // silent no-op.
                let script = if open_in == "tab" {
                    format!(
                        r#"tell application "iTerm"
  if (count of windows) is 0 then
    create window with default profile command "{cmd}"
  else
    tell current window to create tab with default profile command "{cmd}"
  end if
end tell"#
                    )
                } else {
                    format!(
                        r#"tell application "iTerm"
  create window with default profile command "{cmd}"
end tell"#
                    )
                };
                ("iTerm", script)
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
                            keep_open.clone(),
                        ]
                    } else {
                        vec![
                            "-e".to_string(),
                            "sh".to_string(),
                            "-c".to_string(),
                            keep_open.clone(),
                        ]
                    };
                    LaunchSpec { program, args }
                })
                .collect())
        }
        Platform::Windows => {
            // `cmd.exe` does not understand POSIX env assignments or the
            // keep-open chain, so Windows keeps the bare command.
            let cmd = render_command(argv);
            match configured {
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
            }
        }
    }
}

/// A terminal the user can pick in Settings, discovered as installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalInfo {
    /// The value stored in `Settings.terminal` (program name / override id).
    pub id: String,
    /// Human-readable name for the UI.
    pub name: String,
    /// Whether *this build* can ask this terminal for a tab, not whether the
    /// terminal has tabs: Terminal.app has tabs and cannot be asked for one.
    pub tabs: bool,
}

/// Whether the launch arm for `(platform, id)` honours `open_in == "tab"`.
/// A pure function of the pair — never a check of what is installed.
fn supports_tabs(platform: Platform, id: &str) -> bool {
    matches!(
        (platform, id),
        (Platform::MacOs, "iterm") | (Platform::MacOs, "ghostty")
    )
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
                tabs: supports_tabs(Platform::MacOs, "terminal"),
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
                        tabs: supports_tabs(Platform::MacOs, id),
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
                tabs: supports_tabs(Platform::Linux, p),
            })
            .collect(),
        Platform::Windows => {
            let mut out = Vec::new();
            if on_path("wt.exe") {
                out.push(TerminalInfo {
                    id: "wt.exe".into(),
                    name: "Windows Terminal".into(),
                    tabs: supports_tabs(Platform::Windows, "wt.exe"),
                });
            }
            out.push(TerminalInfo {
                id: "cmd.exe".into(),
                name: "Command Prompt".into(),
                tabs: supports_tabs(Platform::Windows, "cmd.exe"),
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
        let specs = terminal_launch_specs(
            Platform::MacOs,
            None,
            &["ssh".into(), "u@h".into()],
            "",
            None,
            "window",
        )
        .unwrap();
        assert_eq!(specs[0].program, "osascript");
        assert!(specs[0].args.iter().any(|a| a.contains("Terminal")));
    }

    #[test]
    fn launches_keep_the_window_open_when_the_connection_fails() {
        let argv = ["ssh".to_string(), "u@h".to_string()];
        // A failed connect must not close the terminal before the error is read.
        for configured in [None, Some("iterm"), Some("ghostty"), Some("kitty")] {
            let specs =
                terminal_launch_specs(Platform::MacOs, configured, &argv, "", None, "window")
                    .unwrap();
            assert!(
                specs[0]
                    .args
                    .join(" ")
                    .contains("|| exec ${SHELL:-/bin/sh}"),
                "macos {configured:?} should keep the window open"
            );
        }
        for spec in terminal_launch_specs(Platform::Linux, None, &argv, "", None, "window").unwrap()
        {
            assert!(spec.args.join(" ").contains("|| exec ${SHELL:-/bin/sh}"));
        }
        // Windows keeps its window with `cmd.exe /k`.
        for spec in
            terminal_launch_specs(Platform::Windows, None, &argv, "", None, "window").unwrap()
        {
            assert!(spec.args.contains(&"/k".to_string()));
        }
        // Off Windows the launched line always carries a report; a failed
        // connect must still keep the window open in that shape too.
        let reports = launch_reports(Platform::MacOs, Path::new("/tmp/h.sock"), "c1").unwrap();
        for configured in [None, Some("iterm"), Some("ghostty"), Some("kitty")] {
            let specs = terminal_launch_specs(
                Platform::MacOs,
                configured,
                &argv,
                "",
                Some(&reports),
                "window",
            )
            .unwrap();
            assert!(
                specs[0]
                    .args
                    .join(" ")
                    .contains("[ $rc -ne 0 ] && exec ${SHELL:-/bin/sh}"),
                "macos {configured:?} should keep the window open with a report"
            );
        }
        for spec in
            terminal_launch_specs(Platform::Linux, None, &argv, "", Some(&reports), "window")
                .unwrap()
        {
            assert!(spec
                .args
                .join(" ")
                .contains("[ $rc -ne 0 ] && exec ${SHELL:-/bin/sh}"));
        }
    }

    #[test]
    fn the_launch_line_carries_the_askpass_env_and_survives_every_terminal() {
        let mut conn = Connection::managed("c1", "web");
        conn.host = "example.com".into();
        conn.user = "deploy".into();
        conn.has_password = true;
        let argv = ssh_invocation(&conn, None).unwrap();
        let prefix = env_prefix(Path::new("/tmp/host.sock"), "c1");
        assert!(prefix.starts_with("SSH_ASKPASS="));
        assert!(prefix.contains("SSH_ASKPASS_REQUIRE=force"));
        assert!(prefix.contains("SIGRUN_CONNECTION_ID=c1"));
        assert!(prefix.contains("SIGRUN_SOCKET=/tmp/host.sock"));
        // The marker makes the helper call unambiguous; see `env_prefix`.
        assert!(prefix.contains("SIGRUN_ASKPASS=1"));

        // The prefix must precede the command everywhere, so it can never end
        // up after the keep-open suffix (which would leave it trailing).
        let prefix_first = |line: &str, label: &str| {
            let prefix_at = line
                .find("SSH_ASKPASS=")
                .unwrap_or_else(|| panic!("{label} should carry the prefix: {line}"));
            let keep_open_at = line
                .find("|| exec")
                .unwrap_or_else(|| panic!("{label} should keep the window open: {line}"));
            assert!(prefix_at < keep_open_at, "{label}: {line}");
        };

        for configured in [None, Some("iterm")] {
            let specs =
                terminal_launch_specs(Platform::MacOs, configured, &argv, &prefix, None, "window")
                    .unwrap();
            let line = specs[0].args.join(" ");
            prefix_first(&line, &format!("macos {configured:?}"));
            // The AppleScript `do script` string ends with the command chain.
            assert!(
                line.contains("|| exec ${SHELL:-/bin/sh}\""),
                "macos {configured:?}: {line}"
            );
        }
        // Every `-e /bin/sh -c` and `sh -c` path carries it too, and the
        // keep-open suffix is the last thing the shell sees.
        for configured in [
            Some("ghostty"),
            Some("kitty"),
            Some("alacritty"),
            Some("wezterm"),
        ] {
            let specs =
                terminal_launch_specs(Platform::MacOs, configured, &argv, &prefix, None, "window")
                    .unwrap();
            let line = specs[0].args.join(" ");
            prefix_first(&line, &format!("macos {configured:?}"));
            assert!(
                line.trim_end().ends_with("|| exec ${SHELL:-/bin/sh}"),
                "macos {configured:?}: {line}"
            );
        }
        for spec in
            terminal_launch_specs(Platform::Linux, None, &argv, &prefix, None, "window").unwrap()
        {
            let line = spec.args.join(" ");
            prefix_first(&line, &format!("linux {}", spec.program));
            assert!(
                line.trim_end().ends_with("|| exec ${SHELL:-/bin/sh}"),
                "linux {}: {line}",
                spec.program
            );
        }
        // With a report the prefix still comes first and the keep-open chain —
        // now built around `rc` — still ends the line.
        let reports = launch_reports(Platform::MacOs, Path::new("/tmp/host.sock"), "c1").unwrap();
        let specs = terminal_launch_specs(
            Platform::MacOs,
            Some("ghostty"),
            &argv,
            &prefix,
            Some(&reports),
            "window",
        )
        .unwrap();
        let line = specs[0].args.last().unwrap();
        // The trap runs before the command, so the prefix no longer leads the
        // line; it still has to be attached to the `ssh` invocation itself.
        let prefix_at = line.find(&prefix).expect("the askpass env is still there");
        let ssh_at = line.find("ssh deploy@example.com").expect("ssh runs");
        assert!(prefix_at < ssh_at, "{line}");
        assert!(
            line.ends_with("[ $rc -ne 0 ] && exec ${SHELL:-/bin/sh}; exit $rc"),
            "{line}"
        );
        // No stored password: the line is unchanged and ssh prompts as before.
        let plain =
            terminal_launch_specs(Platform::MacOs, None, &argv, "", None, "window").unwrap();
        assert!(!plain[0].args.join(" ").contains("SSH_ASKPASS="));
    }

    #[test]
    fn the_launch_line_reports_the_session_start_and_end_to_the_host() {
        let reports = launch_reports(Platform::MacOs, Path::new("/tmp/h.sock"), "c1").unwrap();
        let specs = terminal_launch_specs(
            Platform::MacOs,
            None,
            &["ssh".into(), "u@h".into()],
            "",
            Some(&reports),
            "window",
        )
        .unwrap();
        let line = specs[0].args.join(" ");
        // The pid is the shell's own (`$$`), so the host can watch it: a killed
        // terminal group cannot report its own end.
        assert!(line.contains("session-started c1 $$"), "{line}");
        let started_at = line
            .find("session-started c1")
            .expect("the pid is reported");
        let ssh_at = line.find("ssh u@h").expect("ssh runs");
        assert!(started_at < ssh_at, "{line}");
        assert!(line.contains("connection-ended c1"), "{line}");
        // Closing the window kills this shell with SIGHUP before the end report
        // below runs, so the trap has to be installed first and report the end
        // itself — and the pid report must sit after the trap but before `ssh`,
        // so a session killed instantly is still watched.
        let trap_at = line
            .find(&format!("trap '{}; exit' HUP TERM INT;", reports.ended))
            .expect("the trap is installed first");
        assert!(trap_at < started_at && started_at < ssh_at, "{line}");
        // The rc is captured right after the command and the end report runs
        // before the failure path can take over the shell.
        let rc_at = line.find("rc=$?").expect("rc is captured");
        // The trap carries a copy of the end report as well, so the ordered pair
        // we care about is the trailing one: rc, then the report, then the
        // failure path.
        let ended_at = line
            .rfind("connection-ended c1")
            .expect("the end is reported");
        let keep_at = line
            .find("[ $rc -ne 0 ] && exec ${SHELL:-/bin/sh}; exit $rc")
            .expect("the failure path survives");
        assert!(rc_at < ended_at && ended_at < keep_at, "{line}");
    }

    #[test]
    fn a_launch_line_without_a_report_keeps_the_plain_keep_open_suffix() {
        let specs = terminal_launch_specs(
            Platform::MacOs,
            None,
            &["ssh".into(), "u@h".into()],
            "",
            None,
            "window",
        )
        .unwrap();
        let line = specs[0].args.join(" ");
        assert!(line.contains("ssh u@h || exec ${SHELL:-/bin/sh}"), "{line}");
        assert!(!line.contains("rc=$?"), "{line}");
        assert!(!line.contains("session-started"), "{line}");
    }

    #[test]
    fn the_reports_escape_the_socket_and_the_id() {
        let reports = launch_reports(Platform::MacOs, Path::new("/tmp/a b.sock"), "c 1").unwrap();
        assert!(
            reports.started.contains("--socket '/tmp/a b.sock'"),
            "{}",
            reports.started
        );
        assert!(
            reports.started.ends_with("session-started 'c 1' $$"),
            "{}",
            reports.started
        );
        assert!(
            reports.ended.contains("--socket '/tmp/a b.sock'"),
            "{}",
            reports.ended
        );
        assert!(
            reports.ended.ends_with("connection-ended 'c 1'"),
            "{}",
            reports.ended
        );
    }

    #[test]
    fn the_reports_are_built_only_where_the_launched_line_can_send_them() {
        // Windows keeps `cmd.exe /k` and cannot chain a report, so the host
        // must not mark a session active there.
        assert!(
            launch_reports(Platform::Windows, Path::new("/tmp/h.sock"), "c1").is_none(),
            "windows cannot report a session"
        );
        for platform in [Platform::MacOs, Platform::Linux] {
            let reports = launch_reports(platform, Path::new("/tmp/h.sock"), "c1")
                .unwrap_or_else(|| panic!("{platform:?} should carry a report"));
            assert!(
                reports.started.contains("--socket /tmp/h.sock"),
                "{}",
                reports.started
            );
            // The pid is expanded by our own shell, so it is appended bare.
            assert!(
                reports.started.ends_with("session-started c1 $$"),
                "{}",
                reports.started
            );
            assert!(
                reports.ended.ends_with("connection-ended c1"),
                "{}",
                reports.ended
            );
        }
    }

    #[test]
    #[cfg(not(windows))]
    fn a_stored_password_limits_ssh_to_one_prompt() {
        let mut conn = Connection::managed("c1", "web");
        let plain = ssh_invocation(&conn, None).unwrap();
        assert!(!plain.iter().any(|a| a == "NumberOfPasswordPrompts=1"));

        conn.has_password = true;
        let argv = ssh_invocation(&conn, None).unwrap();
        let tail = &argv[argv.len() - 2..];
        assert_eq!(tail, ["-o", "NumberOfPasswordPrompts=1"]);
    }

    #[test]
    fn macos_iterm_opens_a_window_when_the_setting_says_so() {
        let specs = terminal_launch_specs(
            Platform::MacOs,
            Some("iterm"),
            &["ssh".into(), "u@h".into(), "-p".into(), "22".into()],
            "",
            None,
            "window",
        )
        .unwrap();
        assert_eq!(specs[0].program, "osascript");
        let script = &specs[0].args[3];
        assert!(
            script.contains("create window with default profile command"),
            "script was {script:?}"
        );
        assert!(!script.contains("create tab"), "script was {script:?}");
    }

    #[test]
    fn macos_iterm_reuses_a_tab_when_asked() {
        let specs = terminal_launch_specs(
            Platform::MacOs,
            Some("iterm"),
            &["ssh".into(), "u@h".into(), "-p".into(), "22".into()],
            "",
            None,
            "tab",
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
    fn macos_ghostty_opens_a_tab_when_the_setting_says_so() {
        let reports = launch_reports(Platform::MacOs, Path::new("/tmp/h.sock"), "c1").unwrap();
        let specs = terminal_launch_specs(
            Platform::MacOs,
            Some("ghostty"),
            &["ssh".into(), "u@h".into(), "-p".into(), "22".into()],
            "",
            Some(&reports),
            "tab",
        )
        .unwrap();
        assert_eq!(specs[0].program, "osascript");
        let script = specs[0].args.join(" ");
        // The line is handed to the new surface as its own process, so the next
        // window/tab reuses the front window when there is one.
        assert!(
            script.contains("new tab in front window with configuration"),
            "script was {script:?}"
        );
        assert!(
            script.contains("new window with configuration"),
            "{script:?}"
        );
        assert!(script.contains("(count of windows) is 0"), "{script:?}");
        assert!(script.contains("wait after command:true"), "{script:?}");
        // Ghostty parses this `command` with its own shell first, so the line is
        // single-quoted: a `"…"` wrapper would expand `$?`, `$rc` and `${SHELL}`
        // there, before our own shell ever sees them.
        assert!(script.contains("/bin/sh -c '"), "{script:?}");
        assert!(!script.contains("\\\""), "{script:?}");
        // Inside those single quotes the `$$` stays literal for Ghostty's own
        // parse and reaches our shell to expand into its pid.
        assert!(script.contains("session-started c1 $$"), "{script:?}");
    }

    #[test]
    fn macos_ghostty_keeps_the_command_line_launch_by_default() {
        let specs = terminal_launch_specs(
            Platform::MacOs,
            Some("ghostty"),
            &["ssh".into(), "u@h".into()],
            "",
            None,
            "window",
        )
        .unwrap();
        assert_eq!(specs[0].program, "ghostty");
        assert_eq!(
            specs[0].args[..3],
            ["-e".to_string(), "/bin/sh".to_string(), "-c".to_string()]
        );
        assert!(
            specs[0]
                .args
                .join(" ")
                .contains("ssh u@h || exec ${SHELL:-/bin/sh}"),
            "{:?}",
            specs[0].args
        );
        assert!(!specs.iter().any(|s| s.program == "osascript"));
    }

    #[test]
    fn linux_prefers_override_then_env_then_known_terminals() {
        let specs = terminal_launch_specs(
            Platform::Linux,
            Some("konsole"),
            &["ssh".into()],
            "",
            None,
            "window",
        )
        .unwrap();
        assert_eq!(specs[0].program, "konsole");
        assert!(specs.iter().any(|s| s.program == "gnome-terminal"));
    }

    #[test]
    fn windows_prefers_windows_terminal() {
        let specs =
            terminal_launch_specs(Platform::Windows, None, &["ssh".into()], "", None, "window")
                .unwrap();
        assert_eq!(specs[0].program, "wt.exe");
        assert!(specs.iter().any(|s| s.program == "cmd.exe"));
    }

    #[test]
    fn detect_terminals_returns_launchable_terminals() {
        // Every detected id must be one `terminal_launch_specs` can build a
        // spec for. macOS and Windows always offer a built-in; Linux detection
        // is best-effort, so a bare host (a CI runner) legitimately finds none.
        let platform = Platform::current();
        let found = detect_terminals(platform);
        for t in &found {
            let specs = terminal_launch_specs(
                platform,
                Some(&t.id),
                &["ssh".into(), "u@h".into()],
                "",
                None,
                "window",
            );
            assert!(specs.is_ok(), "{} should be launchable", t.id);
        }
        match platform {
            Platform::MacOs => assert!(found.iter().any(|t| t.id == "terminal")),
            Platform::Windows => assert!(found.iter().any(|t| t.id == "cmd.exe")),
            Platform::Linux => assert!(found.iter().all(|t| [
                "gnome-terminal",
                "konsole",
                "x-terminal-emulator",
                "xterm"
            ]
            .contains(&t.id.as_str()))),
        }
    }

    #[test]
    fn only_the_scriptable_mac_terminals_can_open_a_tab() {
        assert!(supports_tabs(Platform::MacOs, "iterm"));
        assert!(supports_tabs(Platform::MacOs, "ghostty"));
        for id in ["terminal", "kitty", "alacritty", "wezterm", ""] {
            assert!(!supports_tabs(Platform::MacOs, id), "{id}");
        }
        for platform in [Platform::Linux, Platform::Windows] {
            for id in ["gnome-terminal", "konsole", "wt.exe", "cmd.exe", "ghostty"] {
                assert!(!supports_tabs(platform, id), "{id}");
            }
        }
    }

    #[test]
    fn every_detected_terminal_reports_its_tab_capability() {
        for platform in [Platform::MacOs, Platform::Linux, Platform::Windows] {
            for terminal in detect_terminals(platform) {
                assert_eq!(
                    terminal.tabs,
                    supports_tabs(platform, &terminal.id),
                    "{}",
                    terminal.id
                );
            }
        }
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
