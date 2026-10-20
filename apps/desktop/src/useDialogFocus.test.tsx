import { fireEvent, render, screen, within } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { useDialogFocus } from "./useDialogFocus";

function Harness({ onDidClose, name }: { onDidClose: () => void; name: string }) {
  const [open, setOpen] = useState(false);
  const close = () => { setOpen(false); onDidClose(); };
  return (
    <>
      <button onClick={() => setOpen(true)}>{`open ${name}`}</button>
      {open && <Dialog onClose={close} name={name} />}
    </>
  );
}

function Dialog({ onClose, name }: { onClose: () => void; name: string }) {
  const ref = useDialogFocus(true, onClose);
  return (
    <div role="dialog" aria-label={name} ref={ref}>
      <input aria-label="first" />
      <button onClick={onClose}>cancel</button>
    </div>
  );
}

// jsdom does not focus a button on click; a real click (or Enter on it) does.
function open(name: string) {
  const opener = screen.getByRole("button", { name });
  opener.focus();
  fireEvent.click(opener);
  return opener;
}

describe("useDialogFocus", () => {
  it("moves focus into the dialog and back to its opener", () => {
    const did = vi.fn();
    render(<Harness onDidClose={did} name="dlg" />);
    const opener = open("open dlg");

    expect(document.activeElement).toBe(screen.getByLabelText("first"));

    fireEvent.click(screen.getByRole("button", { name: "cancel" }));
    expect(did).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(opener);
  });

  it("closes on Escape and returns focus to the opener", () => {
    const did = vi.fn();
    render(<Harness onDidClose={did} name="dlg" />);
    const opener = open("open dlg");

    fireEvent.keyDown(document, { key: "Escape" });

    expect(did).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(opener);
  });

  it("closes only the topmost dialog per Escape", () => {
    const onFirst = vi.fn();
    const onSecond = vi.fn();
    function Twins() {
      const [second, setSecond] = useState(true);
      return (
        <>
          <Dialog name="first" onClose={onFirst} />
          {second && <Dialog name="second" onClose={() => { setSecond(false); onSecond(); }} />}
        </>
      );
    }
    render(<Twins />);
    // Both dialogs' inputs answer to "first", so scope the query to the second box.
    const dialogs = screen.getAllByRole("dialog");
    expect(document.activeElement).toBe(within(dialogs[1]).getByLabelText("first"));

    fireEvent.keyDown(document, { key: "Escape" });

    // The second dialog is gone, the first is still there.
    expect(onSecond).toHaveBeenCalledTimes(1);
    expect(onFirst).not.toHaveBeenCalled();
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    expect(screen.getByRole("dialog").getAttribute("aria-label")).toBe("first");

    fireEvent.keyDown(document, { key: "Escape" });
    expect(onFirst).toHaveBeenCalledTimes(1);
  });

  it("does not steal focus when a re-render brings a new onClose", () => {
    function TwoFields({ onClose }: { onClose: () => void }) {
      const ref = useDialogFocus(true, onClose);
      return (
        <div role="dialog" aria-label="typed" ref={ref}>
          <input aria-label="first" />
          <input aria-label="second" />
        </div>
      );
    }
    const { rerender } = render(<TwoFields onClose={() => {}} />);
    const second = screen.getByLabelText("second");
    second.focus();

    // A fresh inline closure, the shape a parent re-render produces on a keystroke.
    rerender(<TwoFields onClose={() => {}} />);
    fireEvent.change(second, { target: { value: "abc" } });

    expect(document.activeElement).toBe(second);
  });

  it("ignores Escape while the dialog is not open", () => {
    const onClose = vi.fn();
    function Toggle({ open: isOpen, onClose }: { open: boolean; onClose: () => void }) {
      const ref = useDialogFocus(isOpen, onClose);
      return (
        <div role="dialog" aria-label="toggle" ref={ref}>
          <button>cancel</button>
        </div>
      );
    }
    function ToggleHarness() {
      const [isOpen, setOpen] = useState(false);
      return (
        <>
          <button onClick={() => setOpen(true)}>open toggle</button>
          <Toggle open={isOpen} onClose={() => { setOpen(false); onClose(); }} />
        </>
      );
    }
    render(<ToggleHarness />);

    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();

    open("open toggle");
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);

    // The dialog stays mounted but closed: another Escape is ignored.
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
