export function formatTokens(value?: number): string {
  const n = value ?? 0;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 10_000) return `${Math.round(n / 1000)}k`;
  return `${n}`;
}

export function formatDuration(seconds?: number): string {
  if (seconds === undefined || seconds < 0) return "-";
  if (seconds < 60) return `${Math.floor(seconds)}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
  return `${Math.floor(seconds / 86400)}d ${Math.floor((seconds % 86400) / 3600)}h`;
}

export function relativeTime(iso?: string): string {
  if (!iso) return "-";
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return "-";
  const elapsed = Math.max(0, Date.now() - then);
  if (elapsed < 60_000) return "now";
  if (elapsed < 3_600_000) return `${Math.floor(elapsed / 60_000)}m ago`;
  if (elapsed < 86_400_000) return `${Math.floor(elapsed / 3_600_000)}h ago`;
  return `${Math.floor(elapsed / 86_400_000)}d ago`;
}

export function stateLabel(state: string, usageState?: string): string {
  if (usageState && usageState !== state) return `${state} / ${usageState}`;
  return state;
}

export function statusTone(status: string): "ok" | "active" | "watch" | "action" {
  if (status === "ACTION") return "action";
  if (status === "WATCH") return "watch";
  if (status === "ACTIVE") return "active";
  return "ok";
}
