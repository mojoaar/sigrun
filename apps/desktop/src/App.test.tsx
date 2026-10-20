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
  setWindowTheme: vi.fn().mockResolvedValue(undefined),
}));

// jsdom reports "(darwin)" in its user agent, which is not what the app checks for the macOS title bar.
const MAC_UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)";
const LINUX_UA = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";
const setUserAgent = (ua: string) => Object.defineProperty(window.navigator, "userAgent", { value: ua, configurable: true });

beforeEach(() => {
  window.matchMedia = ((q: string) => ({ matches: true, media: q })) as unknown as typeof window.matchMedia;
});

afterEach(async () => {
  Reflect.deleteProperty(window.navigator, "userAgent");
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

test("marks only the active tab with aria-current", async () => {
  vi.mocked(getStatus).mockResolvedValueOnce({ vault: "Unlocked", agent_socket: null, host_addr: "x", version: "0.1.0" });
  render(<App />);
  const nav = await screen.findByRole("navigation");
  // App applies the persisted language, which this suite's settings mock sets
  // to Danish, so match either label.
  const connectionsTab = () => within(nav).getByRole("button", { name: /connections|forbindelser/i });
  const keysTab = () => within(nav).getByRole("button", { name: /keys|nøgler/i });
  expect(connectionsTab().getAttribute("aria-current")).toBe("page");
  expect(keysTab().getAttribute("aria-current")).toBeNull();
  fireEvent.click(keysTab());
  expect(keysTab().getAttribute("aria-current")).toBe("page");
  expect(connectionsTab().getAttribute("aria-current")).toBeNull();
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

it("gives the header a deep drag region and a macOS inset", () => {
  setUserAgent(MAC_UA);
  render(<App />);
  const header = document.querySelector("header") as HTMLElement;
  // "deep" also drags from the title text and other descendants; a bare
  // attribute only drags from the header element's own padding.
  expect(header.getAttribute("data-tauri-drag-region")).toBe("deep");
  expect(header.className).toContain("pl-20");
});

it("keeps the header inset off Windows and Linux", () => {
  setUserAgent(LINUX_UA);
  render(<App />);
  const header = document.querySelector("header") as HTMLElement;
  expect(header.className).toContain("p-3");
  expect(header.className).not.toContain("pl-20");
});
