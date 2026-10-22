import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, vi } from "vitest";
import { listen } from "@tauri-apps/api/event";
import { act } from "@testing-library/react";
import i18n from "./i18n";
import App from "./App";
import { exportBundle, getSettings, getStatus, vaultLock } from "./api";

const { openMock, saveMock } = vi.hoisted(() => ({ openMock: vi.fn(), saveMock: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: openMock, save: saveMock }));

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));

vi.mock("./api", () => ({
  getStatus: vi.fn().mockResolvedValue({ vault: "Locked", agent_socket: null, host_addr: "x", version: "0.1.0" }),
  getSettings: vi.fn().mockResolvedValue({ theme: "ravn", language: "da", terminal: null, open_in: "window", start_minimized: false, auto_lock_minutes: 30, version: 1 }),
  listConnections: vi.fn().mockResolvedValue([]),
  listKeys: vi.fn().mockResolvedValue([]),
  listCredentials: vi.fn().mockResolvedValue([]),
  listTerminals: vi.fn().mockResolvedValue([]),
  exportBundle: vi.fn(),
  saveSettings: vi.fn().mockResolvedValue(undefined),
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
  openMock.mockReset();
  saveMock.mockReset();
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

test("places the Credentials tab right after Keys", async () => {
  vi.mocked(getStatus).mockResolvedValueOnce({ vault: "Unlocked", agent_socket: null, host_addr: "x", version: "0.1.0" });
  render(<App />);
  const nav = await screen.findByRole("navigation");
  // The suite renders Danish, so match either label.
  const keysTab = within(nav).getByRole("button", { name: /keys|nøgler/i });
  const credentialsTab = within(nav).getByRole("button", { name: /credentials|login/i });
  expect(keysTab.compareDocumentPosition(credentialsTab) & Node.DOCUMENT_POSITION_FOLLOWING)
    .toBe(Node.DOCUMENT_POSITION_FOLLOWING);
});

test("vault-locked event re-gates the UI with a notice", async () => {
  render(<App />);
  await screen.findByRole("button", { name: /lås op|unlock/i });
  expect(screen.queryByRole("status")).toBeNull();
  const handler = vi.mocked(listen).mock.calls.filter(([e]) => e === "vault-locked").at(-1)![1] as () => void;
  act(() => handler());
  expect(await screen.findByRole("status")).toBeTruthy();
});

const menuHandler = () =>
  vi.mocked(listen).mock.calls.filter(([e]) => e === "menu").at(-1)![1] as (e: { payload: string }) => void;

test("a menu action does nothing while the vault is locked", async () => {
  render(<App />);
  await screen.findByRole("button", { name: /lås op|unlock/i });
  // If either action were remembered, unlocking would land on Settings and run
  // the export picker; only the gate guard keeps that from happening.
  act(() => menuHandler()({ payload: "new-connection" }));
  act(() => menuHandler()({ payload: "export" }));

  vi.mocked(getStatus).mockResolvedValueOnce({ vault: "Unlocked", agent_socket: null, host_addr: "x", version: "0.1.0" });
  fireEvent.change(screen.getByLabelText(/passphrase|adgangssætning/i), { target: { value: "pw" } });
  fireEvent.click(screen.getByRole("button", { name: /lås op|unlock/i }));

  const nav = await screen.findByRole("navigation");
  expect(within(nav).getByRole("button", { name: /connections|forbindelser/i }).getAttribute("aria-current")).toBe("page");
  expect(screen.queryByRole("heading", { name: /settings|indstillinger/i })).toBeNull();
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(saveMock).not.toHaveBeenCalled();
  expect(openMock).not.toHaveBeenCalled();
});

test("an export menu action opens Settings and runs the picker once", async () => {
  vi.mocked(getStatus).mockResolvedValueOnce({ vault: "Unlocked", agent_socket: null, host_addr: "x", version: "0.1.0" });
  saveMock.mockResolvedValueOnce("/tmp/out.zip");
  vi.mocked(exportBundle).mockResolvedValueOnce({ path: "/tmp/out.zip", connections: 1, keys: 0 });
  render(<App />);
  await screen.findByRole("navigation");

  act(() => menuHandler()({ payload: "export" }));

  expect(await screen.findByRole("heading", { name: /settings|indstillinger/i })).toBeTruthy();
  await waitFor(() => expect(saveMock).toHaveBeenCalledTimes(1));
  expect(exportBundle).toHaveBeenCalledWith("/tmp/out.zip", true);
  expect(exportBundle).toHaveBeenCalledTimes(1);
});

test("the menu opens the add form when unlocked", async () => {
  vi.mocked(getStatus).mockResolvedValueOnce({ vault: "Unlocked", agent_socket: null, host_addr: "x", version: "0.1.0" });
  render(<App />);
  await screen.findByRole("navigation");
  act(() => menuHandler()({ payload: "new-connection" }));
  expect(await screen.findByRole("dialog", { name: /add connection|tilføj forbindelse/i })).toBeTruthy();
});

test("a tray connection error brings the connections tab forward with its trust dialog", async () => {
  vi.mocked(getStatus).mockResolvedValueOnce({ vault: "Unlocked", agent_socket: null, host_addr: "x", version: "0.1.0" });
  vi.mocked(getSettings).mockResolvedValueOnce({ theme: "ravn", language: "en", terminal: null, open_in: "window", start_minimized: false, auto_lock_minutes: 30, version: 1 });
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

test("locks the vault on Cmd+L", async () => {
  vi.mocked(getStatus).mockResolvedValueOnce({ vault: "Unlocked", agent_socket: null, host_addr: "x", version: "0.1.0" });
  vi.mocked(vaultLock).mockClear().mockResolvedValue(undefined);
  render(<App />);
  await screen.findByRole("button", { name: /^(lås|lock)$/i });

  fireEvent.keyDown(window, { key: "l", metaKey: true });

  await waitFor(() => expect(vaultLock).toHaveBeenCalledTimes(1));
});
