// Pragmatic mirrors of autoteur-core's serialized state.

export interface Beat {
  id: string;
  title: string;
  summary?: string;
  episode?: string;
  act?: number;
  color?: string;
  notes?: string;
}
export interface Episode {
  id: string;
  title: string;
  color?: string;
}
export interface DialogueCue {
  character: string;
  line: string;
  delivery?: string;
}
export interface Shot {
  id: string;
  framing?: string;
  camera?: string;
  action?: string;
  characters?: string[];
  world?: string[];
  dialogue?: DialogueCue[];
  duration_s?: number;
  status: string;
  selected_take?: string;
  prompt?: string;
  prompt_extra?: string;
  negative_extra?: string;
  notes?: string;
}
export interface SceneFile {
  title: string;
  beats?: string[];
  characters?: string[];
  location?: string;
  world?: string[];
  int_ext?: string;
  time?: string;
  mood?: string;
  synopsis?: string;
  director_notes?: string;
}
export interface FileEntry<T> {
  path: string;
  data: T;
  lints: { severity: string; message: string }[];
}
export interface SceneEntry {
  number: number;
  slug: string;
  dir: string;
  scene?: FileEntry<SceneFile> | null;
  shots?: FileEntry<{ shots: Shot[] }> | null;
}
export interface CharacterFile {
  name: string;
  aliases?: string[];
  description?: string;
  voice?: { provider?: string; voice_id?: string; style?: string };
  prompt?: { fragment?: string; negative?: string; variants?: Record<string, string> };
  visual?: { reference_images?: string[]; adapters?: unknown[] };
}
export interface WorldFile {
  name: string;
  kind: string;
  description?: string;
  prompt?: { fragment?: string; negative?: string };
  visual?: { reference_images?: string[] };
}
export interface TakeRecord {
  id: string;
  shot: string;
  provider: string;
  model: string;
  cost_usd?: number;
  created_at?: string;
  resolved_prompt?: string;
  outputs: { hash: string; kind?: string; path?: string }[];
}
export interface ProjectState {
  manifest?: FileEntry<{
    title: string;
    format: string;
    defaults: { provider?: string; video_model?: string; image_model?: string; negative?: string };
  }> | null;
  beats?: FileEntry<{ episodes: Episode[]; beats: Beat[] }> | null;
  scenes: SceneEntry[];
  characters: Record<string, FileEntry<CharacterFile>>;
  world: Record<string, FileEntry<WorldFile>>;
  takes?: FileEntry<{ takes: TakeRecord[] }> | null;
  timeline?: FileEntry<{
    entries: { shot: string; in_s?: number; out_s?: number }[];
    sequences: { episode: string; entries: { shot: string; in_s?: number; out_s?: number }[] }[];
  }> | null;
}
export interface Snapshot {
  root: string;
  state: ProjectState;
}
export type Delta = { type: string } & Record<string, any>;
export interface SyncEvent {
  rev: number;
  origin: "startup" | "local" | "external";
  deltas: Delta[];
}
export interface JobUpdate {
  job: number;
  shot: string;
  stage: { stage: "queued" | "running" | "done" | "failed"; take?: string; message?: string; deduplicated?: boolean };
}
export interface TakeMedia {
  id: string;
  kind?: string;
  path?: string;
  exists: boolean;
  created_at?: string;
  model: string;
  provider: string;
  resolved_prompt?: string;
  cost_usd?: number;
}
export interface ResolvedPrompt {
  prompt: string;
  negative?: string;
  reference_images: { owner: string; path: string }[];
  adapters: { owner: string; adapter: Record<string, unknown> }[];
  warnings: string[];
}
export interface SavePoint {
  id: string;
  summary: string;
  seconds_since_epoch: number;
}
export interface ProviderStatus {
  id: string;
  name: string;
  connected: boolean;
}
export interface ModelInfo {
  slug: string;
  version?: string;
  kind: string;
  display_name: string;
  description?: string;
}
export interface ProjectLint {
  path: string;
  severity: string;
  message: string;
}
export interface ActivityItem {
  at: number;
  text: string;
}
