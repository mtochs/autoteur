import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { useStore } from "../store";
import type { Beat } from "../types";

/* ── Writers' Room ────────────────────────────────────────────────── */

export function WritersRoom() {
  const [doc, setDoc] = useState<"logline" | "treatment">("treatment");
  return (
    <div className="max-w-3xl mx-auto p-8 space-y-4">
      <div className="flex items-center gap-4">
        <h1 className="font-display text-2xl flex-1">Writers' Room</h1>
        {(["logline", "treatment"] as const).map((d) => (
          <button
            key={d}
            className="at-btn text-xs capitalize"
            style={doc === d ? { borderColor: "var(--at-accent)", color: "var(--at-accent)" } : {}}
            onClick={() => setDoc(d)}
          >
            {d}
          </button>
        ))}
      </div>
      <DocEditor key={doc} doc={doc} />
    </div>
  );
}

function DocEditor({ doc }: { doc: "logline" | "treatment" }) {
  const [text, setText] = useState<string | null>(null);
  const [saved, setSaved] = useState(true);
  const dirty = useRef(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const activity = useStore((s) => s.activity); // external updates ride the feed

  useEffect(() => {
    void api.readStoryDoc(doc).then((t) => setText(t));
  }, [doc]);

  // Reload on external change when we have nothing unsaved.
  useEffect(() => {
    if (!dirty.current) void api.readStoryDoc(doc).then((t) => setText(t));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activity.length]);

  const onChange = (value: string) => {
    setText(value);
    setSaved(false);
    dirty.current = true;
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => {
      void api.writeStoryDoc(doc, value).then(() => {
        dirty.current = false;
        setSaved(true);
      });
    }, 600);
  };

  if (text === null) return null;
  return (
    <div className="space-y-2">
      <textarea
        className="at-input font-display text-[15px] leading-7 min-h-[65vh] p-6"
        value={text}
        onChange={(e) => onChange(e.target.value)}
        placeholder={doc === "logline" ? "One sentence: who wants what, and what stands in the way." : "The story, told in prose…"}
        spellCheck
      />
      <div className="text-xs text-right" style={{ color: "var(--at-dim)" }}>
        {saved ? "Saved to the page" : "Writing…"}
      </div>
    </div>
  );
}

/* ── Beat Board ───────────────────────────────────────────────────── */

const ACT_PALETTE = ["#c8b08a", "#e0a83f", "#c96f4a", "#9a6fc9", "#5f8fc9"];
const TOKEN_COLORS: Record<string, string> = {
  rose: "#e07a9a", amber: "#e0a83f", lime: "#9bc94a", teal: "#4ac9b0",
  sky: "#4aa8e0", violet: "#9a7ae0", slate: "#8b93a8", sand: "#c8b08a",
};

function tint(beat: Beat, episodeColor?: string): string {
  const raw = beat.color ?? episodeColor;
  if (raw) return TOKEN_COLORS[raw] ?? raw;
  if (beat.act) return ACT_PALETTE[(beat.act - 1) % ACT_PALETTE.length];
  return "#3a3a46";
}

export function BeatBoard() {
  const state = useStore((s) => s.state);
  const recent = useStore((s) => s.recent);
  const [editing, setEditing] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const dragFrom = useRef<number | null>(null);

  const beats = state?.beats?.data.beats ?? [];
  const episodes = state?.beats?.data.episodes ?? [];
  const episodeColor = (id?: string) => episodes.find((e) => e.id === id)?.color;

  const drop = async (to: number) => {
    const from = dragFrom.current;
    dragFrom.current = null;
    if (from === null || from === to) return;
    if (episodes.length > 0 && beats[from]?.episode !== beats[to]?.episode) return;
    await api.moveBeat(from, to > from ? to : to);
  };

  const groups: { episode?: string; label?: string; items: { beat: Beat; index: number }[] }[] = [];
  if (episodes.length === 0) {
    groups.push({ items: beats.map((beat, index) => ({ beat, index })) });
  } else {
    for (const ep of episodes) {
      groups.push({
        episode: ep.id,
        label: `${ep.id.toUpperCase()} · ${ep.title}`,
        items: beats.map((beat, index) => ({ beat, index })).filter(({ beat }) => beat.episode === ep.id),
      });
    }
    const loose = beats.map((beat, index) => ({ beat, index })).filter(({ beat }) => !beat.episode || !episodes.some((e) => e.id === beat.episode));
    if (loose.length) groups.push({ label: "Unassigned", items: loose });
  }

  return (
    <div className="p-8 space-y-6">
      <div className="flex items-center">
        <h1 className="font-display text-2xl flex-1">Beat Board</h1>
        <button className="at-btn at-btn-primary" onClick={() => setAdding(true)}>+ Pin a card</button>
      </div>
      {beats.length === 0 && !adding && (
        <div className="text-sm italic" style={{ color: "var(--at-dim)" }}>
          An empty corkboard. Pin your first beat — or ask your agent to break the treatment into beats and watch them appear.
        </div>
      )}
      {groups.map((group, gi) => (
        <div key={group.episode ?? gi} className="space-y-3">
          {group.label && (
            <div className="text-xs uppercase tracking-widest" style={{ color: "var(--at-dim)" }}>{group.label}</div>
          )}
          <div className="grid gap-4" style={{ gridTemplateColumns: "repeat(auto-fill, minmax(200px, 1fr))" }}>
            {group.items.map(({ beat, index }) => (
              <div
                key={beat.id}
                draggable
                onDragStart={() => (dragFrom.current = index)}
                onDragOver={(e) => e.preventDefault()}
                onDrop={() => void drop(index)}
                onClick={() => setEditing(beat.id)}
                className={`at-index-card p-3 cursor-pointer min-h-28 ${recent[`beat:${beat.id}`] ? "at-glide" : ""}`}
                style={{ ["--card-tint" as string]: tint(beat, episodeColor(beat.episode)), ["--card-tilt" as string]: `${((index % 5) - 2) * 0.4}deg` }}
              >
                <div className="text-[13px] font-semibold leading-snug">{beat.title}</div>
                {beat.summary && (
                  <div className="text-[11px] mt-1.5 leading-relaxed line-clamp-4" style={{ color: "var(--at-dim)" }}>
                    {beat.summary}
                  </div>
                )}
                {beat.act && <div className="at-chip mt-2 inline-block">Act {beat.act}</div>}
              </div>
            ))}
          </div>
        </div>
      ))}
      {adding && <BeatForm episodes={episodes.map((e) => e.id)} onClose={() => setAdding(false)} />}
      {editing && <BeatEditor id={editing} onClose={() => setEditing(null)} />}
    </div>
  );
}

function BeatForm({ episodes, onClose }: { episodes: string[]; onClose: () => void }) {
  const [title, setTitle] = useState("");
  const [summary, setSummary] = useState("");
  const [episode, setEpisode] = useState(episodes[0] ?? "");
  return (
    <Modal title="Pin a beat" onClose={onClose}>
      <input autoFocus className="at-input" placeholder="Beat title" value={title} onChange={(e) => setTitle(e.target.value)} />
      <textarea className="at-input min-h-24" placeholder="What happens (card back)…" value={summary} onChange={(e) => setSummary(e.target.value)} />
      {episodes.length > 0 && (
        <select className="at-input" value={episode} onChange={(e) => setEpisode(e.target.value)}>
          {episodes.map((e) => <option key={e}>{e}</option>)}
        </select>
      )}
      <button
        className="at-btn at-btn-primary"
        onClick={() => void api.addBeat(title, summary, episodes.length ? episode : undefined).then(onClose)}
        disabled={!title.trim()}
      >
        Pin it
      </button>
    </Modal>
  );
}

function BeatEditor({ id, onClose }: { id: string; onClose: () => void }) {
  const beat = useStore((s) => s.state?.beats?.data.beats.find((b) => b.id === id));
  const [patch, setPatch] = useState<Record<string, unknown>>({});
  if (!beat) return null;
  const value = (k: keyof Beat) => (patch[k] !== undefined ? String(patch[k] ?? "") : String(beat[k] ?? ""));
  const save = async () => {
    if (Object.keys(patch).length) await api.updateBeat(id, patch);
    onClose();
  };
  return (
    <Modal title={`Beat · ${id}`} onClose={onClose}>
      <input className="at-input" value={value("title")} onChange={(e) => setPatch({ ...patch, title: e.target.value })} />
      <textarea className="at-input min-h-28" value={value("summary")} onChange={(e) => setPatch({ ...patch, summary: e.target.value })} />
      <div className="flex gap-2">
        <input className="at-input" placeholder="Act (number)" value={value("act")} onChange={(e) => setPatch({ ...patch, act: e.target.value ? Number(e.target.value) : null })} />
        <select className="at-input" value={value("color")} onChange={(e) => setPatch({ ...patch, color: e.target.value || null })}>
          <option value="">inherited color</option>
          {Object.keys(TOKEN_COLORS).map((c) => <option key={c}>{c}</option>)}
        </select>
      </div>
      <textarea className="at-input min-h-16" placeholder="Director notes…" value={value("notes")} onChange={(e) => setPatch({ ...patch, notes: e.target.value })} />
      <div className="flex gap-2">
        <button className="at-btn at-btn-primary flex-1" onClick={() => void save()}>Done</button>
        <button className="at-btn text-red-400" onClick={() => void api.removeBeat(id).then(onClose)}>Remove</button>
      </div>
    </Modal>
  );
}

/* Shared modal */
export function Modal({ title, children, onClose, wide }: { title: string; children: React.ReactNode; onClose: () => void; wide?: boolean }) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center" style={{ background: "rgba(0,0,0,0.6)" }} onClick={onClose}>
      <div
        className={`${wide ? "w-[760px]" : "w-[460px]"} max-h-[85vh] overflow-y-auto rounded-xl p-5 space-y-3`}
        style={{ background: "var(--at-panel)", border: "1px solid var(--at-line)" }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="font-display text-lg">{title}</div>
        {children}
      </div>
    </div>
  );
}
