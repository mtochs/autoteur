import { useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { api } from "./api";
import { useStore, type Room } from "./store";
import type { SavePoint } from "./types";
import { WritersRoom, BeatBoard } from "./rooms/story";
import { Casting, WorldRoom, ShotList } from "./rooms/production";
import { Dailies, EditingRoom, ScreeningRoom, StudioSettings } from "./rooms/post";

const ROOMS: { id: Room; label: string; glyph: string }[] = [
  { id: "writers", label: "Writers' Room", glyph: "✎" },
  { id: "beats", label: "Beat Board", glyph: "▦" },
  { id: "casting", label: "Casting", glyph: "☺" },
  { id: "world", label: "Locations & Props", glyph: "⌂" },
  { id: "shots", label: "Shot List", glyph: "☰" },
  { id: "dailies", label: "Dailies", glyph: "▣" },
  { id: "editing", label: "Editing Room", glyph: "✂" },
  { id: "screening", label: "Screening Room", glyph: "▶" },
  { id: "settings", label: "Studio Settings", glyph: "⚙" },
];

export default function App() {
  const root = useStore((s) => s.root);
  return root ? <Shell /> : <Onboarding />;
}

function Onboarding() {
  const attach = useStore((s) => s.attach);
  const [title, setTitle] = useState("");
  const [series, setSeries] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const openExisting = async () => {
    const path = await openDialog({ directory: true, title: "Open an Autoteur project" });
    if (typeof path !== "string") return;
    try {
      const snapshot = await api.openProject(path);
      attach(snapshot.root, snapshot.state);
    } catch (e) {
      setError(String(e));
    }
  };

  const createNew = async () => {
    if (!title.trim()) return setError("Give your picture a title first.");
    const parent = await openDialog({ directory: true, title: "Where should the project live?" });
    if (typeof parent !== "string") return;
    setBusy(true);
    try {
      const slug = title.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || "untitled";
      const snapshot = await api.createProject(`${parent}/${slug}`, title.trim(), series);
      attach(snapshot.root, snapshot.state);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="h-full flex items-center justify-center">
      <div className="w-[440px] space-y-6">
        <div className="text-center space-y-2">
          <div className="font-display text-4xl tracking-wide" style={{ color: "var(--at-accent)" }}>
            Autoteur
          </div>
          <div className="text-sm" style={{ color: "var(--at-dim)" }}>
            The director's chair. Your agent is the crew.
          </div>
        </div>
        <div className="rounded-xl p-6 space-y-4" style={{ background: "var(--at-panel)", border: "1px solid var(--at-line)" }}>
          <div className="font-display text-lg">Start a new picture</div>
          <input
            className="at-input"
            placeholder="Working title…"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && void createNew()}
          />
          <label className="flex items-center gap-2 text-sm cursor-pointer" style={{ color: "var(--at-dim)" }}>
            <input type="checkbox" checked={series} onChange={(e) => setSeries(e.target.checked)} />
            It's a series (episodes)
          </label>
          <div className="flex gap-3">
            <button className="at-btn at-btn-primary flex-1" disabled={busy} onClick={() => void createNew()}>
              {busy ? "Rolling…" : "Create project"}
            </button>
            <button className="at-btn flex-1" onClick={() => void openExisting()}>
              Open existing…
            </button>
          </div>
          {error && <div className="text-sm text-red-400">{error}</div>}
        </div>
        <div className="text-xs text-center" style={{ color: "var(--at-dim)" }}>
          A project is a plain git repo of TOML + Markdown — point any coding agent at its AGENTS.md.
        </div>
      </div>
    </div>
  );
}

function Shell() {
  const room = useStore((s) => s.room);
  return (
    <div className="h-full flex">
      <Sidebar />
      <main className="flex-1 min-w-0 overflow-y-auto">
        {room === "writers" && <WritersRoom />}
        {room === "beats" && <BeatBoard />}
        {room === "casting" && <Casting />}
        {room === "world" && <WorldRoom />}
        {room === "shots" && <ShotList />}
        {room === "dailies" && <Dailies />}
        {room === "editing" && <EditingRoom />}
        {room === "screening" && <ScreeningRoom />}
        {room === "settings" && <StudioSettings />}
      </main>
      <ActivityRail />
    </div>
  );
}

function Sidebar() {
  const { room, setRoom, state, validation } = useStore();
  const title = state?.manifest?.data.title ?? "Untitled";
  const problems = validation.filter((v) => v.severity === "error").length;
  return (
    <aside
      className="w-56 shrink-0 flex flex-col border-r"
      style={{ background: "var(--at-panel)", borderColor: "var(--at-line)" }}
    >
      <div className="p-4 border-b" style={{ borderColor: "var(--at-line)" }}>
        <div className="font-display text-lg leading-tight truncate" title={title}>
          {title}
        </div>
        <div className="text-xs mt-0.5" style={{ color: "var(--at-dim)" }}>
          {state?.manifest?.data.format === "series" ? "Series" : "Feature"}
        </div>
      </div>
      <nav className="flex-1 py-2 overflow-y-auto">
        {ROOMS.map((r) => (
          <button
            key={r.id}
            onClick={() => setRoom(r.id)}
            className="w-full text-left px-4 py-2 flex items-center gap-3 text-[13px] transition-colors"
            style={{
              color: room === r.id ? "var(--at-accent)" : "var(--at-ink)",
              background: room === r.id ? "var(--at-accent-soft)" : "transparent",
            }}
          >
            <span className="w-4 text-center opacity-70">{r.glyph}</span>
            {r.label}
            {r.id === "settings" && problems > 0 && (
              <span className="ml-auto at-chip text-red-400 border-red-900">{problems}</span>
            )}
          </button>
        ))}
      </nav>
      <SavePointBox />
    </aside>
  );
}

function SavePointBox() {
  const [history, setHistory] = useState<SavePoint[]>([]);
  const [showHistory, setShowHistory] = useState(false);
  const [saving, setSaving] = useState(false);
  const refresh = useStore((s) => s.refresh);

  const save = async () => {
    setSaving(true);
    try {
      setHistory(await api.savePoint());
    } finally {
      setSaving(false);
    }
  };
  const toggleHistory = async () => {
    if (!showHistory) setHistory(await api.history(30));
    setShowHistory(!showHistory);
  };

  return (
    <div className="border-t p-3 space-y-2" style={{ borderColor: "var(--at-line)" }}>
      {showHistory && (
        <div className="max-h-52 overflow-y-auto space-y-1 pb-1">
          {history.map((p) => (
            <div key={p.id} className="group text-[11px] flex items-start gap-2" style={{ color: "var(--at-dim)" }}>
              <span className="truncate flex-1" title={p.summary}>
                {p.summary}
              </span>
              <button
                className="opacity-0 group-hover:opacity-100 shrink-0"
                style={{ color: "var(--at-accent)" }}
                title="Restore this save point (kept as new history)"
                onClick={() => void api.restoreSavePoint(p.id).then(() => refresh())}
              >
                ↩
              </button>
            </div>
          ))}
        </div>
      )}
      <div className="flex gap-2">
        <button className="at-btn at-btn-primary flex-1 text-xs" disabled={saving} onClick={() => void save()}>
          {saving ? "Saving…" : "● Save point"}
        </button>
        <button className="at-btn text-xs" onClick={() => void toggleHistory()} title="Save point timeline">
          🕐
        </button>
      </div>
    </div>
  );
}

function ActivityRail() {
  const activity = useStore((s) => s.activity);
  const jobs = useStore((s) => s.jobs);
  const active = Object.values(jobs).filter(
    (j) => j.stage.stage === "queued" || j.stage.stage === "running",
  );
  return (
    <aside
      className="w-64 shrink-0 border-l flex flex-col"
      style={{ background: "var(--at-panel)", borderColor: "var(--at-line)" }}
    >
      <div className="px-4 py-3 border-b text-xs uppercase tracking-widest" style={{ borderColor: "var(--at-line)", color: "var(--at-dim)" }}>
        On set
      </div>
      <div className="flex-1 overflow-y-auto p-3 space-y-2">
        {active.map((j) => (
          <div key={j.job} className="text-xs at-pulse" style={{ color: "var(--at-accent)" }}>
            ▶ {j.shot} — {j.stage.stage === "running" ? "generating…" : "queued"}
          </div>
        ))}
        {activity.length === 0 && active.length === 0 && (
          <div className="text-xs italic" style={{ color: "var(--at-dim)" }}>
            Quiet on set. Changes from your agent will appear here live.
          </div>
        )}
        {activity.map((item, i) => (
          <div key={`${item.at}-${i}`} className="text-xs leading-relaxed" style={{ color: "var(--at-dim)" }}>
            <span style={{ color: "var(--at-ink)" }}>{item.text}</span>
            <span className="block text-[10px] opacity-60">
              {new Date(item.at).toLocaleTimeString()}
            </span>
          </div>
        ))}
      </div>
    </aside>
  );
}
