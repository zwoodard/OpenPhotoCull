import { invoke, Channel } from "@tauri-apps/api/core";
import type { ImageEntry, AnalysisResults, ScanProgress } from "../store/types";

export async function scanFolder(
  path: string,
  onProgress: (progress: ScanProgress) => void,
): Promise<ImageEntry[]> {
  const channel = new Channel<ScanProgress>();
  channel.onmessage = onProgress;
  return invoke<ImageEntry[]>("scan_folder", { path, onProgress: channel });
}

export async function runAnalysis(
  onProgress: (progress: ScanProgress) => void,
): Promise<Record<string, AnalysisResults>> {
  const channel = new Channel<ScanProgress>();
  channel.onmessage = onProgress;
  return invoke<Record<string, AnalysisResults>>("run_analysis", {
    onProgress: channel,
  });
}

export async function getDuplicateGroups(): Promise<
  Record<string, string[]>
> {
  return invoke<Record<string, string[]>>("get_duplicate_groups");
}

export async function getSceneGroups(): Promise<Record<string, string[]>> {
  return invoke<Record<string, string[]>>("get_scene_groups");
}

export async function getPersonGroups(): Promise<
  Record<string, Array<{ imageId: string; faceIndex: number }>>
> {
  return invoke("get_person_groups");
}

export async function setMark(
  imageId: string,
  mark: string,
): Promise<void> {
  return invoke("set_mark", { imageId, mark });
}

export async function bulkSetMark(
  imageIds: string[],
  mark: string,
): Promise<void> {
  return invoke("bulk_set_mark", { imageIds, mark });
}

export async function executeDeletes(): Promise<{
  deleted: number;
  errors: string[];
}> {
  return invoke("execute_deletes");
}

export async function getFullImagePath(imageId: string): Promise<string> {
  return invoke<string>("get_full_image_path", { imageId });
}
