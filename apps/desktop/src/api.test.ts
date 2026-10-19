import { beforeEach, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { exportBundle, importBundle } from "./api";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => vi.mocked(invoke).mockReset());

test("exportBundle asks the host to write the bundle at the picked path", async () => {
  const summary = { path: "/tmp/backup", connections: 2, keys: 1 };
  vi.mocked(invoke).mockResolvedValue({ type: "export", value: summary });
  expect(await exportBundle("/tmp/backup", false)).toEqual(summary);
  expect(invoke).toHaveBeenCalledWith("export_bundle", { path: "/tmp/backup", zipped: false });
});

test("importBundle asks the host to merge the bundle at the picked path", async () => {
  const summary = { added: 3, skipped: 1, invalid: 0, unresolved_keys: 2 };
  vi.mocked(invoke).mockResolvedValue({ type: "import", value: summary });
  expect(await importBundle("/tmp/backup.zip")).toEqual(summary);
  expect(invoke).toHaveBeenCalledWith("import_bundle", { path: "/tmp/backup.zip" });
});
