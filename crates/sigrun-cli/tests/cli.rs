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

#[test]
fn prints_version() {
    Command::cargo_bin("sigrun")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("0.1.0"));
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
fn end_to_end_host_vault_key_and_connection() {
    let dir = tempfile::tempdir().unwrap();
    // On Windows the IPC socket is a named pipe, which must be a `\\.\pipe\`
    // path — a filesystem path would make the server reject it. Use a
    // per-test pipe name there; a filesystem path everywhere else.
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
        dir.path().join("host.sock")
    };
    let mut host = std::process::Command::new(assert_cmd::cargo::cargo_bin("sigrun"))
        .env("SIGRUN_CONFIG_DIR", dir.path())
        .env("SIGRUN_DATA_DIR", dir.path())
        .env("SIGRUN_SSH_DIR", dir.path().join("ssh"))
        .args(["host", "--socket"])
        .arg(&sock)
        .spawn()
        .unwrap();
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
