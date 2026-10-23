use assert_cmd::Command;
use predicates::prelude::*;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn wait_for(mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("condition not met within 10s");
}

// Poll a command until it exits successfully. Used to wait for the host: on
// Windows the IPC socket is a named pipe (not a file), so `sock.exists()`
// never becomes true — probe the host by asking it for status instead.
fn wait_until_succeeds(mut cmd: impl FnMut() -> std::process::Output) {
    wait_for(|| cmd().status.success());
}

// Spawn a `sigrun host` rooted at `dir`, returning it with its IPC socket. On
// Windows the IPC socket is a named pipe (a filesystem path makes the server
// reject it), so the pipe name is per-process; elsewhere it lives in `dir`.
fn spawn_host(dir: &std::path::Path) -> (std::process::Child, PathBuf) {
    let sock = if cfg!(windows) {
        PathBuf::from(format!(
            r"\\.\pipe\sigrun-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    } else {
        dir.join("host.sock")
    };
    let host = std::process::Command::new(assert_cmd::cargo::cargo_bin("sigrun"))
        .env("SIGRUN_CONFIG_DIR", dir)
        .env("SIGRUN_DATA_DIR", dir)
        .env("SIGRUN_SSH_DIR", dir.join("ssh"))
        .args(["host", "--socket"])
        .arg(&sock)
        .spawn()
        .unwrap();
    (host, sock)
}

#[test]
fn prints_version() {
    // Read from the crate so a release bump cannot leave this test behind.
    Command::cargo_bin("sigrun")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn status_without_a_host_explains_what_to_do() {
    let dir = tempfile::tempdir().unwrap();
    Command::cargo_bin("sigrun")
        .unwrap()
        .env("SIGRUN_CONFIG_DIR", dir.path())
        .env("SIGRUN_DATA_DIR", dir.path())
        .arg("--socket")
        .arg(dir.path().join("missing.sock"))
        .arg("status")
        .assert()
        .failure()
        .stderr(predicate::str::contains("sigrun host"));
}

#[test]
fn askpass_without_a_host_prints_nothing_and_leaks_nothing() {
    let dir = tempfile::tempdir().unwrap();
    // `sigrun askpass` is the explicit entry point; `sigrun '<prompt>'` is the
    // shape ssh produces from `SSH_ASKPASS=<exe>`. Both must fail the same way.
    for args in [vec!["askpass", "s3cret-prompt"], vec!["s3cret-prompt"]] {
        let out = Command::cargo_bin("sigrun")
            .unwrap()
            .env("SIGRUN_CONFIG_DIR", dir.path())
            .env("SIGRUN_DATA_DIR", dir.path())
            .env("SIGRUN_SSH_DIR", dir.path())
            // What ssh's helper environment carries (see `terminal::env_prefix`).
            .env("SIGRUN_ASKPASS", "1")
            .env("SIGRUN_CONNECTION_ID", "c1")
            // The prompt ssh passes: stdio is ignored, and it must never be echoed.
            .args(args)
            .output()
            .unwrap();
        assert!(!out.status.success());
        assert!(
            out.stdout.is_empty(),
            "stdout was {:?}",
            String::from_utf8_lossy(&out.stdout)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("could not read the stored password"),
            "{stderr:?}"
        );
        assert!(!stderr.contains("s3cret-prompt"), "{stderr:?}");
    }
}

#[test]
fn a_connection_id_alone_is_not_an_askpass_call() {
    let dir = tempfile::tempdir().unwrap();
    // Without `SIGRUN_ASKPASS=1` a mistyped subcommand must be a clap error,
    // never a password fetch.
    let out = Command::cargo_bin("sigrun")
        .unwrap()
        .env("SIGRUN_CONFIG_DIR", dir.path())
        .env("SIGRUN_DATA_DIR", dir.path())
        .env("SIGRUN_SSH_DIR", dir.path())
        .env("SIGRUN_CONNECTION_ID", "c1")
        .arg("statu")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        out.stdout.is_empty(),
        "stdout was {:?}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unrecognized subcommand"), "{stderr:?}");
    assert!(
        !stderr.contains("could not read the stored password"),
        "{stderr:?}"
    );
}

/// Serve one canned reply on a fresh Unix socket and return its path.
#[cfg(unix)]
fn fake_host_once(dir: &std::path::Path, name: &str, reply: Vec<u8>) -> PathBuf {
    use std::io::{BufRead, Write};

    let sock = dir.join(name);
    let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
    std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut line = Vec::new();
            let _ = reader.read_until(b'\n', &mut line);
            let mut stream = stream;
            let _ = stream.write_all(&reply);
            let _ = stream.flush();
        }
    });
    sock
}

#[cfg(unix)]
#[test]
fn askpass_prefers_the_socket_flag_over_the_environment() {
    let dir = tempfile::tempdir().unwrap();
    let sock = fake_host_once(
        dir.path(),
        "flag.sock",
        b"{\"type\":\"password\",\"value\":\"pw-flag\"}\n".to_vec(),
    );
    let out = Command::cargo_bin("sigrun")
        .unwrap()
        .env("SIGRUN_CONNECTION_ID", "c1")
        .env("SIGRUN_SOCKET", dir.path().join("missing.sock"))
        .arg("--socket")
        .arg(&sock)
        .args(["askpass", "deploy@example.com's password:"])
        .output()
        .unwrap();
    assert!(out.status.success());
    // Only the password, on stdout.
    assert_eq!(String::from_utf8_lossy(&out.stdout), "pw-flag\n");
    assert!(out.stderr.is_empty());
}

#[cfg(unix)]
#[test]
fn askpass_fails_on_an_empty_password_and_on_a_host_error() {
    let dir = tempfile::tempdir().unwrap();
    let replies = [
        // A host that reports success with an empty value.
        b"{\"type\":\"password\",\"value\":\"\"}\n".to_vec(),
        // A host that rejects the id (Response::Error).
        sigrun_core::ipc::protocol::encode(&sigrun_core::ipc::protocol::Response::Error {
            code: "not_found".into(),
            message: "not found: c1".into(),
            payload: None,
        })
        .unwrap(),
    ];
    for (i, reply) in replies.into_iter().enumerate() {
        let sock = fake_host_once(dir.path(), &format!("fake-{i}.sock"), reply);
        let out = Command::cargo_bin("sigrun")
            .unwrap()
            .env("SIGRUN_CONFIG_DIR", dir.path())
            .env("SIGRUN_DATA_DIR", dir.path())
            .env("SIGRUN_SSH_DIR", dir.path())
            .env("SIGRUN_CONNECTION_ID", "c1")
            .env("SIGRUN_SOCKET", &sock)
            .args(["askpass", "deploy@example.com's password:"])
            .output()
            .unwrap();
        assert!(!out.status.success(), "case {i}");
        assert!(
            out.stdout.is_empty(),
            "case {i} stdout was {:?}",
            String::from_utf8_lossy(&out.stdout)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("could not read the stored password"),
            "case {i}: {stderr:?}"
        );
        // The host's error text and the empty value never reach the terminal.
        assert!(!stderr.contains("not found"), "case {i}: {stderr:?}");
    }
}

// A credential is the identity source, so `--user` (and `--key`) cannot ride
// along: the host would refuse the hybrid, and clap should say so first.
#[test]
fn connections_add_refuses_a_credential_together_with_its_own_identity() {
    for own in [["--user", "deploy"], ["--key", "k1"]] {
        Command::cargo_bin("sigrun")
            .unwrap()
            .args([
                "connections",
                "add",
                "--label",
                "web",
                "--host",
                "example.com",
                "--credential",
                "prod",
            ])
            .args(own)
            .assert()
            .failure()
            .stderr(predicate::str::contains("cannot be used with"));
    }
}

// With `--credential` the user is not required, and the argument is resolved
// against the credential list before any save.
#[test]
fn connections_add_accepts_a_credential_alone() {
    let dir = tempfile::tempdir().unwrap();
    Command::cargo_bin("sigrun")
        .unwrap()
        .env("SIGRUN_CONFIG_DIR", dir.path())
        .env("SIGRUN_DATA_DIR", dir.path())
        .env("SIGRUN_SSH_DIR", dir.path().join("ssh"))
        .arg("--socket")
        .arg(dir.path().join("missing.sock"))
        .args([
            "connections",
            "add",
            "--label",
            "web",
            "--host",
            "example.com",
            "--credential",
            "prod",
        ])
        .assert()
        .failure()
        // It got as far as talking to the host, which is the parse proof.
        .stderr(predicate::str::contains("sigrun host"));
}

// A label is resolved to the stored row before the save, so the connection
// carries the credential's id and not the text the user typed.
#[test]
fn connections_add_stores_the_resolved_credential_id() {
    let dir = tempfile::tempdir().unwrap();
    let (mut host, sock) = spawn_host(dir.path());
    wait_until_succeeds(|| {
        Command::cargo_bin("sigrun")
            .unwrap()
            .env("SIGRUN_CONFIG_DIR", dir.path())
            .env("SIGRUN_DATA_DIR", dir.path())
            .arg("--socket")
            .arg(&sock)
            .arg("status")
            .output()
            .unwrap()
    });

    let run = |args: &[&str], stdin: &str| {
        let mut cmd = Command::cargo_bin("sigrun").unwrap();
        cmd.env("SIGRUN_CONFIG_DIR", dir.path())
            .env("SIGRUN_DATA_DIR", dir.path())
            .env("SIGRUN_SSH_DIR", dir.path().join("ssh"))
            .arg("--socket")
            .arg(&sock)
            .args(args)
            .write_stdin(stdin);
        cmd
    };

    run(
        &[
            "credentials",
            "add",
            "--label",
            "prod",
            "--user",
            "deploy",
            "--key",
            "k1",
        ],
        "",
    )
    .assert()
    .success();

    run(
        &[
            "connections",
            "add",
            "--label",
            "web",
            "--host",
            "example.com",
            "--credential",
            "prod",
        ],
        "",
    )
    .assert()
    .success();

    let out = run(&["connections", "list", "--json"], "")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rows: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let credential_id = rows[0]["credential_id"].as_str().expect("a credential id");
    assert_ne!(credential_id, "prod", "the label itself must not be stored");
    assert_eq!(credential_id.len(), 36, "{credential_id}");

    // The used-by count matches ids, so one reference proves where the id came from.
    run(&["credentials", "list"], "")
        .assert()
        .success()
        .stdout(predicate::str::contains("prod  deploy  key  1"));

    host.kill().unwrap();
    host.wait().unwrap();
}

// A typo is refused before the save, so the raw text never lands in a
// connection as a dangling credential id.
#[test]
fn connections_add_refuses_an_unknown_credential() {
    let dir = tempfile::tempdir().unwrap();
    let (mut host, sock) = spawn_host(dir.path());
    wait_until_succeeds(|| {
        Command::cargo_bin("sigrun")
            .unwrap()
            .env("SIGRUN_CONFIG_DIR", dir.path())
            .env("SIGRUN_DATA_DIR", dir.path())
            .arg("--socket")
            .arg(&sock)
            .arg("status")
            .output()
            .unwrap()
    });

    let run = |args: &[&str], stdin: &str| {
        let mut cmd = Command::cargo_bin("sigrun").unwrap();
        cmd.env("SIGRUN_CONFIG_DIR", dir.path())
            .env("SIGRUN_DATA_DIR", dir.path())
            .env("SIGRUN_SSH_DIR", dir.path().join("ssh"))
            .arg("--socket")
            .arg(&sock)
            .args(args)
            .write_stdin(stdin);
        cmd
    };

    run(
        &[
            "credentials",
            "add",
            "--label",
            "prod",
            "--user",
            "deploy",
            "--key",
            "k1",
        ],
        "",
    )
    .assert()
    .success();

    run(
        &[
            "connections",
            "add",
            "--label",
            "web",
            "--host",
            "example.com",
            "--credential",
            "prd",
        ],
        "",
    )
    .assert()
    .failure()
    .stderr(predicate::str::contains("prd"))
    .stderr(predicate::str::contains("label"));

    let out = run(&["connections", "list", "--json"], "")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "[]");

    host.kill().unwrap();
    host.wait().unwrap();
}

// `sigrun credentials add` reads the secret from stdin, never argv.
#[test]
fn credentials_add_rejects_an_empty_password_before_contacting_the_host() {
    let dir = tempfile::tempdir().unwrap();
    Command::cargo_bin("sigrun")
        .unwrap()
        .env("SIGRUN_CONFIG_DIR", dir.path())
        .env("SIGRUN_DATA_DIR", dir.path())
        .arg("--socket")
        .arg(dir.path().join("missing.sock"))
        .args([
            "credentials",
            "add",
            "--label",
            "prod",
            "--user",
            "deploy",
            "--password-stdin",
        ])
        .write_stdin("\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("password must not be empty"))
        .stderr(predicate::str::contains("sigrun host").not());
}

#[test]
fn provision_rejects_an_empty_password() {
    Command::cargo_bin("sigrun")
        .unwrap()
        .args(["connections", "provision", "web", "--password-stdin"])
        .write_stdin("\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("password"));
}

#[test]
fn provision_reaches_the_host_without_leaking_the_password() {
    let dir = tempfile::tempdir().unwrap();
    Command::cargo_bin("sigrun")
        .unwrap()
        .env("SIGRUN_CONFIG_DIR", dir.path())
        .env("SIGRUN_DATA_DIR", dir.path())
        .arg("--socket")
        .arg(dir.path().join("missing.sock"))
        .args(["connections", "provision", "web", "--password-stdin"])
        .write_stdin("hunter2\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("sigrun host"))
        .stderr(predicate::str::contains("hunter2").not());
}

#[test]
fn password_rejects_an_empty_password_before_contacting_the_host() {
    let dir = tempfile::tempdir().unwrap();
    Command::cargo_bin("sigrun")
        .unwrap()
        .env("SIGRUN_CONFIG_DIR", dir.path())
        .env("SIGRUN_DATA_DIR", dir.path())
        .arg("--socket")
        .arg(dir.path().join("missing.sock"))
        .args(["connections", "password", "bogus", "--password-stdin"])
        .write_stdin("\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("password must not be empty"))
        // The missing host would say "sigrun host"; reading the secret comes first.
        .stderr(predicate::str::contains("sigrun host").not());
}

// `--force` must parse, and a host error must surface as `code: message`.
// `bogus` is resolved by the host (not by `resolve_connection`, which passes an
// unknown arg through), so this also proves no reachability probe ran first.
#[test]
fn connect_with_force_parses_and_surfaces_a_host_error() {
    let dir = tempfile::tempdir().unwrap();
    let (mut host, sock) = spawn_host(dir.path());
    wait_until_succeeds(|| {
        Command::cargo_bin("sigrun")
            .unwrap()
            .env("SIGRUN_CONFIG_DIR", dir.path())
            .env("SIGRUN_DATA_DIR", dir.path())
            .arg("--socket")
            .arg(&sock)
            .arg("status")
            .output()
            .unwrap()
    });

    Command::cargo_bin("sigrun")
        .unwrap()
        .env("SIGRUN_CONFIG_DIR", dir.path())
        .env("SIGRUN_DATA_DIR", dir.path())
        .arg("--socket")
        .arg(&sock)
        .args(["connections", "connect", "bogus", "--force"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not_found: not found: bogus"));

    host.kill().unwrap();
    host.wait().unwrap();
}

#[test]
fn end_to_end_host_vault_key_and_connection() {
    let dir = tempfile::tempdir().unwrap();
    let (mut host, sock) = spawn_host(dir.path());
    wait_until_succeeds(|| {
        Command::cargo_bin("sigrun")
            .unwrap()
            .env("SIGRUN_CONFIG_DIR", dir.path())
            .env("SIGRUN_DATA_DIR", dir.path())
            .arg("--socket")
            .arg(&sock)
            .arg("status")
            .output()
            .unwrap()
    });

    let run = |args: &[&str], stdin: &str| {
        let mut cmd = Command::cargo_bin("sigrun").unwrap();
        cmd.env("SIGRUN_CONFIG_DIR", dir.path())
            .env("SIGRUN_DATA_DIR", dir.path())
            .env("SIGRUN_SSH_DIR", dir.path().join("ssh"))
            .arg("--socket")
            .arg(&sock)
            .args(args)
            .write_stdin(stdin);
        cmd
    };

    run(&["vault", "init", "--passphrase-stdin"], "pass\n")
        .assert()
        .success();
    run(&["vault", "unlock", "--passphrase-stdin"], "pass\n")
        .assert()
        .success();
    run(
        &[
            "keys",
            "generate",
            "--name",
            "work",
            "--algorithm",
            "ed25519",
            "--passphrase-stdin",
        ],
        "\n",
    )
    .assert()
    .success()
    .stdout(predicate::str::contains("ssh-ed25519"));
    run(
        &[
            "connections",
            "add",
            "--label",
            "web",
            "--host",
            "example.com",
            "--user",
            "deploy",
        ],
        "",
    )
    .assert()
    .success();
    run(
        &[
            "connections",
            "add",
            "--label",
            "db",
            "--host",
            "db.example.com",
            "--user",
            "deploy",
            "--group",
            "prod",
        ],
        "",
    )
    .assert()
    .success();
    run(&["connections", "list", "--json"], "")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"web\""));
    run(&["connections", "list", "--search", "web", "--json"], "")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"web\"").and(predicate::str::contains("\"db\"").not()));
    run(&["connections", "list", "--group", "prod", "--json"], "")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"db\""));
    run(&["connections", "connect", "web", "--print"], "")
        .assert()
        .success()
        .stdout(predicate::str::contains("ssh deploy@example.com -p 22"));

    host.kill().unwrap();
    host.wait().unwrap();
}

// `sigrun connection-ended <id>` is the hidden helper the launched line runs
// once `ssh` exits: it points at the host that launched it and reports its own
// session as over. It runs inside the user's terminal, so it stays silent and
// succeeds even when the host is already gone.
#[test]
fn connection_ended_is_hidden_from_help() {
    Command::cargo_bin("sigrun")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("connection-ended").not());
}

#[test]
fn connection_ended_without_a_host_is_silent_and_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::cargo_bin("sigrun")
        .unwrap()
        .env("SIGRUN_CONFIG_DIR", dir.path())
        .env("SIGRUN_DATA_DIR", dir.path())
        .arg("--socket")
        .arg(dir.path().join("missing.sock"))
        .args(["connection-ended", "c1"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out.status);
    assert!(
        out.stdout.is_empty() && out.stderr.is_empty(),
        "stdout {:?} stderr {:?}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn connection_ended_reaches_a_running_host_silently() {
    let dir = tempfile::tempdir().unwrap();
    let (mut host, sock) = spawn_host(dir.path());
    wait_until_succeeds(|| {
        Command::cargo_bin("sigrun")
            .unwrap()
            .env("SIGRUN_CONFIG_DIR", dir.path())
            .env("SIGRUN_DATA_DIR", dir.path())
            .arg("--socket")
            .arg(&sock)
            .arg("status")
            .output()
            .unwrap()
    });

    // An id the host does not know is a no-op, and the helper says nothing
    // either way — the whole point is that it never writes to the terminal.
    let out = Command::cargo_bin("sigrun")
        .unwrap()
        .env("SIGRUN_CONFIG_DIR", dir.path())
        .env("SIGRUN_DATA_DIR", dir.path())
        .arg("--socket")
        .arg(&sock)
        .args(["connection-ended", "c1"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out.status);
    assert!(
        out.stdout.is_empty() && out.stderr.is_empty(),
        "stdout {:?} stderr {:?}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    host.kill().unwrap();
    host.wait().unwrap();
}

// The connection id is a single opaque argument: nothing else follows it.
#[test]
fn connection_ended_takes_exactly_one_id() {
    let dir = tempfile::tempdir().unwrap();
    Command::cargo_bin("sigrun")
        .unwrap()
        .env("SIGRUN_CONFIG_DIR", dir.path())
        .env("SIGRUN_DATA_DIR", dir.path())
        .args(["connection-ended", "c1", "extra"])
        .assert()
        .failure();
}

// `sigrun session-started <id> <pid>` is the hidden helper the launched line
// runs *before* `ssh`: it hands the host the pid of the shell running the
// session, so a terminal that kills the whole process group is still noticed.
// Like the end report it stays silent and succeeds even when the host is gone.
#[test]
fn session_started_is_hidden_from_help() {
    Command::cargo_bin("sigrun")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("session-started").not());
}

#[test]
fn session_started_without_a_host_is_silent_and_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::cargo_bin("sigrun")
        .unwrap()
        .env("SIGRUN_CONFIG_DIR", dir.path())
        .env("SIGRUN_DATA_DIR", dir.path())
        .arg("--socket")
        .arg(dir.path().join("missing.sock"))
        .args(["session-started", "c1", "4242"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out.status);
    assert!(
        out.stdout.is_empty() && out.stderr.is_empty(),
        "stdout {:?} stderr {:?}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn session_started_reaches_a_running_host_silently() {
    let dir = tempfile::tempdir().unwrap();
    let (mut host, sock) = spawn_host(dir.path());
    wait_until_succeeds(|| {
        Command::cargo_bin("sigrun")
            .unwrap()
            .env("SIGRUN_CONFIG_DIR", dir.path())
            .env("SIGRUN_DATA_DIR", dir.path())
            .arg("--socket")
            .arg(&sock)
            .arg("status")
            .output()
            .unwrap()
    });

    // An id the host does not know is a no-op, and the helper says nothing
    // either way — the whole point is that it never writes to the terminal.
    let out = Command::cargo_bin("sigrun")
        .unwrap()
        .env("SIGRUN_CONFIG_DIR", dir.path())
        .env("SIGRUN_DATA_DIR", dir.path())
        .arg("--socket")
        .arg(&sock)
        .args(["session-started", "c1", "4242"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out.status);
    assert!(
        out.stdout.is_empty() && out.stderr.is_empty(),
        "stdout {:?} stderr {:?}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    host.kill().unwrap();
    host.wait().unwrap();
}

// The id and the pid are exactly two arguments; the shell has already expanded
// `$$` by the time this runs.
#[test]
fn session_started_takes_exactly_an_id_and_a_pid() {
    let dir = tempfile::tempdir().unwrap();
    for argv in [
        vec!["c1"],
        vec!["c1", "4242", "extra"],
        vec!["c1", "not-a-pid"],
    ] {
        Command::cargo_bin("sigrun")
            .unwrap()
            .env("SIGRUN_CONFIG_DIR", dir.path())
            .env("SIGRUN_DATA_DIR", dir.path())
            .arg("session-started")
            .args(argv)
            .assert()
            .failure();
    }
}

#[test]
fn connections_add_and_edit_resolve_key_name_to_id() {
    let dir = tempfile::tempdir().unwrap();
    let (mut host, sock) = spawn_host(dir.path());
    wait_until_succeeds(|| {
        Command::cargo_bin("sigrun")
            .unwrap()
            .env("SIGRUN_CONFIG_DIR", dir.path())
            .env("SIGRUN_DATA_DIR", dir.path())
            .arg("--socket")
            .arg(&sock)
            .arg("status")
            .output()
            .unwrap()
    });

    let run = |args: &[&str], stdin: &str| {
        let mut cmd = Command::cargo_bin("sigrun").unwrap();
        cmd.env("SIGRUN_CONFIG_DIR", dir.path())
            .env("SIGRUN_DATA_DIR", dir.path())
            .env("SIGRUN_SSH_DIR", dir.path().join("ssh"))
            .arg("--socket")
            .arg(&sock)
            .args(args)
            .write_stdin(stdin);
        cmd
    };

    run(&["vault", "init", "--passphrase-stdin"], "pass\n")
        .assert()
        .success();
    run(&["vault", "unlock", "--passphrase-stdin"], "pass\n")
        .assert()
        .success();

    run(
        &[
            "keys",
            "generate",
            "--name",
            "my-key",
            "--algorithm",
            "ed25519",
            "--passphrase-stdin",
        ],
        "\n",
    )
    .assert()
    .success();

    let out = run(&["keys", "list", "--json"], "").output().unwrap();
    assert!(out.status.success());
    let keys: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let key_id = keys[0]["id"].as_str().expect("key id").to_string();

    // connections add with key name resolves to key_id
    run(
        &[
            "connections",
            "add",
            "--label",
            "srv1",
            "--host",
            "srv1.example.com",
            "--user",
            "deploy",
            "--key",
            "my-key",
        ],
        "",
    )
    .assert()
    .success();

    let out = run(&["connections", "list", "--json"], "")
        .output()
        .unwrap();
    assert!(out.status.success());
    let conns: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let conn1 = conns
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["label"] == "srv1")
        .unwrap();
    assert_eq!(conn1["key_id"].as_str().unwrap(), key_id);
    let conn1_id = conn1["id"].as_str().unwrap().to_string();

    // Generate a second key with a different name to test edit
    run(
        &[
            "keys",
            "generate",
            "--name",
            "other-key",
            "--algorithm",
            "ed25519",
            "--passphrase-stdin",
        ],
        "\n",
    )
    .assert()
    .success();

    let out = run(&["keys", "list", "--json"], "").output().unwrap();
    let keys: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let other_key = keys
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["name"] == "other-key")
        .unwrap();
    let other_key_id = other_key["id"].as_str().unwrap().to_string();

    // connections edit with key name resolves to key_id
    run(
        &["connections", "edit", &conn1_id, "--key", "other-key"],
        "",
    )
    .assert()
    .success();

    let out = run(&["connections", "list", "--json"], "")
        .output()
        .unwrap();
    let conns: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let updated = conns
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == conn1_id)
        .unwrap();
    assert_eq!(updated["key_id"].as_str().unwrap(), other_key_id);

    // Generate a duplicate named key to create ambiguity
    run(
        &[
            "keys",
            "generate",
            "--name",
            "my-key",
            "--algorithm",
            "ed25519",
            "--passphrase-stdin",
        ],
        "\n",
    )
    .assert()
    .success();

    // Ambiguous name in connections add errors cleanly
    run(
        &[
            "connections",
            "add",
            "--label",
            "srv2",
            "--host",
            "srv2.example.com",
            "--user",
            "deploy",
            "--key",
            "my-key",
        ],
        "",
    )
    .assert()
    .failure()
    .stderr(predicate::str::contains("ambiguous"));

    // Ambiguous name in connections edit errors cleanly
    run(&["connections", "edit", &conn1_id, "--key", "my-key"], "")
        .assert()
        .failure()
        .stderr(predicate::str::contains("ambiguous"));

    host.kill().unwrap();
    host.wait().unwrap();
}

#[test]
fn credentials_password_set_and_clear() {
    let dir = tempfile::tempdir().unwrap();
    let (mut host, sock) = spawn_host(dir.path());
    wait_until_succeeds(|| {
        Command::cargo_bin("sigrun")
            .unwrap()
            .env("SIGRUN_CONFIG_DIR", dir.path())
            .env("SIGRUN_DATA_DIR", dir.path())
            .arg("--socket")
            .arg(&sock)
            .arg("status")
            .output()
            .unwrap()
    });

    let run = |args: &[&str], stdin: &str| {
        let mut cmd = Command::cargo_bin("sigrun").unwrap();
        cmd.env("SIGRUN_CONFIG_DIR", dir.path())
            .env("SIGRUN_DATA_DIR", dir.path())
            .env("SIGRUN_SSH_DIR", dir.path().join("ssh"))
            .arg("--socket")
            .arg(&sock)
            .args(args)
            .write_stdin(stdin);
        cmd
    };

    run(&["vault", "init", "--passphrase-stdin"], "pass\n")
        .assert()
        .success();
    run(&["vault", "unlock", "--passphrase-stdin"], "pass\n")
        .assert()
        .success();

    run(
        &[
            "keys",
            "generate",
            "--name",
            "k1",
            "--algorithm",
            "ed25519",
            "--passphrase-stdin",
        ],
        "\n",
    )
    .assert()
    .success();

    let out = run(&["keys", "list", "--json"], "").output().unwrap();
    let keys: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let key_id = keys[0]["id"].as_str().unwrap();

    run(
        &[
            "credentials",
            "add",
            "--label",
            "cred-1",
            "--user",
            "deploy",
            "--key",
            key_id,
        ],
        "",
    )
    .assert()
    .success();

    // Verify method is initially key only
    run(&["credentials", "list"], "")
        .assert()
        .success()
        .stdout(predicate::str::contains("cred-1  deploy  key  0"));

    // Set password on credential via label
    run(
        &["credentials", "password", "cred-1", "--password-stdin"],
        "hunter2\n",
    )
    .assert()
    .success()
    .stdout(predicate::str::contains("password saved"));

    // Verify method is now key + password
    run(&["credentials", "list"], "")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "cred-1  deploy  key + password  0",
        ));

    // Clear password
    run(&["credentials", "password", "cred-1", "--clear"], "")
        .assert()
        .success()
        .stdout(predicate::str::contains("password cleared"));

    // Verify method is back to key only
    run(&["credentials", "list"], "")
        .assert()
        .success()
        .stdout(predicate::str::contains("cred-1  deploy  key  0"));

    host.kill().unwrap();
    host.wait().unwrap();
}

#[test]
fn keys_export_writes_private_key() {
    let dir = tempfile::tempdir().unwrap();
    let (mut host, sock) = spawn_host(dir.path());
    wait_until_succeeds(|| {
        Command::cargo_bin("sigrun")
            .unwrap()
            .env("SIGRUN_CONFIG_DIR", dir.path())
            .env("SIGRUN_DATA_DIR", dir.path())
            .arg("--socket")
            .arg(&sock)
            .arg("status")
            .output()
            .unwrap()
    });

    let run = |args: &[&str], stdin: &str| {
        let mut cmd = Command::cargo_bin("sigrun").unwrap();
        cmd.env("SIGRUN_CONFIG_DIR", dir.path())
            .env("SIGRUN_DATA_DIR", dir.path())
            .env("SIGRUN_SSH_DIR", dir.path().join("ssh"))
            .arg("--socket")
            .arg(&sock)
            .args(args)
            .write_stdin(stdin);
        cmd
    };

    run(&["vault", "init", "--passphrase-stdin"], "pass\n")
        .assert()
        .success();
    run(&["vault", "unlock", "--passphrase-stdin"], "pass\n")
        .assert()
        .success();

    run(
        &[
            "keys",
            "generate",
            "--name",
            "export-me",
            "--algorithm",
            "ed25519",
            "--passphrase-stdin",
        ],
        "\n",
    )
    .assert()
    .success();

    let export_path = dir.path().join("exported_key");
    run(
        &[
            "keys",
            "export",
            "export-me",
            "--out",
            export_path.to_str().unwrap(),
        ],
        "",
    )
    .assert()
    .success()
    .stdout(predicate::str::contains("key exported"));

    assert!(export_path.exists());
    let content = std::fs::read_to_string(&export_path).unwrap();
    assert!(content.contains("BEGIN OPENSSH PRIVATE KEY"));

    host.kill().unwrap();
    host.wait().unwrap();
}
