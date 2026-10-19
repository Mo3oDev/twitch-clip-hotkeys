export interface HotkeyBinding {
  id: string;
  modifiers: string[];
  code: string;
  duration_seconds: number;
  label: string;
}

const KNOWN_MODIFIERS = ["Control", "Alt", "Shift", "Super"];

/** Mirrors the backend's `validate_bindings` (Rust is still the source of
 * truth — this is a UX nicety so the user sees the problem before saving
 * instead of only after the command round-trips and rejects it). */
export function validateClientSide(bindings: HotkeyBinding[]): string | null {
  for (const b of bindings) {
    if (b.duration_seconds < 5 || b.duration_seconds > 60) {
      return `"${b.label}": la duración debe estar entre 5 y 60 segundos.`;
    }
    if (b.modifiers.length === 0) {
      return `"${b.label}": necesita al menos un modificador (Ctrl/Alt/Shift).`;
    }
  }
  const seen = new Map<string, string>();
  for (const b of bindings) {
    const key = [...b.modifiers].sort().join("+") + "|" + b.code;
    if (seen.has(key)) {
      return `"${seen.get(key)}" y "${b.label}" usan la misma combinación.`;
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

/**
 * Renders an editable list that mutates `bindings` **in place** as the user
 * types, without re-rendering on every keystroke. An earlier version called
 * back into a full re-render (and an immediate `save_hotkeys` + OS
 * re-registration) on every `input` event — that destroyed the focused
 * `<input>` each keystroke and persisted half-typed bindings. Field edits
 * now only mutate the array; `onStructuralChange` fires only for add/remove,
 * which do need a re-render. The caller is responsible for calling
 * `save_hotkeys` explicitly (a "Guardar" button), not on every edit.
 */
export function renderHotkeyList(
  container: HTMLElement,
  bindings: HotkeyBinding[],
  onStructuralChange: (bindings: HotkeyBinding[]) => void,
): void {
  container.textContent = "";

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
    labelInput.placeholder = "Etiqueta";
    labelInput.addEventListener("input", () => {
      binding.label = labelInput.value;
    });

    const durationInput = document.createElement("input");
    durationInput.type = "number";
    durationInput.min = "5";
    durationInput.max = "60";
    durationInput.value = String(binding.duration_seconds);
    durationInput.title = "Duración en segundos (5-60)";
    durationInput.addEventListener("input", () => {
      binding.duration_seconds = Number(durationInput.value) || 0;
    });

    const durationSuffix = document.createElement("span");
    durationSuffix.className = "hotkey-duration-suffix";
    durationSuffix.textContent = "s";

    const removeBtn = document.createElement("button");
    removeBtn.textContent = "Eliminar";
    removeBtn.addEventListener("click", () => {
      onStructuralChange(bindings.filter((_, i) => i !== index));
    });

    topLine.append(labelInput, durationInput, durationSuffix, removeBtn);

    const bottomLine = document.createElement("div");
    bottomLine.className = "hotkey-row-line";

    const modifiersInput = document.createElement("input");
    modifiersInput.value = binding.modifiers.join("+");
    modifiersInput.placeholder = `Modificadores (${KNOWN_MODIFIERS.join("/")})`;
    modifiersInput.title = `Separados por "+", p. ej. Control+Alt`;
    modifiersInput.addEventListener("input", () => {
      binding.modifiers = modifiersInput.value
        .split("+")
        .map((m) => m.trim())
        .filter((m) => m.length > 0);
    });

    const codeInput = document.createElement("input");
    codeInput.value = binding.code;
    codeInput.placeholder = "Código (p. ej. Digit1)";
    codeInput.addEventListener("input", () => {
      binding.code = codeInput.value;
    });

    bottomLine.append(modifiersInput, codeInput);

    row.append(topLine, bottomLine);
    container.appendChild(row);
  });
}
