export interface ExifMetadata {
  dateTimeOriginal: string | null;
  gpsLat: number | null;
  gpsLng: number | null;
  cameraMake: string | null;
  cameraModel: string | null;
  focalLengthMm: number | null;
  aperture: number | null;
  iso: number | null;
  shutterSpeed: string | null;
}

export interface ImageEntry {
  id: string;
  path: string;
  fileName: string;
  fileSize: number;
  modifiedAt: number;
  width: number | null;
  height: number | null;
  thumbnailPath: string | null;
  exif: ExifMetadata | null;
}

export interface BlurResult {
  laplacianVariance: number;
  isBlurry: boolean;
}

export interface ExposureResult {
  meanLuminance: number;
  pctUnderexposed: number;
  pctOverexposed: number;
  verdict: "Normal" | "Underexposed" | "Overexposed" | "HighContrast";
}

export interface FaceEyeResult {
  leftEyeOpen: number;
  rightEyeOpen: number;
  eyesClosed: boolean;
  boundingBox: [number, number, number, number] | null;
}

export interface ClosedEyesResult {
  faceCount: number;
  faces: FaceEyeResult[];
  hasClosedEyes: boolean;
}

export interface SubjectFocusResult {
  subjectBlurVariance: number;
  backgroundBlurVariance: number;
  focusRatio: number;
  verdict: "SubjectSharp" | "SubjectBlurry" | "BackFocus" | "AllBlurry";
}

export interface FaceInfo {
  faceIndex: number;
  boundingBox: [number, number, number, number];
  personId: string | null;
  faceThumbnailPath: string | null;
}

export interface AnalysisResults {
  blur: BlurResult | null;
  exposure: ExposureResult | null;
  duplicateGroupId: string | null;
  sceneGroupId: string | null;
  closedEyes: ClosedEyesResult | null;
  subjectFocus: SubjectFocusResult | null;
  faces: FaceInfo[] | null;
}

export interface AnalysisSettings {
  blurThreshold: number;
  exposureThreshold: number;
  duplicateThreshold: number;
  sceneWindowSecs: number;
}

export type Mark = "keep" | "delete" | "unmarked";

export type AppPhase = "picker" | "scanning" | "analyzing" | "review";

export interface ScanProgress {
  phase: string;
  current: number;
  total: number;
  elapsedMs: number;
  currentFile: string | null;
  stepTimings: Record<string, number> | null;
}
