import { useMemo, useState } from "react";
import { useStore, isBlurry, hasExposureIssue } from "../store";
import { SettingsPanel } from "./SettingsPanel";

export function FilterBar() {
  const filters = useStore((s) => s.filters);
  const setFilter = useStore((s) => s.setFilter);
  const images = useStore((s) => s.images);
  const filteredImages = useStore((s) => s.filteredImages);
  const marks = useStore((s) => s.marks);
  const analysisMap = useStore((s) => s.analysisMap);
  const settings = useStore((s) => s.settings);
  const [showSettings, setShowSettings] = useState(false);

  const filtered = filteredImages();
  const deleteCount = Object.values(marks).filter(
    (m) => m === "delete",
  ).length;
  const keepCount = Object.values(marks).filter((m) => m === "keep").length;

  const { blurryCount, exposureCount, dupCount, closedEyesCount, backFocusCount, sceneCount } =
    useMemo(() => {
      let blurry = 0, exposure = 0, dup = 0, closedEyes = 0, backFocus = 0;
      const scenes = new Set<string>();
      for (const img of images) {
        const a = analysisMap[img.id];
        if (!a) continue;
        if (isBlurry(a.blur, a.subjectFocus, settings)) blurry++;
        if (hasExposureIssue(a.exposure, settings)) exposure++;
        if (a.duplicateGroupId != null) dup++;
        if (a.closedEyes?.hasClosedEyes === true) closedEyes++;
        if (a.subjectFocus?.verdict === "BackFocus") backFocus++;
        if (a.sceneGroupId) scenes.add(a.sceneGroupId);
      }
      return {
        blurryCount: blurry,
        exposureCount: exposure,
        dupCount: dup,
        closedEyesCount: closedEyes,
        backFocusCount: backFocus,
        sceneCount: scenes.size,
      };
    }, [images, analysisMap, settings]);

  const chipStyle = (active: boolean) => ({
    padding: "6px 12px",
    borderRadius: "var(--radius)",
    fontSize: 12,
    fontWeight: 600 as const,
    background: active ? "var(--bg-surface)" : "transparent",
    color: active ? "var(--text-primary)" : "var(--text-secondary)",
    border: `1px solid ${active ? "var(--accent)" : "var(--border)"}`,
    cursor: "pointer" as const,
  });

  return (
    <>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          padding: "8px 12px",
          borderBottom: "1px solid var(--border)",
          background: "var(--bg-secondary)",
          flexWrap: "wrap",
        }}
      >
        <span
          style={{ fontSize: 12, color: "var(--text-muted)", marginRight: 4 }}
        >
          {filtered.length} of {images.length} photos
        </span>
        <span style={{ fontSize: 12, color: "var(--success)" }}>
          {keepCount} keep
        </span>
        <span style={{ fontSize: 12, color: "var(--danger)" }}>
          {deleteCount} delete
        </span>
        {sceneCount > 0 && (
          <span style={{ fontSize: 12, color: "var(--text-muted)" }}>
            {sceneCount} scenes
          </span>
        )}

        <div
          style={{
            width: 1,
            height: 20,
            background: "var(--border)",
            margin: "0 4px",
          }}
        />

        <button
          style={chipStyle(filters.showBlurry === true)}
          onClick={() =>
            setFilter("showBlurry", filters.showBlurry === true ? null : true)
          }
        >
          Blurry ({blurryCount})
        </button>
        <button
          style={chipStyle(filters.showExposureIssues === true)}
          onClick={() =>
            setFilter(
              "showExposureIssues",
              filters.showExposureIssues === true ? null : true,
            )
          }
        >
          Exposure ({exposureCount})
        </button>
        {closedEyesCount > 0 && (
          <button
            style={chipStyle(filters.showClosedEyes === true)}
            onClick={() =>
              setFilter(
                "showClosedEyes",
                filters.showClosedEyes === true ? null : true,
              )
            }
          >
            Closed Eyes ({closedEyesCount})
          </button>
        )}
        {backFocusCount > 0 && (
          <button
            style={chipStyle(filters.showBackFocus === true)}
            onClick={() =>
              setFilter(
                "showBackFocus",
                filters.showBackFocus === true ? null : true,
              )
            }
          >
            Back-focused ({backFocusCount})
          </button>
        )}
        <button
          style={chipStyle(filters.showDuplicatesOnly)}
          onClick={() =>
            setFilter("showDuplicatesOnly", !filters.showDuplicatesOnly)
          }
        >
          Duplicates ({dupCount})
        </button>

        <div style={{ flex: 1 }} />

        <button
          onClick={() => setShowSettings(true)}
          style={{
            padding: "6px 12px",
            borderRadius: "var(--radius-sm)",
            fontSize: 12,
            fontWeight: 600,
            background: "var(--bg-surface)",
            color: "var(--text-secondary)",
            border: "1px solid var(--border)",
            cursor: "pointer",
          }}
        >
          Settings
        </button>

        <select
          value={filters.sortBy}
          onChange={(e) => setFilter("sortBy", e.target.value)}
          style={{
            padding: "6px 8px",
            borderRadius: "var(--radius-sm)",
            fontSize: 12,
            background: "var(--bg-surface)",
            color: "var(--text-primary)",
            border: "1px solid var(--border)",
          }}
        >
          <option value="date">Sort: Date</option>
          <option value="name">Sort: Name</option>
          <option value="blurScore">Sort: Blur Score</option>
          <option value="exposure">Sort: Exposure</option>
        </select>
      </div>

      {showSettings && (
        <SettingsPanel onClose={() => setShowSettings(false)} />
      )}
    </>
  );
}
