import { convertFileSrc } from "@tauri-apps/api/core";
import { useStore } from "../store";

export function PersonFilter() {
  const personGroups = useStore((s) => s.personGroups);
  const analysisMap = useStore((s) => s.analysisMap);
  const filters = useStore((s) => s.filters);
  const setFilter = useStore((s) => s.setFilter);

  const personIds = Object.keys(personGroups);
  if (personIds.length === 0) return null;

  // Build person summaries with best face thumbnail
  const persons = personIds.map((pid) => {
    const members = personGroups[pid];
    const imageCount = new Set(members.map((m) => m.imageId)).size;

    // Find the best face thumbnail for this person
    let thumbPath: string | null = null;
    for (const m of members) {
      const faces = analysisMap[m.imageId]?.faces;
      if (faces) {
        const face = faces.find(
          (f) => f.faceIndex === m.faceIndex && f.personId === pid,
        );
        if (face?.faceThumbnailPath) {
          thumbPath = face.faceThumbnailPath;
          break;
        }
      }
    }

    return { pid, imageCount, thumbPath };
  });

  // Sort by image count descending
  persons.sort((a, b) => b.imageCount - a.imageCount);

  const active = filters.filterByPersonId;

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 6,
        padding: "6px 12px",
        borderBottom: "1px solid var(--border)",
        background: "var(--bg-secondary)",
        overflowX: "auto",
      }}
    >
      <span
        style={{
          fontSize: 11,
          color: "var(--text-muted)",
          whiteSpace: "nowrap",
          marginRight: 4,
        }}
      >
        People:
      </span>

      {active && (
        <button
          onClick={() => setFilter("filterByPersonId", null)}
          style={{
            padding: "3px 8px",
            borderRadius: "var(--radius-sm)",
            fontSize: 10,
            fontWeight: 600,
            background: "var(--bg-surface)",
            color: "var(--text-secondary)",
            border: "1px solid var(--border)",
            cursor: "pointer",
            whiteSpace: "nowrap",
          }}
        >
          Show All
        </button>
      )}

      {persons.map(({ pid, imageCount, thumbPath }) => {
        const isActive = active === pid;
        const num = pid.replace(/\D/g, "");
        return (
          <button
            key={pid}
            onClick={() =>
              setFilter("filterByPersonId", isActive ? null : pid)
            }
            style={{
              display: "flex",
              alignItems: "center",
              gap: 4,
              padding: "3px 8px 3px 3px",
              borderRadius: 16,
              fontSize: 11,
              fontWeight: 600,
              background: isActive ? "var(--accent)" : "var(--bg-surface)",
              color: isActive ? "#fff" : "var(--text-secondary)",
              border: `1px solid ${isActive ? "var(--accent)" : "var(--border)"}`,
              cursor: "pointer",
              whiteSpace: "nowrap",
            }}
          >
            {thumbPath ? (
              <img
                src={convertFileSrc(thumbPath)}
                alt={`Person ${num}`}
                style={{
                  width: 22,
                  height: 22,
                  borderRadius: "50%",
                  objectFit: "cover",
                }}
              />
            ) : (
              <div
                style={{
                  width: 22,
                  height: 22,
                  borderRadius: "50%",
                  background: "var(--border)",
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  fontSize: 10,
                  color: "var(--text-muted)",
                }}
              >
                {num}
              </div>
            )}
            <span>{imageCount}</span>
          </button>
        );
      })}
    </div>
  );
}
