import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { renderStatusBadge, type AuthStatus } from "./components/statusBadge";
import {
  renderHotkeyList,
  validateClientSide,
  defaultBinding,
  type HotkeyBinding,
} from "./components/hotkeyList";

let currentBindings: HotkeyBinding[] = [];

function el<T extends HTMLElement>(id: string): T {
  const found = document.getElementById(id);
  if (!found) throw new Error(`missing element #${id}`);
  return found as T;
}

async function refreshStatus(): Promise<void> {
  const status = await invoke<AuthStatus>("auth_status");
  renderStatusBadge(el("status-badge"), status);
}

function rerenderHotkeys(next: HotkeyBinding[]): void {
  currentBindings = next;
  renderHotkeyList(el("hotkey-list"), currentBindings, rerenderHotkeys);
}

async function refreshHotkeys(): Promise<void> {
  currentBindings = await invoke<HotkeyBinding[]>("get_hotkeys");
  renderHotkeyList(el("hotkey-list"), currentBindings, rerenderHotkeys);
}

async function refreshHotkeyErrors(): Promise<void> {
  const errors = await invoke<{ id: string; label: string; message: string }[]>(
    "get_hotkey_errors",
  );
  const line = el("hotkey-startup-error");
  if (errors.length === 0) {
    line.classList.add("hidden");
    return;
  }
  line.textContent =
    "No se pudieron registrar estos atajos (combinación en uso por otra app): " +
    errors.map((e) => `${e.label} (${e.message})`).join("; ");
  line.classList.remove("hidden");
}

// Saves the current in-memory draft (`renderHotkeyList` mutates it in
// place as the user types, without re-rendering or auto-saving per
// keystroke — see that module's comment for why).
async function saveCurrentHotkeys(): Promise<void> {
  const errorLine = el("hotkey-error");
  const statusLine = el("hotkey-save-status");
  statusLine.classList.add("hidden");

  const clientError = validateClientSide(currentBindings);
  if (clientError) {
    errorLine.textContent = clientError;
    errorLine.classList.remove("hidden");
    return;
  }

  try {
    await invoke("save_hotkeys", { bindings: currentBindings });
    errorLine.classList.add("hidden");
    statusLine.textContent = "Guardado.";
    statusLine.classList.remove("hidden");
  } catch (err) {
    errorLine.textContent = String(err);
    errorLine.classList.remove("hidden");
  }
  await refreshHotkeyErrors();
}

function setDiagnostic(text: string): void {
  el("diagnostic-line").textContent = text;
}

function describeClipOutcome(outcome: { type: string; data?: unknown }): string {
  switch (outcome.type) {
    case "VodSuccess": {
      const data = outcome.data as { duration_seconds: number };
      return `Clip creado (${data.duration_seconds}s, duración exacta).`;
    }
    case "FallbackSuccess":
      return "Clip creado (duración aproximada, método alterno).";
    case "StillProcessing":
      return "Clip en proceso, puede tardar un poco más en aparecer.";
    case "Offline":
      return "El stream está offline, no se creó ningún clip.";
    case "Failed": {
      const message = outcome.data as string;
      return `Último error: ${message}`;
    }
    default:
      return "Evento de clip desconocido.";
  }
}

window.addEventListener("DOMContentLoaded", async () => {
  await refreshStatus();
  await refreshHotkeys();
  await refreshHotkeyErrors();

  el<HTMLInputElement>("client-id").value =
    (await invoke<string | null>("get_client_id")) ?? "";

  el("save-client-id").addEventListener("click", async () => {
    const id = el<HTMLInputElement>("client-id").value.trim();
    const status = el("client-id-status");
    try {
      await invoke("set_client_id", { id });
      status.textContent = "Guardado.";
      status.classList.remove("hidden");
    } catch (err) {
      status.textContent = `Error: ${String(err)}`;
      status.classList.remove("hidden");
    }
  });

  el("connect-btn").addEventListener("click", async () => {
    try {
      await invoke("auth_login");
    } catch (err) {
      setDiagnostic(`No se pudo conectar: ${String(err)}`);
    }
  });

  el("disconnect-btn").addEventListener("click", async () => {
    await invoke("auth_logout");
    await refreshStatus();
  });

  el("add-hotkey-btn").addEventListener("click", () => {
    rerenderHotkeys([...currentBindings, defaultBinding()]);
  });

  el("save-hotkeys-btn").addEventListener("click", saveCurrentHotkeys);

  await listen<string>("auth:user_code", (event) => {
    const line = el("user-code-line");
    line.textContent = `Código de verificación: ${event.payload} (se abrió en tu navegador)`;
    line.classList.remove("hidden");
  });

  await listen<string>("auth:success", async () => {
    el("user-code-line").classList.add("hidden");
    await refreshStatus();
  });

  await listen<string>("auth:timeout", (event) => {
    setDiagnostic(`Conexión con Twitch falló: ${event.payload}`);
    el("user-code-line").classList.add("hidden");
  });

  await listen("auth:required", () => {
    setDiagnostic("Twitch no conectado. Abre Ajustes y conecta tu cuenta.");
  });

  await listen<{ type: string; data?: unknown }>("clip:result", (event) => {
    setDiagnostic(describeClipOutcome(event.payload));
  });
});
