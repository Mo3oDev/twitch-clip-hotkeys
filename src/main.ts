import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
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
  el<HTMLButtonElement>("connect-btn").disabled = status.connected;
  el<HTMLButtonElement>("disconnect-btn").disabled = !status.connected;
}

// While a combo is being captured, adding or saving hotkeys would either
// abandon the pending capture silently or save a half-set binding — both
// buttons are disabled for the duration.
function setHotkeyControlsDisabled(disabled: boolean): void {
  el<HTMLButtonElement>("add-hotkey-btn").disabled = disabled;
  el<HTMLButtonElement>("save-hotkeys-btn").disabled = disabled;
}

function rerenderHotkeys(next: HotkeyBinding[]): void {
  currentBindings = next;
  renderHotkeyList(el("hotkey-list"), currentBindings, rerenderHotkeys, setHotkeyControlsDisabled);
}

async function refreshHotkeys(): Promise<void> {
  currentBindings = await invoke<HotkeyBinding[]>("get_hotkeys");
  renderHotkeyList(el("hotkey-list"), currentBindings, rerenderHotkeys, setHotkeyControlsDisabled);
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
    "Couldn't register these hotkeys (combo already in use by another app): " +
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
    statusLine.textContent = "Saved.";
    statusLine.classList.remove("hidden");
  } catch (err) {
    errorLine.textContent = String(err);
    errorLine.classList.remove("hidden");
  }
  await refreshHotkeyErrors();
}

function setDiagnostic(text: string, url?: string): void {
  const line = el("diagnostic-line");
  if (!url) {
    line.textContent = text;
    return;
  }
  line.textContent = "";
  line.append(`${text} `);
  const link = document.createElement("a");
  link.href = url;
  link.textContent = "View clip";
  // A plain <a target="_blank"> never navigates inside the Tauri webview —
  // there's no browser chrome to open a new tab in. `openUrl` is the
  // documented way to hand a link to the OS's default browser instead.
  link.addEventListener("click", (event) => {
    event.preventDefault();
    void openUrl(url);
  });
  line.append(link);
}

type ClipRef = { id: string; edit_url: string };

function describeClipOutcome(outcome: { type: string; data?: unknown }): {
  text: string;
  url?: string;
} {
  switch (outcome.type) {
    case "VodSuccess": {
      const data = outcome.data as { clip: ClipRef; duration_seconds: number };
      return {
        text: `Clip created (${data.duration_seconds}s, exact duration).`,
        url: data.clip.edit_url,
      };
    }
    case "FallbackSuccess": {
      const data = outcome.data as { clip: ClipRef };
      return {
        text: "Clip created (approximate duration, fallback method).",
        url: data.clip.edit_url,
      };
    }
    case "StillProcessing":
      return { text: "Clip is processing, it may take a bit longer to show up." };
    case "Offline":
      return { text: "The stream is offline, no clip was created." };
    case "Failed": {
      const message = outcome.data as string;
      return { text: `Last error: ${message}` };
    }
    default:
      return { text: "Unknown clip event." };
  }
}

window.addEventListener("DOMContentLoaded", async () => {
  el("dev-console-link").addEventListener("click", (event) => {
    event.preventDefault();
    void openUrl("https://dev.twitch.tv/console");
  });

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
      status.textContent = "Saved.";
      status.classList.remove("hidden");
    } catch (err) {
      status.textContent = `Error: ${String(err)}`;
      status.classList.remove("hidden");
    }
  });

  el("connect-btn").addEventListener("click", async () => {
    const btn = el<HTMLButtonElement>("connect-btn");
    // Disabled synchronously before the first await — prevents a second
    // click from starting a second, parallel device-flow login while this
    // one (which can wait up to the device code's full expiry, ~30 min) is
    // still polling. `refreshStatus` in `finally` is the single source of
    // truth for the final disabled state on both buttons, whether login
    // succeeded, failed, or timed out.
    btn.disabled = true;
    btn.textContent = "Connecting...";
    try {
      await invoke("auth_login");
    } catch (err) {
      setDiagnostic(`Couldn't connect: ${String(err)}`);
    } finally {
      btn.textContent = "Connect to Twitch";
      await refreshStatus();
    }
  });

  el("disconnect-btn").addEventListener("click", async () => {
    const btn = el<HTMLButtonElement>("disconnect-btn");
    btn.disabled = true;
    try {
      await invoke("auth_logout");
    } finally {
      await refreshStatus();
    }
  });

  el("add-hotkey-btn").addEventListener("click", () => {
    rerenderHotkeys([...currentBindings, defaultBinding()]);
  });

  el("save-hotkeys-btn").addEventListener("click", saveCurrentHotkeys);

  await listen<string>("auth:user_code", (event) => {
    const line = el("user-code-line");
    line.textContent = `Verification code: ${event.payload} (opened in your browser)`;
    line.classList.remove("hidden");
  });

  await listen<string>("auth:success", async () => {
    el("user-code-line").classList.add("hidden");
    await refreshStatus();
  });

  await listen<string>("auth:timeout", (event) => {
    setDiagnostic(`Connection to Twitch failed: ${event.payload}`);
    el("user-code-line").classList.add("hidden");
  });

  await listen("auth:required", () => {
    setDiagnostic("Twitch not connected. Open Settings and connect your account.");
  });

  await listen<{ type: string; data?: unknown }>("clip:result", (event) => {
    const { text, url } = describeClipOutcome(event.payload);
    setDiagnostic(text, url);
  });
});
