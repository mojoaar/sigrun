# Sigrun import and export — design

**Status:** implemented 2026-10-19.

## Why

A Sigrun install holds two things worth moving: the connections you have built
up, and the metadata for the keys you use (names, fingerprints, the public
halves). Before this pass there was no way to get either onto a second machine
or back after a reinstall — you retyped connections and re-imported keys by
hand.

This added an **Import and export** section to Settings that writes a folder or
a `.zip` file, and reads it back. It is deliberately *not* a backup, and the
exported material says so itself.

## Goals

- Export every connection and every key's metadata plus its public key, to a
  folder **or** a `.zip` file, through the native save dialog.
- Import from either form, through the native open dialog.
- Never overwrite what is already there: conflicts are skipped and counted.
- Report what happened — added, skipped, and any connection whose key is not on
  this machine.
- Work with the vault **locked**: nothing here reads or writes the vault.

## Non-goals

- Not a backup. Private key material, stored connection passwords and vault
  contents never leave the vault.
- No cloud, sync, scheduling or merge beyond "skip if the id exists".
- No selective export (all connections, all keys) and no per-item import in v1.
- No CSV or other interchange formats.

## Anti-goals — what would make this a failure

- A secret in the output. Ever.
- Overwriting or deleting an existing connection or key during import.
- A zip that cannot be read back, or a folder and a zip that hold different
  things.
- Writing into `~/.ssh`, which is read-only by rule.
- A crafted export that writes outside the chosen destination or exhausts
  memory.

## Constraints

- **The dependency pins guard the MSRV.** The floor is Rust 1.90, so
  `tauri-plugin-dialog` stays on **2.7.x** and `zip` on **7.3.x**; the `~`
  pins stop a later release from needing a newer toolchain.
  Both go in `[workspace.dependencies]`; `zip` with
  `default-features = false` and only its deflate backend, because the default
  set drags in bzip2, zstd, lzma, ppmd and AES for no reason here.
- The frontend needs `@tauri-apps/plugin-dialog`, and the webview's calls need
  `dialog:allow-open` and `dialog:allow-save` in
  `apps/desktop/src-tauri/capabilities/default.json` — the narrower pair, not the
  wider `dialog:default` — without them the pickers
  are rejected and a swallowed promise hides it. The Rust plugin is registered
  with `tauri_plugin_dialog::init()`.
- Every user-facing string goes through `t(...)` with the key in both
  `apps/desktop/src/i18n/en.json` and `da.json`.
- Nothing here is a secret, so the written files are ordinary `0644`. No
  `write_atomic`, no `0600`: they sit wherever the user chose.
- The logic lives in `sigrun-core`; the Tauri layer only marshals.

## The format, version 1

Folder and zip hold exactly the same entries:

```
sigrun-export-2026-10-19/
  README.txt            what this is, and what it is not
  export.json           the machine-readable part
  keys/<safe-name>-<fingerprint>.pub   one OpenSSH public key per key, human-readable
```

`export.json`:

```json
{
  "format": 1,
  "exported_at": 1792300800,
  "connections": [ /* every Connection field */ ],
  "keys": [ /* every KeyMetadata field */ ]
}
```

- `exported_at` is Unix seconds, so no date crate is needed.
- Connections carry the host-filled fields (`has_password`, `active`) forced to
  `false` on write and again on import.
- Keys carry the whole `KeyMetadata` record, including `public_key`,
  `fingerprint`, `source` and `has_passphrase`. A public key and the fact that
  a private half is encrypted are not secrets.
- `format` is the only version marker. An unknown value is refused with a
  message saying the export came from a newer Sigrun.

`README.txt` states plainly: this is a transfer, not a backup; private keys and
stored passwords are not here; the vault file plus the passphrase is the only
way to restore those.

## Keys in the export and import — superseded

**Superseded by the Revision 2026-10-19 below: import is connections-only, and
this section does not describe the shipped behaviour.** The original draft
proposed importing key metadata too, so a reinstall would bring a key list back
by name and fingerprint. The revision records why that was dropped;
`bundle::merge` reads only `bundle.connections`, and the `keys/*.pub` files are
reference material for the human.

## Behaviour

### Export

Two buttons: **Export to a folder…** and **Export to a `.zip` file…**.

1. The native save dialog opens, defaulting to `sigrun-export-<date>` (folder)
   or `sigrun-export-<date>.zip`.
2. The host writes the three entries above via
   `Request::ExportBundle { path, zipped }`.
3. The destination is **refused if it is inside `~/.ssh`**, compared after
   canonicalising both paths.
4. If `export.json` already exists at the destination, the export is refused
   rather than half-overwriting someone's folder.
5. The section then shows the summary: "Exported 12 connections and 3 keys."

The vault does not have to be unlocked: connections and key metadata both come
from `data.toml`.

### Import

Two buttons: **Import from a folder…** and **Import from a `.zip` file…**.

`Request::ImportBundle { path }` then:

1. Reads `export.json`. A missing or unparsable file is an error, not a panic;
   an unknown `format` is refused with a "newer Sigrun" message.
2. Keys: no key record is imported; `keys/*.pub` files are ignored on import —
   they are for the human. A connection whose `key_id` names a key not on this
   machine is still added and counted in `unresolved_keys`.
3. Connections: any entry whose id is already present is skipped. Entries that
   fail validation (empty host, a host that is not a plain name, a port of 0,
   an empty label) are skipped and counted separately rather than stored.
   `has_password` and `active` are forced `false`.
4. Nothing is deleted and nothing is overwritten.
5. The summary reports added, skipped and invalid counts, plus how many
   imported connections now reference a key that is not on this machine.

Importing the same export twice adds nothing the second time.

### Format changes

`export.json` carries one integer, `format`, and `FORMAT_VERSION` is `1`.
`read_bundle` refuses any other value with "this export was written by a newer
Sigrun (format N); update Sigrun to import it", so an older build fails closed
instead of misreading a newer file. When the format changes, bump
`FORMAT_VERSION` and teach `read_bundle` the old shape in the same commit —
never reinterpret an old file silently. A second version does not justify a
migration framework before it exists. `bundle.rs`'s
`an_unknown_format_is_refused_with_a_newer_sigrun_message` test pins the
refusal.

### Reading a zip safely

A zip is untrusted input. We read only the entry named `export.json`; other
entries are ignored, never written anywhere. Limits: at most 512 entries, at
most 1 MiB per entry, at most 8 MiB total uncompressed. Exceeding a limit is an
error, so a zip bomb cannot be used to exhaust memory. No entry name is ever
used as a path.

## Interfaces

`sigrun-core`:

```rust
pub struct ExportSummary { pub path: String, pub connections: usize, pub keys: usize }
pub struct ImportSummary {
    pub added: usize, pub skipped: usize, pub invalid: usize,
    pub unresolved_keys: usize,
}
impl HostState {
    async fn export_bundle(&self, path: &str, zipped: bool) -> Result<ExportSummary>;
    async fn import_bundle(&self, path: &str) -> Result<ImportSummary>;
}
```

Both summaries derive `Serialize`/`Deserialize` so they ride the IPC wire.
The public helpers carry the decisions a test can pin without a running host:
`build_bundle(&DataFile, now_unix: u64) -> Bundle`, `read_bundle(&Path) ->
Result<Bundle>`, `merge(&mut DataFile, &Bundle) -> ImportSummary`,
`valid_connection(&Connection) -> bool` and `destination_is_allowed(&Path,
&Path) -> bool`.

IPC: `Request::ExportBundle { path: String, zipped: bool }` →
`Response::Export(ExportSummary)`, and `Request::ImportBundle { path: String }` →
`Response::Import(ImportSummary)`. Neither touches the vault, so neither belongs
in the idle-auto-lock `last_vault_activity` list.

Desktop: `export_bundle(state, path, zipped)` and `import_bundle(state, path)`
app commands, thin wrappers over `handle`, registered in the `invoke_handler`
list. Frontend: `exportBundle(path, zipped)` and `importBundle(path)` in
`api.ts`, and a Settings section holding the four buttons, a one-line hint and
a result line.

## What is left to the builder

- The exact copy, and whether the result line is one sentence or a short list.
- Where the section sits among the existing Settings controls (my default: last,
  after Start minimized).
- Zip compression level, and whether the archive stores modification times (my
  default: do not bother).
- Whether a later plan should add the same two operations to the CLI, now that
  the IPC carries them.

## Review focus

1. An export pointed inside `~/.ssh` must be refused, not written.
2. An imported connection must never arrive with `has_password` or `active`
   set, whatever the file says.
3. Importing the same export twice must add nothing the second time.
4. A crafted zip — a `../` entry name, an oversized entry, thousands of entries
   — must not write outside the destination, exhaust memory, or panic.
5. The folder and the zip must round-trip to the same state, entry for entry.

---

## Revision 2026-10-19 (after the first implementation review)

1. **Import is connections-only.** The earlier idea of importing key *records*
   too is dropped. `keys_list` lists only vault-source keys, so an imported
   disk-source record would be invisible rather than "unavailable"; and a
   record upserted by id would overwrite. The keys half of a bundle is
   reference material — public key text, name, fingerprint and algorithm — and
   import ignores it. Disk-key ids are fingerprints, so a reinstall
   re-discovers the same ids and an imported connection still resolves; a vault
   key can only come back from the vault file.
2. **A key's `disk_path` is stripped on export.** It is an absolute path on the
   exporting machine: meaningless elsewhere, and a leak of the exporter's
   filesystem layout.
3. **The dependency pins use a `~` range.** `zip` 7.4 needs Rust 1.88 and
   `tauri-plugin-dialog` 2.8 needs 1.90, so a caret range can pull a release
   that raises the floor. Pinned to 7.3.x and 2.7.x.
4. **The `~/.ssh` refusal must not canonicalise a destination that does not
   exist yet**, which is the normal case for a picker result. Compare against
   the destination's longest existing ancestor plus the remainder, and refuse
   an existing zip target the way the folder case refuses an existing
   `export.json`.
5. Revised during implementation: this block now carries the shipped
   signatures and summary fields. `bundle.rs` remains the reference.
