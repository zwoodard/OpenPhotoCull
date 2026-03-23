import { useRef, useCallback, useMemo } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useStore, isBlurry, hasExposureIssue, exposureVerdict } from "../store";
import type { Mark } from "../store/types";

const COLUMN_COUNT = 6;
const ROW_HEIGHT = 180;
const GAP = 4;

import { dupGroupLabel, dupGroupColor } from "../lib/duplicates";

export function PhotoGrid() {
  const filteredImages = useStore((s) => s.filteredImages);
  const marks = useStore((s) => s.marks);
  const selectedId = useStore((s) => s.selectedId);
  const multiSelection = useStore((s) => s.multiSelection);
  const setSelectedId = useStore((s) => s.setSelectedId);
  const toggleMultiSelect = useStore((s) => s.toggleMultiSelect);
  const selectRange = useStore((s) => s.selectRange);
  const analysisMap = useStore((s) => s.analysisMap);
  const settings = useStore((s) => s.settings);
  const filters = useStore((s) => s.filters);
  const storeImages = useStore((s) => s.images);
  const sceneBreaksFn = useStore((s) => s.sceneBreaks);

  // Re-compute when filters, settings, images, or analysis change
  const images = useMemo(
    () => filteredImages(),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [filteredImages, filters, settings, storeImages, analysisMap],
  );
  const sceneBreaks = useMemo(
    () => sceneBreaksFn(),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [sceneBreaksFn, images, analysisMap],
  );
  const rowCount = Math.ceil(images.length / COLUMN_COUNT);
  const parentRef = useRef<HTMLDivElement>(null);

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_HEIGHT + GAP,
    overscan: 5,
  });

  const handleClick = useCallback(
    (id: string, e: React.MouseEvent) => {
      if (e.metaKey || e.ctrlKey) {
        toggleMultiSelect(id);
      } else if (e.shiftKey && selectedId) {
        selectRange(selectedId, id);
      } else {
        setSelectedId(id);
      }
    },
    [selectedId, setSelectedId, toggleMultiSelect, selectRange],
  );

  return (
    <div
      ref={parentRef}
      style={{
        flex: 1,
        overflow: "auto",
        padding: GAP,
      }}
    >
      <div
        style={{
          height: virtualizer.getTotalSize(),
          width: "100%",
          position: "relative",
        }}
      >
        {virtualizer.getVirtualItems().map((virtualRow) => {
          const startIdx = virtualRow.index * COLUMN_COUNT;
          const rowImages = images.slice(startIdx, startIdx + COLUMN_COUNT);

          // Check if this row starts at a scene boundary
          const hasSceneBreak = sceneBreaks.has(startIdx);
          const sceneId = hasSceneBreak
            ? analysisMap[images[startIdx]?.id]?.sceneGroupId
            : null;

          return (
            <div
              key={virtualRow.key}
              style={{
                position: "absolute",
                top: 0,
                left: 0,
                width: "100%",
                transform: `translateY(${virtualRow.start}px)`,
              }}
            >
              {hasSceneBreak && sceneId && (
                <div
                  style={{
                    height: 20,
                    display: "flex",
                    alignItems: "center",
                    gap: 8,
                    paddingLeft: 4,
                    marginBottom: 2,
                  }}
                >
                  <div
                    style={{
                      flex: 1,
                      height: 1,
                      background: "var(--border)",
                    }}
                  />
                  <span
                    style={{
                      fontSize: 10,
                      color: "var(--text-muted)",
                      whiteSpace: "nowrap",
                    }}
                  >
                    Scene {sceneId.replace(/\D/g, "")}
                  </span>
                  <div
                    style={{
                      flex: 1,
                      height: 1,
                      background: "var(--border)",
                    }}
                  />
                </div>
              )}
              <div
                style={{
                  height: hasSceneBreak ? ROW_HEIGHT - 22 : ROW_HEIGHT,
                  display: "grid",
                  gridTemplateColumns: `repeat(${COLUMN_COUNT}, 1fr)`,
                  gap: GAP,
                }}
              >
              {rowImages.map((img) => {
                const mark: Mark = marks[img.id] || "unmarked";
                const isSelected =
                  selectedId === img.id || multiSelection.has(img.id);
                const analysis = analysisMap[img.id];

                return (
                  <div
                    key={img.id}
                    onClick={(e) => handleClick(img.id, e)}
                    style={{
                      position: "relative",
                      borderRadius: "var(--radius-sm)",
                      overflow: "hidden",
                      cursor: "pointer",
                      outline: isSelected
                        ? "2px solid var(--accent)"
                        : "2px solid transparent",
                      opacity: mark === "delete" ? 0.4 : 1,
                      transition: "outline 0.1s, opacity 0.15s",
                    }}
                  >
                    {img.thumbnailPath ? (
                      <img
                        src={convertFileSrc(img.thumbnailPath)}
                        alt={img.fileName}
                        loading="lazy"
                        style={{
                          width: "100%",
                          height: "100%",
                          objectFit: "cover",
                          display: "block",
                        }}
                      />
                    ) : (
                      <div
                        style={{
                          width: "100%",
                          height: "100%",
                          background: "var(--bg-secondary)",
                          display: "flex",
                          alignItems: "center",
                          justifyContent: "center",
                          color: "var(--text-muted)",
                          fontSize: 12,
                        }}
                      >
                        {img.fileName}
                      </div>
                    )}

                    {/* Mark badge */}
                    {mark !== "unmarked" && (
                      <div
                        style={{
                          position: "absolute",
                          top: 4,
                          left: 4,
                          padding: "2px 6px",
                          borderRadius: "var(--radius-sm)",
                          fontSize: 10,
                          fontWeight: 700,
                          background:
                            mark === "keep"
                              ? "var(--success)"
                              : "var(--danger)",
                          color: "#000",
                        }}
                      >
                        {mark === "keep" ? "KEEP" : "DEL"}
                      </div>
                    )}

                    {/* Analysis badges */}
                    <div
                      style={{
                        position: "absolute",
                        top: 4,
                        right: 4,
                        display: "flex",
                        gap: 2,
                        flexDirection: "column",
                      }}
                    >
                      {analysis?.subjectFocus?.verdict === "BackFocus" ? (
                        <span
                          style={{
                            padding: "1px 4px",
                            borderRadius: 2,
                            fontSize: 9,
                            fontWeight: 700,
                            background: "var(--danger)",
                            color: "#fff",
                          }}
                        >
                          BACKFOCUS
                        </span>
                      ) : (
                        // Don't show BLUR if subject is sharp (intentional bokeh)
                        analysis?.subjectFocus?.verdict !== "SubjectSharp" &&
                        isBlurry(analysis?.blur, settings) && (
                          <span
                            style={{
                              padding: "1px 4px",
                              borderRadius: 2,
                              fontSize: 9,
                              fontWeight: 700,
                              background: "var(--warning)",
                              color: "#000",
                            }}
                          >
                            BLUR
                          </span>
                        )
                      )}
                      {hasExposureIssue(analysis?.exposure, settings) && (
                          <span
                            style={{
                              padding: "1px 4px",
                              borderRadius: 2,
                              fontSize: 9,
                              fontWeight: 700,
                              background: "var(--warning)",
                              color: "#000",
                            }}
                          >
                            {exposureVerdict(analysis?.exposure, settings) === "Overexposed"
                              ? "OVER"
                              : exposureVerdict(analysis?.exposure, settings) === "Underexposed"
                                ? "UNDER"
                                : "HICON"}
                          </span>
                        )}
                      {analysis?.closedEyes?.hasClosedEyes && (
                        <span
                          style={{
                            padding: "1px 4px",
                            borderRadius: 2,
                            fontSize: 9,
                            fontWeight: 700,
                            background: "#fb923c",
                            color: "#000",
                          }}
                        >
                          EYES
                        </span>
                      )}
                      {analysis?.duplicateGroupId && (
                        <span
                          style={{
                            padding: "1px 4px",
                            borderRadius: 2,
                            fontSize: 9,
                            fontWeight: 700,
                            background: dupGroupColor(analysis.duplicateGroupId),
                            color: "#000",
                          }}
                        >
                          DUP {dupGroupLabel(analysis.duplicateGroupId)}
                        </span>
                      )}
                    </div>
                  </div>
                );
              })}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
