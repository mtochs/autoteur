import { invoke } from "@tauri-apps/api/core";
import type {
  ModelInfo,
  ProjectLint,
  ProviderStatus,
  ResolvedPrompt,
  SavePoint,
  Snapshot,
  TakeMedia,
} from "./types";

type Patch = Record<string, unknown>;

export const api = {
  createProject: (path: string, title: string, series: boolean) =>
    invoke<Snapshot>("create_project", { path, title, series }),
  openProject: (path: string) => invoke<Snapshot>("open_project", { path }),
  getSnapshot: () => invoke<Snapshot>("get_snapshot"),
  getValidation: () => invoke<ProjectLint[]>("get_validation"),
  readStoryDoc: (doc: "logline" | "treatment") => invoke<string>("read_story_doc", { doc }),
  writeStoryDoc: (doc: "logline" | "treatment", content: string) =>
    invoke<void>("write_story_doc", { doc, content }),
  addBeat: (title: string, summary: string, episode?: string) =>
    invoke<string>("add_beat", { title, summary, episode: episode ?? null }),
  updateBeat: (id: string, patch: Patch) => invoke<void>("update_beat", { id, patch }),
  removeBeat: (id: string) => invoke<void>("remove_beat", { id }),
  moveBeat: (from: number, to: number) => invoke<void>("move_beat", { from, to }),
  createScene: (title: string) => invoke<string>("create_scene", { title }),
  createCharacter: (name: string) => invoke<string>("create_character", { name }),
  createWorld: (name: string, kind: string) => invoke<string>("create_world", { name, kind }),
  updateScene: (slug: string, patch: Patch) => invoke<void>("update_scene", { slug, patch }),
  updateEntity: (kind: "character" | "world", slug: string, patch: Patch) =>
    invoke<void>("update_entity", { kind, slug, patch }),
  addShot: (scene: string, patch: Patch) => invoke<string>("add_shot", { scene, patch }),
  updateShot: (scene: string, id: string, patch: Patch) =>
    invoke<void>("update_shot", { scene, id, patch }),
  moveShot: (scene: string, from: number, to: number) =>
    invoke<void>("move_shot", { scene, from, to }),
  circleTake: (scene: string, shot: string, take: string | null) =>
    invoke<void>("circle_take", { scene, shot, take }),
  setShotStatus: (scene: string, shot: string, status: string) =>
    invoke<void>("set_shot_status", { scene, shot, status }),
  resolveShotPrompt: (scene: string, shot: string) =>
    invoke<ResolvedPrompt>("resolve_shot_prompt", { scene, shot }),
  generateShots: (refs: string[], provider?: string, model?: string) =>
    invoke<number[]>("generate_shots", { refs, provider: provider ?? null, model: model ?? null }),
  savePoint: (message?: string) =>
    invoke<SavePoint[]>("save_point", { message: message ?? null }),
  history: (limit: number) => invoke<SavePoint[]>("history", { limit }),
  restoreSavePoint: (id: string) => invoke<void>("restore_save_point", { id }),
  keyStatus: () => invoke<ProviderStatus[]>("key_status"),
  keySet: (provider: string, key: string) => invoke<void>("key_set", { provider, key }),
  keyClear: (provider: string) => invoke<void>("key_clear", { provider }),
  recommendedModels: (provider?: string) =>
    invoke<ModelInfo[]>("recommended_models", { provider: provider ?? null }),
  setDefaults: (patch: Patch) => invoke<void>("set_defaults", { patch }),
  ffmpegStatus: () => invoke<string | null>("ffmpeg_status"),
  exportCut: (output: string) => invoke<void>("export_cut", { output }),
  setTrim: (shot: string, inS: number | null, outS: number | null) =>
    invoke<void>("set_trim", { shot, inS, outS }),
  takeMedia: () => invoke<Record<string, TakeMedia[]>>("take_media"),
};
