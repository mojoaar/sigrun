import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import "../i18n";
import Settings from "./Settings";
import * as api from "../api";

vi.mock("../api", () => ({
  getSettings: vi.fn().mockResolvedValue({
    theme: "nordlys",
    language: "en",
    terminal: null,
    auto_lock_minutes: 30,
    start_minimized: false,
  }),
  listTerminals: vi.fn().mockResolvedValue([]),
  saveSettings: vi.fn().mockResolvedValue(undefined),
}));

describe("Settings", () => {
  it("lists start minimized after the other controls", async () => {
    const { container } = render(<Settings />);
    await screen.findByRole("checkbox");

    const labels = Array.from(container.querySelectorAll("label"));
    const order = labels.map((l) => l.textContent ?? "");
    expect(order).toHaveLength(6);
    expect(order[0].startsWith("Theme")).toBe(true);
    expect(order[1].startsWith("Appearance")).toBe(true);
    expect(order[2].startsWith("Language")).toBe(true);
    expect(order[3].startsWith("Terminal")).toBe(true);
    expect(order[4].startsWith("Auto-lock")).toBe(true);
    expect(order[5].startsWith("Start minimized")).toBe(true);
    // Start minimized must come after auto-lock in the DOM, not just in the list.
    expect(labels[4].compareDocumentPosition(labels[5]) & Node.DOCUMENT_POSITION_FOLLOWING)
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
});
