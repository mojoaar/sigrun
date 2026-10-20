import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import "../i18n";
import Credentials from "./Credentials";
import * as api from "../api";

vi.mock("../api", () => ({
  listCredentials: vi.fn().mockResolvedValue([]),
  listConnections: vi.fn().mockResolvedValue([]),
  listKeys: vi.fn().mockResolvedValue([]),
  addCredential: vi.fn(),
  updateCredential: vi.fn(),
  removeCredential: vi.fn(),
  setCredentialPassword: vi.fn(),
}));

const conn = (over: Partial<api.Connection> = {}): api.Connection => ({
  id: "c1", label: "web", host: "h", user: "", port: 22, key_id: null, credential_id: null,
  group: null, tags: [], source: "managed", disable_password_auth: false, has_password: false, active: false, ...over,
});

const cred = (over: Partial<api.Credential> = {}): api.Credential =>
  ({ id: "cred1", label: "Deploy", user: "deploy", key_id: null, has_password: true, ...over });

const vaultKey = {
  key: { id: "k1", name: "work", algorithm: "ed25519" as const, public_key: "ssh-ed25519 AAAA", fingerprint: "SHA256:x", source: "vault", disk_path: null, has_passphrase: false, group: null, tags: [], notes: null, created_at: "" },
  available: true,
};

describe("Credentials", () => {
  it("lists a credential with its method and how many connections use it", async () => {
    vi.mocked(api.listCredentials).mockResolvedValue([cred()]);
    vi.mocked(api.listConnections).mockResolvedValue([conn({ credential_id: "cred1" })]);
    render(<Credentials onVaultLocked={() => {}} />);

    expect(await screen.findByText("Deploy")).toBeTruthy();
    expect(screen.getByText("deploy")).toBeTruthy();
    expect(screen.getByText("Password")).toBeTruthy();
    expect(screen.getByText("Used by 1")).toBeTruthy();
  });

  it("derives the key + password method", async () => {
    vi.mocked(api.listCredentials).mockResolvedValue([cred({ key_id: "k1", has_password: true })]);
    vi.mocked(api.listConnections).mockResolvedValue([]);
    render(<Credentials onVaultLocked={() => {}} />);
    expect(await screen.findByText("Key + password")).toBeTruthy();
    expect(screen.getByText("Used by 0")).toBeTruthy();
  });

  it("adds a key-only credential once", async () => {
    vi.mocked(api.listCredentials).mockResolvedValue([]);
    vi.mocked(api.listConnections).mockResolvedValue([]);
    vi.mocked(api.listKeys).mockResolvedValue([vaultKey]);
    vi.mocked(api.addCredential).mockClear().mockResolvedValue(undefined);
    render(<Credentials onVaultLocked={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: "Add credential" }));
    const dialog = await screen.findByRole("dialog", { name: "Add credential" });
    fireEvent.change(within(dialog).getByLabelText("Label"), { target: { value: "  Shared  " } });
    fireEvent.change(within(dialog).getByLabelText("User"), { target: { value: "deploy" } });
    fireEvent.change(within(dialog).getByLabelText("Key"), { target: { value: "k1" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

    await waitFor(() => expect(api.addCredential).toHaveBeenCalledTimes(1));
    expect(api.addCredential).toHaveBeenCalledWith(
      expect.objectContaining({ id: "", label: "Shared", user: "deploy", key_id: "k1" }),
      null,
    );
    expect(screen.queryByRole("dialog", { name: "Add credential" })).toBeNull();
  });

  it("refuses an empty label and user, and focuses the label", async () => {
    vi.mocked(api.listCredentials).mockResolvedValue([]);
    vi.mocked(api.listConnections).mockResolvedValue([]);
    vi.mocked(api.addCredential).mockClear().mockResolvedValue(undefined);
    render(<Credentials onVaultLocked={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: "Add credential" }));
    const dialog = await screen.findByRole("dialog", { name: "Add credential" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

    expect(api.addCredential).not.toHaveBeenCalled();
    expect(within(dialog).getByText("A label is required.")).toBeTruthy();
    expect(within(dialog).getByText("A user is required.")).toBeTruthy();
    const label = within(dialog).getByLabelText("Label");
    expect(label.getAttribute("aria-invalid")).toBe("true");
    expect(label.getAttribute("aria-describedby")).toBe("err-label");
    expect(document.activeElement).toBe(label);
  });

  it("refuses to delete a credential that a connection still uses", async () => {
    vi.mocked(api.listCredentials).mockResolvedValue([cred()]);
    vi.mocked(api.listConnections).mockResolvedValue([conn({ credential_id: "cred1" })]);
    vi.mocked(api.removeCredential).mockClear().mockResolvedValue(undefined);
    render(<Credentials onVaultLocked={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: "Delete" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("This credential is used by one connection. Point it elsewhere before deleting.")).toBeTruthy();
    expect((within(dialog).getByRole("button", { name: "Delete" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    expect(api.removeCredential).not.toHaveBeenCalled();
  });

  it("deletes a credential nothing uses", async () => {
    vi.mocked(api.listCredentials).mockResolvedValue([cred()]);
    vi.mocked(api.listConnections).mockResolvedValue([]);
    vi.mocked(api.removeCredential).mockClear().mockResolvedValue(undefined);
    render(<Credentials onVaultLocked={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: "Delete" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).queryByText(/used by/i)).toBeNull();
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(api.removeCredential).toHaveBeenCalledWith("cred1"));
  });

  it("stores a rotated password under the credential it belongs to", async () => {
    vi.mocked(api.listCredentials).mockResolvedValue([cred()]);
    vi.mocked(api.listConnections).mockResolvedValue([]);
    vi.mocked(api.updateCredential).mockClear().mockResolvedValue(undefined);
    vi.mocked(api.setCredentialPassword).mockClear().mockResolvedValue(undefined);
    render(<Credentials onVaultLocked={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: "Edit credential" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit credential" });
    fireEvent.change(within(dialog).getByLabelText("Password"), { target: { value: "newpw" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

    await waitFor(() => expect(api.setCredentialPassword).toHaveBeenCalledWith("cred1", "newpw"));
    expect(api.updateCredential).toHaveBeenCalledWith(expect.objectContaining({ id: "cred1", label: "Deploy" }));
    // Metadata first, so the secret lands on a row that exists.
    expect(vi.mocked(api.updateCredential).mock.invocationCallOrder[0])
      .toBeLessThan(vi.mocked(api.setCredentialPassword).mock.invocationCallOrder[0]);
  });

  it("refuses to remove the last secret from a credential with no key", async () => {
    vi.mocked(api.listCredentials).mockResolvedValue([cred()]);
    vi.mocked(api.listConnections).mockResolvedValue([]);
    vi.mocked(api.updateCredential).mockClear().mockResolvedValue(undefined);
    vi.mocked(api.setCredentialPassword).mockClear().mockResolvedValue(undefined);
    render(<Credentials onVaultLocked={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: "Edit credential" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit credential" });
    fireEvent.click(within(dialog).getByLabelText(/remove the saved password/i));
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

    expect(api.updateCredential).not.toHaveBeenCalled();
    expect(api.setCredentialPassword).not.toHaveBeenCalled();
    expect(within(dialog).getByText("A key is required unless a password is saved.")).toBeTruthy();
    const key = within(dialog).getByLabelText("Key");
    expect(key.getAttribute("aria-invalid")).toBe("true");
    expect(document.activeElement).toBe(key);
  });

  it("clears the saved password when asked", async () => {
    vi.mocked(api.listCredentials).mockResolvedValue([cred({ key_id: "k1" })]);
    vi.mocked(api.listConnections).mockResolvedValue([]);
    vi.mocked(api.setCredentialPassword).mockClear().mockResolvedValue(undefined);
    render(<Credentials onVaultLocked={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: "Edit credential" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit credential" });
    fireEvent.click(within(dialog).getByLabelText(/remove the saved password/i));
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

    await waitFor(() => expect(api.setCredentialPassword).toHaveBeenCalledWith("cred1", null));
  });
});
