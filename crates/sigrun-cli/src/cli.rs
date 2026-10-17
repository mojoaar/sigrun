//! The `sigrun` command-line interface: starts the host or drives it over IPC.

use std::io::Write;
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use sigrun_core::ipc::{Request, Response};
use sigrun_core::model::{Connection, ConnectionSource, KeyAlgorithm, KeySource, KeyView};
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
    #[command(subcommand)]
    Vault(VaultCommand),
    #[command(subcommand)]
    Connections(ConnectionsCommand),
    #[command(subcommand)]
    Keys(KeysCommand),
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
    /// Lock the vault.
    Lock,
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
        #[arg(long)]
        user: String,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        key: Option<String>,
        #[arg(long)]
        group: Option<String>,
        #[arg(long)]
        tag: Vec<String>,
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
    let Cli { socket, command } = Cli::parse();
    match command {
        Command::Host => {
            sigrun_core::host::run(Paths::resolve()?, socket.clone()).await?;
            Ok(())
        }
        Command::Status => status(&socket).await,
        Command::Vault(cmd) => vault(&socket, cmd).await,
        Command::Connections(cmd) => connections(&socket, cmd).await,
        Command::Keys(cmd) => keys(&socket, cmd).await,
    }
}

fn resolve_addr(socket: &Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let paths = Paths::resolve()?;
    Ok(socket.clone().unwrap_or_else(|| paths.socket_path()))
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
        VaultCommand::Lock => {
            let resp = client::request(&addr, &Request::VaultLock).await?;
            match resp {
                Response::Ok => println!("vault locked"),
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
        } => {
            let connection = Connection {
                id: String::new(),
                label,
                host,
                user,
                port: port.unwrap_or(22),
                key_id: key,
                group,
                tags: tag,
                source: ConnectionSource::Managed,
                disable_password_auth: false,
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
                connection.key_id = Some(v);
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
            let resp = client::request(&addr, &Request::KeysList).await?;
            let keys = match resp {
                Response::Keys(k) => k,
                _ => anyhow::bail!("internal: unexpected response"),
            };
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
            let resp = client::request(&addr, &Request::KeysList).await?;
            let keys = match resp {
                Response::Keys(k) => k,
                _ => anyhow::bail!("internal: unexpected response"),
            };
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
    }
    Ok(())
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
