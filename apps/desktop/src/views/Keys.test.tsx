import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import "../i18n";
import Keys from "./Keys";
import * as api from "../api";

vi.mock("../api", () => ({
  listKeys: vi.fn(),
  removeKey: vi.fn(),
  generateKey: vi.fn(),
  importKey: vi.fn(),
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
});
