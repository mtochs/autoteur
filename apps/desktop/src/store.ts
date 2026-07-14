import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import type {
  ActivityItem,
  Delta,
  JobUpdate,
  ProjectLint,
  ProjectState,
  SyncEvent,
  TakeMedia,
} from "./types";

export type Room =
  | "writers"
  | "beats"
  | "casting"
  | "world"
  | "shots"
  | "dailies"
  | "editing"
  | "screening"
  | "settings";

interface AppStore {
  root: string | null;
  state: ProjectState | null;
  room: Room;
  activity: ActivityItem[];
  jobs: Record<number, JobUpdate>;
  media: Record<string, TakeMedia[]>;
  validation: ProjectLint[];
  recent: Record<string, number>; // entity key -> timestamp, for glide-in
  renderStatus: { phase: string; message: string } | null;
  setRoom: (room: Room) => void;
  attach: (root: string, state: ProjectState) => void;
  refresh: () => Promise<void>;
  applyEvent: (event: SyncEvent) => void;
}

function describe(delta: Delta): string | null {
  switch (delta.type) {
    case "BeatAdded":
      return `New beat “${delta.beat?.title ?? delta.beat?.id}” on the board`;
    case "BeatUpdated":
      return `Beat “${delta.beat?.title ?? delta.beat?.id}” updated`;
    case "BeatMoved":
      return `Beat “${delta.id}” reordered`;
    case "BeatRemoved":
      return `Beat “${delta.id}” removed`;
    case "SceneAdded":
      return `Scene “${delta.slug}” created`;
    case "SceneUpdated":
      return `Scene “${delta.slug}” updated`;
    case "SceneRemoved":
      return `Scene “${delta.slug}” removed`;
    case "ShotAdded":
      return `New shot ${delta.scene}/${delta.shot?.id}`;
    case "ShotUpdated":
      return `Shot ${delta.scene}/${delta.shot?.id} updated`;
    case "ShotMoved":
      return `Shot ${delta.scene}/${delta.id} reordered`;
    case "SelectedTakeChanged":
      return delta.take
        ? `Take circled on ${delta.scene}/${delta.id}`
        : `Selection cleared on ${delta.scene}/${delta.id}`;
    case "TakesAdded": {
      const takes = delta.takes ?? [];
      if (takes.length === 1) return `New take for ${takes[0]?.shot}`;
      return `${takes.length} new takes arrived`;
    }
    case "CharacterChanged":
      return `Cast member “${delta.slug}” updated`;
    case "WorldChanged":
      return `“${delta.slug}” updated in Locations & Props`;
    case "StoryDocChanged":
      return delta.doc === "treatment" ? "Treatment updated" : "Logline updated";
    case "TimelineChanged":
      return "The cut changed";
    case "FileProblem":
      return `⚠ ${String(delta.path).split(/[\\/]/).slice(-2).join("/")}: ${delta.message}`;
    case "FileProblemCleared":
      return `Fixed: ${String(delta.path).split(/[\\/]/).pop()}`;
    default:
      return null;
  }
}

function recentKeys(delta: Delta): string[] {
  switch (delta.type) {
    case "BeatAdded":
    case "BeatUpdated":
      return [`beat:${delta.beat?.id}`];
    case "BeatMoved":
      return [`beat:${delta.id}`];
    case "ShotAdded":
    case "ShotUpdated":
      return [`shot:${delta.scene}/${delta.shot?.id}`];
    case "ShotMoved":
    case "SelectedTakeChanged":
      return [`shot:${delta.scene}/${delta.id}`];
    case "TakesAdded":
      return (delta.takes ?? []).map((t: { id: string }) => `take:${t.id}`);
    default:
      return [];
  }
}

let refreshTimer: ReturnType<typeof setTimeout> | null = null;

export const useStore = create<AppStore>((set, get) => ({
  root: null,
  state: null,
  room: "writers",
  activity: [],
  jobs: {},
  media: {},
  validation: [],
  recent: {},
  renderStatus: null,
  setRoom: (room) => set({ room }),

  attach: (root, state) => {
    set({ root, state, activity: [], jobs: {}, recent: {} });
    void get().refresh();
  },

  refresh: async () => {
    try {
      const [snapshot, media, validation] = await Promise.all([
        api.getSnapshot(),
        api.takeMedia(),
        api.getValidation(),
      ]);
      set({ root: snapshot.root, state: snapshot.state, media, validation });
    } catch {
      // no project open
    }
  },

  applyEvent: (event) => {
    const now = Date.now();
    const store = get();
    // Deltas are hints for feel; the snapshot refresh is truth.
    if (event.origin === "external") {
      const items = event.deltas
        .map(describe)
        .filter((t): t is string => !!t)
        .map((text) => ({ at: now, text }));
      const recent = { ...store.recent };
      for (const delta of event.deltas) {
        for (const key of recentKeys(delta)) recent[key] = now;
      }
      set({
        activity: [...items.reverse(), ...store.activity].slice(0, 120),
        recent,
      });
    }
    if (refreshTimer) clearTimeout(refreshTimer);
    refreshTimer = setTimeout(() => void get().refresh(), 120);
  },
}));

/** True (once) if this entity changed externally in the last few seconds. */
export function useGlide(key: string): boolean {
  const at = useStore((s) => s.recent[key]);
  return !!at && Date.now() - at < 4000;
}

export async function bootstrapListeners() {
  await listen<SyncEvent>("project-delta", (event) => {
    useStore.getState().applyEvent(event.payload);
  });
  await listen<JobUpdate>("generation-update", (event) => {
    const update = event.payload;
    useStore.setState((s) => ({ jobs: { ...s.jobs, [update.job]: update } }));
    if (update.stage.stage === "done") {
      setTimeout(() => void useStore.getState().refresh(), 200);
    }
  });
  await listen<{ phase: string; message: string }>("render-status", (event) => {
    useStore.setState({ renderStatus: event.payload });
  });
}
