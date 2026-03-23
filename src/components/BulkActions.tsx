import { useState } from "react";
import { useStore, isBlurry } from "../store";
import { executeDeletes as execDeletes } from "../lib/tauri";

export function BulkActions() {
  const multiSelection = useStore((s) => s.multiSelection);
  const bulkMark = useStore((s) => s.bulkMark);
  const clearMultiSelection = useStore((s) => s.clearMultiSelection);
  const marks = useStore((s) => s.marks);
  const images = useStore((s) => s.images);
  const setImages = useStore((s) => s.setImages);
  const analysisMap = useStore((s) => s.analysisMap);
  const settings = useStore((s) => s.settings);
  const [confirming, setConfirming] = useState(false);
  const [deleting, setDeleting] = useState(false);

  const selectedIds = Array.from(multiSelection);
  const deleteMarkedIds = Object.entries(marks)
    .filter(([, m]) => m === "delete")
    .map(([id]) => id);

  const autoSuggest = () => {
    const toDelete: string[] = [];
    for (const img of images) {
      const a = analysisMap[img.id];
      if (isBlurry(a?.blur, settings)) toDelete.push(img.id);
    }
    if (toDelete.length > 0) bulkMark(toDelete, "delete");
  };

  const handleExecuteDeletes = async () => {
    if (!confirming) {
      setConfirming(true);
      return;
    }
    setDeleting(true);
    try {
      const result = await execDeletes();
      // Remove deleted images from store
      const deletedSet = new Set(deleteMarkedIds);
      setImages(images.filter((i) => !deletedSet.has(i.id)));
      setConfirming(false);
      if (result.errors.length > 0) {
        console.error("Delete errors:", result.errors);
      }
    } catch (err) {
      console.error("Delete failed:", err);
    } finally {
      setDeleting(false);
    }
  };

  const btnStyle = {
    padding: "6px 12px",
    borderRadius: "var(--radius-sm)",
    fontSize: 12,
    fontWeight: 600 as const,
    color: "var(--text-primary)",
  };

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 8,
        padding: "8px 12px",
        borderTop: "1px solid var(--border)",
        background: "var(--bg-secondary)",
      }}
    >
      {selectedIds.length > 0 && (
        <>
          <span style={{ fontSize: 12, color: "var(--text-secondary)" }}>
            {selectedIds.length} selected
          </span>
          <button
            onClick={() => bulkMark(selectedIds, "keep")}
            style={{ ...btnStyle, background: "var(--success)", color: "#000" }}
          >
            Keep Selected
          </button>
          <button
            onClick={() => bulkMark(selectedIds, "delete")}
            style={{ ...btnStyle, background: "var(--danger)", color: "#fff" }}
          >
            Delete Selected
          </button>
          <button
            onClick={clearMultiSelection}
            style={{ ...btnStyle, background: "var(--bg-surface)" }}
          >
            Clear Selection
          </button>
          <div style={{ width: 1, height: 20, background: "var(--border)" }} />
        </>
      )}

      <button
        onClick={autoSuggest}
        style={{ ...btnStyle, background: "var(--bg-surface)" }}
      >
        Auto-Suggest Deletions
      </button>

      <div style={{ flex: 1 }} />

      {deleteMarkedIds.length > 0 && (
        <button
          onClick={handleExecuteDeletes}
          disabled={deleting}
          style={{
            ...btnStyle,
            background: confirming ? "var(--danger)" : "var(--bg-surface)",
            color: confirming ? "#fff" : "var(--danger)",
            border: confirming ? "none" : "1px solid var(--danger)",
          }}
        >
          {deleting
            ? "Deleting..."
            : confirming
              ? `Confirm: Delete ${deleteMarkedIds.length} files`
              : `Delete ${deleteMarkedIds.length} marked files`}
        </button>
      )}
    </div>
  );
}
