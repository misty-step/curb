import { Moon, Sun } from "lucide-react";
import type { ReactNode } from "react";

// The aesthetic mode toggle, ported from the system's recipes/mode.js: pin
// the opposite of the effective scheme, persist it, and let the change be one
// soft view-transition breath (instant under reduced motion). The boot script
// in index.html applies the persisted choice before first paint.
export function ModeToggle(): ReactNode {
  return (
    <button type="button" className="ae-mode" aria-label="toggle color mode" onClick={toggleMode}>
      <Sun className="ae-icon ae-sun" />
      <Moon className="ae-icon ae-moon" />
    </button>
  );
}

function toggleMode(): void {
  const root = document.documentElement;
  const dark = root.classList.contains("dark")
    ? true
    : root.classList.contains("light")
      ? false
      : window.matchMedia("(prefers-color-scheme: dark)").matches;
  const flip = () => {
    root.classList.toggle("dark", !dark);
    root.classList.toggle("light", dark);
    root.style.colorScheme = dark ? "light" : "dark";
    try {
      localStorage.setItem("ae-mode", dark ? "light" : "dark");
    } catch {
      // Private mode: the choice simply does not persist.
    }
  };
  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
    flip();
  } else if (document.startViewTransition) {
    root.classList.add("ae-vt-mode");
    document
      .startViewTransition(flip)
      .finished.finally(() => root.classList.remove("ae-vt-mode"));
  } else {
    root.classList.add("ae-mode-easing");
    flip();
    window.setTimeout(() => root.classList.remove("ae-mode-easing"), 520);
  }
}
