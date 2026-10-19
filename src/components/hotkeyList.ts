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

export function renderHotkeyList(
  container: HTMLElement,
  bindings: HotkeyBinding[],
  onChange: (bindings: HotkeyBinding[]) => void,
): void {
  container.textContent = "";

  bindings.forEach((binding, index) => {
    const row = document.createElement("div");
    row.className = "hotkey-row";

    const labelInput = document.createElement("input");
    labelInput.value = binding.label;
    labelInput.placeholder = "Etiqueta";
    labelInput.addEventListener("input", () => {
      const next = [...bindings];
      next[index] = { ...binding, label: labelInput.value };
      onChange(next);
    });

    const modifiersInput = document.createElement("input");
    modifiersInput.value = binding.modifiers.join("+");
    modifiersInput.placeholder = `Modificadores (${KNOWN_MODIFIERS.join("/")})`;
    modifiersInput.addEventListener("input", () => {
      const next = [...bindings];
      const modifiers = modifiersInput.value
        .split("+")
        .map((m) => m.trim())
        .filter((m) => m.length > 0);
      next[index] = { ...binding, modifiers };
      onChange(next);
    });

    const codeInput = document.createElement("input");
    codeInput.value = binding.code;
    codeInput.placeholder = "Código (p. ej. Digit1)";
    codeInput.addEventListener("input", () => {
      const next = [...bindings];
      next[index] = { ...binding, code: codeInput.value };
      onChange(next);
    });

    const durationInput = document.createElement("input");
    durationInput.type = "number";
    durationInput.min = "5";
    durationInput.max = "60";
    durationInput.value = String(binding.duration_seconds);
    durationInput.addEventListener("input", () => {
      const next = [...bindings];
      next[index] = {
        ...binding,
        duration_seconds: Number(durationInput.value) || 0,
      };
      onChange(next);
    });

    const removeBtn = document.createElement("button");
    removeBtn.textContent = "Eliminar";
    removeBtn.addEventListener("click", () => {
      onChange(bindings.filter((_, i) => i !== index));
    });

    row.append(labelInput, modifiersInput, codeInput, durationInput, removeBtn);
    container.appendChild(row);
  });
}
