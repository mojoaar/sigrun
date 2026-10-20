import { useEffect, useRef, type RefObject } from "react";

const FOCUSABLE = 'input:not([disabled]), select:not([disabled]), textarea:not([disabled]), button:not([disabled])';

// Mount order, so Escape acts on the top dialog only: the trust prompt renders
// above the provision panel and one Escape must not close both.
const stack: object[] = [];

export function useDialogFocus(open: boolean, onClose: () => void): RefObject<HTMLDivElement | null> {
  const ref = useRef<HTMLDivElement>(null);
  // Kept in a ref so the effect below runs only when `open` flips: an inline
  // onClose would otherwise re-run it on every keystroke and steal focus.
  const close = useRef(onClose);
  close.current = onClose;

  useEffect(() => {
    if (!open) return;
    const opener = document.activeElement as HTMLElement | null;
    const token = {};
    stack.push(token);
    (ref.current?.querySelector<HTMLElement>(FOCUSABLE) ?? ref.current)?.focus();

    const onKey = (e: KeyboardEvent) => {
      // A held key auto-repeats; one press must not walk down the stack.
      if (e.repeat || e.key !== "Escape" || stack[stack.length - 1] !== token) return;
      e.preventDefault();
      close.current();
    };
    document.addEventListener("keydown", onKey);

    return () => {
      document.removeEventListener("keydown", onKey);
      const at = stack.indexOf(token);
      if (at !== -1) stack.splice(at, 1);
      // A closed dialog's opener may have unmounted too; focus() is then a no-op.
      opener?.focus();
    };
  }, [open]);

  return ref;
}
