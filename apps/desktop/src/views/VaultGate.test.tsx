import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import "../i18n";
import VaultGate from "./VaultGate";

vi.mock("../api", () => ({ vaultUnlock: vi.fn() }));

describe("VaultGate", () => {
  it("shows the unlock affordance when the vault is locked", async () => {
    render(<VaultGate />);
    expect(await screen.findByRole("button", { name: /unlock/i })).toBeTruthy();
  });
});
