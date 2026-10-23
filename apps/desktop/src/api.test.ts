import { beforeEach, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { exportBundle, getSettings, importBundle, printCommand, saveSettings, vaultBackup, vaultChangePassphrase, type Settings } from "./api";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => vi.mocked(invoke).mockReset());

test("getSettings requests settings and returns typed Settings object", async () => {
  const settings: Settings = {
    version: 1,
    theme: "nordlys",
    language: "en",
    terminal: null,
    open_in: "window",
    start_minimized: false,
    auto_lock_minutes: 30,
  };
  vi.mocked(invoke).mockResolvedValue({ type: "settings", value: settings });
  expect(await getSettings()).toEqual(settings);
  expect(invoke).toHaveBeenCalledWith("get_settings", undefined);
});

test("saveSettings sends settings to host", async () => {
  const settings: Settings = {
    version: 1,
    theme: "nordlys",
    language: "en",
    terminal: null,
    open_in: "window",
    start_minimized: false,
    auto_lock_minutes: 30,
  };
  vi.mocked(invoke).mockResolvedValue({ type: "ok" });
  await saveSettings(settings);
  expect(invoke).toHaveBeenCalledWith("save_settings", { settings });
});

test("exportBundle asks the host to write the bundle at the picked path", async () => {
  const summary = { path: "/tmp/backup", connections: 2, keys: 1 };
  vi.mocked(invoke).mockResolvedValue({ type: "export", value: summary });
  expect(await exportBundle("/tmp/backup", false)).toEqual(summary);
  expect(invoke).toHaveBeenCalledWith("export_bundle", { path: "/tmp/backup", zipped: false });
});

test("importBundle asks the host to merge the bundle at the picked path", async () => {
  const summary = { added: 3, skipped: 1, invalid: 0, unresolved_keys: 2 };
  vi.mocked(invoke).mockResolvedValue({ type: "import", value: summary });
  expect(await importBundle("/tmp/backup.zip")).toEqual(summary);
  expect(invoke).toHaveBeenCalledWith("import_bundle", { path: "/tmp/backup.zip" });
});

test("printCommand asks host to render the ssh command for connection", async () => {
  vi.mocked(invoke).mockResolvedValue({ type: "command", value: "ssh -p 22 deploy@prod.example.com" });
  expect(await printCommand("c1")).toBe("ssh -p 22 deploy@prod.example.com");
  expect(invoke).toHaveBeenCalledWith("connect", { id: "c1", printOnly: true, force: false });
});

test("vaultChangePassphrase calls host with current and new passphrase", async () => {
  vi.mocked(invoke).mockResolvedValue({ type: "ok" });
  await vaultChangePassphrase("current", "new");
  expect(invoke).toHaveBeenCalledWith("vault_change_passphrase", {
    currentPassphrase: "current",
    newPassphrase: "new",
  });
});

test("vaultBackup calls host with destination", async () => {
  vi.mocked(invoke).mockResolvedValue({ type: "ok" });
  await vaultBackup("/path/to/backup.sigrun");
  expect(invoke).toHaveBeenCalledWith("vault_backup", {
    destination: "/path/to/backup.sigrun",
  });
});
