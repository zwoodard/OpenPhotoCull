import { open } from "@tauri-apps/plugin-dialog";
import { useStore } from "../store";
import { scanFolder } from "../lib/tauri";

export function FolderPicker() {
  const setFolderPath = useStore((s) => s.setFolderPath);
  const setImages = useStore((s) => s.setImages);
  const setPhase = useStore((s) => s.setPhase);
  const updateScanProgress = useStore((s) => s.updateScanProgress);
  const setBulkAnalysis = useStore((s) => s.setBulkAnalysis);
  const setDuplicateGroups = useStore((s) => s.setDuplicateGroups);
  const setSceneGroups = useStore((s) => s.setSceneGroups);
  const setPersonGroups = useStore((s) => s.setPersonGroups);

  const handlePickFolder = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (!selected) return;

    const path = typeof selected === "string" ? selected : selected;
    setFolderPath(path);
    setPhase("scanning");

    try {
      // Single IPC call returns everything — images, analysis, groups
      const result = await scanFolder(path, (progress) => {
        updateScanProgress(progress);
      });
      setImages(result.images);
      setBulkAnalysis(result.analysis);
      setDuplicateGroups(result.duplicateGroups);
      setSceneGroups(result.sceneGroups);
      setPersonGroups(result.personGroups);

      setPhase("review");
    } catch (err) {
      console.error("Scan failed:", err);
      setPhase("picker");
    }
  };

  return (
    <div
      style={{
        height: "100%",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: 24,
      }}
    >
      <h1 style={{ fontSize: 48, fontWeight: 700, letterSpacing: -1 }}>
        OpenPhotoCull
      </h1>
      <p style={{ color: "var(--text-secondary)", fontSize: 16, maxWidth: 400, textAlign: "center" }}>
        Select a folder to scan for photos. Analyze for blur,
        exposure issues, duplicates, and closed eyes.
      </p>
      <button
        onClick={handlePickFolder}
        style={{
          padding: "14px 36px",
          fontSize: 16,
          fontWeight: 600,
          background: "var(--accent)",
          color: "white",
          borderRadius: "var(--radius)",
          transition: "background 0.15s",
        }}
        onMouseEnter={(e) =>
          (e.currentTarget.style.background = "var(--accent-hover)")
        }
        onMouseLeave={(e) =>
          (e.currentTarget.style.background = "var(--accent)")
        }
      >
        Choose Folder
      </button>
    </div>
  );
}
