import { convertFileSrc } from "@tauri-apps/api/core";
import { useStore, isBlurry } from "../store";

export function ComparisonView() {
  const selectedId = useStore((s) => s.selectedId);
  const analysisMap = useStore((s) => s.analysisMap);
  const duplicateGroups = useStore((s) => s.duplicateGroups);
  const images = useStore((s) => s.images);
  const marks = useStore((s) => s.marks);
  const setMark = useStore((s) => s.setMark);
  const bulkMark = useStore((s) => s.bulkMark);
  const setComparisonMode = useStore((s) => s.setComparisonMode);
  const settings = useStore((s) => s.settings);

  const groupId = selectedId
    ? analysisMap[selectedId]?.duplicateGroupId
    : null;
  const memberIds = groupId ? duplicateGroups[groupId] || [] : [];
  const members = memberIds
    .map((id) => images.find((i) => i.id === id))
    .filter(Boolean) as typeof images;

  if (members.length === 0) {
    setComparisonMode(false);
    return null;
  }

  // Find the "best" image (highest blur variance = sharpest)
  const bestId = members.reduce((best, img) => {
    const bestVar =
      analysisMap[best.id]?.blur?.laplacianVariance ?? 0;
    const imgVar =
      analysisMap[img.id]?.blur?.laplacianVariance ?? 0;
    return imgVar > bestVar ? img : best;
  }, members[0]).id;

  const pickBest = () => {
    const toDelete = memberIds.filter((id) => id !== bestId);
    setMark(bestId, "keep");
    bulkMark(toDelete, "delete");
  };

  return (
    <div
      style={{
        height: "100%",
        display: "flex",
        flexDirection: "column",
        background: "var(--bg-primary)",
      }}
    >
      {/* Header */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 12,
          padding: "10px 16px",
          borderBottom: "1px solid var(--border)",
          background: "var(--bg-secondary)",
        }}
      >
        <h3 style={{ fontSize: 14, fontWeight: 700 }}>
          Comparing Duplicate Group {groupId?.replace(/\D/g, "")} — {members.length} photos
        </h3>
        <div style={{ flex: 1 }} />
        <button
          onClick={pickBest}
          style={{
            padding: "6px 14px",
            borderRadius: "var(--radius-sm)",
            fontSize: 12,
            fontWeight: 600,
            background: "var(--success)",
            color: "#000",
          }}
        >
          Pick Best (keep sharpest)
        </button>
        <button
          onClick={() => setComparisonMode(false)}
          style={{
            padding: "6px 14px",
            borderRadius: "var(--radius-sm)",
            fontSize: 12,
            fontWeight: 600,
            background: "var(--bg-surface)",
            color: "var(--text-primary)",
          }}
        >
          Close (Esc)
        </button>
      </div>

      {/* Side-by-side cards */}
      <div
        style={{
          flex: 1,
          display: "flex",
          gap: 8,
          padding: 8,
          overflowX: "auto",
          minHeight: 0,
        }}
      >
        {members.map((img) => {
          const analysis = analysisMap[img.id];
          const mark = marks[img.id] || "unmarked";
          const isBest = img.id === bestId;
          const blurVar =
            analysis?.blur?.laplacianVariance ?? 0;
          const blurry = isBlurry(analysis?.blur, settings);

          return (
            <div
              key={img.id}
              style={{
                flex: "1 0 250px",
                maxWidth: 500,
                display: "flex",
                flexDirection: "column",
                borderRadius: "var(--radius)",
                border: `2px solid ${
                  mark === "keep"
                    ? "var(--success)"
                    : mark === "delete"
                      ? "var(--danger)"
                      : isBest
                        ? "var(--accent)"
                        : "var(--border)"
                }`,
                overflow: "hidden",
                background: "var(--bg-secondary)",
                opacity: mark === "delete" ? 0.5 : 1,
              }}
            >
              {/* Image */}
              <div
                style={{
                  flex: 1,
                  minHeight: 0,
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  background: "#000",
                }}
              >
                <img
                  src={convertFileSrc(img.path)}
                  alt={img.fileName}
                  style={{
                    maxWidth: "100%",
                    maxHeight: "100%",
                    objectFit: "contain",
                  }}
                />
              </div>

              {/* Info */}
              <div style={{ padding: "8px 10px", fontSize: 12 }}>
                <div
                  style={{
                    fontWeight: 600,
                    marginBottom: 4,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {img.fileName}
                  {isBest && (
                    <span
                      style={{
                        marginLeft: 6,
                        fontSize: 10,
                        color: "var(--accent)",
                      }}
                    >
                      SHARPEST
                    </span>
                  )}
                </div>

                {/* Metrics bar */}
                <div
                  style={{
                    display: "flex",
                    gap: 8,
                    color: "var(--text-secondary)",
                    marginBottom: 6,
                  }}
                >
                  <span
                    style={{
                      color: blurry
                        ? "var(--warning)"
                        : "var(--success)",
                    }}
                  >
                    Blur: {blurVar.toFixed(0)}
                  </span>
                  {analysis?.exposure && (
                    <span>
                      Exp: {(analysis.exposure.meanLuminance * 100).toFixed(0)}%
                    </span>
                  )}
                  {analysis?.closedEyes?.hasClosedEyes && (
                    <span style={{ color: "#fb923c" }}>Eyes closed</span>
                  )}
                  {analysis?.subjectFocus?.verdict === "BackFocus" && (
                    <span style={{ color: "var(--danger)" }}>Back-focused</span>
                  )}
                </div>

                {/* Mark buttons */}
                <div style={{ display: "flex", gap: 4 }}>
                  <button
                    onClick={() => setMark(img.id, "keep")}
                    style={{
                      flex: 1,
                      padding: "5px 0",
                      borderRadius: "var(--radius-sm)",
                      fontSize: 11,
                      fontWeight: 600,
                      background:
                        mark === "keep"
                          ? "var(--success)"
                          : "var(--bg-surface)",
                      color:
                        mark === "keep" ? "#000" : "var(--text-primary)",
                    }}
                  >
                    Keep
                  </button>
                  <button
                    onClick={() => setMark(img.id, "delete")}
                    style={{
                      flex: 1,
                      padding: "5px 0",
                      borderRadius: "var(--radius-sm)",
                      fontSize: 11,
                      fontWeight: 600,
                      background:
                        mark === "delete"
                          ? "var(--danger)"
                          : "var(--bg-surface)",
                      color:
                        mark === "delete" ? "#fff" : "var(--text-primary)",
                    }}
                  >
                    Delete
                  </button>
                </div>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
