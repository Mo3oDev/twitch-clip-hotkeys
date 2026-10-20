export interface AuthStatus {
  connected: boolean;
  login: string | null;
}

export function renderStatusBadge(container: HTMLElement, status: AuthStatus): void {
  container.textContent = "";
  const badge = document.createElement("span");
  badge.className = status.connected ? "badge badge-connected" : "badge badge-disconnected";
  badge.textContent = status.connected
    ? `Connected${status.login ? ` as ${status.login}` : ""}`
    : "Disconnected";
  container.appendChild(badge);
}
