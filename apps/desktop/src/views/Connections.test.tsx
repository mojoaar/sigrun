import { act, render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { listen } from "@tauri-apps/api/event";
import "../i18n";
import Connections, { filterConnections, groupPaths, groupTree, validateConnection, type ConnectionDraft } from "./Connections";
import * as api from "../api";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));

vi.mock("../api", () => ({
  listConnections: vi.fn().mockResolvedValue([
    { id: "1", label: "Prod", host: "prod.example.com", user: "deploy", port: 22, credential_id: null, key_id: null, group: null, tags: [], source: "managed", disable_password_auth: false, has_password: false, active: false },
  ]),
  listKeys: vi.fn().mockResolvedValue([]),
  listCredentials: vi.fn().mockResolvedValue([]),
  addConnection: vi.fn(),
  updateConnection: vi.fn(),
  removeConnection: vi.fn(),
  connectGuarded: vi.fn(),
  connectForce: vi.fn(),
  trustHost: vi.fn().mockResolvedValue(undefined),
  importKey: vi.fn(),
  provisionConnection: vi.fn(),
  setConnectionPassword: vi.fn(),
}));

const vaultKey = {
  key: { id: "k1", name: "work", algorithm: "ed25519" as const, public_key: "ssh-ed25519 AAAA", fingerprint: "SHA256:x", source: "vault", disk_path: null, has_passphrase: false, group: null, tags: [], notes: null, created_at: "" },
  available: true,
};

const diskKey = {
  key: { id: "d1", name: "laptop", algorithm: "ed25519" as const, public_key: "ssh-ed25519 BBBB", fingerprint: "SHA256:y", source: "disk", disk_path: "/home/me/.ssh/id_ed25519", has_passphrase: false, group: null, tags: [], notes: null, created_at: "" },
  available: true,
};

const conn: api.Connection = { id: "c0", label: "base", host: "h", user: "u", port: 22, credential_id: null, key_id: null, group: null, tags: [], source: "managed", disable_password_auth: false, has_password: false, active: false };

const draft = (over: Partial<ConnectionDraft> = {}): ConnectionDraft => ({ ...conn, port: "22", ...over });

const credential = (over: Partial<api.Credential> = {}): api.Credential =>
  ({ id: "cred1", label: "Deploy", user: "deploy", key_id: null, has_password: true, ...over });

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

  it("nests group paths like PVE/Folder 1", async () => {
    vi.mocked(api.listConnections).mockResolvedValueOnce([
      { ...conn, id: "c1", label: "pve01", group: "PVE" },
      { ...conn, id: "c2", label: "web1", group: "PVE/Folder 1" },
      { ...conn, id: "c3", label: "web2", group: "PVE/Folder 2" },
    ]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    expect(await screen.findByRole("heading", { name: "PVE" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Folder 1" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Folder 2" })).toBeTruthy();
    expect(screen.getByText("pve01")).toBeTruthy();
  });

  it("treats extra or blank separators as one level", () => {
    const tree = groupTree([{ ...conn, id: "a", group: " PVE//Folder 1/ " }]);
    expect(tree).toHaveLength(1);
    expect(tree[0].name).toBe("PVE");
    expect(tree[0].children[0].name).toBe("Folder 1");
  });

  it("treats a blank group as ungrouped", () => {
    const out = groupTree([{ ...conn, id: "a", group: "   " }]);
    expect(out).toHaveLength(1);
    expect(out[0].path).toBe("");
  });

  it("renders ungrouped connections before the first group", async () => {
    vi.mocked(api.listConnections).mockResolvedValueOnce([
      { ...conn, id: "c1", label: "pve01", group: "PVE" },
      { ...conn, id: "c2", label: "web", group: null },
    ]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    const loose = await screen.findByText("web");
    const heading = screen.getByRole("heading", { name: "PVE" });
    // FOLLOWING means the PVE heading comes after the ungrouped row.
    expect(loose.compareDocumentPosition(heading) & Node.DOCUMENT_POSITION_FOLLOWING)
      .toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  });

  it("sorts group roots case-insensitively", async () => {
    vi.mocked(api.listConnections).mockResolvedValueOnce([
      { ...conn, id: "c1", label: "banana1", group: "Banana" },
      { ...conn, id: "c2", label: "apple1", group: "apple" },
    ]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    const apple = await screen.findByRole("heading", { name: "apple" });
    const banana = screen.getByRole("heading", { name: "Banana" });
    expect(apple.compareDocumentPosition(banana) & Node.DOCUMENT_POSITION_FOLLOWING)
      .toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  });

  it("sorts child groups case-insensitively", async () => {
    vi.mocked(api.listConnections).mockResolvedValueOnce([
      { ...conn, id: "c1", label: "z1", group: "PVE/Zebra" },
      { ...conn, id: "c2", label: "a1", group: "PVE/apple" },
    ]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    const apple = await screen.findByRole("heading", { name: "apple" });
    const zebra = screen.getByRole("heading", { name: "Zebra" });
    expect(apple.compareDocumentPosition(zebra) & Node.DOCUMENT_POSITION_FOLLOWING)
      .toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  });

  it("renders a child group inside its parent section, indented", async () => {
    vi.mocked(api.listConnections).mockResolvedValueOnce([
      { ...conn, id: "c1", label: "pve01", group: "PVE" },
      { ...conn, id: "c2", label: "web1", group: "PVE/Folder 1" },
    ]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    const parent = (await screen.findByRole("heading", { name: "PVE" })).closest("section") as HTMLElement;
    const child = screen.getByRole("heading", { name: "Folder 1" });
    expect(parent.contains(child)).toBe(true);
    // The child section sits in the parent's pl-3 indentation wrapper.
    expect(child.closest("section")?.parentElement?.className).toContain("pl-3");
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

  it("opens the trust dialog from a tray error", async () => {
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} connectionError={{ id: "c1", code: "host_key_unknown", message: "SHA256:x ssh-ed25519 AAAA", host: "h", port: 22 }} onConnectionErrorHandled={() => {}} />);
    const dialog = await screen.findByRole("dialog");
    expect(dialog.textContent).toContain("SHA256:x");
    fireEvent.click(within(dialog).getByRole("button", { name: /trust and connect/i }));
    await waitFor(() => expect(api.connectGuarded).toHaveBeenCalledWith("c1"));
  });

  it("opens the unreachable dialog from a tray error", async () => {
    vi.mocked(api.connectForce).mockClear().mockResolvedValueOnce(undefined);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} connectionError={{ id: "c1", code: "unreachable", message: "connection refused on h:22", host: "h", port: 22 }} onConnectionErrorHandled={() => {}} />);
    const dialog = await screen.findByRole("dialog");
    expect(dialog.textContent).toContain("connection refused on h:22");
    fireEvent.click(within(dialog).getByRole("button", { name: /connect anyway/i }));
    await waitFor(() => expect(api.connectForce).toHaveBeenCalledWith("c1"));
    // A successful retry closes the dialog instead of leaving it on screen.
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("shows the error alert when the tray unreachable retry fails", async () => {
    vi.mocked(api.connectForce).mockClear().mockRejectedValueOnce({ code: "internal", message: "boom" });
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} connectionError={{ id: "c1", code: "unreachable", message: "connection refused on h:22", host: "h", port: 22 }} onConnectionErrorHandled={() => {}} />);
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: /connect anyway/i }));
    expect((await screen.findByRole("alert")).textContent).toMatch(/something went wrong/i);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("fails closed on a tray host-key mismatch", async () => {
    vi.mocked(api.trustHost).mockClear();
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} connectionError={{ id: "c1", code: "host_key_mismatch", message: "SHA256:x ssh-ed25519 AAAA", host: "h", port: 22 }} onConnectionErrorHandled={() => {}} />);
    const banner = await screen.findByRole("alert");
    expect(banner.textContent).toMatch(/fingerprint has changed/i);
    // No accept affordance: the changed key is never recorded.
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(api.trustHost).not.toHaveBeenCalled();
  });

  it("warns before disabling password auth", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([
      { id: "c1", label: "web", host: "h", user: "u", port: 22, credential_id: null, key_id: "k1", group: null, tags: [], source: "managed", disable_password_auth: false, has_password: false, active: false },
    ]);
    vi.mocked(api.listKeys).mockResolvedValue([vaultKey]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: /install ssh key/i }));
    fireEvent.click(await screen.findByLabelText(/disable password auth/i));
    expect((await screen.findByRole("alert")).textContent).toMatch(/lock you out/i);
  });

  it("provisions, persists the choice, and reports the outcome", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([
      { id: "c1", label: "web", host: "h", user: "u", port: 22, credential_id: null, key_id: "k1", group: null, tags: [], source: "managed", disable_password_auth: false, has_password: false, active: false },
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
      { id: "c1", label: "web", host: "h", user: "u", port: 22, credential_id: null, key_id: "k1", group: null, tags: [], source: "managed", disable_password_auth: false, has_password: false, active: false },
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
      { id: "c1", label: "web", host: "h", user: "u", port: 22, credential_id: null, key_id: "k1", group: null, tags: [], source: "managed", disable_password_auth: false, has_password: false, active: false },
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
      { id: "c1", label: "web", host: "h", user: "u", port: 22, credential_id: null, key_id: "d1", group: null, tags: [], source: "managed", disable_password_auth: false, has_password: false, active: false },
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
      { id: "c1", label: "web", host: "h", user: "u", port: 22, credential_id: null, key_id: "k1", group: null, tags: [], source: "managed", disable_password_auth: false, has_password: false, active: false },
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
      { id: "c1", label: "web", host: "h", user: "u", port: 22, credential_id: null, key_id: "k1", group: null, tags: [], source: "managed", disable_password_auth: false, has_password: false, active: false },
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
      { id: "c1", label: "web", host: "h", user: "u", port: 22, credential_id: null, key_id: "k1", group: null, tags: [], source: "managed", disable_password_auth: false, has_password: false, active: false },
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
      { id: "c1", label: "web", host: "h", user: "u", port: 22, credential_id: null, key_id: null, group: null, tags: [], source: "managed", disable_password_auth: false, has_password: false, active: false },
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
      { id: "c1", label: "web", host: "h", user: "u", port: 22, credential_id: null, key_id: "k1", group: null, tags: [], source: "managed", disable_password_auth: false, has_password: false, active: false },
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

  it("asks before removing a connection, then removes it", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([{ ...conn, id: "c1", label: "web" }]);
    vi.mocked(api.removeConnection).mockResolvedValue(undefined);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: /delete/i }));
    expect(api.removeConnection).not.toHaveBeenCalled();
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: /delete/i }));
    await waitFor(() => expect(api.removeConnection).toHaveBeenCalledWith("c1"));
  });

  it("removes nothing when the confirm card is cancelled", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([{ ...conn, id: "c1", label: "web" }]);
    vi.mocked(api.removeConnection).mockClear().mockResolvedValue(undefined);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: /delete/i }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: /cancel/i }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(api.removeConnection).not.toHaveBeenCalled();
  });

  it("re-targets the confirm card to the last row whose delete was clicked", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([
      { ...conn, id: "c1", label: "web" },
      { ...conn, id: "c2", label: "db" },
    ]);
    vi.mocked(api.removeConnection).mockClear().mockResolvedValue(undefined);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    const row = (label: string) => screen.getByText(label).closest("li") as HTMLElement;
    await screen.findByText("web");
    fireEvent.click(within(row("web")).getByRole("button", { name: /delete/i }));
    expect(api.removeConnection).not.toHaveBeenCalled();
    fireEvent.click(within(row("db")).getByRole("button", { name: /delete/i }));

    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: /delete/i }));
    await waitFor(() => expect(api.removeConnection).toHaveBeenCalledWith("c2"));
    expect(api.removeConnection).toHaveBeenCalledTimes(1);
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
      { id: "c1", label: "web", host: "h", user: "u", port: 22, credential_id: null, key_id: null, group: null, tags: [], source: "managed", disable_password_auth: false, has_password: false, active: false },
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

  it("shows the import error and never provisions when a disk key needs a passphrase", async () => {
    vi.mocked(api.provisionConnection).mockClear();
    vi.mocked(api.listConnections).mockResolvedValue([
      { id: "c1", label: "web", host: "h", user: "u", port: 22, credential_id: null, key_id: null, group: null, tags: [], source: "managed", disable_password_auth: false, has_password: false, active: false },
    ]);
    vi.mocked(api.listKeys).mockResolvedValue([
      { key: { ...diskKey.key, has_passphrase: true }, available: true },
    ]);
    vi.mocked(api.importKey).mockReset().mockRejectedValueOnce({ code: "invalid_input", message: "key is encrypted; a passphrase is required" });
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: /install/i }));
    const panel = await screen.findByRole("dialog", { name: /install ssh key/i });
    fireEvent.click(within(panel).getByRole("checkbox", { name: "laptop" }));
    fireEvent.change(within(panel).getByLabelText(/^password$/i), { target: { value: "pw" } });
    fireEvent.click(within(panel).getByRole("button", { name: /install/i }));

    const alert = await within(panel).findByRole("alert");
    expect(alert.textContent).toContain("key is encrypted; a passphrase is required");
    expect(api.provisionConnection).not.toHaveBeenCalled();
  });

  it("focuses the first control in the add dialog and escapes back to the toolbar", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([]);
    vi.mocked(api.addConnection).mockClear();
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    const opener = await screen.findByRole("button", { name: "Add connection" });
    // jsdom does not focus a button on click; a real click (or Enter on it) does.
    opener.focus();
    fireEvent.click(opener);

    const dialog = await screen.findByRole("dialog", { name: "Add connection" });
    // The source switch now leads the form, so it takes the opening focus.
    expect(document.activeElement).toBe(within(dialog).getByLabelText("Enter my own"));

    fireEvent.keyDown(document, { key: "Escape" });

    expect(screen.queryByRole("dialog", { name: "Add connection" })).toBeNull();
    expect(api.addConnection).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(opener);
  });

  it("focuses Cancel in the remove confirmation and escapes without deleting", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([{ ...conn, id: "c1", label: "web" }]);
    vi.mocked(api.removeConnection).mockClear().mockResolvedValue(undefined);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    const opener = await screen.findByRole("button", { name: "Delete" });
    opener.focus();
    fireEvent.click(opener);

    const dialog = await screen.findByRole("dialog");
    expect(document.activeElement).toBe(within(dialog).getByRole("button", { name: "Cancel" }));

    fireEvent.keyDown(document, { key: "Escape" });

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(api.removeConnection).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(opener);
  });

  it("closes only the trust prompt when Escape is pressed above the provision panel", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([
      { id: "c1", label: "web", host: "h", user: "u", port: 22, credential_id: null, key_id: "k1", group: null, tags: [], source: "managed", disable_password_auth: false, has_password: false, active: false },
    ]);
    vi.mocked(api.listKeys).mockResolvedValue([vaultKey]);
    vi.mocked(api.provisionConnection).mockClear()
      .mockRejectedValueOnce({ code: "host_key_unknown", message: "SHA256:abc ssh-ed25519 AAAA" });
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: /install ssh key/i }));
    const panel = await screen.findByRole("dialog", { name: /install ssh key/i });
    fireEvent.change(within(panel).getByLabelText(/^password$/i), { target: { value: "pw" } });
    fireEvent.click(within(panel).getByRole("button", { name: /install ssh key/i }));
    await screen.findByRole("dialog", { name: /trust this host/i });

    // A held key must not cascade down the stack.
    fireEvent.keyDown(document, { key: "Escape", repeat: true });
    expect(screen.getByRole("dialog", { name: /trust this host/i })).toBeTruthy();
    expect(screen.getByRole("dialog", { name: /install ssh key/i })).toBeTruthy();

    fireEvent.keyDown(document, { key: "Escape" });

    expect(screen.queryByRole("dialog", { name: /trust this host/i })).toBeNull();
    expect(screen.getByRole("dialog", { name: /install ssh key/i })).toBeTruthy();
  });

  it("saves a connection, then stores the typed password under the form's id", async () => {
    vi.mocked(api.addConnection).mockClear().mockResolvedValue(undefined);
    vi.mocked(api.setConnectionPassword).mockClear().mockResolvedValue(undefined);
    vi.mocked(api.listConnections).mockResolvedValue([{ ...conn, id: "c1", label: "web" }]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: "Add connection" }));
    const dialog = await screen.findByRole("dialog", { name: "Add connection" });
    fireEvent.change(within(dialog).getByLabelText("Label"), { target: { value: "fresh" } });
    fireEvent.change(within(dialog).getByLabelText("Host"), { target: { value: "h" } });
    fireEvent.change(within(dialog).getByLabelText("User"), { target: { value: "u" } });
    const password = within(dialog).getByLabelText("Password") as HTMLInputElement;
    expect(password.getAttribute("type")).toBe("password");
    fireEvent.change(password, { target: { value: "pw" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

    await waitFor(() => expect(api.setConnectionPassword).toHaveBeenCalled());
    const id = vi.mocked(api.addConnection).mock.calls[0][0].id;
    expect(id).toBeTruthy();
    expect(api.setConnectionPassword).toHaveBeenCalledWith(id, "pw");
    // The connection must exist before its password is stored.
    expect(vi.mocked(api.addConnection).mock.invocationCallOrder[0])
      .toBeLessThan(vi.mocked(api.setConnectionPassword).mock.invocationCallOrder[0]);
  });

  it("marks a row whose connection has a saved password", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([
      { ...conn, id: "c1", label: "withpw", has_password: true },
      { ...conn, id: "c2", label: "nopw" },
    ]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    await screen.findByText("withpw");

    const row = (label: string) => screen.getByText(label).closest("li") as HTMLElement;
    expect(within(row("withpw")).getByText("Password saved")).toBeTruthy();
    expect(within(row("nopw")).queryByText("Password saved")).toBeNull();
  });

  it("clears a saved password when the remove checkbox is ticked", async () => {
    vi.mocked(api.updateConnection).mockClear().mockResolvedValue(undefined);
    vi.mocked(api.setConnectionPassword).mockClear().mockResolvedValue(undefined);
    vi.mocked(api.listConnections).mockResolvedValue([{ ...conn, id: "c1", label: "web", has_password: true }]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: "Edit connection" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit connection" });
    fireEvent.click(within(dialog).getByLabelText(/remove the saved password/i));
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

    await waitFor(() => expect(api.setConnectionPassword).toHaveBeenCalledWith("c1", null));
    expect(api.setConnectionPassword).toHaveBeenCalledTimes(1);
    expect(api.updateConnection).toHaveBeenCalledWith(expect.objectContaining({ id: "c1" }));
  });

  it("shows the remove checkbox only when a password is saved", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([{ ...conn, id: "c1", label: "web" }]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: "Add connection" }));
    const add = await screen.findByRole("dialog", { name: "Add connection" });
    expect(within(add).queryByLabelText(/remove the saved password/i)).toBeNull();
    fireEvent.click(within(add).getByRole("button", { name: /cancel/i }));

    fireEvent.click(await screen.findByRole("button", { name: "Edit connection" }));
    const edit = await screen.findByRole("dialog", { name: "Edit connection" });
    // has_password is false on this row.
    expect(within(edit).queryByLabelText(/remove the saved password/i)).toBeNull();
  });

  it("lets a typed password win over the remove checkbox", async () => {
    vi.mocked(api.setConnectionPassword).mockClear().mockResolvedValue(undefined);
    vi.mocked(api.listConnections).mockResolvedValue([{ ...conn, id: "c1", label: "web", has_password: true }]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: "Edit connection" }));
    let dialog = await screen.findByRole("dialog", { name: "Edit connection" });
    fireEvent.change(within(dialog).getByLabelText("Password"), { target: { value: "pw" } });
    fireEvent.click(within(dialog).getByLabelText(/remove the saved password/i));
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(api.setConnectionPassword).toHaveBeenCalledWith("c1", "pw"));

    vi.mocked(api.setConnectionPassword).mockClear();
    fireEvent.click(await screen.findByRole("button", { name: "Edit connection" }));
    dialog = await screen.findByRole("dialog", { name: "Edit connection" });
    fireEvent.click(within(dialog).getByLabelText(/remove the saved password/i));
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(api.setConnectionPassword).toHaveBeenCalledWith("c1", null));
    expect(api.setConnectionPassword).toHaveBeenCalledTimes(1);
  });

  it("routes a locked vault during the password save to onVaultLocked", async () => {
    vi.mocked(api.addConnection).mockClear().mockResolvedValue(undefined);
    vi.mocked(api.setConnectionPassword).mockClear().mockRejectedValueOnce({ code: "vault_locked", message: "vault is locked" });
    vi.mocked(api.listConnections).mockResolvedValue([{ ...conn, id: "c1", label: "web" }]);
    const onVaultLocked = vi.fn();
    render(<Connections onVaultLocked={onVaultLocked} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: "Add connection" }));
    const dialog = await screen.findByRole("dialog", { name: "Add connection" });
    fireEvent.change(within(dialog).getByLabelText("Label"), { target: { value: "fresh" } });
    fireEvent.change(within(dialog).getByLabelText("Host"), { target: { value: "h" } });
    fireEvent.change(within(dialog).getByLabelText("User"), { target: { value: "u" } });
    fireEvent.change(within(dialog).getByLabelText("Password"), { target: { value: "pw" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

    await waitFor(() => expect(onVaultLocked).toHaveBeenCalled());
    // No password was stored, and the form stays open for a retry.
    expect(within(dialog).queryByRole("alert")).toBeNull();
    expect(screen.getByRole("dialog", { name: "Add connection" })).toBeTruthy();
  });

  it("shows a connected dot on a live row and hides it otherwise", async () => {
    vi.mocked(api.listConnections).mockResolvedValueOnce([
      { ...conn, id: "c1", label: "live", active: true },
      { ...conn, id: "c2", label: "idle", active: false },
    ]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    await screen.findByText("live");
    const row = (label: string) => screen.getByText(label).closest("li") as HTMLElement;
    expect(within(row("live")).getByRole("img", { name: "Connected" })).toBeTruthy();
    expect(within(row("idle")).queryByRole("img", { name: "Connected" })).toBeNull();
  });

  it("suggests every prefix of the group paths already in use", () => {
    expect(groupPaths([
      { ...conn, id: "a", group: "PVE/Folder 1" },
      { ...conn, id: "b", group: "PVE" },
      { ...conn, id: "c", group: "/Cloud//EU/" },
      { ...conn, id: "d", group: null },
      { ...conn, id: "e", group: "   " },
    ])).toEqual(["Cloud", "Cloud/EU", "PVE", "PVE/Folder 1"]);
  });

  it("validates label, host, user and port the way the launcher does", () => {
    expect(validateConnection(draft())).toEqual({});
    // label
    expect(validateConnection(draft({ label: "" })).label).toBe("connections.invalidLabel");
    expect(validateConnection(draft({ label: "   " })).label).toBe("connections.invalidLabel");
    // host: the charset ssh_invocation enforces (terminal.rs:39)
    for (const host of ["", "   ", "web example.com", "web/example", "example.com; rm -rf /", "exa$mple"]) {
      expect(validateConnection(draft({ host })).host).toBe("connections.invalidHost");
    }
    for (const host of ["web", "web.example.com", "10.0.0.5", "host_1", "a-b"]) {
      expect(validateConnection(draft({ host })).host).toBeUndefined();
    }
    // user: whitespace-only is not a user the launcher refuses, it trims to empty
    expect(validateConnection(draft({ user: "" })).user).toBeUndefined();
    expect(validateConnection(draft({ user: "   " })).user).toBeUndefined();
    expect(validateConnection(draft({ user: "deploy" })).user).toBeUndefined();
    for (const user of ["de ploy", "deploy$(id)", "deploy/x"]) {
      expect(validateConnection(draft({ user })).user).toBe("connections.invalidUser");
    }
    // With a credential the login name comes from it, so the field's value is moot.
    expect(validateConnection(draft({ credential_id: "cred1", user: "de ploy" })).user).toBeUndefined();
    // port: integers only, 1..=65535, both ends inclusive
    for (const port of ["", " ", "abc", "0", "-1", "1.5", "1e5", "65536"]) {
      expect(validateConnection(draft({ port })).port).toBe("connections.invalidPort");
    }
    for (const port of ["1", "22", " 22 ", "65535"]) {
      expect(validateConnection(draft({ port })).port).toBeUndefined();
    }
  });

  it("reports every offending field at once", () => {
    expect(validateConnection(draft({ label: "", host: "a b", user: "a;b", port: "0" }))).toEqual({
      label: "connections.invalidLabel",
      host: "connections.invalidHost",
      user: "connections.invalidUser",
      port: "connections.invalidPort",
    });
  });

  it("offers the known group paths on the group field", async () => {
    vi.mocked(api.listConnections).mockResolvedValueOnce([{ ...conn, id: "a", group: "PVE/Folder 1" }]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: "Add connection" }));
    const group = screen.getByLabelText("Group") as HTMLInputElement;
    expect(group.getAttribute("list")).toBe("sigrun-groups");
    const options = Array.from(document.querySelectorAll("#sigrun-groups option")).map((o) => o.getAttribute("value"));
    expect(options).toEqual(["PVE", "PVE/Folder 1"]);
  });

  it("warns when the label duplicates another connection, but not about itself", async () => {
    vi.mocked(api.listConnections).mockResolvedValueOnce([
      { id: "1", label: "Prod", host: "prod.example.com", user: "deploy", port: 22, credential_id: null, key_id: null, group: null, tags: [], source: "managed", disable_password_auth: false, has_password: false, active: false },
      { ...conn, id: "2", label: "other" },
      { ...conn, id: "3", label: "  padded  " },
    ]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    // Add mode: typing another connection's label warns.
    fireEvent.click(await screen.findByRole("button", { name: "Add connection" }));
    fireEvent.change(screen.getByLabelText("Label"), { target: { value: "other" } });
    expect(screen.getByText("Another connection already uses this label.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /cancel/i }));

    // Edit mode: a connection never warns about its own label...
    const row = screen.getByText("Prod").closest("li") as HTMLElement;
    fireEvent.click(within(row).getByRole("button", { name: "Edit connection" }));
    const edit = await screen.findByRole("dialog", { name: "Edit connection" });
    const own = within(edit).getByLabelText("Label") as HTMLInputElement;
    expect(own.value).toBe("Prod");
    expect(screen.queryByText("Another connection already uses this label.")).toBeNull();
    // ...but the list is live, so another connection's label still warns.
    fireEvent.change(own, { target: { value: "other" } });
    expect(screen.getByText("Another connection already uses this label.")).toBeTruthy();

    // Stray whitespace on a stored label does not hide the match: both sides
    // are compared trimmed.
    fireEvent.change(own, { target: { value: "padded" } });
    expect(screen.getByText("Another connection already uses this label.")).toBeTruthy();
  });

  it("refetches when the host reports a changed active set", async () => {
    vi.mocked(api.listConnections).mockClear().mockResolvedValue([]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    await waitFor(() => expect(api.listConnections).toHaveBeenCalledTimes(1));
    const call = vi.mocked(listen).mock.calls.find(([event]) => event === "connections-changed");
    expect(call).toBeTruthy();
    await act(async () => { (call![1] as () => void)(); });
    await waitFor(() => expect(api.listConnections).toHaveBeenCalledTimes(2));
  });

  it("filters connections by label, user, host, port and group, case-insensitively", () => {
    const rows: api.Connection[] = [
      { ...conn, id: "1", label: "Prod", user: "deploy", host: "web.example.com", port: 22, group: "PVE/Cloud" },
      { ...conn, id: "2", label: "Staging", user: "root", host: "10.0.0.5", port: 2222, group: null },
    ];
    expect(filterConnections(rows, "").map((c) => c.id)).toEqual(["1", "2"]);
    expect(filterConnections(rows, "   ").map((c) => c.id)).toEqual(["1", "2"]);
    expect(filterConnections(rows, "prod").map((c) => c.id)).toEqual(["1"]);
    expect(filterConnections(rows, "ROOT").map((c) => c.id)).toEqual(["2"]);
    expect(filterConnections(rows, "example").map((c) => c.id)).toEqual(["1"]);
    expect(filterConnections(rows, "22").map((c) => c.id)).toEqual(["1", "2"]);
    expect(filterConnections(rows, "pve").map((c) => c.id)).toEqual(["1"]);
    expect(filterConnections(rows, "nothing-here")).toEqual([]);
  });

  it("filters the list from the search field and restores it when cleared", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([
      { ...conn, id: "1", label: "Prod", group: null },
      { ...conn, id: "2", label: "Staging", group: null },
    ]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    await screen.findByText("Prod");

    fireEvent.change(screen.getByLabelText("Search connections"), { target: { value: "prod" } });
    expect(screen.getByText("Prod")).toBeTruthy();
    expect(screen.queryByText("Staging")).toBeNull();

    fireEvent.change(screen.getByLabelText("Search connections"), { target: { value: "" } });
    expect(screen.getByText("Prod")).toBeTruthy();
    expect(screen.getByText("Staging")).toBeTruthy();
  });

  it("says when nothing matches the search", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([
      { ...conn, id: "1", label: "Prod", group: null },
    ]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    await screen.findByText("Prod");

    fireEvent.change(screen.getByLabelText("Search connections"), { target: { value: "zzz" } });
    expect(screen.getByText("No connections match.")).toBeTruthy();
    expect(within(screen.getByRole("status")).getByText("No connections match.")).toBeTruthy();
  });

  it("refuses a bad port and focuses it", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([]);
    vi.mocked(api.addConnection).mockClear();
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: "Add connection" }));
    const dialog = await screen.findByRole("dialog", { name: "Add connection" });
    fireEvent.change(within(dialog).getByLabelText("Label"), { target: { value: "Prod" } });
    fireEvent.change(within(dialog).getByLabelText("Host"), { target: { value: "web.example.com" } });
    fireEvent.change(within(dialog).getByLabelText("Port"), { target: { value: "0" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

    expect(api.addConnection).not.toHaveBeenCalled();
    expect(within(dialog).getByText("Enter a port between 1 and 65535.")).toBeTruthy();
    const port = within(dialog).getByLabelText("Port");
    expect(port.getAttribute("aria-invalid")).toBe("true");
    expect(port.getAttribute("aria-describedby")).toBe("err-port");
    expect(document.activeElement).toBe(port);
  });

  it("refuses a host with a space, then saves a padded valid one", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([]);
    vi.mocked(api.addConnection).mockClear().mockResolvedValue(undefined);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: "Add connection" }));
    const dialog = await screen.findByRole("dialog", { name: "Add connection" });
    fireEvent.change(within(dialog).getByLabelText("Label"), { target: { value: "Prod" } });
    fireEvent.change(within(dialog).getByLabelText("Host"), { target: { value: "web example.com" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

    expect(api.addConnection).not.toHaveBeenCalled();
    expect(within(dialog).getByText("Use only letters, digits, dots, underscores and hyphens.")).toBeTruthy();

    fireEvent.change(within(dialog).getByLabelText("Label"), { target: { value: "  Prod  " } });
    fireEvent.change(within(dialog).getByLabelText("Host"), { target: { value: "  web.example.com  " } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

    await waitFor(() => expect(api.addConnection).toHaveBeenCalledTimes(1));
    const call = vi.mocked(api.addConnection).mock.calls[0][0];
    expect(call.label).toBe("Prod");
    expect(call.host).toBe("web.example.com");
    expect(typeof call.port).toBe("number");
    expect(call.port).toBe(22);
  });

  it("saves an imported connection with an empty user", async () => {
    vi.mocked(api.updateConnection).mockClear().mockResolvedValue(undefined);
    vi.mocked(api.listConnections).mockResolvedValue([{ ...conn, id: "imp", label: "Imported", user: "", source: "managed" }]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    await screen.findByText("Imported");
    const row = screen.getByText("Imported").closest("li") as HTMLElement;
    fireEvent.click(within(row).getByRole("button", { name: "Edit connection" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit connection" });
    // No native constraint validation in jsdom: `required` would silently block
    // the save without these, so the empty user is refused by hand.
    for (const name of ["Label", "Host", "User", "Port"])
      expect((within(dialog).getByLabelText(name) as HTMLInputElement).hasAttribute("required")).toBe(false);
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

    await waitFor(() => expect(api.updateConnection).toHaveBeenCalledTimes(1));
    const call = vi.mocked(api.updateConnection).mock.calls[0][0];
    expect(call.user).toBe("");
    expect(typeof call.port).toBe("number");
    expect(call.port).toBe(22);
  });

  it("opens the add form when the menu asks for a new connection", async () => {
    const opened = vi.fn();
    render(<Connections openNew onNewOpened={opened} onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    expect(await screen.findByRole("dialog", { name: "Add connection" })).toBeTruthy();
    expect(opened).toHaveBeenCalled();
  });

  it("keeps an open edit form when the menu asks for a new connection", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([{ ...conn, id: "c1", label: "web" }]);
    vi.mocked(api.addConnection).mockClear();
    vi.mocked(api.updateConnection).mockClear().mockResolvedValue(undefined);
    const opened = vi.fn();
    const { rerender } = render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: "Edit connection" }));
    await screen.findByRole("dialog", { name: "Edit connection" });

    rerender(<Connections openNew onNewOpened={opened} onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    // The request is consumed, but the form under it is left as it was.
    expect(opened).toHaveBeenCalled();
    const edit = screen.getByRole("dialog", { name: "Edit connection" });
    expect((within(edit).getByLabelText("Label") as HTMLInputElement).value).toBe("web");

    fireEvent.click(within(edit).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(api.updateConnection).toHaveBeenCalledWith(expect.objectContaining({ id: "c1" })));
    expect(api.addConnection).not.toHaveBeenCalled();
  });

  it("shows a blank form when Add is clicked over an open edit form", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([{ ...conn, id: "c1", label: "web" }]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: "Edit connection" }));
    await screen.findByRole("dialog", { name: "Edit connection" });

    fireEvent.click(screen.getByRole("button", { name: "Add connection" }));
    const add = await screen.findByRole("dialog", { name: "Add connection" });
    expect((within(add).getByLabelText("Label") as HTMLInputElement).value).toBe("");
    expect((within(add).getByLabelText("Host") as HTMLInputElement).value).toBe("");
  });

  it("hides the form's own login fields when a credential is chosen", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([]);
    vi.mocked(api.listCredentials).mockResolvedValueOnce([credential()]);
    vi.mocked(api.addConnection).mockClear().mockResolvedValue(undefined);
    vi.mocked(api.setConnectionPassword).mockClear();
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: "Add connection" }));
    const dialog = await screen.findByRole("dialog", { name: "Add connection" });
    expect((within(dialog).getByLabelText("Enter my own") as HTMLInputElement).checked).toBe(true);
    const useCredential = within(dialog).getByLabelText("Use a credential") as HTMLInputElement;
    await waitFor(() => expect(useCredential.disabled).toBe(false));

    fireEvent.click(useCredential);
    expect(within(dialog).queryByLabelText("User")).toBeNull();
    expect(within(dialog).queryByLabelText("Password")).toBeNull();
    expect(within(dialog).queryByLabelText("Key")).toBeNull();

    fireEvent.change(within(dialog).getByLabelText("Label"), { target: { value: "web" } });
    fireEvent.change(within(dialog).getByLabelText("Host"), { target: { value: "h" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

    await waitFor(() => expect(api.addConnection).toHaveBeenCalledTimes(1));
    expect(api.addConnection).toHaveBeenCalledWith(expect.objectContaining({
      label: "web", user: "", key_id: null, has_password: false, credential_id: "cred1",
    }));
    expect(api.setConnectionPassword).not.toHaveBeenCalled();
  });

  it("clears the credential when the user switches back to their own details", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([]);
    vi.mocked(api.listCredentials).mockResolvedValueOnce([credential()]);
    vi.mocked(api.addConnection).mockClear().mockResolvedValue(undefined);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: "Add connection" }));
    const dialog = await screen.findByRole("dialog", { name: "Add connection" });
    const useCredential = within(dialog).getByLabelText("Use a credential") as HTMLInputElement;
    await waitFor(() => expect(useCredential.disabled).toBe(false));

    fireEvent.click(useCredential);
    fireEvent.click(within(dialog).getByLabelText("Enter my own"));
    fireEvent.change(within(dialog).getByLabelText("Label"), { target: { value: "web" } });
    fireEvent.change(within(dialog).getByLabelText("Host"), { target: { value: "h" } });
    fireEvent.change(within(dialog).getByLabelText("User"), { target: { value: "u" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

    await waitFor(() => expect(api.addConnection).toHaveBeenCalledTimes(1));
    expect(api.addConnection).toHaveBeenCalledWith(expect.objectContaining({ credential_id: null, user: "u" }));
  });

  it("shows and disables a credential the connection references but the host does not have", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([{ ...conn, id: "c1", label: "web", credential_id: "gone" }]);
    vi.mocked(api.listCredentials).mockResolvedValue([]);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: "Edit connection" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit connection" });
    expect(within(dialog).getByRole("alert").textContent).toContain("gone");
    const option = within(dialog).getByRole("option", { name: /gone/i }) as HTMLOptionElement;
    expect(option.disabled).toBe(true);
    // The missing id stays selected, so saving cannot silently repoint the row.
    expect((within(dialog).getByLabelText("Credential") as HTMLSelectElement).value).toBe("gone");
  });

  it("clears a saved password before adopting a credential", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([{ ...conn, id: "c1", label: "web", has_password: true }]);
    vi.mocked(api.listCredentials).mockResolvedValueOnce([credential()]);
    vi.mocked(api.setConnectionPassword).mockClear().mockResolvedValue(undefined);
    vi.mocked(api.updateConnection).mockClear().mockResolvedValue(undefined);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: "Edit connection" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit connection" });
    const useCredential = within(dialog).getByLabelText("Use a credential") as HTMLInputElement;
    await waitFor(() => expect(useCredential.disabled).toBe(false));

    fireEvent.click(useCredential);
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

    await waitFor(() => expect(api.updateConnection).toHaveBeenCalledTimes(1));
    expect(api.setConnectionPassword).toHaveBeenCalledWith("c1", null);
    // The host refuses the switch while the secret is still there, so it goes first.
    expect(vi.mocked(api.setConnectionPassword).mock.invocationCallOrder[0])
      .toBeLessThan(vi.mocked(api.updateConnection).mock.invocationCallOrder[0]);
    expect(api.updateConnection).toHaveBeenCalledWith(expect.objectContaining({ credential_id: "cred1", has_password: false }));
  });

  it("refuses a keyless credential without deleting the saved password first", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([
      { ...conn, id: "c1", label: "web", has_password: true, disable_password_auth: true },
    ]);
    vi.mocked(api.listCredentials).mockResolvedValueOnce([credential({ key_id: null })]);
    vi.mocked(api.setConnectionPassword).mockClear().mockResolvedValue(undefined);
    vi.mocked(api.updateConnection).mockClear().mockResolvedValue(undefined);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: "Edit connection" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit connection" });
    const useCredential = within(dialog).getByLabelText("Use a credential") as HTMLInputElement;
    await waitFor(() => expect(useCredential.disabled).toBe(false));

    fireEvent.click(useCredential);
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

    expect((await within(dialog).findByRole("alert")).textContent).toMatch(/has no key/i);
    // The password and the row are untouched: the host would refuse this anyway.
    expect(api.setConnectionPassword).not.toHaveBeenCalled();
    expect(api.updateConnection).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(within(dialog).getByLabelText("Credential"));
  });

  it("says the saved password was removed when the save fails afterwards", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([{ ...conn, id: "c1", label: "web", has_password: true }]);
    vi.mocked(api.listCredentials).mockResolvedValueOnce([credential({ key_id: "k1" })]);
    vi.mocked(api.setConnectionPassword).mockClear().mockResolvedValue(undefined);
    vi.mocked(api.updateConnection).mockClear().mockRejectedValueOnce({ code: "internal", message: "boom" });
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: "Edit connection" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit connection" });
    const useCredential = within(dialog).getByLabelText("Use a credential") as HTMLInputElement;
    await waitFor(() => expect(useCredential.disabled).toBe(false));

    fireEvent.click(useCredential);
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

    await waitFor(() => expect(api.setConnectionPassword).toHaveBeenCalledWith("c1", null));
    expect(await within(dialog).findByText(/saved password for this connection was removed/i)).toBeTruthy();
    expect(within(dialog).getByText("boom")).toBeTruthy();
  });

  it("saves an imported credential-mode row with its own key as having no key", async () => {
    vi.mocked(api.listConnections).mockResolvedValue([
      { ...conn, id: "c1", label: "web", user: "", key_id: "k1", credential_id: "cred1" },
    ]);
    vi.mocked(api.listCredentials).mockResolvedValueOnce([credential({ key_id: "k1" })]);
    vi.mocked(api.updateConnection).mockClear().mockResolvedValue(undefined);
    render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: "Edit connection" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit connection" });
    expect(within(dialog).queryByLabelText("Key")).toBeNull();
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

    await waitFor(() => expect(api.updateConnection).toHaveBeenCalledTimes(1));
    expect(api.updateConnection).toHaveBeenCalledWith(expect.objectContaining({
      credential_id: "cred1", key_id: null, has_password: false,
    }));
  });
});
