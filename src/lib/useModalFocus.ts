import { useLayoutEffect, useRef } from "react";

const FOCUSABLE = "button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), a[href], summary, [tabindex]:not([tabindex='-1'])";

/** Focus belongs to the sheet; closing it restores the trigger, never cancels work. */
export function useModalFocus(onClose: () => void) {
  const ref = useRef<HTMLDivElement>(null);
  const close = useRef(onClose);
  close.current = onClose;

  useLayoutEffect(() => {
    const modal = ref.current;
    if (!modal) return;
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const focusables = () => Array.from(modal.querySelectorAll<HTMLElement>(FOCUSABLE))
      .sort((a, b) => a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1)
      .filter((element) => !element.closest("[hidden]") && getComputedStyle(element).display !== "none");
    const first = () => (focusables()[0] ?? modal).focus();
    const background: [HTMLElement, boolean][] = [];
    let branch: HTMLElement | null = modal.closest(".overlay");
    while (branch && branch !== document.body) {
      const parent: HTMLElement | null = branch.parentElement;
      if (!parent) break;
      for (const sibling of parent.children) {
        if (sibling instanceof HTMLElement && sibling !== branch) {
          background.push([sibling, sibling.inert]);
          sibling.inert = true;
        }
      }
      branch = parent;
    }
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        close.current();
      } else if (event.key === "Tab") {
        const elements = focusables();
        const firstElement = elements[0] ?? modal;
        const lastElement = elements[elements.length - 1] ?? modal;
        if (event.shiftKey && (document.activeElement === firstElement || !modal.contains(document.activeElement))) {
          event.preventDefault(); lastElement.focus();
        } else if (!event.shiftKey && (document.activeElement === lastElement || !modal.contains(document.activeElement))) {
          event.preventDefault(); firstElement.focus();
        }
      }
    };
    const containFocus = () => { if (!modal.contains(document.activeElement)) first(); };
    document.addEventListener("keydown", keydown, true);
    document.addEventListener("focusin", containFocus);
    first();
    return () => {
      document.removeEventListener("keydown", keydown, true);
      document.removeEventListener("focusin", containFocus);
      background.forEach(([element, inert]) => { element.inert = inert; });
      if (previous?.isConnected && !previous.inert) previous.focus();
    };
  }, []);

  useLayoutEffect(() => {
    // The focused Install/Cancel button can disappear when a snapshot terminalizes.
    const modal = ref.current;
    if (modal && (!modal.contains(document.activeElement) || document.activeElement?.matches(":disabled"))) {
      (modal.querySelector<HTMLElement>(FOCUSABLE) ?? modal).focus();
    }
  });
  return ref;
}
