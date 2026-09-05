import { useEffect, useRef, type RefObject } from "react";

// tabindex="-1" is excluded everywhere so a roving-tabindex radio group (the
// language grid) counts as ONE Tab stop — otherwise the trap's first/last
// cycling would disagree with what the browser actually tabs to.
const FOCUSABLE =
  'button:not(:disabled):not([tabindex="-1"]), [href]:not([tabindex="-1"]), input:not(:disabled):not([tabindex="-1"]), select:not(:disabled):not([tabindex="-1"]), textarea:not(:disabled):not([tabindex="-1"]), [tabindex]:not([tabindex="-1"])';

export function getFocusable(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter((el) => {
    if (el.hasAttribute("hidden") || el.getAttribute("aria-hidden") === "true") {
      return false;
    }
    const style = window.getComputedStyle?.(el);
    return (
      el.offsetParent !== null ||
      el === document.activeElement ||
      !style ||
      (style.display !== "none" && style.visibility !== "hidden")
    );
  });
}

export function cycleFocusTarget(
  items: HTMLElement[],
  active: Element | null,
  shift: boolean,
): HTMLElement | null {
  if (items.length === 0) {
    return null;
  }
  const first = items[0];
  const last = items[items.length - 1];
  if (shift && active === first) {
    return last;
  }
  if (!shift && active === last) {
    return first;
  }
  return null;
}

export function useFocusTrap(
  ref: RefObject<HTMLElement | null>,
  opts: {
    onEsc?: () => void;
    initialFocus?: "first" | "primary" | "dismiss" | "container";
    /** When false the trap is inert: it grabs no focus and, on the transition
     *  to false, restores focus to the opener. Lets a dialog keep playing an
     *  exit animation (still mounted) without holding focus. Defaults to true. */
    active?: boolean;
  },
) {
  // Destructure so the effect depends on the specific option values (stable
  // primitives / callbacks) rather than the freshly-spread `opts` object, which
  // would re-arm the trap every render.
  const { onEsc, initialFocus } = opts;
  const onEscRef = useRef(onEsc);
  useEffect(() => { onEscRef.current = onEsc; }, [onEsc]);
  const active = opts.active ?? true;
  useEffect(() => {
    const node = ref.current;
    if (!node || !active) {
      return;
    }
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;

    const focusables = getFocusable(node);
    const pick = () => {
      if (initialFocus === "dismiss") {
        return node.querySelector<HTMLElement>(".btn.ghost") ?? focusables[0];
      }
      if (initialFocus === "primary") {
        return node.querySelector<HTMLElement>(".btn.primary, .btn.danger") ?? focusables[0];
      }
      if (initialFocus === "container") {
        node.tabIndex = -1;
        return node;
      }
      return (
        node.querySelector<HTMLElement>('[role="radio"][aria-checked="true"]') ?? focusables[0]
      );
    };
    pick()?.focus();

    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onEscRef.current?.();
        return;
      }
      if (event.key !== "Tab") {
        return;
      }
      const target = cycleFocusTarget(getFocusable(node), document.activeElement, event.shiftKey);
      if (target) {
        event.preventDefault();
        target.focus();
      }
    };

    node.addEventListener("keydown", onKey);
    return () => {
      node.removeEventListener("keydown", onKey);
      if (opener && document.contains(opener)) {
        opener.focus();
      }
    };
  }, [ref, initialFocus, active]);
}
