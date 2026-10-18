import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, vi } from "vitest";
import { listen } from "@tauri-apps/api/event";
import { act } from "@testing-library/react";
import i18n from "./i18n";
import App from "./App";
import { getSettings, getStatus } from "./api";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));

vi.mock("./api", () => ({
  getStatus: vi.fn().mockResolvedValue({ vault: "Locked", agent_socket: null, host_addr: "x", version: "0.1.0" }),
  getSettings: vi.fn().mockResolvedValue({ theme: "ravn", language: "da", terminal: null, agent_enabled: true, version: 1 }),
  listConnections: vi.fn().mockResolvedValue([]),
  listKeys: vi.fn().mockResolvedValue([]),
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

test("styles the lock button as a danger action", async () => {
  vi.mocked(getStatus).mockResolvedValueOnce({ vault: "Unlocked", agent_socket: null, host_addr: "x", version: "0.1.0" });
  render(<App />);
  const lock = await screen.findByRole("button", { name: /^(lås|lock)$/i });
  expect(lock.className).toContain("text-danger");
});

test("vault-locked event re-gates the UI with a notice", async () => {
  render(<App />);
  await screen.findByRole("button", { name: /lås op|unlock/i });
  expect(screen.queryByRole("status")).toBeNull();
  const handler = vi.mocked(listen).mock.calls.filter(([e]) => e === "vault-locked").at(-1)![1] as () => void;
  act(() => handler());
  expect(await screen.findByRole("status")).toBeTruthy();
});

test("a tray connection error brings the connections tab forward with its trust dialog", async () => {
  vi.mocked(getStatus).mockResolvedValueOnce({ vault: "Unlocked", agent_socket: null, host_addr: "x", version: "0.1.0" });
  vi.mocked(getSettings).mockResolvedValueOnce({ theme: "ravn", language: "en", terminal: null, open_in: "window", agent_enabled: true, start_minimized: false, auto_lock_minutes: 30, version: 1 });
  render(<App />);
  // A tray click can find the window parked on another tab.
  fireEvent.click(await screen.findByRole("button", { name: "Keys" }));
  await screen.findByRole("heading", { name: "Keys" });

  const handler = vi.mocked(listen).mock.calls.filter(([e]) => e === "connection-error").at(-1)![1] as (e: { payload: unknown }) => void;
  await act(async () => { handler({ payload: { id: "c1", code: "host_key_unknown", message: "SHA256:abc ssh-ed25519 AAAA", host: "h", port: 22 } }); });

  expect(await screen.findByRole("heading", { name: "Trust this host?" })).toBeTruthy();
  const dialog = screen.getByRole("dialog", { name: "Trust this host?" });
  expect(within(dialog).getByText("SHA256:abc")).toBeTruthy();
  expect(within(dialog).getByRole("button", { name: "Trust and connect" })).toBeTruthy();
});
