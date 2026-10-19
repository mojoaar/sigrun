import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import "../i18n";
import Settings from "./Settings";
import * as api from "../api";

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
});
