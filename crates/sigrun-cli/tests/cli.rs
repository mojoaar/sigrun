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
