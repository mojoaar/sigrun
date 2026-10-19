import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import "../i18n";
import Help from "./Help";
import * as api from "../api";

vi.mock("../api", () => ({
  getStatus: vi.fn().mockResolvedValue({ version: "0.1.0", vault: "Unlocked", agent_socket: null, host_addr: "x" }),
  openExternal: vi.fn().mockResolvedValue(undefined),
}));

describe("Help", () => {
  it("shows the version, the links, and opens them through the host", async () => {
    render(<Help />);
    expect(await screen.findByText(/0\.1\.0/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /johansen\.foo/ }));
    expect(api.openExternal).toHaveBeenCalledWith("https://johansen.foo");
  });

  it("states the approved tagline in the About section", () => {
    render(<Help />);
    expect(
      screen.getByText(
        "SSH connections and keys in a local encrypted vault, served to ssh through a built-in agent.",
      ),
    ).toBeTruthy();
  });
});
