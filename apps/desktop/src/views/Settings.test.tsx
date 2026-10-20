import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import "../i18n";
import Settings from "./Settings";
import * as api from "../api";

const { openMock, saveMock } = vi.hoisted(() => ({ openMock: vi.fn(), saveMock: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: openMock, save: saveMock }));

const settings = vi.hoisted((): api.Settings => ({
  version: 1,
  theme: "nordlys",
  language: "en",
  terminal: null,
  open_in: "window",
  agent_enabled: true,
  auto_lock_minutes: 30,
  start_minimized: false,
}));

vi.mock("../api", () => ({
  getSettings: vi.fn().mockResolvedValue(settings),
  listTerminals: vi.fn().mockResolvedValue([
    { id: "terminal", name: "Terminal", tabs: false },
    { id: "ghostty", name: "Ghostty", tabs: true },
  ]),
  saveSettings: vi.fn().mockResolvedValue(undefined),
  setWindowTheme: vi.fn().mockResolvedValue(undefined),
  exportBundle: vi.fn(),
  importBundle: vi.fn(),
}));

describe("Settings", () => {
  it("lists start minimized after the other controls", async () => {
    const { container } = render(<Settings />);
    await screen.findByRole("checkbox");

    const labels = Array.from(container.querySelectorAll("label"));
    const order = labels.map((l) => l.textContent ?? "");
    expect(order).toHaveLength(7);
    expect(order[0].startsWith("Theme")).toBe(true);
    expect(order[1].startsWith("Appearance")).toBe(true);
    expect(order[2].startsWith("Language")).toBe(true);
    expect(order[3].startsWith("Terminal")).toBe(true);
    expect(order[4].startsWith("Open in")).toBe(true);
    expect(order[5].startsWith("Auto-lock")).toBe(true);
    expect(order[6].startsWith("Start minimized")).toBe(true);
    // Start minimized must come after auto-lock in the DOM, not just in the list.
    expect(labels[5].compareDocumentPosition(labels[6]) & Node.DOCUMENT_POSITION_FOLLOWING)
      .toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  });

  it("saves start_minimized when the checkbox is ticked", async () => {
    vi.mocked(api.saveSettings).mockClear();
    render(<Settings />);
    fireEvent.click(await screen.findByRole("checkbox"));
    await waitFor(() =>
      expect(api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ start_minimized: true })),
    );
  });

  it("saves open_in as tab when a new tab is chosen", async () => {
    vi.mocked(api.saveSettings).mockClear();
    render(<Settings />);
    fireEvent.change(await screen.findByLabelText("Terminal"), { target: { value: "ghostty" } });
    fireEvent.change(screen.getByLabelText("Open in"), { target: { value: "tab" } });
    await waitFor(() =>
      expect(api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ open_in: "tab" })),
    );
  });

  it("only offers New tab for a terminal that can open one", async () => {
    render(<Settings />);
    const tab = (await screen.findByRole("option", { name: "New tab" })) as HTMLOptionElement;
    expect(tab.disabled).toBe(true);                       // terminal is null: System default
    fireEvent.change(screen.getByLabelText("Terminal"), { target: { value: "ghostty" } });
    expect((screen.getByRole("option", { name: "New tab" }) as HTMLOptionElement).disabled).toBe(false);
    fireEvent.change(screen.getByLabelText("Terminal"), { target: { value: "terminal" } });
    expect((screen.getByRole("option", { name: "New tab" }) as HTMLOptionElement).disabled).toBe(true);
  });

  it("does not keep New tab selected for a terminal that cannot open one", async () => {
    vi.mocked(api.saveSettings).mockClear();
    vi.mocked(api.getSettings).mockResolvedValueOnce({ ...settings, terminal: "terminal", open_in: "tab" });
    render(<Settings />);
    await screen.findByRole("option", { name: "New tab" });
    expect((screen.getByLabelText("Open in") as HTMLSelectElement).value).toBe("window");
    expect(api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ open_in: "window" }));
  });

  it("repaints the native window when the appearance changes", async () => {
    vi.mocked(api.setWindowTheme).mockClear();
    render(<Settings />);
    fireEvent.change(await screen.findByLabelText("Appearance"), { target: { value: "light" } });
    await waitFor(() => expect(api.setWindowTheme).toHaveBeenCalledWith("light"));
  });
});

describe("Settings import and export", () => {
  beforeEach(() => {
    openMock.mockReset();
    saveMock.mockReset();
    vi.mocked(api.exportBundle).mockReset();
    vi.mocked(api.importBundle).mockReset();
  });

  it("exports to the folder the picker returned", async () => {
    openMock.mockResolvedValue("/tmp/backup");
    vi.mocked(api.exportBundle).mockResolvedValue({ path: "/tmp/backup", connections: 2, keys: 1 });
    render(<Settings />);
    fireEvent.click(await screen.findByRole("button", { name: "Export to a folder…" }));
    // A folder export must ask for a directory: anything else hands the host a file path.
    await waitFor(() => expect(openMock).toHaveBeenCalledWith(expect.objectContaining({ directory: true })));
    expect(saveMock).not.toHaveBeenCalled();
    await waitFor(() => expect(api.exportBundle).toHaveBeenCalledWith("/tmp/backup", false));
    expect(screen.getByRole("status").textContent).toContain("2 connections");
  });

  it("exports to a zip when that button is used", async () => {
    saveMock.mockResolvedValue("/tmp/backup.zip");
    vi.mocked(api.exportBundle).mockResolvedValue({ path: "/tmp/backup.zip", connections: 2, keys: 1 });
    render(<Settings />);
    fireEvent.click(await screen.findByRole("button", { name: "Export to a .zip file…" }));
    // A zip export must go through save (a new file), filtered to .zip.
    await waitFor(() =>
      expect(saveMock).toHaveBeenCalledWith(
        expect.objectContaining({ filters: [expect.objectContaining({ extensions: ["zip"] })] }),
      ),
    );
    expect(openMock).not.toHaveBeenCalled();
    await waitFor(() => expect(api.exportBundle).toHaveBeenCalledWith("/tmp/backup.zip", true));
  });

  it("reports what an import did", async () => {
    openMock.mockResolvedValue("/tmp/backup");
    vi.mocked(api.importBundle).mockResolvedValue({ added: 3, skipped: 1, invalid: 0, unresolved_keys: 2 });
    render(<Settings />);
    fireEvent.click(await screen.findByRole("button", { name: "Import from a folder…" }));
    // A folder import must ask for a directory too.
    await waitFor(() => expect(openMock).toHaveBeenCalledWith(expect.objectContaining({ directory: true })));
    expect(saveMock).not.toHaveBeenCalled();
    const status = await screen.findByRole("status");
    await waitFor(() => expect(status.textContent).toContain("3 added, 1 skipped"));
    expect(status.textContent).toContain("2 unresolved keys");
  });

  it("imports a zip the picker returned", async () => {
    openMock.mockResolvedValue("/tmp/backup.zip");
    vi.mocked(api.importBundle).mockResolvedValue({ added: 4, skipped: 0, invalid: 1, unresolved_keys: 0 });
    render(<Settings />);
    fireEvent.click(await screen.findByRole("button", { name: "Import from a .zip file…" }));
    // A zip import must open a .zip-filtered file, not a directory.
    await waitFor(() =>
      expect(openMock).toHaveBeenCalledWith(
        expect.objectContaining({ filters: [expect.objectContaining({ extensions: ["zip"] })] }),
      ),
    );
    expect(openMock).not.toHaveBeenCalledWith(expect.objectContaining({ directory: true }));
    expect(saveMock).not.toHaveBeenCalled();
    await waitFor(() => expect(api.importBundle).toHaveBeenCalledWith("/tmp/backup.zip"));
    const status = await screen.findByRole("status");
    await waitFor(() => expect(status.textContent).toContain("4 added, 0 skipped"));
    expect(status.textContent).toContain("1 invalid");
  });

  it("says nothing when the picker is cancelled", async () => {
    openMock.mockResolvedValue(null);
    render(<Settings />);
    fireEvent.click(await screen.findByRole("button", { name: "Import from a .zip file…" }));
    await waitFor(() =>
      expect(openMock).toHaveBeenCalledWith(
        expect.objectContaining({ filters: [expect.objectContaining({ extensions: ["zip"] })] }),
      ),
    );
    expect(api.importBundle).not.toHaveBeenCalled();
    expect(screen.getByRole("status").textContent).toBe("");
  });

  it("runs the export from a menu request", async () => {
    saveMock.mockResolvedValueOnce("/tmp/out.zip");
    vi.mocked(api.exportBundle).mockResolvedValueOnce({ path: "/tmp/out.zip", connections: 2, keys: 1 });
    const done = vi.fn();
    render(<Settings transferRequest="export" onTransferDone={done} />);
    await waitFor(() => expect(saveMock).toHaveBeenCalledWith(expect.objectContaining({ defaultPath: "sigrun-export.zip" })));
    expect(api.exportBundle).toHaveBeenCalledWith("/tmp/out.zip", true);
    expect(done).toHaveBeenCalled();
  });

  it("runs the import from a menu request", async () => {
    openMock.mockResolvedValueOnce("/tmp/in.zip");
    vi.mocked(api.importBundle).mockResolvedValueOnce({ added: 1, skipped: 0, invalid: 0, unresolved_keys: 0 });
    const done = vi.fn();
    render(<Settings transferRequest="import" onTransferDone={done} />);
    await waitFor(() =>
      expect(openMock).toHaveBeenCalledWith(
        expect.objectContaining({ filters: [expect.objectContaining({ extensions: ["zip"] })] }),
      ),
    );
    expect(api.importBundle).toHaveBeenCalledWith("/tmp/in.zip");
    expect(done).toHaveBeenCalled();
  });

  it("shows an error when the command refuses", async () => {
    openMock.mockResolvedValue("/root/.ssh");
    vi.mocked(api.exportBundle).mockRejectedValue({ code: "invalid_input", message: "cannot export inside the SSH directory" });
    render(<Settings />);
    fireEvent.click(await screen.findByRole("button", { name: "Export to a folder…" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("cannot export inside the SSH directory");
    expect(screen.getByRole("status").textContent).toBe("");
  });
});
