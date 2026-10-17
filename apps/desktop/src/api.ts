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
export type KeyAlgorithm = "ed25519" | "rsa" | "ecdsa";
export type Settings = { version: number; theme: string; language: string; terminal: string | null; agent_enabled: boolean };

// Unwraps the adjacently tagged Response envelope: errors throw, success yields `value`.
async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const res: any = await invoke(command, args);
  if (res?.type === "error") throw res.value as CommandError;
  return res?.value as T;
}

export const getStatus = () => call<StatusInfo>("get_status");
export const vaultInit = (passphrase: string) => call<void>("vault_init", { passphrase });
export const vaultUnlock = (passphrase: string) => call<void>("vault_unlock", { passphrase });
export const vaultLock = () => call<void>("vault_lock");
export const listConnections = () => call<Connection[]>("list_connections");
export const addConnection = (conn: Connection) => call<void>("add_connection", { conn });
export const updateConnection = (conn: Connection) => call<void>("update_connection", { conn });
export const removeConnection = (id: string) => call<void>("remove_connection", { id });
export const connect = (id: string, printOnly = false) =>
  call<string | undefined>("connect", { id, printOnly }).then((v) => (printOnly ? v : undefined));
export const listKeys = () => call<KeyView[]>("list_keys");
export const generateKey = (name: string, algorithm: KeyAlgorithm, passphrase?: string) =>
  call<void>("generate_key", { name, algorithm, passphrase: passphrase || null });
export const importKey = (path: string, name: string, passphrase?: string) =>
  call<void>("import_key", { path, name, passphrase: passphrase || null });
export const removeKey = (id: string) => call<void>("remove_key", { id });
export const getSettings = async () => JSON.parse(await call<string>("get_settings")) as Settings;
export const saveSettings = (settings: Settings) => call<void>("save_settings", { settings });
