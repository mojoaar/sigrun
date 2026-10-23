//! The `sigrun` command-line interface: starts the host or drives it over IPC.

use std::io::Write;
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use sigrun_core::ipc::{Request, Response};
use sigrun_core::model::{
    Connection, ConnectionSource, Credential, KeyAlgorithm, KeySource, KeyView,
};
use sigrun_core::paths::Paths;
use sigrun_core::provision::Hardening;
use sigrun_core::vault::VaultStatus;

use crate::client;

#[derive(Parser)]
#[command(name = "sigrun", version, about = "Sigrun — victory over key chaos.")]
pub struct Cli {
    /// Override the host IPC socket path.
    #[arg(long, global = true)]
    pub socket: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run the headless Sigrun host.
    Host,
    /// Show host, vault, and agent status.
    Status,
    /// SSH askpass helper: print a stored password. Internal.
    #[command(hide = true)]
    Askpass {
        /// The prompt ssh passes; ignored.
        prompt: Option<String>,
    },
    /// Report that a launched session ended. Internal.
    #[command(hide = true)]
    ConnectionEnded {
        /// The connection whose session ended.
        id: String,
    },
    /// Report that a launched session started. Internal.
    #[command(hide = true)]
    SessionStarted {
        /// The connection whose session started.
        id: String,
        /// The pid of the shell running the session.
        pid: u32,
    },
    #[command(subcommand)]
    Vault(VaultCommand),
    #[command(subcommand)]
    Connections(ConnectionsCommand),
    #[command(subcommand)]
    Keys(KeysCommand),
    #[command(subcommand)]
    Credentials(CredentialsCommand),
}

#[derive(Subcommand)]
pub enum VaultCommand {
    /// Create the encrypted vault.
    Init {
        #[arg(long)]
        passphrase_stdin: bool,
    },
    /// Unlock the vault.
    Unlock {
        #[arg(long)]
        passphrase_stdin: bool,
    },
    /// Change the vault passphrase.
    ChangePassphrase {
        #[arg(long, visible_alias = "passphrase-stdin")]
        password_stdin: bool,
    },
    /// Lock the vault.
    Lock,
    /// Back up the sealed vault file.
    Backup {
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum ConnectionsCommand {
    /// List connections.
    List {
        #[arg(long)]
        search: Option<String>,
        #[arg(long)]
        group: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Add a connection.
    Add {
        #[arg(long)]
        label: String,
        #[arg(long)]
        host: String,
        /// The connection's own user, for a connection without a credential.
        #[arg(long, required_unless_present = "credential")]
        user: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long, conflicts_with = "credential")]
        key: Option<String>,
        #[arg(long)]
        group: Option<String>,
        #[arg(long)]
        tag: Vec<String>,
        /// Use a shared credential (id or label) instead of this connection's
        /// own user and key.
        #[arg(long, conflicts_with_all = ["user", "key"])]
        credential: Option<String>,
    },
    /// Edit a connection by id.
    Edit {
        id: String,
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        key: Option<String>,
        #[arg(long)]
        group: Option<String>,
        #[arg(long)]
        tag: Vec<String>,
    },
    /// Remove a connection by id.
    Remove {
        id: String,
        #[arg(long)]
        yes: bool,
    },
    /// Store or clear the password for a connection, resolved by id or label.
    ///
    /// The password is read from stdin with `--password-stdin`, or prompted for;
    /// it never appears on argv.
    Password {
        id_or_label: String,
        /// Read the password from stdin instead of prompting.
        #[arg(long, conflicts_with = "clear")]
        password_stdin: bool,
        /// Forget the stored password instead of setting one.
        #[arg(long)]
        clear: bool,
    },
    /// Open a connection, resolved by id or label.
    Connect {
        id_or_label: String,
        #[arg(long)]
        print: bool,
        /// Skip the reachability check and connect anyway.
        #[arg(long)]
        force: bool,
    },
    /// Install keys on a connection, resolved by id or label.
    ///
    /// Key ids come from `sigrun keys list`. The password is read from stdin
    /// with `--password-stdin`, or prompted for; it never appears on argv.
    Provision {
        id_or_label: String,
        /// Key id to install; repeat for several keys.
        #[arg(long)]
        key: Vec<String>,
        /// After installing, disable password authentication on the server.
        #[arg(long)]
        disable_password_auth: bool,
        #[arg(long)]
        password_stdin: bool,
    },
}

#[derive(Subcommand)]
pub enum KeysCommand {
    /// List keys.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Generate a new key into the vault.
    Generate {
        #[arg(long)]
        name: String,
        #[arg(long, value_enum)]
        algorithm: KeyAlgorithmArg,
        #[arg(long)]
        passphrase_stdin: bool,
    },
    /// Import a private key into the vault.
    Import {
        path: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long)]
        passphrase_stdin: bool,
    },
    /// Remove a key by id or name.
    Remove {
        id_or_name: String,
        #[arg(long)]
        yes: bool,
    },
    /// Export a key's private half to disk.
    Export {
        key: String,
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Subcommand)]
pub enum CredentialsCommand {
    /// List credentials with their method and how many connections use them.
    List,
    /// Add a credential. The secret never appears on argv.
    Add {
        #[arg(long)]
        label: String,
        #[arg(long)]
        user: String,
        /// A key id from `sigrun keys list --json`.
        #[arg(long)]
        key: Option<String>,
        /// Read a password from stdin.
        #[arg(long)]
        password_stdin: bool,
    },
    /// Store or clear the password for a credential, resolved by id or label.
    ///
    /// The password is read from stdin with `--password-stdin`, or prompted for;
    /// it never appears on argv.
    Password {
        credential: String,
        /// Read the password from stdin instead of prompting.
        #[arg(long, conflicts_with = "clear")]
        password_stdin: bool,
        /// Forget the stored password instead of setting one.
        #[arg(long)]
        clear: bool,
    },
    /// Remove a credential by id or label.
    Remove { id_or_label: String },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum KeyAlgorithmArg {
    Ed25519,
    Rsa,
    Ecdsa,
}

impl From<KeyAlgorithmArg> for KeyAlgorithm {
    fn from(a: KeyAlgorithmArg) -> Self {
        match a {
            KeyAlgorithmArg::Ed25519 => KeyAlgorithm::Ed25519,
            KeyAlgorithmArg::Rsa => KeyAlgorithm::Rsa,
            KeyAlgorithmArg::Ecdsa => KeyAlgorithm::Ecdsa,
        }
    }
}

pub async fn run() -> anyhow::Result<()> {
    // ssh execs `$SSH_ASKPASS` with the prompt as its only argument, so an
    // askpass call has no subcommand to parse. Both markers are required (see
    // `terminal::env_prefix`): a stray connection id in the environment must
    // still produce clap's normal "unrecognized subcommand" error.
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) if matches!(err.kind(), clap::error::ErrorKind::InvalidSubcommand) => {
            if askpass_marker() {
                return askpass(None);
            }
            err.exit()
        }
        Err(err) => err.exit(),
    };
    let Cli { socket, command } = cli;
    match command {
        Command::Host => {
            sigrun_core::host::run(Paths::resolve()?, socket.clone()).await?;
            Ok(())
        }
        Command::Status => status(&socket).await,
        // Explicit entry point: no environment marker needed.
        Command::Askpass { prompt: _ } => askpass(socket.as_deref()),
        // Explicit entry point too: the launched line passes the id outright.
        Command::ConnectionEnded { id } => connection_ended(socket, id).await,
        Command::SessionStarted { id, pid } => session_started(socket, id, pid).await,
        Command::Vault(cmd) => vault(&socket, cmd).await,
        Command::Connections(cmd) => connections(&socket, cmd).await,
        Command::Keys(cmd) => keys(&socket, cmd).await,
        Command::Credentials(cmd) => credentials(&socket, cmd).await,
    }
}

/// True only for the environment `terminal::env_prefix` builds for ssh.
fn askpass_marker() -> bool {
    std::env::var("SIGRUN_ASKPASS").as_deref() == Ok("1")
        && std::env::var("SIGRUN_CONNECTION_ID").is_ok_and(|id| !id.is_empty())
}

fn resolve_addr(socket: &Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let paths = Paths::resolve()?;
    Ok(socket.clone().unwrap_or_else(|| paths.socket_path()))
}

/// `sigrun askpass`: ssh's `SSH_ASKPASS` helper. The password is the only thing
/// on stdout; the prompt argument and stdin are ignored. Every failure exits 2
/// with a fixed reason — never the password, never the underlying error.
///
/// ssh finds this through `SSH_ASKPASS=<exe>` and runs it as `<exe> '<prompt>'`
/// (no arguments allowed), so `run` also routes such an invocation here.
/// `socket` is the global `--socket` override, which wins over the environment.
fn askpass(socket: Option<&std::path::Path>) -> anyhow::Result<()> {
    let target = socket
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("SIGRUN_SOCKET").map(PathBuf::from))
        .or_else(|| Paths::resolve().ok().map(|p| p.socket_path()))
        .zip(std::env::var("SIGRUN_CONNECTION_ID").ok());
    if let Some((socket, id)) = target {
        if let Ok(password) = sigrun_core::askpass::answer(&socket, &id) {
            println!("{password}");
            return Ok(());
        }
    }
    eprintln!("sigrun: could not read the stored password from the Sigrun host");
    std::process::exit(2)
}

/// `sigrun connection-ended <id>`: the launched line tells the host at `socket`
/// that the session ended. It runs inside the user's terminal, so it never
/// prints and never fails — the host may already be gone, and that is not the
/// user's problem.
async fn connection_ended(socket: Option<PathBuf>, id: String) -> anyhow::Result<()> {
    if let Some(addr) = sigrun_core::connection_ended::helper_socket(socket) {
        sigrun_core::connection_ended::report(&addr, &id).await;
    }
    Ok(())
}

/// `sigrun session-started <id> <pid>`: the launched line tells the host at
/// `socket` the pid of the shell running the session, so the host can notice a
/// terminal that kills the whole process group. Like the end report it runs
/// inside the user's terminal: it never prints and never fails.
async fn session_started(socket: Option<PathBuf>, id: String, pid: u32) -> anyhow::Result<()> {
    if let Some(addr) = sigrun_core::session_started::helper_socket(socket) {
        sigrun_core::session_started::report(&addr, &id, pid).await;
    }
    Ok(())
}

async fn status(socket: &Option<PathBuf>) -> anyhow::Result<()> {
    let addr = resolve_addr(socket)?;
    let resp = client::request(&addr, &Request::Status).await?;
    match resp {
        Response::Status(info) => {
            println!("host: running");
            println!("vault: {}", vault_status_str(info.vault));
            println!("agent: {}", info.agent_socket.as_deref().unwrap_or("-"));
            Ok(())
        }
        _ => anyhow::bail!("internal: unexpected response"),
    }
}

async fn vault(socket: &Option<PathBuf>, cmd: VaultCommand) -> anyhow::Result<()> {
    let addr = resolve_addr(socket)?;
    match cmd {
        VaultCommand::Init { passphrase_stdin } => {
            let passphrase =
                require_passphrase(read_passphrase("passphrase: ", passphrase_stdin, true)?)?;
            let resp = client::request(&addr, &Request::VaultInit { passphrase }).await?;
            match resp {
                Response::Ok => println!("vault initialized"),
                _ => anyhow::bail!("internal: unexpected response"),
            }
        }
        VaultCommand::Unlock { passphrase_stdin } => {
            let passphrase =
                require_passphrase(read_passphrase("passphrase: ", passphrase_stdin, false)?)?;
            let resp = client::request(&addr, &Request::VaultUnlock { passphrase }).await?;
            match resp {
                Response::Unlocked { warnings } => {
                    for w in warnings {
                        eprintln!("{w}");
                    }
                    println!("vault unlocked");
                }
                _ => anyhow::bail!("internal: unexpected response"),
            }
        }
        VaultCommand::ChangePassphrase { password_stdin } => {
            let current_passphrase = require_passphrase(read_passphrase(
                "current passphrase: ",
                password_stdin,
                false,
            )?)?;
            let new_passphrase = require_passphrase(read_passphrase(
                "new passphrase: ",
                password_stdin,
                !password_stdin,
            )?)?;
            let resp = client::request(
                &addr,
                &Request::VaultChangePassphrase {
                    current_passphrase,
                    new_passphrase,
                },
            )
            .await?;
            match resp {
                Response::Ok => println!("passphrase changed"),
                Response::Error { message, .. } => anyhow::bail!("{message}"),
                _ => anyhow::bail!("internal: unexpected response"),
            }
        }
        VaultCommand::Lock => {
            let resp = client::request(&addr, &Request::VaultLock).await?;
            match resp {
                Response::Ok => println!("vault locked"),
                _ => anyhow::bail!("internal: unexpected response"),
            }
        }
        VaultCommand::Backup { out } => {
            let destination = out.unwrap_or_else(|| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                PathBuf::from(format!("./sigrun-vault-{now}.bak"))
            });
            let resp = client::request(
                &addr,
                &Request::VaultBackup {
                    destination: destination.clone(),
                },
            )
            .await?;
            match resp {
                Response::Ok => println!("vault backed up to {}", destination.display()),
                Response::Error { message, .. } => anyhow::bail!("{message}"),
                _ => anyhow::bail!("internal: unexpected response"),
            }
        }
    }
    Ok(())
}

async fn connections(socket: &Option<PathBuf>, cmd: ConnectionsCommand) -> anyhow::Result<()> {
    let addr = resolve_addr(socket)?;
    match cmd {
        ConnectionsCommand::List {
            search,
            group,
            json,
        } => {
            let resp = client::request(&addr, &Request::ConnectionsList).await?;
            let conns = match resp {
                Response::Connections(c) => c,
                _ => anyhow::bail!("internal: unexpected response"),
            };
            let filtered: Vec<Connection> = conns
                .into_iter()
                .filter(|c| {
                    let search_ok = match &search {
                        Some(t) => {
                            let t = t.to_lowercase();
                            c.label.to_lowercase().contains(&t)
                                || c.host.to_lowercase().contains(&t)
                                || c.user.to_lowercase().contains(&t)
                        }
                        None => true,
                    };
                    let group_ok = match &group {
                        Some(g) => c.group.as_deref() == Some(g.as_str()),
                        None => true,
                    };
                    search_ok && group_ok
                })
                .collect();
            if json {
                println!("{}", serde_json::to_string_pretty(&filtered)?);
            } else {
                println!("id  label  host  user  port  group");
                for c in &filtered {
                    println!(
                        "{}  {}  {}  {}  {}  {}",
                        c.id,
                        c.label,
                        c.host,
                        c.user,
                        c.port,
                        c.group.as_deref().unwrap_or("-")
                    );
                }
            }
        }
        ConnectionsCommand::Add {
            label,
            host,
            user,
            port,
            key,
            group,
            tag,
            credential,
        } => {
            // A credential is the whole identity, so its id is resolved before
            // anything is stored; clap has already refused `--user`/`--key`.
            let credential_id = match credential {
                Some(arg) => {
                    let creds = list_credentials(&addr).await?;
                    let id = resolve_credential(&creds, &arg)?;
                    // `resolve_credential` passes an unknown value through for
                    // `remove`, which lets the host name it. Adding a
                    // connection must not store that text as an id.
                    if !creds.iter().any(|c| c.id == id) {
                        anyhow::bail!(
                            "invalid_input: no credential matches '{arg}'; credentials are matched by id or label"
                        );
                    }
                    Some(id)
                }
                None => None,
            };
            let key_id = match key {
                Some(arg) => {
                    let keys = list_keys(&addr).await?;
                    Some(resolve_key(&keys, &arg)?)
                }
                None => None,
            };
            let connection = Connection {
                id: String::new(),
                label,
                host,
                user: user.unwrap_or_default(),
                port: port.unwrap_or(22),
                key_id,
                credential_id,
                group,
                tags: tag,
                source: ConnectionSource::Managed,
                disable_password_auth: false,
                has_password: false,
                active: false,
            };
            let resp = client::request(&addr, &Request::ConnectionsAdd { connection }).await?;
            match resp {
                Response::Ok => println!("connection added"),
                _ => anyhow::bail!("internal: unexpected response"),
            }
        }
        ConnectionsCommand::Edit {
            id,
            label,
            host,
            user,
            port,
            key,
            group,
            tag,
        } => {
            let resp = client::request(&addr, &Request::ConnectionsList).await?;
            let conns = match resp {
                Response::Connections(c) => c,
                _ => anyhow::bail!("internal: unexpected response"),
            };
            let mut connection = conns
                .into_iter()
                .find(|c| c.id == id)
                .ok_or_else(|| anyhow::anyhow!("not_found: no connection with id {id}"))?;
            if let Some(v) = label {
                connection.label = v;
            }
            if let Some(v) = host {
                connection.host = v;
            }
            if let Some(v) = user {
                connection.user = v;
            }
            if let Some(v) = port {
                connection.port = v;
            }
            if let Some(v) = key {
                let keys = list_keys(&addr).await?;
                connection.key_id = Some(resolve_key(&keys, &v)?);
            }
            if let Some(v) = group {
                connection.group = Some(v);
            }
            if !tag.is_empty() {
                connection.tags = tag;
            }
            let resp = client::request(&addr, &Request::ConnectionsUpdate { connection }).await?;
            match resp {
                Response::Ok => println!("connection updated"),
                _ => anyhow::bail!("internal: unexpected response"),
            }
        }
        ConnectionsCommand::Remove { id, yes } => {
            if !yes && !confirm(&format!("remove connection {id}? [y/N] "))? {
                anyhow::bail!("aborted");
            }
            let resp = client::request(&addr, &Request::ConnectionsRemove { id }).await?;
            match resp {
                Response::Ok => println!("connection removed"),
                _ => anyhow::bail!("internal: unexpected response"),
            }
        }
        ConnectionsCommand::Password {
            id_or_label,
            password_stdin,
            clear,
        } => {
            // Read (or skip) the secret before any network call; argv is never used.
            let password = if clear {
                None
            } else {
                match read_passphrase("password: ", password_stdin, false)? {
                    Some(p) => Some(p),
                    None => anyhow::bail!("invalid_input: password must not be empty"),
                }
            };
            let resp = client::request(&addr, &Request::ConnectionsList).await?;
            let conns = match resp {
                Response::Connections(c) => c,
                _ => anyhow::bail!("internal: unexpected response"),
            };
            let id = resolve_connection(&conns, &id_or_label)?;
            let resp =
                client::request(&addr, &Request::ConnectionsSetPassword { id, password }).await?;
            match resp {
                Response::Ok => println!("password {}", if clear { "cleared" } else { "saved" }),
                _ => anyhow::bail!("internal: unexpected response"),
            }
        }
        ConnectionsCommand::Connect {
            id_or_label,
            print,
            force,
        } => {
            let resp = client::request(&addr, &Request::ConnectionsList).await?;
            let conns = match resp {
                Response::Connections(c) => c,
                _ => anyhow::bail!("internal: unexpected response"),
            };
            let id = resolve_connection(&conns, &id_or_label)?;
            let resp = client::request(
                &addr,
                &Request::ConnectionsConnect {
                    id,
                    print_only: print,
                    force,
                },
            )
            .await?;
            match resp {
                Response::Command(cmd) => println!("{cmd}"),
                Response::Ok => println!("connection launched"),
                _ => anyhow::bail!("internal: unexpected response"),
            }
        }
        ConnectionsCommand::Provision {
            id_or_label,
            key,
            disable_password_auth,
            password_stdin,
        } => {
            // Read the secret before any network call; argv is never used.
            let password = match read_passphrase("password: ", password_stdin, false)? {
                Some(p) => p,
                None => anyhow::bail!("invalid_input: password must not be empty"),
            };
            let resp = client::request(&addr, &Request::ConnectionsList).await?;
            let conns = match resp {
                Response::Connections(c) => c,
                _ => anyhow::bail!("internal: unexpected response"),
            };
            let id = resolve_connection(&conns, &id_or_label)?;
            let resp = client::request(
                &addr,
                &Request::ConnectionsProvision {
                    id,
                    password,
                    key_ids: key,
                    disable_password_auth,
                },
            )
            .await?;
            match resp {
                Response::Provisioned(outcome) => {
                    println!("installed: {}", outcome.installed.join(", "));
                    println!("verified: {}", outcome.verified.join(", "));
                    match outcome.hardening {
                        Hardening::NotRequested => {}
                        Hardening::Applied => println!("disabled password authentication"),
                        Hardening::Skipped(reason) => println!("hardening skipped: {reason}"),
                    }
                }
                _ => anyhow::bail!("internal: unexpected response"),
            }
        }
    }
    Ok(())
}

async fn keys(socket: &Option<PathBuf>, cmd: KeysCommand) -> anyhow::Result<()> {
    let addr = resolve_addr(socket)?;
    match cmd {
        KeysCommand::List { json } => {
            let keys = list_keys(&addr).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&keys)?);
            } else {
                println!("name  algorithm  fingerprint  source  available");
                for k in &keys {
                    println!(
                        "{}  {}  {}  {}  {}",
                        k.key.name,
                        algorithm_str(k.key.algorithm),
                        k.key.fingerprint,
                        source_str(k.key.source),
                        k.available
                    );
                }
            }
        }
        KeysCommand::Generate {
            name,
            algorithm,
            passphrase_stdin,
        } => {
            let passphrase = read_passphrase("passphrase: ", passphrase_stdin, false)?;
            let resp = client::request(
                &addr,
                &Request::KeysGenerate {
                    name,
                    algorithm: algorithm.into(),
                    passphrase,
                },
            )
            .await?;
            match resp {
                Response::Key(k) => println!("{}", k.key.public_key),
                _ => anyhow::bail!("internal: unexpected response"),
            }
        }
        KeysCommand::Import {
            path,
            name,
            passphrase_stdin,
        } => {
            let passphrase = read_passphrase("passphrase: ", passphrase_stdin, false)?;
            let resp = client::request(
                &addr,
                &Request::KeysImport {
                    path,
                    name,
                    passphrase,
                },
            )
            .await?;
            match resp {
                Response::Key(k) => println!("{}", k.key.public_key),
                _ => anyhow::bail!("internal: unexpected response"),
            }
        }
        KeysCommand::Remove { id_or_name, yes } => {
            let keys = list_keys(&addr).await?;
            let id = resolve_key(&keys, &id_or_name)?;
            if !yes && !confirm(&format!("remove key {id_or_name}? [y/N] "))? {
                anyhow::bail!("aborted");
            }
            let resp = client::request(&addr, &Request::KeysRemove { id }).await?;
            match resp {
                Response::Ok => println!("key removed"),
                _ => anyhow::bail!("internal: unexpected response"),
            }
        }
        KeysCommand::Export { key, out } => {
            let keys = list_keys(&addr).await?;
            let id = resolve_key(&keys, &key)?;
            let path = out.to_string_lossy().into_owned();
            let resp = client::request(&addr, &Request::KeysExport { id, path }).await?;
            match resp {
                Response::Ok => println!("key exported"),
                _ => anyhow::bail!("internal: unexpected response"),
            }
        }
    }
    Ok(())
}

async fn credentials(socket: &Option<PathBuf>, cmd: CredentialsCommand) -> anyhow::Result<()> {
    let addr = resolve_addr(socket)?;
    match cmd {
        CredentialsCommand::List => {
            let creds = list_credentials(&addr).await?;
            let resp = client::request(&addr, &Request::ConnectionsList).await?;
            let conns = match resp {
                Response::Connections(c) => c,
                _ => anyhow::bail!("internal: unexpected response"),
            };
            println!("label  user  method  used by");
            for c in &creds {
                let used_by = conns
                    .iter()
                    .filter(|conn| conn.credential_id.as_deref() == Some(c.id.as_str()))
                    .count();
                println!(
                    "{}  {}  {}  {}",
                    c.label,
                    c.user,
                    credential_method(c),
                    used_by
                );
            }
        }
        CredentialsCommand::Add {
            label,
            user,
            key,
            password_stdin,
        } => {
            // Read the secret before any network call; argv is never used.
            let password = if password_stdin {
                match read_passphrase("password: ", true, false)? {
                    Some(p) => Some(p),
                    None => anyhow::bail!("invalid_input: password must not be empty"),
                }
            } else {
                None
            };
            let credential = Credential {
                id: String::new(),
                label,
                user,
                key_id: key,
                has_password: password.is_some(),
            };
            let resp = client::request(
                &addr,
                &Request::CredentialsAdd {
                    credential,
                    password,
                },
            )
            .await?;
            match resp {
                Response::Ok => {}
                _ => anyhow::bail!("internal: unexpected response"),
            }
            println!("credential added");
        }
        CredentialsCommand::Password {
            credential,
            password_stdin,
            clear,
        } => {
            let password = if clear {
                None
            } else {
                match read_passphrase("password: ", password_stdin, false)? {
                    Some(p) => Some(p),
                    None => anyhow::bail!("invalid_input: password must not be empty"),
                }
            };
            let creds = list_credentials(&addr).await?;
            let id = resolve_credential(&creds, &credential)?;
            let resp =
                client::request(&addr, &Request::CredentialsSetPassword { id, password }).await?;
            match resp {
                Response::Ok => println!("password {}", if clear { "cleared" } else { "saved" }),
                _ => anyhow::bail!("internal: unexpected response"),
            }
        }
        CredentialsCommand::Remove { id_or_label } => {
            let id = resolve_credential(&list_credentials(&addr).await?, &id_or_label)?;
            let resp = client::request(&addr, &Request::CredentialsRemove { id }).await?;
            match resp {
                Response::Ok => println!("credential removed"),
                _ => anyhow::bail!("internal: unexpected response"),
            }
        }
    }
    Ok(())
}

async fn list_keys(addr: &std::path::Path) -> anyhow::Result<Vec<KeyView>> {
    match client::request(addr, &Request::KeysList).await? {
        Response::Keys(k) => Ok(k),
        _ => anyhow::bail!("internal: unexpected response"),
    }
}

async fn list_credentials(addr: &std::path::Path) -> anyhow::Result<Vec<Credential>> {
    match client::request(addr, &Request::CredentialsList).await? {
        Response::Credentials(c) => Ok(c),
        _ => anyhow::bail!("internal: unexpected response"),
    }
}

fn credential_method(c: &Credential) -> &'static str {
    match (c.key_id.is_some(), c.has_password) {
        (true, true) => "key + password",
        (true, false) => "key",
        (false, true) => "password",
        (false, false) => "-",
    }
}

fn vault_status_str(status: VaultStatus) -> &'static str {
    match status {
        VaultStatus::Uninitialized => "uninitialized",
        VaultStatus::Locked => "locked",
        VaultStatus::Unlocked => "unlocked",
    }
}

fn algorithm_str(a: KeyAlgorithm) -> &'static str {
    match a {
        KeyAlgorithm::Ed25519 => "ed25519",
        KeyAlgorithm::Rsa => "rsa",
        KeyAlgorithm::Ecdsa => "ecdsa",
    }
}

fn source_str(s: KeySource) -> &'static str {
    match s {
        KeySource::Vault => "vault",
        KeySource::Disk => "disk",
    }
}

/// Read a passphrase from stdin (`stdin == true`) or an interactive prompt.
/// Empty input yields `Ok(None)`.
fn read_passphrase(prompt: &str, stdin: bool, confirm: bool) -> anyhow::Result<Option<String>> {
    if stdin {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        if line.ends_with('\n') {
            line.pop();
        }
        while line.ends_with('\r') {
            line.pop();
        }
        Ok(if line.is_empty() { None } else { Some(line) })
    } else {
        let first = rpassword::prompt_password(prompt)?;
        if first.is_empty() {
            return Ok(None);
        }
        if confirm {
            let second = rpassword::prompt_password("confirm passphrase: ")?;
            if first != second {
                anyhow::bail!("invalid_input: passphrases do not match");
            }
        }
        Ok(Some(first))
    }
}

fn require_passphrase(passphrase: Option<String>) -> anyhow::Result<String> {
    match passphrase {
        Some(p) => Ok(p),
        None => anyhow::bail!("invalid_input: passphrase must not be empty"),
    }
}

/// Prompt on stderr and read one line; `true` for `y`/`yes` (case-insensitive).
fn confirm(prompt: &str) -> anyhow::Result<bool> {
    eprint!("{prompt}");
    std::io::stderr().flush().ok();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let answer = line.trim().to_lowercase();
    Ok(answer == "y" || answer == "yes")
}

fn resolve_connection(conns: &[Connection], arg: &str) -> anyhow::Result<String> {
    if let Some(c) = conns.iter().find(|c| c.id == arg) {
        return Ok(c.id.clone());
    }
    let matches: Vec<&Connection> = conns
        .iter()
        .filter(|c| c.label.to_lowercase() == arg.to_lowercase())
        .collect();
    match matches.len() {
        1 => Ok(matches[0].id.clone()),
        0 => Ok(arg.to_string()),
        _ => {
            let ids = matches
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!("invalid_input: ambiguous label '{arg}' matches: {ids}")
        }
    }
}

/// Resolve a credential argument: an exact id wins, otherwise a
/// case-insensitive label. An unknown value is passed through so the host can
/// name it in its error, exactly like [`resolve_connection`].
fn resolve_credential(creds: &[Credential], arg: &str) -> anyhow::Result<String> {
    if let Some(c) = creds.iter().find(|c| c.id == arg) {
        return Ok(c.id.clone());
    }
    let matches: Vec<&Credential> = creds
        .iter()
        .filter(|c| c.label.to_lowercase() == arg.to_lowercase())
        .collect();
    match matches.len() {
        1 => Ok(matches[0].id.clone()),
        0 => Ok(arg.to_string()),
        _ => {
            let ids = matches
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!("invalid_input: ambiguous label '{arg}' matches: {ids}")
        }
    }
}

fn resolve_key(keys: &[KeyView], arg: &str) -> anyhow::Result<String> {
    if let Some(k) = keys.iter().find(|k| k.key.id == arg) {
        return Ok(k.key.id.clone());
    }
    let matches: Vec<&KeyView> = keys
        .iter()
        .filter(|k| k.key.name.to_lowercase() == arg.to_lowercase())
        .collect();
    match matches.len() {
        1 => Ok(matches[0].key.id.clone()),
        0 => Ok(arg.to_string()),
        _ => anyhow::bail!("invalid_input: ambiguous name '{arg}'"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credential(id: &str, label: &str) -> Credential {
        Credential {
            id: id.into(),
            label: label.into(),
            user: "deploy".into(),
            key_id: Some("k1".into()),
            has_password: false,
        }
    }

    #[test]
    fn a_credential_argument_resolves_an_id_first_then_a_label() {
        let creds = vec![
            credential("cred-1", "prod"),
            credential("cred-2", "staging"),
        ];
        assert_eq!(resolve_credential(&creds, "cred-2").unwrap(), "cred-2");
        // Labels are unique, so a case-insensitive match has one answer.
        assert_eq!(resolve_credential(&creds, "PROD").unwrap(), "cred-1");
    }

    #[test]
    fn an_unknown_credential_labels_the_host_s_error_and_a_duplicate_is_ambiguous() {
        // Passed through so the host can name the id it did not find.
        assert_eq!(resolve_credential(&[], "prod").unwrap(), "prod");
        let dup = vec![credential("a", "prod"), credential("b", "prod")];
        let err = resolve_credential(&dup, "prod").unwrap_err();
        assert!(err.to_string().contains("ambiguous"), "{err}");
    }
}
