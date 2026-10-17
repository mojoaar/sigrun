import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import "../i18n";
import Connections from "./Connections";
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
  trustHost: vi.fn().mockResolvedValue(undefined),
  provisionConnection: vi.fn(),
}));

const vaultKey = {
  key: { id: "k1", name: "work", algorithm: "ed25519" as const, public_key: "ssh-ed25519 AAAA", fingerprint: "SHA256:x", source: "vault", disk_path: null, has_passphrase: false, group: null, tags: [], notes: null, created_at: "" },
  available: true,
};

describe("Connections", () => {
  it("renders rows from listConnections", async () => {
    render(<Connections onVaultLocked={() => {}} />);
    expect(await screen.findByText("Prod")).toBeTruthy();
    expect(screen.getByText(/prod\.example\.com/)).toBeTruthy();
    expect(screen.getByText(/deploy/)).toBeTruthy();
  });

  it("calls onVaultLocked when connect is rejected with vault_locked", async () => {
    vi.mocked(api.connectGuarded).mockRejectedValueOnce({ code: "vault_locked", message: "vault is locked" });
    const onVaultLocked = vi.fn();
    render(<Connections onVaultLocked={onVaultLocked} />);
    fireEvent.click(await screen.findByRole("button", { name: "Connect" }));
    await waitFor(() => expect(onVaultLocked).toHaveBeenCalled());
  });

  it("shows the fingerprint when the host key is unknown", async () => {
    vi.mocked(api.connectGuarded).mockRejectedValueOnce({ code: "host_key_unknown", message: "SHA256:abc ssh-ed25519 AAAA" });
    render(<Connections onVaultLocked={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: "Connect" }));
    expect(await screen.findByText("SHA256:abc")).toBeTruthy();
  });

  it("warns before disabling password auth", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([
      { id: "c1", label: "web", host: "h", user: "u", port: 22, key_id: "k1", group: null, tags: [], source: "managed", disable_password_auth: false },
    ]);
    vi.mocked(api.listKeys).mockResolvedValue([vaultKey]);
    render(<Connections onVaultLocked={() => {}} />);
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
    render(<Connections onVaultLocked={() => {}} />);

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
    render(<Connections onVaultLocked={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: /install ssh key/i }));
    const panel = await screen.findByRole("dialog", { name: /install ssh key/i });
    fireEvent.change(within(panel).getByLabelText(/^password$/i), { target: { value: "pw" } });
    fireEvent.click(within(panel).getByRole("button", { name: /install ssh key/i }));

    const trust = await screen.findByRole("dialog", { name: /trust this host/i });
    expect(within(trust).getByText("SHA256:abc")).toBeTruthy();
    fireEvent.click(within(trust).getByRole("button", { name: "Trust and connect" }));

    await waitFor(() => expect(api.trustHost).toHaveBeenCalledWith("h", 22, "ssh-ed25519", "AAAA"));
    await waitFor(() => expect(api.provisionConnection).toHaveBeenCalledTimes(2));
    expect((await screen.findByRole("status")).textContent).toMatch(/sudo is not available/);
  });
});
