export interface AuthStatus {
  connected: boolean;
  login: string | null;
}

export function renderStatusBadge(container: HTMLElement, status: AuthStatus): void {
  container.textContent = "";
  const badge = document.createElement("span");
  badge.className = status.connected ? "badge badge-connected" : "badge badge-disconnected";
  badge.textContent = status.connected
    ? `Conectado${status.login ? ` como ${status.login}` : ""}`
    : "Desconectado";
  container.appendChild(badge);
}
