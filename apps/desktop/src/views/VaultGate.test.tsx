import { fireEvent, render, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import "../i18n";
import VaultGate from "./VaultGate";
import { vaultUnlock } from "../api";

vi.mock("../api", () => ({ vaultUnlock: vi.fn(), vaultInit: vi.fn() }));

describe("VaultGate", () => {
  it("shows the unlock affordance when the vault is locked", async () => {
    render(<VaultGate />);
    expect(await screen.findByRole("button", { name: /unlock/i })).toBeTruthy();
  });

  it("displays failed attempts when unlock error contains payload with attempts > 1", async () => {
    vi.mocked(vaultUnlock).mockRejectedValueOnce({
      code: "invalid_passphrase",
      message: "invalid passphrase",
      payload: { attempts: 3, backoff_ms: 2000 },
    });

    render(<VaultGate />);
    const input = screen.getByLabelText(/passphrase/i);
    fireEvent.change(input, { target: { value: "wrong" } });
    fireEvent.click(screen.getByRole("button", { name: /unlock/i }));

    expect(await screen.findByText(/Failed unlock attempts: 3\./i)).toBeTruthy();
  });
});
