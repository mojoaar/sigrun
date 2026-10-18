import { invoke } from "@tauri-apps/api/core";

export type VaultState = "Uninitialized" | "Locked" | "Unlocked";
export type CommandError = { code: string; message: string };
export type StatusInfo = { version: string; vault: VaultState; agent_socket: string | null; host_addr: string };
export type Connection = {
  id: string;
  label: string;
  host: string;
  user: string;
  port: number;
  key_id: string | null;
  group: string | null;
  tags: string[];
  source: "managed" | "discovered";
  disable_password_auth: boolean;
};
export type KeyMetadata = {
  id: string;
  name: string;
  algorithm: KeyAlgorithm;
  public_key: string;
  fingerprint: string;
  source: string;
  disk_path: string | null;
  has_passphrase: boolean;
  group: string | null;
  tags: string[];
  notes: string | null;
  created_at: string;
};
export type KeyView = { key: KeyMetadata; available: boolean };
export type Hardening = "not_requested" | "applied" | { skipped: string };
export type ProvisionOutcome = { installed: string[]; verified: string[]; hardening: Hardening };
export type KeyAlgorithm = "ed25519" | "rsa" | "ecdsa";
export type Settings = { version: number; theme: string; language: string; terminal: string | null; agent_enabled: boolean; start_minimized: boolean; auto_lock_minutes: number };
export type TerminalInfo = { id: string; name: string };

// Unwraps the adjacently tagged Response envelope: `error` rejects, `ok` has
// no payload (void success), everything else resolves with `value`.
async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const res: any = await invoke(command, args);
  if (res == null || typeof res !== "object" || typeof res.type !== "string")
    throw { code: "internal", message: `malformed response from ${command}` } as CommandError;
  if (res.type === "error") throw res.value as CommandError;
  return (res as { value?: unknown }).value as T;
}

export const getStatus = () => call<StatusInfo>("get_status");
export const vaultInit = (passphrase: string) => call<void>("vault_init", { passphrase });
export const vaultUnlock = (passphrase: string) => call<void>("vault_unlock", { passphrase });
export const vaultLock = () => call<void>("vault_lock");
export const listConnections = () => call<Connection[]>("list_connections");
export const addConnection = (conn: Connection) => call<void>("add_connection", { conn });
export const updateConnection = (conn: Connection) => call<void>("update_connection", { conn });
export const removeConnection = (id: string) => call<void>("remove_connection", { id });
export const connectGuarded = (id: string) => call<void>("connect", { id, printOnly: false, force: false });
export const connectForce = (id: string) => call<void>("connect", { id, printOnly: false, force: true });
export const trustHost = (host: string, port: number, algo: string, keyB64: string) =>
  call<void>("trust_host", { host, port, algo, keyB64 });
export const provisionConnection = (id: string, password: string, keyIds: string[], disablePasswordAuth: boolean) =>
  call<ProvisionOutcome>("provision_connection", { id, password, keyIds, disablePasswordAuth });
// Rust `KeyView` flattens `key` onto the wire; re-nest it for the UI.
export const listKeys = async (): Promise<KeyView[]> => {
  const rows = await call<Array<KeyMetadata & { available: boolean }>>("list_keys");
  return rows.map(({ available, ...key }) => ({ key, available }));
};
export const listTerminals = () => call<TerminalInfo[]>("list_terminals");
export const generateKey = (name: string, algorithm: KeyAlgorithm, passphrase?: string) =>
  call<void>("generate_key", { name, algorithm, passphrase: passphrase || null });
export const importKey = (path: string, name: string, passphrase?: string | null) =>
  call<KeyMetadata & { available: boolean }>("import_key", { path, name, passphrase: passphrase || null })
    .then(({ available, ...key }) => ({ key, available }));
export const removeKey = (id: string) => call<void>("remove_key", { id });
export const getSettings = async () => JSON.parse(await call<string>("get_settings")) as Settings;
export const saveSettings = (settings: Settings) => call<void>("save_settings", { settings });
