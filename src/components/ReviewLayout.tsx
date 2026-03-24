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
  const bulkMark = useStore((s) => s.bulkMark);
  const filteredImages = useStore((s) => s.filteredImages);
  const multiSelection = useStore((s) => s.multiSelection);
  const clearMultiSelection = useStore((s) => s.clearMultiSelection);
  const selectAll = useStore((s) => s.selectAll);
  const invertSelection = useStore((s) => s.invertSelection);
  const selectRange = useStore((s) => s.selectRange);
  const comparisonMode = useStore((s) => s.comparisonMode);
  const setComparisonMode = useStore((s) => s.setComparisonMode);
  const analysisMap = useStore((s) => s.analysisMap);

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      const meta = e.metaKey || e.ctrlKey;

      // Cmd/Ctrl+A — Select all filtered images
      if (meta && e.key === "a") {
        e.preventDefault();
        selectAll();
        return;
      }

      // Cmd/Ctrl+Shift+I — Invert selection
      if (meta && e.shiftKey && (e.key === "i" || e.key === "I")) {
        e.preventDefault();
        invertSelection();
        return;
      }

      // C toggles comparison mode
      if (e.key === "c" || e.key === "C") {
        if (selectedId && analysisMap[selectedId]?.duplicateGroupId) {
          setComparisonMode(!comparisonMode);
        }
        return;
      }

      // Escape — clear multi-selection first, then exit comparison mode
      if (e.key === "Escape") {
        if (multiSelection.size > 0) {
          clearMultiSelection();
          return;
        }
        if (comparisonMode) {
          setComparisonMode(false);
          return;
        }
        return;
      }

      if (comparisonMode) return;

      const filtered = filteredImages();
      if (!filtered.length) return;

      const idx = selectedId
        ? filtered.findIndex((i) => i.id === selectedId)
        : -1;

      // Arrow key navigation
      if (
        e.key === "ArrowRight" ||
        e.key === "ArrowDown" ||
        e.key === "ArrowLeft" ||
        e.key === "ArrowUp"
      ) {
        e.preventDefault();
        const forward = e.key === "ArrowRight" || e.key === "ArrowDown";
        const nextIdx = forward
          ? Math.min(idx + 1, filtered.length - 1)
          : Math.max(idx - 1, 0);
        const nextId = filtered[nextIdx].id;

        if (e.shiftKey && selectedId) {
          // Shift+Arrow — extend range selection
          selectRange(selectedId, nextId);
        } else if (!e.shiftKey && multiSelection.size > 0) {
          // Plain arrow clears multi-selection
          clearMultiSelection();
        }

        setSelectedId(nextId);
        return;
      }

      // K/D/U — mark selected image(s)
      const markKey =
        e.key === "k" || e.key === "K"
          ? "keep"
          : e.key === "d" || e.key === "D"
            ? "delete"
            : e.key === "u" || e.key === "U"
              ? "unmarked"
              : null;

      if (markKey) {
        if (multiSelection.size > 0) {
          // Apply to all multi-selected images
          const ids = [...multiSelection];
          if (selectedId && !multiSelection.has(selectedId)) {
            ids.push(selectedId);
          }
          bulkMark(ids, markKey as "keep" | "delete" | "unmarked");
        } else if (selectedId) {
          setMark(selectedId, markKey as "keep" | "delete" | "unmarked");
        }
        return;
      }
    },
    [
      selectedId,
      filteredImages,
      setSelectedId,
      setMark,
      bulkMark,
      multiSelection,
      clearMultiSelection,
      selectAll,
      invertSelection,
      selectRange,
      comparisonMode,
      setComparisonMode,
      analysisMap,
    ],
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
