import { useStore } from "../store";

export function ScanProgress() {
  const progress = useStore((s) => s.scanProgress);
  const pct =
    progress.total > 0
      ? Math.round((progress.current / progress.total) * 100)
      : 0;

  const elapsed = progress.elapsedMs ?? 0;
  const rate =
    elapsed > 0 && progress.current > 0
      ? ((progress.current / elapsed) * 1000).toFixed(1)
      : null;

  const eta =
    rate && progress.total > progress.current
      ? Math.round(
          ((progress.total - progress.current) / parseFloat(rate)) * 1000,
        )
      : null;

  const formatMs = (ms: number) => {
    if (ms < 1000) return `${ms}ms`;
    if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
    return `${Math.floor(ms / 60000)}m ${Math.round((ms % 60000) / 1000)}s`;
  };

  const timings = progress.stepTimings;

  return (
    <div
      style={{
        height: "100%",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: 16,
      }}
    >
      <h2 style={{ fontSize: 24, fontWeight: 600 }}>
        {progress.phase || "Scanning..."}
      </h2>

      {/* Progress bar */}
      <div
        style={{
          width: 500,
          height: 8,
          background: "var(--bg-secondary)",
          borderRadius: 4,
          overflow: "hidden",
        }}
      >
        <div
          style={{
            width: `${pct}%`,
            height: "100%",
            background: "var(--accent)",
            borderRadius: 4,
            transition: "width 0.15s ease-out",
          }}
        />
      </div>

      {/* Counts */}
      <p style={{ color: "var(--text-secondary)", fontSize: 14 }}>
        {progress.current} / {progress.total} {pct > 0 && `(${pct}%)`}
      </p>

      {/* Current file */}
      {progress.currentFile && (
        <p
          style={{
            color: "var(--text-muted)",
            fontSize: 12,
            maxWidth: 500,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {progress.currentFile}
        </p>
      )}

      {/* Debug timing panel */}
      <div
        style={{
          marginTop: 16,
          padding: 16,
          background: "var(--bg-secondary)",
          borderRadius: "var(--radius)",
          border: "1px solid var(--border)",
          width: 500,
          fontFamily: "monospace",
          fontSize: 12,
        }}
      >
        <div
          style={{
            fontWeight: 700,
            marginBottom: 8,
            color: "var(--text-secondary)",
            fontSize: 11,
            textTransform: "uppercase",
            letterSpacing: 1,
          }}
        >
          Performance
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
          <Row label="Elapsed" value={formatMs(elapsed)} />
          {rate && <Row label="Throughput" value={`${rate} images/sec`} />}
          {eta !== null && <Row label="ETA" value={formatMs(eta)} />}
        </div>

        {timings && Object.keys(timings).length > 0 && (
          <>
            <div
              style={{
                borderTop: "1px solid var(--border)",
                margin: "8px 0",
              }}
            />
            <div
              style={{
                fontWeight: 700,
                marginBottom: 6,
                color: "var(--text-secondary)",
                fontSize: 11,
                textTransform: "uppercase",
                letterSpacing: 1,
              }}
            >
              Step Breakdown
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 3 }}>
              {Object.entries(timings)
                .sort(([, a], [, b]) => b - a)
                .map(([step, ms]) => (
                  <div key={step} style={{ display: "flex", alignItems: "center", gap: 8 }}>
                    <span style={{ color: "var(--text-muted)", width: 140 }}>
                      {step}
                    </span>
                    <div
                      style={{
                        flex: 1,
                        height: 6,
                        background: "var(--bg-primary)",
                        borderRadius: 3,
                        overflow: "hidden",
                      }}
                    >
                      <div
                        style={{
                          width: `${elapsed > 0 ? Math.min(100, (ms / elapsed) * 100) : 0}%`,
                          height: "100%",
                          background: barColor(step),
                          borderRadius: 3,
                        }}
                      />
                    </div>
                    <span
                      style={{
                        color: "var(--text-primary)",
                        width: 70,
                        textAlign: "right",
                      }}
                    >
                      {formatMs(ms)}
                    </span>
                  </div>
                ))}
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div style={{ display: "flex", justifyContent: "space-between" }}>
      <span style={{ color: "var(--text-muted)" }}>{label}</span>
      <span style={{ color: "var(--text-primary)" }}>{value}</span>
    </div>
  );
}

function barColor(step: string): string {
  const colors: Record<string, string> = {
    discovery: "#4ade80",
    indexing: "#e94560",
    "blur+exposure": "#fbbf24",
    dup_image_load: "#818cf8",
    duplicate_detect: "#f472b6",
  };
  return colors[step] ?? "var(--accent)";
}
