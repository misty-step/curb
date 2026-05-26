import React from "react";
import { AbsoluteFill, interpolate, spring, useCurrentFrame, useVideoConfig } from "remotion";

const scenes = [
  {
    label: "Observe",
    title: "Curb starts in visibility mode",
    body: "Local process metadata and usage metadata become an append-only evidence ledger.",
    metric: "0 stops possible",
  },
  {
    label: "Warn",
    title: "A synthetic worker crosses the warning line",
    body: "Alert mode notifies the operator but cannot terminate anything.",
    metric: "warning event",
  },
  {
    label: "Acknowledge",
    title: "The operator extends the run",
    body: "The acknowledgement is bounded, visible, and recorded in the ledger.",
    metric: "+10s extension",
  },
  {
    label: "Enforce",
    title: "Only the controlled worker is stopped",
    body: "Curb revalidates PID and start time before stopping the synthetic process tree.",
    metric: "desktop apps untouched",
  },
  {
    label: "Privacy",
    title: "Evidence without content capture",
    body: "No prompts, responses, screenshots, keystrokes, or file contents are recorded.",
    metric: "metadata only",
  },
];

export function CurbDemo() {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const sceneIndex = Math.min(scenes.length - 1, Math.floor(frame / (fps * 6)));
  const scene = scenes[sceneIndex];
  const progress = spring({ frame: frame - sceneIndex * fps * 6, fps, config: { damping: 30 } });
  const barWidth = interpolate(frame, [0, fps * 30], [8, 100], { extrapolateRight: "clamp" });

  return (
    <AbsoluteFill style={styles.root}>
      <div style={styles.shell}>
        <header style={styles.header}>
          <div>
            <div style={styles.brand}>Curb</div>
            <div style={styles.subtitle}>local agent visibility and safe enforcement</div>
          </div>
          <div style={styles.badge}>synthetic demo</div>
        </header>

        <main style={styles.grid}>
          <section style={styles.stage}>
            <span style={styles.sceneLabel}>{scene.label}</span>
            <h1 style={{ ...styles.title, transform: `translateY(${(1 - progress) * 24}px)`, opacity: progress }}>
              {scene.title}
            </h1>
            <p style={styles.body}>{scene.body}</p>
            <div style={styles.progressTrack}>
              <div style={{ ...styles.progressFill, width: `${barWidth}%` }} />
            </div>
          </section>

          <aside style={styles.panel}>
            <div style={styles.panelTitle}>What Curb records</div>
            <Row label="process" value="pid + start time" />
            <Row label="usage" value="tokens + models" />
            <Row label="events" value="warn, ack, stop" />
            <Row label="content" value="not captured" />
            <div style={styles.metric}>{scene.metric}</div>
          </aside>
        </main>
      </div>
    </AbsoluteFill>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div style={styles.row}>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  root: {
    background: "#f5f7fb",
    color: "#172033",
    fontFamily: "Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif",
  },
  shell: {
    padding: 72,
    height: "100%",
    boxSizing: "border-box",
  },
  header: {
    display: "flex",
    justifyContent: "space-between",
    alignItems: "center",
    marginBottom: 70,
  },
  brand: {
    fontSize: 42,
    fontWeight: 800,
  },
  subtitle: {
    fontSize: 24,
    color: "#55657d",
    marginTop: 8,
  },
  badge: {
    border: "2px solid #c8d5e6",
    borderRadius: 999,
    padding: "16px 24px",
    fontSize: 22,
    fontWeight: 700,
    color: "#006b62",
    background: "#e7fbf5",
  },
  grid: {
    display: "grid",
    gridTemplateColumns: "1.4fr 0.8fr",
    gap: 36,
  },
  stage: {
    background: "#ffffff",
    border: "2px solid #d7e1ee",
    borderRadius: 18,
    padding: 52,
    minHeight: 560,
  },
  sceneLabel: {
    textTransform: "uppercase",
    fontSize: 22,
    fontWeight: 800,
    color: "#006b62",
    letterSpacing: 1.5,
  },
  title: {
    fontSize: 74,
    lineHeight: 1.02,
    margin: "32px 0 28px",
    maxWidth: 980,
  },
  body: {
    fontSize: 32,
    lineHeight: 1.35,
    color: "#42516a",
    maxWidth: 900,
  },
  progressTrack: {
    height: 18,
    background: "#edf2f8",
    borderRadius: 999,
    marginTop: 92,
    overflow: "hidden",
  },
  progressFill: {
    height: "100%",
    background: "#006b62",
  },
  panel: {
    background: "#172033",
    color: "#ffffff",
    borderRadius: 18,
    padding: 42,
    minHeight: 560,
  },
  panelTitle: {
    fontSize: 28,
    fontWeight: 800,
    marginBottom: 34,
  },
  row: {
    display: "flex",
    justifyContent: "space-between",
    gap: 24,
    padding: "20px 0",
    borderTop: "1px solid rgba(255,255,255,0.16)",
    fontSize: 24,
  },
  metric: {
    marginTop: 54,
    padding: 28,
    borderRadius: 14,
    background: "#e7fbf5",
    color: "#006b62",
    fontSize: 30,
    fontWeight: 800,
  },
};
