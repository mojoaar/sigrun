import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import "../i18n";
import Connections, { groupConnections } from "./Connections";
import * as api from "../api";

vi.mock("../api", () => ({
  listConnections: vi.fn().mockResolvedValue([
    { id: "1", label: "Prod", host: "prod.example.com", user: "deploy", port: 22, key_id: null, group: null, tags: [], source: "managed", disable_password_auth: false },
  ]),
  listKeys: vi.fn().mockResolvedValue([]),
  addConnection: vi.fn(),
  updateConnection: vi.fn(),
  removeConnection: vi.fn(),
  connectGuarded: vi.fn(),
  connectForce: vi.fn(),
  trustHost: vi.fn().mockResolvedValue(undefined),
  importKey: vi.fn(),
  provisionConnection: vi.fn(),
}));

const vaultKey = {
  key: { id: "k1", name: "work", algorithm: "ed25519" as const, public_key: "ssh-ed25519 AAAA", fingerprint: "SHA256:x", source: "vault", disk_path: null, has_passphrase: false, group: null, tags: [], notes: null, created_at: "" },
  available: true,
};

const diskKey = {
  key: { id: "d1", name: "laptop", algorithm: "ed25519" as const, public_key: "ssh-ed25519 BBBB", fingerprint: "SHA256:y", source: "disk", disk_path: "/home/me/.ssh/id_ed25519", has_passphrase: false, group: null, tags: [], notes: null, created_at: "" },
  available: true,
};

const conn: api.Connection = { id: "c0", label: "base", host: "h", user: "u", port: 22, key_id: null, group: null, tags: [], source: "managed", disable_password_auth: false };

describe("Connections", () => {
  it("groups connections under headings", async () => {
    vi.mocked(api.listConnections).mockResolvedValueOnce([
      { ...conn, id: "c1", label: "pve01", group: "PVE" },
      { ...conn, id: "c2", label: "web", group: null },
    ]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    expect(await screen.findByRole("heading", { name: "PVE" })).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "Ungrouped" })).toBeNull();
  });

  it("treats a blank group as ungrouped", () => {
    const out = groupConnections([{ ...conn, id: "a", group: "   " }]);
    expect(out).toHaveLength(1);
    expect(out[0].group).toBeNull();
  });

  it("renders rows from listConnections", async () => {
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    expect(await screen.findByText("Prod")).toBeTruthy();
    expect(screen.getByText(/prod\.example\.com/)).toBeTruthy();
    expect(screen.getByText(/deploy/)).toBeTruthy();
  });

  it("calls onVaultLocked when connect is rejected with vault_locked", async () => {
    vi.mocked(api.connectGuarded).mockRejectedValueOnce({ code: "vault_locked", message: "vault is locked" });
    const onVaultLocked = vi.fn();
    render(<Connections onVaultLocked={onVaultLocked} onGoToKeys={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: "Connect" }));
    await waitFor(() => expect(onVaultLocked).toHaveBeenCalled());
  });

  it("shows the fingerprint when the host key is unknown", async () => {
    vi.mocked(api.connectGuarded).mockRejectedValueOnce({ code: "host_key_unknown", message: "SHA256:abc ssh-ed25519 AAAA" });
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: "Connect" }));
    expect(await screen.findByText("SHA256:abc")).toBeTruthy();
  });

  it("offers to connect anyway when the host is unreachable", async () => {
    vi.mocked(api.connectGuarded).mockRejectedValueOnce({ code: "unreachable", message: "connection refused on h:22" });
    vi.mocked(api.connectForce).mockResolvedValueOnce(undefined);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: "Connect" }));

    const dialog = await screen.findByRole("dialog", { name: "Host is not reachable" });
    expect(within(dialog).getByText("connection refused on h:22")).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "Connect anyway" }));

    await waitFor(() => expect(api.connectForce).toHaveBeenCalledWith("1"));
  });

  it("warns before disabling password auth", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([
      { id: "c1", label: "web", host: "h", user: "u", port: 22, key_id: "k1", group: null, tags: [], source: "managed", disable_password_auth: false },
    ]);
    vi.mocked(api.listKeys).mockResolvedValue([vaultKey]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: /install ssh key/i }));
    fireEvent.click(await screen.findByLabelText(/disable password auth/i));
    expect((await screen.findByRole("alert")).textContent).toMatch(/lock you out/i);
  });

  it("provisions, persists the choice, and reports the outcome", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([
      { id: "c1", label: "web", host: "h", user: "u", port: 22, key_id: "k1", group: null, tags: [], source: "managed", disable_password_auth: false },
    ]);
    vi.mocked(api.listKeys).mockResolvedValue([vaultKey]);
    vi.mocked(api.provisionConnection).mockResolvedValue({ installed: ["work"], verified: ["work"], hardening: "applied" });
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: /install ssh key/i }));
    const dialog = await screen.findByRole("dialog", { name: /install ssh key/i });
    const password = within(dialog).getByLabelText(/^password$/i);
    expect(password.getAttribute("type")).toBe("password");
    fireEvent.change(password, { target: { value: "hunter2" } });
    fireEvent.click(within(dialog).getByLabelText(/disable password auth/i));
    fireEvent.click(within(dialog).getByRole("button", { name: /install ssh key/i }));

    await waitFor(() => expect(api.provisionConnection).toHaveBeenCalledWith("c1", "hunter2", ["k1"], true));
    expect((await screen.findByRole("status")).textContent).toMatch(/work/);
    await waitFor(() => expect(api.updateConnection).toHaveBeenCalledWith(expect.objectContaining({ id: "c1", disable_password_auth: true })));
    expect(screen.queryByRole("dialog", { name: /install ssh key/i })).toBeNull();
  });

  it("retries provisioning after trusting an unknown host key", async () => {
    vi.mocked(api.provisionConnection).mockClear();
    vi.mocked(api.listConnections).mockResolvedValue([
      { id: "c1", label: "web", host: "h", user: "u", port: 22, key_id: "k1", group: null, tags: [], source: "managed", disable_password_auth: false },
    ]);
    vi.mocked(api.listKeys).mockResolvedValue([vaultKey]);
    vi.mocked(api.provisionConnection)
      .mockRejectedValueOnce({ code: "host_key_unknown", message: "SHA256:abc ssh-ed25519 AAAA" })
      .mockResolvedValueOnce({ installed: ["work"], verified: ["work"], hardening: { skipped: "sudo is not available" } });
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: /install ssh key/i }));
    const panel = await screen.findByRole("dialog", { name: /install ssh key/i });
    fireEvent.change(within(panel).getByLabelText(/^password$/i), { target: { value: "pw" } });
    fireEvent.click(within(panel).getByRole("button", { name: /install ssh key/i }));

    const trust = await screen.findByRole("dialog", { name: /trust this host/i });
    expect(within(trust).getByText("SHA256:abc")).toBeTruthy();
    fireEvent.click(within(trust).getByRole("button", { name: "Trust and connect" }));

    await waitFor(() => expect(api.trustHost).toHaveBeenCalledWith("h", 22, "ssh-ed25519", "AAAA"));
    await waitFor(() => expect(api.provisionConnection).toHaveBeenCalledTimes(2));
    // The retry must reuse the typed password and the fingerprint parsed off the wire.
    expect(api.provisionConnection).toHaveBeenLastCalledWith("c1", "pw", ["k1"], false);
    expect((await screen.findByRole("status")).textContent).toMatch(/sudo is not available/);
  });

  it("does not persist the flag when hardening was skipped", async () => {
    vi.mocked(api.updateConnection).mockClear();
    vi.mocked(api.listConnections).mockResolvedValue([
      { id: "c1", label: "web", host: "h", user: "u", port: 22, key_id: "k1", group: null, tags: [], source: "managed", disable_password_auth: false },
    ]);
    vi.mocked(api.listKeys).mockResolvedValue([vaultKey]);
    vi.mocked(api.provisionConnection).mockResolvedValue({ installed: ["work"], verified: ["work"], hardening: { skipped: "sudo is not available" } });
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: /install ssh key/i }));
    const panel = await screen.findByRole("dialog", { name: /install ssh key/i });
    fireEvent.change(within(panel).getByLabelText(/^password$/i), { target: { value: "pw" } });
    fireEvent.click(within(panel).getByLabelText(/disable password auth/i));
    fireEvent.click(within(panel).getByRole("button", { name: /install ssh key/i }));

    await waitFor(() => expect(api.updateConnection).toHaveBeenCalled());
    expect(api.updateConnection).toHaveBeenLastCalledWith(expect.objectContaining({ id: "c1", disable_password_auth: false }));
  });

  it("ignores a disk key_id when preselecting vault keys", async () => {
    vi.mocked(api.provisionConnection).mockClear();
    vi.mocked(api.listConnections).mockResolvedValue([
      { id: "c1", label: "web", host: "h", user: "u", port: 22, key_id: "d1", group: null, tags: [], source: "managed", disable_password_auth: false },
    ]);
    vi.mocked(api.listKeys).mockResolvedValue([vaultKey, diskKey]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: /install ssh key/i }));
    const panel = await screen.findByRole("dialog", { name: /install ssh key/i });
    for (const box of within(panel).getAllByRole("checkbox"))
      expect((box as HTMLInputElement).checked).toBe(false);
    expect((within(panel).getByRole("button", { name: /install ssh key/i }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("clears the password after a failed submit and on close", async () => {
    vi.mocked(api.provisionConnection).mockClear();
    vi.mocked(api.provisionConnection).mockRejectedValueOnce({ code: "internal", message: "boom" });
    vi.mocked(api.listConnections).mockResolvedValue([
      { id: "c1", label: "web", host: "h", user: "u", port: 22, key_id: "k1", group: null, tags: [], source: "managed", disable_password_auth: false },
    ]);
    vi.mocked(api.listKeys).mockResolvedValue([vaultKey]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: /install ssh key/i }));
    const panel = await screen.findByRole("dialog", { name: /install ssh key/i });
    const password = () => (within(panel).getByLabelText(/^password$/i) as HTMLInputElement).value;
    fireEvent.change(within(panel).getByLabelText(/^password$/i), { target: { value: "secret" } });
    fireEvent.click(within(panel).getByRole("button", { name: /install ssh key/i }));

    await waitFor(() => expect(password()).toBe(""));
    expect(within(panel).getByRole("alert")).toBeTruthy();

    fireEvent.change(within(panel).getByLabelText(/^password$/i), { target: { value: "again" } });
    fireEvent.click(within(panel).getByRole("button", { name: /cancel/i }));
    expect(screen.queryByRole("dialog", { name: /install ssh key/i })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /install ssh key/i }));
    const reopened = await screen.findByRole("dialog", { name: /install ssh key/i });
    expect((within(reopened).getByLabelText(/^password$/i) as HTMLInputElement).value).toBe("");
  });

  it("maps a locked vault during provisioning to onVaultLocked", async () => {
    vi.mocked(api.provisionConnection).mockClear();
    vi.mocked(api.provisionConnection).mockRejectedValueOnce({ code: "vault_locked", message: "vault is locked" });
    vi.mocked(api.listConnections).mockResolvedValue([
      { id: "c1", label: "web", host: "h", user: "u", port: 22, key_id: "k1", group: null, tags: [], source: "managed", disable_password_auth: false },
    ]);
    vi.mocked(api.listKeys).mockResolvedValue([vaultKey]);
    const onVaultLocked = vi.fn();
    render(<Connections onVaultLocked={onVaultLocked} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: /install ssh key/i }));
    const panel = await screen.findByRole("dialog", { name: /install ssh key/i });
    fireEvent.change(within(panel).getByLabelText(/^password$/i), { target: { value: "pw" } });
    fireEvent.click(within(panel).getByRole("button", { name: /install ssh key/i }));

    await waitFor(() => expect(onVaultLocked).toHaveBeenCalled());
    expect(within(panel).queryByRole("alert")).toBeNull();
  });

  it("offers installation only when editing a saved connection", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([
      { id: "c1", label: "web", host: "h", user: "u", port: 22, key_id: "k1", group: null, tags: [], source: "managed", disable_password_auth: false },
    ]);
    vi.mocked(api.listKeys).mockResolvedValue([vaultKey]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: "Add connection" }));
    const add = await screen.findByRole("dialog", { name: "Add connection" });
    expect(within(add).queryByRole("button", { name: /install ssh key/i })).toBeNull();
    fireEvent.click(within(add).getByRole("button", { name: /cancel/i }));

    fireEvent.click(screen.getByRole("button", { name: "Edit connection" }));
    const edit = await screen.findByRole("dialog", { name: "Edit connection" });
    expect(within(edit).getByRole("button", { name: /install ssh key/i })).toBeTruthy();
  });

  it("imports a selected disk key and provisions with the new vault id", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([
      { id: "c1", label: "web", host: "h", user: "u", port: 22, key_id: null, group: null, tags: [], source: "managed", disable_password_auth: false },
    ]);
    vi.mocked(api.listKeys).mockResolvedValue([
      { key: { ...diskKey.key, id: "sha256:disk", name: "work", disk_path: "/home/u/.ssh/id_work", has_passphrase: false }, available: true },
    ]);
    vi.mocked(api.importKey).mockResolvedValue({ key: { ...vaultKey.key, id: "k-new", name: "work" }, available: true });
    vi.mocked(api.provisionConnection).mockResolvedValue({ installed: ["work"], verified: ["work"], hardening: "applied" });
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: /install/i }));
    const panel = await screen.findByRole("dialog", { name: /install ssh key/i });
    fireEvent.click(within(panel).getByRole("checkbox", { name: "work" }));
    fireEvent.change(within(panel).getByLabelText(/^password$/i), { target: { value: "pw" } });
    fireEvent.click(within(panel).getByRole("button", { name: /install/i }));

    await waitFor(() => expect(api.importKey).toHaveBeenCalledWith("/home/u/.ssh/id_work", "work", null));
    await waitFor(() => expect(api.provisionConnection).toHaveBeenCalledWith("c1", "pw", ["k-new"], false));
  });

  it("shows the host's message when provisioning fails for a non-trust reason", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([
      { id: "c1", label: "web", host: "h", user: "u", port: 22, key_id: "k1", group: null, tags: [], source: "managed", disable_password_auth: false },
    ]);
    vi.mocked(api.listKeys).mockResolvedValue([vaultKey]);
    vi.mocked(api.provisionConnection).mockRejectedValueOnce({ code: "unreachable", message: "connection refused" });
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: /install/i }));
    const panel = await screen.findByRole("dialog", { name: /install ssh key/i });
    fireEvent.change(within(panel).getByLabelText(/^password$/i), { target: { value: "pw" } });
    fireEvent.click(within(panel).getByRole("button", { name: /install/i }));

    expect((await screen.findByRole("alert")).textContent).toContain("connection refused");
  });

  it("offers to import a key when it has no usable key", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([{ ...conn, id: "c1", label: "web" }]);
    vi.mocked(api.listKeys).mockResolvedValue([]);
    const onGoToKeys = vi.fn();
    render(<Connections onVaultLocked={() => {}} onGoToKeys={onGoToKeys} />);

    fireEvent.click(await screen.findByRole("button", { name: "Import a key" }));
    expect(onGoToKeys).toHaveBeenCalled();
  });

  it("imports a disk key once and reuses it across the trust retry", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([
      { id: "c1", label: "web", host: "h", user: "u", port: 22, key_id: null, group: null, tags: [], source: "managed", disable_password_auth: false },
    ]);
    vi.mocked(api.listKeys).mockResolvedValue([diskKey]);
    // A second import would mint a different vault id, so the retry must reuse the first.
    vi.mocked(api.importKey)
      .mockClear()
      .mockResolvedValueOnce({ key: { ...vaultKey.key, id: "k-new", name: "laptop" }, available: true })
      .mockResolvedValueOnce({ key: { ...vaultKey.key, id: "k-dup", name: "laptop" }, available: true });
    vi.mocked(api.provisionConnection)
      .mockClear()
      .mockRejectedValueOnce({ code: "host_key_unknown", message: "SHA256:abc ssh-ed25519 AAAA" })
      .mockResolvedValueOnce({ installed: ["laptop"], verified: ["laptop"], hardening: "applied" });
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: /install/i }));
    const panel = await screen.findByRole("dialog", { name: /install ssh key/i });
    fireEvent.click(within(panel).getByRole("checkbox", { name: "laptop" }));
    fireEvent.change(within(panel).getByLabelText(/^password$/i), { target: { value: "pw" } });
    fireEvent.click(within(panel).getByRole("button", { name: /install/i }));

    const trust = await screen.findByRole("dialog", { name: /trust this host/i });
    fireEvent.click(within(trust).getByRole("button", { name: "Trust and connect" }));

    await waitFor(() => expect(api.provisionConnection).toHaveBeenCalledTimes(2));
    expect(api.importKey).toHaveBeenCalledTimes(1);
    expect(api.provisionConnection).toHaveBeenLastCalledWith("c1", "pw", ["k-new"], false);
  });
});
