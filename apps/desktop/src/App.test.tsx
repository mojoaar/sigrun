import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, vi } from "vitest";
import i18n from "./i18n";
import App from "./App";

vi.mock("./api", () => ({
  getStatus: vi.fn().mockResolvedValue({ vault: "Locked", agent_socket: null, host_addr: "x", version: "0.1.0" }),
  getSettings: vi.fn().mockResolvedValue({ theme: "ravn", language: "da", terminal: null, agent_enabled: true, version: 1 }),
  vaultUnlock: vi.fn(),
  vaultInit: vi.fn(),
  vaultLock: vi.fn(),
}));

beforeEach(() => {
  window.matchMedia = ((q: string) => ({ matches: true, media: q })) as unknown as typeof window.matchMedia;
});

afterEach(async () => {
  await i18n.changeLanguage("en");
});

test("applies persisted theme and language before showing the gate", async () => {
  render(<App />);
  expect(await screen.findByRole("button", { name: /lås op|unlock/i })).toBeTruthy();
  expect(screen.getByRole("heading", { name: "Sigrun" })).toBeTruthy();
  expect(document.documentElement.getAttribute("data-theme")).toBe("ravn");
  expect(i18n.language).toBe("da");
});
