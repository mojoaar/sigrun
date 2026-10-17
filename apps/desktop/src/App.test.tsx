import { render, screen } from "@testing-library/react";
import { vi } from "vitest";
import "./i18n";
import App from "./App";

vi.mock("./api", () => ({
  getStatus: vi.fn().mockResolvedValue({ vault: "Locked", agent_socket: null, host_addr: "x", version: "0.1.0" }),
  vaultUnlock: vi.fn(),
  vaultInit: vi.fn(),
  vaultLock: vi.fn(),
}));

test("renders the Sigrun heading and the unlock gate", async () => {
  render(<App />);
  expect(screen.getByRole("heading", { name: "Sigrun" })).toBeTruthy();
  expect(await screen.findByRole("button", { name: /unlock/i })).toBeTruthy();
});
