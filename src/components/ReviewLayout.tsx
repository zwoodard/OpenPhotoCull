import { useEffect, useCallback } from "react";
import { useStore } from "../store";
import { FilterBar } from "./FilterBar";
import { PhotoGrid } from "./PhotoGrid";
import { PhotoDetail } from "./PhotoDetail";
import { BulkActions } from "./BulkActions";
import { ComparisonView } from "./ComparisonView";
import { PersonFilter } from "./PersonFilter";

export function ReviewLayout() {
  const selectedId = useStore((s) => s.selectedId);
  const setSelectedId = useStore((s) => s.setSelectedId);
  const setMark = useStore((s) => s.setMark);
  const filteredImages = useStore((s) => s.filteredImages);
  const comparisonMode = useStore((s) => s.comparisonMode);
  const setComparisonMode = useStore((s) => s.setComparisonMode);
  const analysisMap = useStore((s) => s.analysisMap);

  // Keyboard navigation
  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      // C toggles comparison mode (only if selected image is in a duplicate group)
      if (e.key === "c" || e.key === "C") {
        if (selectedId && analysisMap[selectedId]?.duplicateGroupId) {
          setComparisonMode(!comparisonMode);
        }
        return;
      }

      // Escape exits comparison mode
      if (e.key === "Escape" && comparisonMode) {
        setComparisonMode(false);
        return;
      }

      // Don't navigate while in comparison mode
      if (comparisonMode) return;

      const filtered = filteredImages();
      if (!filtered.length) return;

      const idx = selectedId
        ? filtered.findIndex((i) => i.id === selectedId)
        : -1;

      switch (e.key) {
        case "ArrowRight":
        case "ArrowDown": {
          e.preventDefault();
          const next = Math.min(idx + 1, filtered.length - 1);
          setSelectedId(filtered[next].id);
          break;
        }
        case "ArrowLeft":
        case "ArrowUp": {
          e.preventDefault();
          const prev = Math.max(idx - 1, 0);
          setSelectedId(filtered[prev].id);
          break;
        }
        case "k":
        case "K":
          if (selectedId) setMark(selectedId, "keep");
          break;
        case "d":
        case "D":
          if (selectedId) setMark(selectedId, "delete");
          break;
        case "u":
        case "U":
          if (selectedId) setMark(selectedId, "unmarked");
          break;
      }
    },
    [selectedId, filteredImages, setSelectedId, setMark, comparisonMode, setComparisonMode, analysisMap],
  );

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  return (
    <div
      style={{
        height: "100%",
        display: "flex",
        flexDirection: "column",
      }}
    >
      <FilterBar />
      <PersonFilter />
      {comparisonMode ? (
        <div style={{ flex: 1, minHeight: 0 }}>
          <ComparisonView />
        </div>
      ) : (
        <>
          <div style={{ flex: 1, display: "flex", minHeight: 0 }}>
            <div
              style={{
                flex: 1,
                display: "flex",
                flexDirection: "column",
                minWidth: 0,
              }}
            >
              <PhotoGrid />
            </div>
            {selectedId && (
              <div
                style={{
                  width: 420,
                  borderLeft: "1px solid var(--border)",
                  display: "flex",
                  flexDirection: "column",
                }}
              >
                <PhotoDetail />
              </div>
            )}
          </div>
          <BulkActions />
        </>
      )}
    </div>
  );
}
