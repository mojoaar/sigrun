import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import "../i18n";
import Connections from "./Connections";

vi.mock("../api", () => ({
  listConnections: vi.fn().mockResolvedValue([
    { id: "1", label: "Prod", host: "prod.example.com", user: "deploy", port: 22, key_id: null, group: null, tags: [], source: "managed" },
  ]),
  listKeys: vi.fn().mockResolvedValue([]),
  addConnection: vi.fn(),
  updateConnection: vi.fn(),
  removeConnection: vi.fn(),
  connect: vi.fn().mockRejectedValue({ code: "vault_locked", message: "vault is locked" }),
}));

describe("Connections", () => {
  it("renders rows from listConnections", async () => {
    render(<Connections onVaultLocked={() => {}} />);
    expect(await screen.findByText("Prod")).toBeTruthy();
    expect(screen.getByText(/prod\.example\.com/)).toBeTruthy();
    expect(screen.getByText(/deploy/)).toBeTruthy();
  });

  it("calls onVaultLocked when connect is rejected with vault_locked", async () => {
    const onVaultLocked = vi.fn();
    render(<Connections onVaultLocked={onVaultLocked} />);
    fireEvent.click(await screen.findByRole("button", { name: "Connect" }));
    await waitFor(() => expect(onVaultLocked).toHaveBeenCalled());
  });
});
