# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Rust workspace with the `sigrun-core` domain library and the `sigrun` CLI, plus CI for build, lint, and test.
- Encrypted vault: Argon2id key derivation, XChaCha20-Poly1305 sealed envelope, `0600` atomic writes, and a lockable in-memory lifecycle.
- Key generation for Ed25519, RSA, and ECDSA.
- Key discovery and import from `~/.ssh`, including passphrase-protected private keys.
- An in-process OpenSSH-compatible SSH agent with Ed25519, RSA, and ECDSA signing, served over a private socket.
- Connection manager with `ssh_config` host discovery and adoption.
- Terminal launch: build a validated `ssh` argv and open it in the user's terminal with `SSH_AUTH_SOCK`.
- The `sigrun` command-line interface (`host`, `vault`, `keys`, `connections`, `status`).
- A headless host that owns the vault, agent, and store and serves requests over local IPC.
