import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import "../i18n";
import Keys from "./Keys";
import * as api from "../api";

const { saveMock } = vi.hoisted(() => ({ saveMock: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: saveMock }));
vi.mock("../api", () => ({
  listKeys: vi.fn(),
  removeKey: vi.fn(),
  generateKey: vi.fn(),
  importKey: vi.fn(),
  exportKey: vi.fn(),
}));

const vaultKey = {
  key: { id: "k1", name: "work", algorithm: "ed25519" as const, public_key: "ssh-ed25519 AAAA", fingerprint: "SHA256:x", source: "vault", disk_path: null, has_passphrase: false, group: null, tags: [], notes: null, created_at: "" },
  available: true,
};

describe("Keys", () => {
  it("focuses Cancel in the remove confirmation and escapes without removing", async () => {
    vi.mocked(api.listKeys).mockResolvedValueOnce([vaultKey]);
    vi.mocked(api.removeKey).mockClear();
    render(<Keys onVaultLocked={() => {}} />);
    const opener = await screen.findByRole("button", { name: "Delete" });
    // jsdom does not focus a button on click; a real click (or Enter on it) does.
    opener.focus();
    fireEvent.click(opener);

    const dialog = await screen.findByRole("dialog", { name: "Remove this key? This cannot be undone." });
    expect(document.activeElement).toBe(within(dialog).getByRole("button", { name: "Cancel" }));

    fireEvent.keyDown(document, { key: "Escape" });

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(api.removeKey).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(opener);
  });
  it("copies a key on disk into the vault through the add dialog", async () => {
    const diskKey = { key: { ...vaultKey.key, id: "k2", name: "homelab", source: "disk" as const, disk_path: "/Users/x/.ssh/id_ed25519", has_passphrase: true }, available: true };
    vi.mocked(api.listKeys).mockResolvedValue([diskKey]);
    vi.mocked(api.importKey).mockResolvedValueOnce(vaultKey);
    render(<Keys onVaultLocked={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: "Add to vault" }));
    const dialog = await screen.findByRole("dialog", { name: "Add to vault" });
    expect(within(dialog).getByText("Copies this key into the vault. Leave the passphrase empty if the key has none.")).toBeDefined();
    expect(within(dialog).queryByText("keys.addToVaultBody")).toBeNull();
    fireEvent.change(within(dialog).getByLabelText("Key passphrase"), { target: { value: "s3cret" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Add to vault" }));

    expect(api.importKey).toHaveBeenCalledWith("/Users/x/.ssh/id_ed25519", "homelab", "s3cret");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("writes a vault key to the path chosen in the save dialog", async () => {
    vi.mocked(api.listKeys).mockResolvedValue([vaultKey]);
    vi.mocked(api.exportKey).mockResolvedValueOnce(undefined as never);
    saveMock.mockResolvedValueOnce("/Users/x/Downloads/work.pem");
    render(<Keys onVaultLocked={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: "Save to disk…" }));

    await waitFor(() =>
      expect(api.exportKey).toHaveBeenCalledWith("k1", "/Users/x/Downloads/work.pem"),
    );
  });

  it("offers no add button for a key already in the vault", async () => {
    vi.mocked(api.listKeys).mockResolvedValueOnce([vaultKey]);
    render(<Keys onVaultLocked={() => {}} />);
    await screen.findByText("work");
    expect(screen.queryByRole("button", { name: "Add to vault" })).toBeNull();
  });
});
