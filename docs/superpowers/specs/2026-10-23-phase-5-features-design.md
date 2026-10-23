# Phase 5: Features Design (F1–F3, G1, G3)

## Goals
Implement top quick-win and missing features identified during the audit:
- **F1:** Copy SSH Command button on connection rows in Desktop app.
- **F2:** CLI `sigrun credentials password <credential> [--password-stdin] [--clear]` to set/clear credential passwords.
- **F3:** CLI `sigrun keys export <key> --out <path>` to export a public key.
- **G1:** Vault passphrase change across Core, IPC, CLI, and Desktop Settings view.
- **G3:** Vault backup across Core, IPC, CLI, and Desktop Settings view.

## Security & Architecture Constraints
- **Zero Secrets on argv:** Per AGENTS.md, passwords and passphrases are never accepted via argv. CLI commands use interactive terminal prompts or `--password-stdin`.
- **Vault Crypto Invariants:** Passphrase change derives a fresh Argon2id key with pinned parameters (m=65536, t=3, p=4) and fresh 16-byte salt, re-seals payload with fresh 24-byte nonce, writes atomically at 0600. If disk persist fails, in-memory state is rolled back.
- **Vault Backup Invariants:** Verifies destination is not within `~/.ssh`, fails closed if destination already exists (`create_new(true)`), and sets 0600 permissions.
- **One Identity Source:** Credentials and connections remain cleanly decoupled.
- **Full i18n Parity:** All user-facing strings in Desktop views are routed through `t(...)` across all seven locales (`en`, `da`, `de`, `es`, `fr`, `nb`, `sv`).
