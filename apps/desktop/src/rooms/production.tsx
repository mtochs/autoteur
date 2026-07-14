import { useRef, useState } from "react";
import { api } from "../api";
import { useStore } from "../store";
import type { ResolvedPrompt, Shot } from "../types";
import { Modal } from "./story";

/* ── Casting ──────────────────────────────────────────────────────── */

export function Casting() {
  return <EntityRoom kind="character" title="Casting" addLabel="+ Cast someone" />;
}

export function WorldRoom() {
  return <EntityRoom kind="world" title="Locations & Props" addLabel="+ Add to the world" />;
}

function EntityRoom({ kind, title, addLabel }: { kind: "character" | "world"; title: string; addLabel: string }) {
  const state = useStore((s) => s.state);
  const refresh = useStore((s) => s.refresh);
  const [editing, setEditing] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState("");
  const [worldKind, setWorldKind] = useState("location");

  const entries = Object.entries(kind === "character" ? state?.characters ?? {} : state?.world ?? {});

  const create = async () => {
    if (!name.trim()) return;
    if (kind === "character") await api.createCharacter(name.trim());
    else await api.createWorld(name.trim(), worldKind);
    setName("");
    setAdding(false);
    await refresh();
  };

  return (
    <div className="p-8 space-y-6">
      <div className="flex items-center">
        <h1 className="font-display text-2xl flex-1">{title}</h1>
        <button className="at-btn at-btn-primary" onClick={() => setAdding(true)}>{addLabel}</button>
      </div>
      <div className="grid gap-4" style={{ gridTemplateColumns: "repeat(auto-fill, minmax(190px, 1fr))" }}>
        {entries.map(([slug, entry]) => {
          const data = entry.data as { name: string; description?: string; kind?: string; prompt?: { fragment?: string } };
          return (
            <button
              key={slug}
              onClick={() => setEditing(slug)}
              className="at-index-card p-4 text-left space-y-2"
              style={{ ["--card-tint" as string]: kind === "character" ? "#9a7ae0" : "#4ac9b0" }}
            >
              <div className="w-12 h-12 rounded-full flex items-center justify-center font-display text-lg" style={{ background: "var(--at-accent-soft)", color: "var(--at-accent)" }}>
                {data.name?.slice(0, 1).toUpperCase() || "?"}
              </div>
              <div className="text-[13px] font-semibold">{data.name}</div>
              {data.kind && <span className="at-chip">{data.kind}</span>}
              <div className="text-[11px] line-clamp-3" style={{ color: "var(--at-dim)" }}>
                {data.prompt?.fragment || data.description || "No look defined yet."}
              </div>
            </button>
          );
        })}
        {entries.length === 0 && (
          <div className="text-sm italic col-span-full" style={{ color: "var(--at-dim)" }}>
            {kind === "character"
              ? "Nobody's been cast yet. Every character here keeps the same face in every shot that features them."
              : "An empty world. Locations, props, vehicles, and style bibles all live here for visual consistency."}
          </div>
        )}
      </div>
      {adding && (
        <Modal title={addLabel.replace("+ ", "")} onClose={() => setAdding(false)}>
          <input autoFocus className="at-input" placeholder="Name" value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void create()} />
          {kind === "world" && (
            <select className="at-input" value={worldKind} onChange={(e) => setWorldKind(e.target.value)}>
              {["location", "prop", "vehicle", "style"].map((k) => <option key={k}>{k}</option>)}
            </select>
          )}
          <button className="at-btn at-btn-primary" onClick={() => void create()}>Create</button>
        </Modal>
      )}
      {editing && <EntityEditor kind={kind} slug={editing} onClose={() => setEditing(null)} />}
    </div>
  );
}

function EntityEditor({ kind, slug, onClose }: { kind: "character" | "world"; slug: string; onClose: () => void }) {
  const state = useStore((s) => s.state);
  const refresh = useStore((s) => s.refresh);
  const entry = kind === "character" ? state?.characters[slug] : state?.world[slug];
  const [patch, setPatch] = useState<Record<string, unknown>>({});
  if (!entry) return null;
  const data = entry.data as {
    name: string;
    description?: string;
    kind?: string;
    prompt?: { fragment?: string; negative?: string };
    voice?: { provider?: string; voice_id?: string; style?: string };
    visual?: { reference_images?: string[] };
  };
  const current = (key: string, fallback: string) => (patch[key] !== undefined ? String(patch[key] ?? "") : fallback);
  const save = async () => {
    if (Object.keys(patch).length) await api.updateEntity(kind, slug, patch);
    await refresh();
    onClose();
  };
  return (
    <Modal title={`${data.name} · ${slug}`} onClose={onClose} wide>
      <div className="grid grid-cols-2 gap-3">
        <Field label="Name">
          <input className="at-input" value={current("name", data.name)} onChange={(e) => setPatch({ ...patch, name: e.target.value })} />
        </Field>
        {kind === "world" && (
          <Field label="Kind">
            <select className="at-input" value={current("kind", data.kind ?? "location")} onChange={(e) => setPatch({ ...patch, kind: e.target.value })}>
              {["location", "prop", "vehicle", "style"].map((k) => <option key={k}>{k}</option>)}
            </select>
          </Field>
        )}
      </div>
      <Field label="Description">
        <textarea className="at-input min-h-20" value={current("description", data.description ?? "")} onChange={(e) => setPatch({ ...patch, description: e.target.value })} />
      </Field>
      <Field label="The look (injected into every shot prompt)">
        <textarea className="at-input min-h-16" placeholder="comma-phrase visual description…" value={current("fragment", data.prompt?.fragment ?? "")} onChange={(e) => setPatch({ ...patch, fragment: e.target.value })} />
      </Field>
      <UnderTheHood>
        <Field label="Negative fragment">
          <input className="at-input" value={current("negative", data.prompt?.negative ?? "")} onChange={(e) => setPatch({ ...patch, negative: e.target.value })} />
        </Field>
        <Field label="Reference images (one repo-relative path per line)">
          <textarea
            className="at-input min-h-16 font-mono text-xs"
            value={patch.reference_images !== undefined ? (patch.reference_images as string[]).join("\n") : (data.visual?.reference_images ?? []).join("\n")}
            onChange={(e) => setPatch({ ...patch, reference_images: e.target.value.split("\n").map((l) => l.trim()).filter(Boolean) })}
          />
        </Field>
        {kind === "character" && (
          <div className="grid grid-cols-3 gap-2">
            <Field label="Voice provider">
              <input className="at-input" value={current("voice_provider", data.voice?.provider ?? "")} onChange={(e) => setPatch({ ...patch, voice_provider: e.target.value })} />
            </Field>
            <Field label="Voice id">
              <input className="at-input" value={current("voice_id", data.voice?.voice_id ?? "")} onChange={(e) => setPatch({ ...patch, voice_id: e.target.value })} />
            </Field>
            <Field label="Delivery">
              <input className="at-input" value={current("voice_style", data.voice?.style ?? "")} onChange={(e) => setPatch({ ...patch, voice_style: e.target.value })} />
            </Field>
          </div>
        )}
      </UnderTheHood>
      <button className="at-btn at-btn-primary w-full" onClick={() => void save()}>Done</button>
    </Modal>
  );
}

export function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="block space-y-1">
      <span className="text-[11px] uppercase tracking-wide" style={{ color: "var(--at-dim)" }}>{label}</span>
      {children}
    </label>
  );
}

export function UnderTheHood({ children }: { children: React.ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="rounded-lg border" style={{ borderColor: "var(--at-line)" }}>
      <button className="w-full text-left px-3 py-2 text-xs" style={{ color: "var(--at-dim)" }} onClick={() => setOpen(!open)}>
        {open ? "▾" : "▸"} Under the hood
      </button>
      {open && <div className="p-3 pt-0 space-y-3">{children}</div>}
    </div>
  );
}

/* ── Shot List ────────────────────────────────────────────────────── */

const STATUS_COLORS: Record<string, string> = {
  planned: "#8b8794", ready: "#4aa8e0", locked: "#9bc94a", omitted: "#5c5c66",
};

export function ShotList() {
  const state = useStore((s) => s.state);
  const [sceneSlug, setSceneSlug] = useState<string | null>(null);
  const [newScene, setNewScene] = useState(false);
  const [title, setTitle] = useState("");
  const refresh = useStore((s) => s.refresh);

  const scenes = state?.scenes ?? [];
  const active = scenes.find((s) => s.slug === sceneSlug) ?? scenes[0];

  return (
    <div className="flex h-full">
      <div className="w-52 shrink-0 border-r p-3 space-y-1 overflow-y-auto" style={{ borderColor: "var(--at-line)" }}>
        <div className="flex items-center justify-between px-1 pb-2">
          <span className="text-xs uppercase tracking-widest" style={{ color: "var(--at-dim)" }}>Scenes</span>
          <button className="at-btn text-xs px-2 py-0.5" onClick={() => setNewScene(true)}>+</button>
        </div>
        {scenes.map((scene) => (
          <button
            key={scene.slug}
            className="w-full text-left px-2 py-1.5 rounded text-[13px]"
            style={active?.slug === scene.slug ? { background: "var(--at-accent-soft)", color: "var(--at-accent)" } : {}}
            onClick={() => setSceneSlug(scene.slug)}
          >
            <span className="opacity-50 mr-1.5">{scene.number}</span>
            {scene.scene?.data.title ?? scene.slug}
          </button>
        ))}
      </div>
      <div className="flex-1 overflow-y-auto">
        {active ? <SceneShots key={active.slug} slug={active.slug} /> : (
          <div className="p-8 text-sm italic" style={{ color: "var(--at-dim)" }}>No scenes yet — create one, or let the crew break the treatment down.</div>
        )}
      </div>
      {newScene && (
        <Modal title="New scene" onClose={() => setNewScene(false)}>
          <input autoFocus className="at-input" placeholder="Scene title" value={title} onChange={(e) => setTitle(e.target.value)} />
          <button
            className="at-btn at-btn-primary"
            onClick={() => void api.createScene(title).then(() => { setNewScene(false); setTitle(""); return refresh(); })}
            disabled={!title.trim()}
          >
            Create
          </button>
        </Modal>
      )}
    </div>
  );
}

function SceneShots({ slug }: { slug: string }) {
  const state = useStore((s) => s.state);
  const recent = useStore((s) => s.recent);
  const scene = state?.scenes.find((s) => s.slug === slug);
  const shots = scene?.shots?.data.shots ?? [];
  const dragFrom = useRef<number | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);

  if (!scene) return null;
  return (
    <div className="p-6 space-y-4 max-w-3xl">
      <div>
        <h1 className="font-display text-2xl">{scene.scene?.data.title ?? slug}</h1>
        {scene.scene?.data.synopsis && (
          <p className="text-[13px] mt-1 leading-relaxed" style={{ color: "var(--at-dim)" }}>{scene.scene.data.synopsis}</p>
        )}
      </div>
      {shots.map((shot, index) => (
        <div
          key={shot.id}
          draggable={expanded !== shot.id}
          onDragStart={() => (dragFrom.current = index)}
          onDragOver={(e) => e.preventDefault()}
          onDrop={() => {
            const from = dragFrom.current;
            dragFrom.current = null;
            if (from !== null && from !== index) void api.moveShot(slug, from, index);
          }}
          className={`at-index-card p-4 ${recent[`shot:${slug}/${shot.id}`] ? "at-glide" : ""}`}
          style={{ ["--card-tint" as string]: STATUS_COLORS[shot.status] ?? "#3a3a46" }}
        >
          <ShotCard scene={slug} shot={shot} expanded={expanded === shot.id} onToggle={() => setExpanded(expanded === shot.id ? null : shot.id)} />
        </div>
      ))}
      <button className="at-btn" onClick={() => void api.addShot(slug, { status: "planned" })}>+ Add shot</button>
    </div>
  );
}

function ShotCard({ scene, shot, expanded, onToggle }: { scene: string; shot: Shot; expanded: boolean; onToggle: () => void }) {
  const [patch, setPatch] = useState<Record<string, unknown>>({});
  const [flap, setFlap] = useState<ResolvedPrompt | null>(null);
  const value = (key: keyof Shot, fallback = "") => (patch[key] !== undefined ? String(patch[key] ?? "") : String(shot[key] ?? fallback));

  const commit = async () => {
    if (Object.keys(patch).length) {
      await api.updateShot(scene, shot.id, patch);
      setPatch({});
    }
  };
  const openFlap = async () => setFlap(await api.resolveShotPrompt(scene, shot.id));

  return (
    <div className="space-y-2">
      <div className="flex items-center gap-3 cursor-pointer" onClick={onToggle}>
        <span className="font-display text-lg w-8" style={{ color: "var(--at-accent)" }}>{shot.id.toUpperCase()}</span>
        {shot.framing && <span className="at-chip">{shot.framing}</span>}
        <span className="flex-1 text-[13px] truncate">{shot.action ?? <i style={{ color: "var(--at-dim)" }}>describe the shot…</i>}</span>
        <select
          className="at-input !w-auto text-xs"
          value={shot.status}
          onClick={(e) => e.stopPropagation()}
          onChange={(e) => void api.setShotStatus(scene, shot.id, e.target.value)}
        >
          {["planned", "ready", "locked", "omitted"].map((s) => <option key={s}>{s}</option>)}
        </select>
        {shot.selected_take && <span title={`circled ${shot.selected_take}`} style={{ color: "var(--at-accent)" }}>⊙</span>}
      </div>
      {expanded && (
        <div className="space-y-3 pt-2 border-t" style={{ borderColor: "var(--at-line)" }} onClick={(e) => e.stopPropagation()}>
          <div className="grid grid-cols-3 gap-2">
            <Field label="Framing"><input className="at-input" value={value("framing")} onChange={(e) => setPatch({ ...patch, framing: e.target.value })} /></Field>
            <Field label="Camera move"><input className="at-input" value={value("camera")} onChange={(e) => setPatch({ ...patch, camera: e.target.value })} /></Field>
            <Field label="Target seconds"><input className="at-input" value={value("duration_s")} onChange={(e) => setPatch({ ...patch, duration_s: e.target.value ? Number(e.target.value) : null })} /></Field>
          </div>
          <Field label="Action (what the camera sees)">
            <textarea className="at-input min-h-20" value={value("action")} onChange={(e) => setPatch({ ...patch, action: e.target.value })} />
          </Field>
          <Field label="Director notes">
            <textarea className="at-input min-h-12" value={value("notes")} onChange={(e) => setPatch({ ...patch, notes: e.target.value })} />
          </Field>
          <UnderTheHood>
            <Field label="Custom prompt template (placeholders keep the cast's look attached)">
              <textarea className="at-input min-h-16 font-mono text-xs" value={value("prompt")} onChange={(e) => setPatch({ ...patch, prompt: e.target.value || null })} />
            </Field>
            <div className="grid grid-cols-2 gap-2">
              <Field label="Prompt extra"><input className="at-input" value={value("prompt_extra")} onChange={(e) => setPatch({ ...patch, prompt_extra: e.target.value || null })} /></Field>
              <Field label="Negative extra"><input className="at-input" value={value("negative_extra")} onChange={(e) => setPatch({ ...patch, negative_extra: e.target.value || null })} /></Field>
            </div>
          </UnderTheHood>
          <div className="flex gap-2">
            <button className="at-btn at-btn-primary" onClick={() => void commit()}>Save shot</button>
            <button className="at-btn" onClick={() => void openFlap()}>Composed prompt</button>
            <button
              className="at-btn"
              onClick={() => void api.generateShots([`${scene}/${shot.id}`]).catch((e) => alert(String(e)))}
              title="Send to the lab"
            >
              🎬 Generate
            </button>
          </div>
        </div>
      )}
      {flap && (
        <Modal title={`Composed prompt · ${scene}/${shot.id}`} onClose={() => setFlap(null)} wide>
          <pre className="at-input whitespace-pre-wrap text-xs leading-relaxed">{flap.prompt || "(empty)"}</pre>
          {flap.negative && <div className="text-xs"><b>Negative:</b> <span style={{ color: "var(--at-dim)" }}>{flap.negative}</span></div>}
          <div className="text-xs" style={{ color: "var(--at-dim)" }}>
            Identity attached: {flap.reference_images.length} reference image(s), {flap.adapters.length} adapter(s)
          </div>
          {flap.warnings.map((w, i) => <div key={i} className="text-xs text-amber-400">note: {w}</div>)}
        </Modal>
      )}
    </div>
  );
}
