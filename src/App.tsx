import { useStore } from "./store";
import { FolderPicker } from "./components/FolderPicker";
import { ScanProgress } from "./components/ScanProgress";
import { ReviewLayout } from "./components/ReviewLayout";

export default function App() {
  const phase = useStore((s) => s.phase);

  return (
    <div style={{ height: "100%", width: "100%" }}>
      {phase === "picker" && <FolderPicker />}
      {(phase === "scanning" || phase === "analyzing") && <ScanProgress />}
      {phase === "review" && <ReviewLayout />}
    </div>
  );
}
