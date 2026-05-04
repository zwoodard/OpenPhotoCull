import { convertFileSrc } from "@tauri-apps/api/core";
import { useStore, isBlurry, hasExposureIssue, exposureVerdict, blurIntent } from "../store";
import { dupGroupColor } from "../lib/duplicates";

export function PhotoDetail() {
  const selectedId = useStore((s) => s.selectedId);
  const settings = useStore((s) => s.settings);
  const images = useStore((s) => s.images);
  const analysisMap = useStore((s) => s.analysisMap);
  const duplicateGroups = useStore((s) => s.duplicateGroups);
  const marks = useStore((s) => s.marks);
  const setMark = useStore((s) => s.setMark);
  const setComparisonMode = useStore((s) => s.setComparisonMode);

  const image = images.find((i) => i.id === selectedId);
  if (!image) {
    return (
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          height: "100%",
          color: "var(--text-muted)",
        }}
      >
        Select a photo to view details
      </div>
    );
  }

  const analysis = selectedId ? analysisMap[selectedId] : null;
  const mark = selectedId ? marks[selectedId] || "unmarked" : "unmarked";

  const formatSize = (bytes: number) => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  };

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100%",
        overflow: "hidden",
      }}
    >
      {/* Preview */}
      <div
        style={{
          flex: 1,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          background: "#000",
          minHeight: 0,
        }}
      >
        <img
          src={convertFileSrc(image.path)}
          alt={image.fileName}
          style={{
            maxWidth: "100%",
            maxHeight: "100%",
            objectFit: "contain",
          }}
        />
      </div>

      {/* Info panel */}
      <div
        style={{
          padding: 16,
          borderTop: "1px solid var(--border)",
          background: "var(--bg-secondary)",
          overflow: "auto",
          maxHeight: 280,
        }}
      >
        <h3
          style={{
            fontSize: 14,
            fontWeight: 600,
            marginBottom: 8,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {image.fileName}
        </h3>

        <div
          style={{
            display: "grid",
            gridTemplateColumns: "1fr 1fr",
            gap: "4px 12px",
            fontSize: 12,
            color: "var(--text-secondary)",
          }}
        >
          <span>Size: {formatSize(image.fileSize)}</span>
          {image.width && image.height && (
            <span>
              {image.width} x {image.height}
            </span>
          )}
          {image.exif?.cameraMake && (
            <span>
              {image.exif.cameraMake} {image.exif.cameraModel || ""}
            </span>
          )}
          {image.exif?.iso && <span>ISO {image.exif.iso}</span>}
          {image.exif?.aperture && <span>f/{image.exif.aperture}</span>}
          {image.exif?.shutterSpeed && <span>{image.exif.shutterSpeed}s</span>}
          {image.exif?.focalLengthMm && (
            <span>{image.exif.focalLengthMm}mm</span>
          )}
        </div>

        {/* Analysis results */}
        {analysis && (
          <div style={{ marginTop: 12, fontSize: 12 }}>
            <div style={{ fontWeight: 600, marginBottom: 4 }}>Analysis</div>
            {analysis.blur && (
              <div
                style={{
                  color: isBlurry(analysis.blur, analysis.subjectFocus, settings)
                    ? "var(--warning)"
                    : "var(--success)",
                }}
              >
                Blur: {blurIntent(analysis.blur, analysis.subjectFocus)} — global{" "}
                {analysis.blur.laplacianVariance.toFixed(0)}, max-tile{" "}
                {analysis.blur.maxTileVariance.toFixed(0)}, sharp{" "}
                {(analysis.blur.sharpTileFraction * 100).toFixed(0)}%
                {analysis.blur.bokehLikely && " · bokeh-likely"}
                {analysis.blur.shakeRisk && " · shake-risk"}
              </div>
            )}
            {analysis.exposure && (
              <div
                style={{
                  color: hasExposureIssue(analysis.exposure, settings)
                    ? "var(--warning)"
                    : "var(--success)",
                }}
              >
                Exposure: {exposureVerdict(analysis.exposure, settings)} (mean:{" "}
                {(analysis.exposure.meanLuminance * 100).toFixed(0)}%,
                under: {(analysis.exposure.pctUnderexposed * 100).toFixed(0)}%,
                over: {(analysis.exposure.pctOverexposed * 100).toFixed(0)}%)
              </div>
            )}
            {analysis.subjectFocus && (
              <div
                style={{
                  color:
                    analysis.subjectFocus.verdict === "BackFocus"
                      ? "var(--danger)"
                      : analysis.subjectFocus.verdict === "SubjectSharp"
                        ? "var(--success)"
                        : "var(--warning)",
                }}
              >
                Focus: {analysis.subjectFocus.verdict === "SubjectSharp"
                  ? "Subject sharp"
                  : analysis.subjectFocus.verdict === "BackFocus"
                    ? "Back-focused (subject blurry, background sharp)"
                    : analysis.subjectFocus.verdict === "AllBlurry"
                      ? "All blurry"
                      : "Subject blurry"
                }
                {" "}(subject: {analysis.subjectFocus.subjectBlurVariance.toFixed(0)},
                bg: {analysis.subjectFocus.backgroundBlurVariance.toFixed(0)},
                ratio: {analysis.subjectFocus.focusRatio.toFixed(2)},
                source: {analysis.subjectFocus.subjectSource})
              </div>
            )}
            {analysis.closedEyes && (
              <div
                style={{
                  color: analysis.closedEyes.hasClosedEyes
                    ? "#fb923c"
                    : "var(--success)",
                }}
              >
                Faces: {analysis.closedEyes.faceCount}
                {analysis.closedEyes.faceCount > 0 && (
                  <>
                    {" "}({analysis.closedEyes.faces.map((f, i) => (
                      <span key={i}>
                        {i > 0 && ", "}
                        {f.eyesClosed ? "closed" : "open"}
                      </span>
                    ))})
                  </>
                )}
                {analysis.closedEyes.hasClosedEyes && " — someone blinked!"}
              </div>
            )}
            {analysis.duplicateGroupId && (() => {
              const groupId = analysis.duplicateGroupId!;
              const groupNum = groupId.replace(/\D/g, "") || groupId;
              const members = duplicateGroups[groupId] || [];
              const color = dupGroupColor(groupId);
              return (
                <div style={{ color }}>
                  <span style={{
                    display: "inline-block",
                    background: color,
                    color: "#000",
                    borderRadius: 3,
                    padding: "0 4px",
                    fontSize: 11,
                    fontWeight: 700,
                    marginRight: 4,
                  }}>
                    DUP {groupNum}
                  </span>
                  {members.length} photos in group
                  <button
                    onClick={() => setComparisonMode(true)}
                    style={{
                      marginLeft: 8,
                      padding: "2px 8px",
                      borderRadius: "var(--radius-sm)",
                      fontSize: 10,
                      fontWeight: 700,
                      background: color,
                      color: "#000",
                      cursor: "pointer",
                    }}
                  >
                    Compare (C)
                  </button>
                  {members.length > 0 && (
                    <div style={{ fontSize: 11, marginTop: 2, color: "var(--text-muted)" }}>
                      {members.map((mid) => {
                        const m = images.find((i) => i.id === mid);
                        return m ? m.fileName : mid.slice(0, 8);
                      }).join(", ")}
                    </div>
                  )}
                </div>
              );
            })()}
          </div>
        )}

        {/* Mark buttons */}
        <div style={{ marginTop: 12, display: "flex", gap: 8 }}>
          <button
            onClick={() => selectedId && setMark(selectedId, "keep")}
            style={{
              flex: 1,
              padding: "8px 0",
              borderRadius: "var(--radius-sm)",
              fontWeight: 600,
              fontSize: 13,
              background:
                mark === "keep" ? "var(--success)" : "var(--bg-surface)",
              color: mark === "keep" ? "#000" : "var(--text-primary)",
            }}
          >
            Keep (K)
          </button>
          <button
            onClick={() => selectedId && setMark(selectedId, "unmarked")}
            style={{
              flex: 1,
              padding: "8px 0",
              borderRadius: "var(--radius-sm)",
              fontWeight: 600,
              fontSize: 13,
              background:
                mark === "unmarked" ? "var(--bg-hover)" : "var(--bg-surface)",
              color: "var(--text-primary)",
            }}
          >
            Unmark (U)
          </button>
          <button
            onClick={() => selectedId && setMark(selectedId, "delete")}
            style={{
              flex: 1,
              padding: "8px 0",
              borderRadius: "var(--radius-sm)",
              fontWeight: 600,
              fontSize: 13,
              background:
                mark === "delete" ? "var(--danger)" : "var(--bg-surface)",
              color: mark === "delete" ? "#fff" : "var(--text-primary)",
            }}
          >
            Delete (D)
          </button>
        </div>
      </div>
    </div>
  );
}
