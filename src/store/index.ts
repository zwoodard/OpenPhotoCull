import { create } from "zustand";
import type {
  ImageEntry,
  AnalysisResults,
  AnalysisSettings,
  Mark,
  AppPhase,
  ScanProgress,
} from "./types";

// ── Threshold helpers ──

function hasStrongSharpRegion(blur: NonNullable<AnalysisResults["blur"]>): boolean {
  // Mirrors the backend rule in compute_blur:
  //  - either a strong peak tile (clearly in-focus content), or
  //  - meaningful sharp area (fraction AND cluster together).
  // Bokeh-likely shots use relaxed thresholds.
  const peakThresh = blur.bokehLikely ? 700 : 900;
  const fracThresh = blur.bokehLikely ? 0.04 : 0.12;
  const clusterThresh = blur.bokehLikely ? 0.025 : 0.06;
  if (blur.maxTileVariance >= peakThresh) return true;
  return (
    blur.sharpTileFraction >= fracThresh &&
    blur.largestSharpCluster >= clusterThresh
  );
}

export function isBlurry(
  blur: AnalysisResults["blur"],
  subjectFocus: AnalysisResults["subjectFocus"],
  settings: AnalysisSettings,
): boolean {
  if (!blur) return false;
  // Subject-focus (Vision face or saliency) gives a semantic ground truth
  // when available. If Vision found a subject and reports BOTH subject and
  // background are below the sharpness floor, the photo is blurry — even
  // if a few scattered sharp tiles happen to clear the tile-cluster gate.
  // Conversely, an explicit SubjectSharp verdict beats the tile heuristic.
  if (subjectFocus) {
    if (subjectFocus.verdict === "AllBlurry") return true;
    if (subjectFocus.verdict === "SubjectBlurry") return true;
    if (subjectFocus.verdict === "SubjectSharp") return false;
    // BackFocus falls through — has its own filter; tile rule decides whether
    // it also counts as "blurry" for the headline filter.
  }
  // If the photo has clearly-in-focus content somewhere, it isn't "blurry"
  // regardless of how soft the rest of the frame is. This is what saves
  // shallow-DOF shots (dogs, portraits, products) where the global Laplacian
  // is dragged down by intentional background blur.
  if (hasStrongSharpRegion(blur)) return false;
  // No strong sharp region — fall back to global threshold check.
  const globalScore = blur.meanTileVariance || blur.laplacianVariance;
  return globalScore < settings.blurThreshold;
}

export function blurIntent(
  blur: AnalysisResults["blur"],
  subjectFocus: AnalysisResults["subjectFocus"],
): "Sharp" | "IntentionalBokeh" | "ShakeBlur" | "OutOfFocus" | "Unknown" {
  if (!blur) return "Unknown";
  // Trust definitive subject-focus verdicts over tile heuristics.
  if (subjectFocus) {
    if (subjectFocus.verdict === "AllBlurry") {
      return blur.shakeRisk ? "ShakeBlur" : "OutOfFocus";
    }
    if (subjectFocus.verdict === "SubjectBlurry") {
      return blur.shakeRisk ? "ShakeBlur" : "OutOfFocus";
    }
    if (subjectFocus.verdict === "SubjectSharp") {
      return blur.bokehLikely || blur.meanTileVariance < 100
        ? "IntentionalBokeh"
        : "Sharp";
    }
  }
  if (hasStrongSharpRegion(blur)) {
    if (blur.meanTileVariance >= 100 && !blur.bokehLikely) return "Sharp";
    return "IntentionalBokeh";
  }
  return blur.shakeRisk ? "ShakeBlur" : "OutOfFocus";
}

export function hasExposureIssue(
  exposure: AnalysisResults["exposure"],
  settings: AnalysisSettings,
): boolean {
  if (!exposure) return false;
  return (
    exposure.pctUnderexposed > settings.exposureThreshold ||
    exposure.pctOverexposed > settings.exposureThreshold
  );
}

export function exposureVerdict(
  exposure: AnalysisResults["exposure"],
  settings: AnalysisSettings,
): string {
  if (!exposure) return "Normal";
  const under = exposure.pctUnderexposed > settings.exposureThreshold;
  const over = exposure.pctOverexposed > settings.exposureThreshold;
  if (under && over) return "HighContrast";
  if (under) return "Underexposed";
  if (over) return "Overexposed";
  return "Normal";
}

const DEFAULT_SETTINGS: AnalysisSettings = {
  blurThreshold: 100,
  exposureThreshold: 0.3,
  duplicateThreshold: 10,
  sceneWindowSecs: 60,
};

interface AppState {
  phase: AppPhase;
  setPhase: (phase: AppPhase) => void;

  folderPath: string | null;
  images: ImageEntry[];
  scanProgress: ScanProgress;
  setFolderPath: (path: string) => void;
  setImages: (images: ImageEntry[]) => void;
  updateScanProgress: (progress: ScanProgress) => void;

  analysisMap: Record<string, AnalysisResults>;
  duplicateGroups: Record<string, string[]>;
  sceneGroups: Record<string, string[]>;
  personGroups: Record<string, Array<{ imageId: string; faceIndex: number }>>;
  setAnalysis: (imageId: string, results: AnalysisResults) => void;
  setBulkAnalysis: (map: Record<string, AnalysisResults>) => void;
  setDuplicateGroups: (groups: Record<string, string[]>) => void;
  setSceneGroups: (groups: Record<string, string[]>) => void;
  setPersonGroups: (groups: Record<string, Array<{ imageId: string; faceIndex: number }>>) => void;

  marks: Record<string, Mark>;
  selectedId: string | null;
  multiSelection: Set<string>;
  comparisonMode: boolean;
  setMark: (imageId: string, mark: Mark) => void;
  bulkMark: (imageIds: string[], mark: Mark) => void;
  setSelectedId: (id: string | null) => void;
  toggleMultiSelect: (id: string) => void;
  clearMultiSelection: () => void;
  selectRange: (fromId: string, toId: string) => void;
  selectAll: () => void;
  invertSelection: () => void;
  setComparisonMode: (mode: boolean) => void;

  settings: AnalysisSettings;
  setSetting: <K extends keyof AnalysisSettings>(
    key: K,
    value: AnalysisSettings[K],
  ) => void;

  filters: {
    showBlurry: boolean | null;
    showExposureIssues: boolean | null;
    showClosedEyes: boolean | null;
    showBackFocus: boolean | null;
    filterByPersonId: string | null;
    showDuplicatesOnly: boolean;
    sortBy: "date" | "name" | "blurScore" | "exposure";
  };
  setFilter: (key: string, value: unknown) => void;

  filteredImages: () => ImageEntry[];
  /** Returns indices in filteredImages() where a new scene starts */
  sceneBreaks: () => Set<number>;
}

export const useStore = create<AppState>((set, get) => ({
  phase: "picker",
  setPhase: (phase) => set({ phase }),

  folderPath: null,
  images: [],
  scanProgress: {
    phase: "",
    current: 0,
    total: 0,
    elapsedMs: 0,
    currentFile: null,
    stepTimings: null,
  },
  setFolderPath: (path) => set({ folderPath: path }),
  setImages: (images) => set({ images }),
  updateScanProgress: (progress) => set({ scanProgress: progress }),

  analysisMap: {},
  duplicateGroups: {},
  sceneGroups: {},
  personGroups: {},
  setAnalysis: (imageId, results) =>
    set((state) => ({
      analysisMap: { ...state.analysisMap, [imageId]: results },
    })),
  setBulkAnalysis: (map) =>
    set((state) => ({
      analysisMap: { ...state.analysisMap, ...map },
    })),
  setDuplicateGroups: (groups) => set({ duplicateGroups: groups }),
  setSceneGroups: (groups) => set({ sceneGroups: groups }),
  setPersonGroups: (groups) => set({ personGroups: groups }),

  marks: {},
  selectedId: null,
  multiSelection: new Set(),
  comparisonMode: false,
  setMark: (imageId, mark) => {
    set((state) => ({ marks: { ...state.marks, [imageId]: mark } }));
    import("../lib/tauri").then((t) => t.setMark(imageId, mark));
  },
  bulkMark: (imageIds, mark) => {
    set((state) => {
      const next = { ...state.marks };
      for (const id of imageIds) next[id] = mark;
      return { marks: next };
    });
    import("../lib/tauri").then((t) => t.bulkSetMark(imageIds, mark));
  },
  setSelectedId: (id) => set({ selectedId: id }),
  toggleMultiSelect: (id) =>
    set((state) => {
      const next = new Set(state.multiSelection);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return { multiSelection: next };
    }),
  clearMultiSelection: () => set({ multiSelection: new Set() }),
  selectRange: (fromId, toId) =>
    set((state) => {
      const images = state.filteredImages();
      const fromIdx = images.findIndex((i) => i.id === fromId);
      const toIdx = images.findIndex((i) => i.id === toId);
      if (fromIdx === -1 || toIdx === -1) return {};
      const start = Math.min(fromIdx, toIdx);
      const end = Math.max(fromIdx, toIdx);
      const next = new Set(state.multiSelection);
      for (let i = start; i <= end; i++) next.add(images[i].id);
      return { multiSelection: next };
    }),
  selectAll: () =>
    set((state) => {
      const images = state.filteredImages();
      return { multiSelection: new Set(images.map((i) => i.id)) };
    }),
  invertSelection: () =>
    set((state) => {
      const images = state.filteredImages();
      const next = new Set<string>();
      for (const img of images) {
        if (!state.multiSelection.has(img.id)) next.add(img.id);
      }
      return { multiSelection: next };
    }),
  setComparisonMode: (mode) => set({ comparisonMode: mode }),

  settings: { ...DEFAULT_SETTINGS },
  setSetting: (key, value) =>
    set((state) => ({ settings: { ...state.settings, [key]: value } })),

  filters: {
    showBlurry: null,
    showExposureIssues: null,
    showClosedEyes: null,
    showBackFocus: null,
    filterByPersonId: null,
    showDuplicatesOnly: false,
    sortBy: "date",
  },
  setFilter: (key, value) =>
    set((state) => ({ filters: { ...state.filters, [key]: value } })),

  filteredImages: () => {
    const state = get();
    const s = state.settings;
    let imgs = [...state.images];

    if (state.filters.showBlurry === true) {
      imgs = imgs.filter((i) => {
        const a = state.analysisMap[i.id];
        return isBlurry(a?.blur, a?.subjectFocus, s);
      });
    } else if (state.filters.showBlurry === false) {
      imgs = imgs.filter((i) => {
        const a = state.analysisMap[i.id];
        return !isBlurry(a?.blur, a?.subjectFocus, s);
      });
    }

    if (state.filters.showExposureIssues === true) {
      imgs = imgs.filter((i) =>
        hasExposureIssue(state.analysisMap[i.id]?.exposure, s),
      );
    }

    if (state.filters.showClosedEyes === true) {
      imgs = imgs.filter(
        (i) => state.analysisMap[i.id]?.closedEyes?.hasClosedEyes === true,
      );
    }

    if (state.filters.showBackFocus === true) {
      imgs = imgs.filter(
        (i) => state.analysisMap[i.id]?.subjectFocus?.verdict === "BackFocus",
      );
    }

    if (state.filters.filterByPersonId) {
      const pid = state.filters.filterByPersonId;
      imgs = imgs.filter((i) =>
        state.analysisMap[i.id]?.faces?.some((f) => f.personId === pid),
      );
    }

    if (state.filters.showDuplicatesOnly) {
      imgs = imgs.filter(
        (i) => state.analysisMap[i.id]?.duplicateGroupId != null,
      );
    }

    // Primary sort
    const sortBy = state.filters.sortBy;
    imgs.sort((a, b) => {
      if (sortBy === "date") return (b.modifiedAt || 0) - (a.modifiedAt || 0);
      if (sortBy === "name") return a.fileName.localeCompare(b.fileName);
      if (sortBy === "blurScore") {
        const av =
          state.analysisMap[a.id]?.blur?.laplacianVariance ?? Infinity;
        const bv =
          state.analysisMap[b.id]?.blur?.laplacianVariance ?? Infinity;
        return av - bv;
      }
      if (sortBy === "exposure") {
        const av = state.analysisMap[a.id]?.exposure?.meanLuminance ?? 0.5;
        const bv = state.analysisMap[b.id]?.exposure?.meanLuminance ?? 0.5;
        return Math.abs(av - 0.5) - Math.abs(bv - 0.5);
      }
      return 0;
    });

    // Group duplicates together
    const am = state.analysisMap;
    const hasDupes = imgs.some((i) => am[i.id]?.duplicateGroupId != null);
    if (hasDupes) {
      const groupFirstIndex = new Map<string, number>();
      for (let i = 0; i < imgs.length; i++) {
        const gid = am[imgs[i].id]?.duplicateGroupId;
        if (gid && !groupFirstIndex.has(gid)) {
          groupFirstIndex.set(gid, i);
        }
      }

      const indexed = imgs.map((img, i) => ({ img, origIdx: i }));
      indexed.sort((a, b) => {
        const gidA = am[a.img.id]?.duplicateGroupId;
        const gidB = am[b.img.id]?.duplicateGroupId;
        const anchorA = gidA ? groupFirstIndex.get(gidA)! : a.origIdx;
        const anchorB = gidB ? groupFirstIndex.get(gidB)! : b.origIdx;
        if (anchorA !== anchorB) return anchorA - anchorB;
        if (gidA && !gidB) return -1;
        if (!gidA && gidB) return 1;
        return a.origIdx - b.origIdx;
      });

      imgs = indexed.map((x) => x.img);
    }

    return imgs;
  },

  sceneBreaks: () => {
    const state = get();
    const imgs = state.filteredImages();
    const am = state.analysisMap;
    const breaks = new Set<number>();

    for (let i = 1; i < imgs.length; i++) {
      const prevScene = am[imgs[i - 1].id]?.sceneGroupId;
      const currScene = am[imgs[i].id]?.sceneGroupId;
      // Break when scene changes (including from a scene to no-scene or vice versa)
      if (currScene !== prevScene) {
        breaks.add(i);
      }
    }
    return breaks;
  },
}));
