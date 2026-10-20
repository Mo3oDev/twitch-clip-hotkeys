export interface HotkeyBinding {
  id: string;
  modifiers: string[];
  code: string;
  duration_seconds: number;
  label: string;
}

// DOM KeyboardEvent.code values are, by design, the same strings Tauri's
// `Code` enum (keyboard-types) parses — no translation table needed, a
// captured `event.code` is already a valid backend `code`.
const MODIFIER_CODES = new Set([
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "ShiftLeft",
  "ShiftRight",
  "MetaLeft",
  "MetaRight",
]);

const MODIFIER_DISPLAY: Record<string, string> = {
  Control: "Ctrl",
  Alt: "Alt",
  Shift: "Shift",
  Super: "Win",
};

function displayCode(code: string): string {
  if (code.startsWith("Digit")) return code.slice("Digit".length);
  if (code.startsWith("Key")) return code.slice("Key".length);
  return code;
}

function displayCombo(modifiers: string[], code: string): string {
  const parts = modifiers.map((m) => MODIFIER_DISPLAY[m] ?? m);
  parts.push(displayCode(code));
  return parts.join("+");
}

/** Mirrors the backend's `validate_bindings` (Rust is still the source of
 * truth — this is a UX nicety so the user sees the problem before saving
 * instead of only after the command round-trips and rejects it). */
export function validateClientSide(bindings: HotkeyBinding[]): string | null {
  for (const b of bindings) {
    if (b.duration_seconds < 5 || b.duration_seconds > 60) {
      return `"${b.label}": duration must be between 5 and 60 seconds.`;
    }
    if (b.modifiers.length === 0) {
      return `"${b.label}": needs at least one modifier (Ctrl/Alt/Shift).`;
    }
  }
  const seen = new Map<string, string>();
  for (const b of bindings) {
    const key = [...b.modifiers].sort().join("+") + "|" + b.code;
    if (seen.has(key)) {
      return `"${seen.get(key)}" and "${b.label}" use the same combo.`;
    }
    seen.set(key, b.label);
  }
  return null;
}

function newBindingId(): string {
  return `hk-${Date.now()}-${Math.floor(Math.random() * 10000)}`;
}

export function defaultBinding(): HotkeyBinding {
  return {
    id: newBindingId(),
    modifiers: ["Control", "Alt"],
    code: "Digit1",
    duration_seconds: 30,
    label: "Clip 30s",
  };
}

// A capture listens on `window` (so it fires regardless of which element
// has focus) until a non-modifier key resolves the combo or Escape cancels
// it. Only one can be active at a time; starting a new one, or any
// structural re-render, must cancel a stale listener rather than leak it.
let activeCaptureCleanup: (() => void) | null = null;

function beginComboCapture(
  button: HTMLButtonElement,
  onCaptured: (modifiers: string[], code: string) => void,
  onEnd: () => void,
): void {
  activeCaptureCleanup?.();

  const onKeyDown = (event: KeyboardEvent) => {
    event.preventDefault();
    event.stopPropagation();

    const noModifierHeld = !event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey;
    if (event.code === "Escape" && noModifierHeld) {
      cancel();
      return;
    }
    // Still just holding a modifier down — wait for the actual key.
    if (MODIFIER_CODES.has(event.code)) return;

    const modifiers: string[] = [];
    if (event.ctrlKey) modifiers.push("Control");
    if (event.altKey) modifiers.push("Alt");
    if (event.shiftKey) modifiers.push("Shift");
    if (event.metaKey) modifiers.push("Super");

    cleanup();
    onCaptured(modifiers, event.code);
  };

  const cleanup = () => {
    window.removeEventListener("keydown", onKeyDown, true);
    button.classList.remove("capturing");
    activeCaptureCleanup = null;
    onEnd();
  };

  const cancel = () => {
    cleanup();
    button.textContent = button.dataset.combo ?? "";
  };

  window.addEventListener("keydown", onKeyDown, true);
  button.classList.add("capturing");
  button.textContent = "Press a key combo… (Esc to cancel)";
  activeCaptureCleanup = cancel;
}

/**
 * Renders an editable list that mutates `bindings` **in place** as the user
 * types, without re-rendering on every keystroke. An earlier version called
 * back into a full re-render (and an immediate `save_hotkeys` + OS
 * re-registration) on every `input` event — that destroyed the focused
 * `<input>` each keystroke and persisted half-typed bindings. Field edits
 * now only mutate the array; `onStructuralChange` fires only for add/remove,
 * which do need a re-render. The caller is responsible for calling
 * `save_hotkeys` explicitly (a "Save" button), not on every edit.
 *
 * `onCapturingChange` fires true/false around a combo capture so the
 * caller can disable "+ Add hotkey" / "Save hotkeys" while it's active —
 * otherwise those stayed clickable mid-capture, letting the user add more
 * rows (or save) while one combo was still pending.
 */
export function renderHotkeyList(
  container: HTMLElement,
  bindings: HotkeyBinding[],
  onStructuralChange: (bindings: HotkeyBinding[]) => void,
  onCapturingChange: (capturing: boolean) => void,
): void {
  activeCaptureCleanup?.();
  onCapturingChange(false);
  container.textContent = "";

  // Every remove/combo button across all rows — disabled as a group while
  // any one row is capturing, so editing elsewhere can't race a pending
  // combo capture.
  const allRowButtons: HTMLButtonElement[] = [];

  // Two stacked lines per binding, not one 5-wide inline row — at the
  // settings window's fixed (non-resizable) width, five inline inputs
  // squeezed every field down to roughly the width of its placeholder
  // text, unreadable even for short values.
  bindings.forEach((binding, index) => {
    const row = document.createElement("div");
    row.className = "hotkey-row";

    const topLine = document.createElement("div");
    topLine.className = "hotkey-row-line";

    const labelInput = document.createElement("input");
    labelInput.value = binding.label;
    labelInput.placeholder = "Label";
    labelInput.addEventListener("input", () => {
      binding.label = labelInput.value;
    });

    const durationInput = document.createElement("input");
    durationInput.type = "number";
    durationInput.min = "5";
    durationInput.max = "60";
    durationInput.value = String(binding.duration_seconds);
    durationInput.title = "Duration in seconds (5-60)";
    durationInput.addEventListener("input", () => {
      binding.duration_seconds = Number(durationInput.value) || 0;
    });

    const durationSuffix = document.createElement("span");
    durationSuffix.className = "hotkey-duration-suffix";
    durationSuffix.textContent = "s";

    const removeBtn = document.createElement("button");
    removeBtn.textContent = "Remove";
    removeBtn.addEventListener("click", () => {
      onStructuralChange(bindings.filter((_, i) => i !== index));
    });
    allRowButtons.push(removeBtn);

    topLine.append(labelInput, durationInput, durationSuffix, removeBtn);

    const bottomLine = document.createElement("div");
    bottomLine.className = "hotkey-row-line";

    const comboBtn = document.createElement("button");
    comboBtn.type = "button";
    comboBtn.className = "hotkey-combo-btn";
    comboBtn.title = "Click, then press the key combo you want";
    const refreshComboLabel = () => {
      const label = displayCombo(binding.modifiers, binding.code);
      comboBtn.textContent = label;
      comboBtn.dataset.combo = label;
    };
    refreshComboLabel();
    allRowButtons.push(comboBtn);
    comboBtn.addEventListener("click", () => {
      if (comboBtn.classList.contains("capturing")) {
        activeCaptureCleanup?.();
        return;
      }
      const others = allRowButtons.filter((b) => b !== comboBtn);
      others.forEach((b) => (b.disabled = true));
      onCapturingChange(true);
      beginComboCapture(
        comboBtn,
        (modifiers, code) => {
          binding.modifiers = modifiers;
          binding.code = code;
          refreshComboLabel();
        },
        () => {
          others.forEach((b) => (b.disabled = false));
          onCapturingChange(false);
        },
      );
    });

    bottomLine.append(comboBtn);

    row.append(topLine, bottomLine);
    container.appendChild(row);
  });
}
