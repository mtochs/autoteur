import { useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import { useStore } from "../store";
import type { ModelInfo, ProviderStatus, TakeMedia } from "../types";
import { Modal } from "./story";
import { Field, UnderTheHood } from "./production";

/* ── Dailies ──────────────────────────────────────────────────────── */

export function Dailies() {
  const state = useStore((s) => s.state);
  const media = useStore((s) => s.media);
  const jobs = useStore((s) => s.jobs);
  const recent = useStore((s) => s.recent);
  const [playing, setPlaying] = useState<TakeMedia | null>(null);

  const active = Object.values(jobs).filter((j) => j.stage.stage === "queued" || j.stage.stage === "running");
  const failed = Object.values(jobs).filter((j) => j.stage.stage === "failed");
  const totalCost = useMemo(
    () => Object.values(media).flat().reduce((sum, t) => sum + (t.cost_usd ?? 0), 0),
    [media],
  );

  const rows = (state?.scenes ?? []).flatMap((scene) =>
    (scene.shots?.data.shots ?? []).map((shot) => ({
      scene: scene.slug,
      shot,
      takes: media[`${scene.slug}/${shot.id}`] ?? [],
    })),
  ).filter((row) => row.takes.length > 0 || active.some((j) => j.shot === `${row.scene}/${row.shot.id}`));

  return (
    <div className="p-8 space-y-6">
      <div className="flex items-center gap-4">
        <h1 className="font-display text-2xl flex-1">Dailies</h1>
        {totalCost > 0 && (
          <span className="text-xs" style={{ color: "var(--at-dim)" }} title="Total recorded generation spend">
            ≈ ${totalCost.toFixed(2)}
          </span>
        )}
      </div>

      {active.length > 0 && (
        <div className="rounded-lg p-4 space-y-1" style={{ background: "var(--at-panel)", border: "1px solid var(--at-line)" }}>
          <div className="text-xs uppercase tracking-widest mb-2" style={{ color: "var(--at-dim)" }}>In the lab</div>
          {active.map((j) => (
            <div key={j.job} className="text-sm at-pulse" style={{ color: "var(--at-accent)" }}>
              ▶ {j.shot} — {j.stage.stage === "running" ? "developing…" : "waiting"}
            </div>
          ))}
        </div>
      )}
      {failed.map((j) => (
        <div key={j.job} className="text-xs text-red-400">✗ {j.shot}: {j.stage.message}</div>
      ))}

      {rows.length === 0 && active.length === 0 && (
        <div className="text-sm italic" style={{ color: "var(--at-dim)" }}>
          Nothing back from the lab yet. Mark shots <b>ready</b> in the Shot List and hit Generate.
        </div>
      )}

      {rows.map(({ scene, shot, takes }) => (
        <div key={`${scene}/${shot.id}`} className="space-y-2">
          <div className="flex items-baseline gap-3">
            <span className="font-display" style={{ color: "var(--at-accent)" }}>{scene} / {shot.id.toUpperCase()}</span>
            <span className="text-xs truncate" style={{ color: "var(--at-dim)" }}>{shot.action}</span>
          </div>
          <div className="flex gap-3 flex-wrap">
            {takes.map((take) => {
              const circled = shot.selected_take === take.id;
              return (
                <div key={take.id} className={`space-y-1 ${recent[`take:${take.id}`] ? "at-glide" : ""}`}>
                  <button className="at-frame w-44 h-28 overflow-hidden block" onClick={() => setPlaying(take)} style={circled ? { borderColor: "var(--at-accent)", boxShadow: "0 0 0 1px var(--at-accent)" } : {}}>
                    <TakeThumb take={take} />
                  </button>
                  <div className="flex items-center gap-2">
                    <button
                      className="text-lg leading-none"
                      title={circled ? "Un-circle" : "Circle this take"}
                      style={{ color: circled ? "var(--at-accent)" : "var(--at-dim)" }}
                      onClick={() => void api.circleTake(scene, shot.id, circled ? null : take.id)}
                    >
                      {circled ? "⊙" : "○"}
                    </button>
                    <span className="text-[10px] font-mono" style={{ color: "var(--at-dim)" }}>{take.id.slice(3, 9)}</span>
                    {!take.exists && <span className="at-chip text-red-400 border-red-900">media missing</span>}
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      ))}

      {playing && (
        <Modal title={`Take ${playing.id}`} onClose={() => setPlaying(null)} wide>
          {playing.path && playing.exists ? (
            <TakePlayer take={playing} />
          ) : (
            <div className="text-sm" style={{ color: "var(--at-dim)" }}>
              The media isn't on this machine — regenerate it, or sync takes/ from wherever it was made.
            </div>
          )}
          <UnderTheHood>
            <div className="text-xs space-y-1" style={{ color: "var(--at-dim)" }}>
              <div><b>Model:</b> {playing.model} · <b>Provider:</b> {playing.provider}</div>
              {playing.created_at && <div><b>Printed:</b> {playing.created_at}</div>}
              {playing.cost_usd != null && <div><b>Cost:</b> ${playing.cost_usd.toFixed(3)}</div>}
              {playing.resolved_prompt && <pre className="whitespace-pre-wrap mt-2">{playing.resolved_prompt}</pre>}
            </div>
          </UnderTheHood>
        </Modal>
      )}
    </div>
  );
}

function TakeThumb({ take }: { take: TakeMedia }) {
  if (!take.path || !take.exists) {
    return <div className="w-full h-full flex items-center justify-center text-xs" style={{ color: "var(--at-dim)" }}>no media</div>;
  }
  const src = convertFileSrc(take.path);
  if (take.kind === "image") return <img src={src} className="w-full h-full object-cover" />;
  return <video src={src} preload="metadata" muted className="w-full h-full object-cover" />;
}

function TakePlayer({ take }: { take: TakeMedia }) {
  const src = convertFileSrc(take.path!);
  if (take.kind === "image") return <img src={src} className="w-full rounded" />;
  return <video src={src} controls autoPlay className="w-full rounded bg-black" />;
}

/* ── Editing Room ─────────────────────────────────────────────────── */

export function EditingRoom() {
  const state = useStore((s) => s.state);
  const media = useStore((s) => s.media);
  const [cursor, setCursor] = useState(0);
  const videoRef = useRef<HTMLVideoElement>(null);

  // The cut: circled, non-omitted takes in story order.
  const cut = (state?.scenes ?? []).flatMap((scene) =>
    (scene.shots?.data.shots ?? [])
      .filter((shot) => shot.selected_take && shot.status !== "omitted")
      .map((shot) => {
        const take = (media[`${scene.slug}/${shot.id}`] ?? []).find((t) => t.id === shot.selected_take);
        return { ref: `${scene.slug}/${shot.id}`, shot, take };
      }),
  );

  const current = cut[cursor];

  useEffect(() => {
    videoRef.current?.load();
    void videoRef.current?.play().catch(() => {});
  }, [cursor, current?.take?.id]);

  if (cut.length === 0) {
    return (
      <div className="p-8">
        <h1 className="font-display text-2xl mb-4">Editing Room</h1>
        <div className="text-sm italic" style={{ color: "var(--at-dim)" }}>
          The cut assembles itself from circled takes, in story order. Circle takes in Dailies to see it here.
        </div>
      </div>
    );
  }

  return (
    <div className="p-8 space-y-5">
      <h1 className="font-display text-2xl">Editing Room</h1>
      <div className="rounded-lg overflow-hidden bg-black aspect-video max-w-3xl">
        {current?.take?.path && current.take.exists ? (
          <video
            ref={videoRef}
            src={convertFileSrc(current.take.path)}
            controls
            className="w-full h-full"
            onEnded={() => setCursor((c) => Math.min(c + 1, cut.length - 1))}
          />
        ) : (
          <div className="w-full h-full flex items-center justify-center text-sm" style={{ color: "var(--at-dim)" }}>
            media missing for {current?.ref}
          </div>
        )}
      </div>
      <div className="flex gap-2 overflow-x-auto pb-2">
        {cut.map((entry, i) => (
          <button
            key={entry.ref}
            className="at-frame w-32 h-20 shrink-0 overflow-hidden"
            style={i === cursor ? { borderColor: "var(--at-accent)", boxShadow: "0 0 0 1px var(--at-accent)" } : {}}
            onClick={() => setCursor(i)}
            title={entry.ref}
          >
            {entry.take ? <TakeThumb take={entry.take} /> : <div className="text-[10px] p-2">{entry.ref}</div>}
          </button>
        ))}
      </div>
      {current && <TrimControls shotRef={current.ref} />}
    </div>
  );
}

function TrimControls({ shotRef }: { shotRef: string }) {
  const state = useStore((s) => s.state);
  const entry = state?.timeline?.data.entries.find((e) => e.shot === shotRef);
  const [inS, setInS] = useState(entry?.in_s?.toString() ?? "");
  const [outS, setOutS] = useState(entry?.out_s?.toString() ?? "");
  useEffect(() => {
    setInS(entry?.in_s?.toString() ?? "");
    setOutS(entry?.out_s?.toString() ?? "");
  }, [shotRef, entry?.in_s, entry?.out_s]);

  const apply = () =>
    void api
      .setTrim(shotRef, inS ? Number(inS) : null, outS ? Number(outS) : null)
      .catch((e) => alert(String(e)));

  return (
    <div className="flex items-end gap-3 max-w-md">
      <Field label={`Trim in (s) · ${shotRef}`}>
        <input className="at-input" value={inS} onChange={(e) => setInS(e.target.value)} />
      </Field>
      <Field label="Trim out (s)">
        <input className="at-input" value={outS} onChange={(e) => setOutS(e.target.value)} />
      </Field>
      <button className="at-btn" onClick={apply}>Set trim</button>
    </div>
  );
}

/* ── Screening Room ───────────────────────────────────────────────── */

export function ScreeningRoom() {
  const root = useStore((s) => s.root);
  const status = useStore((s) => s.renderStatus);
  const [lastOutput, setLastOutput] = useState<string | null>(null);

  useEffect(() => {
    if (status?.phase === "done") setLastOutput(status.message);
  }, [status]);

  const exportCut = async () => {
    const output = await saveDialog({
      title: "Export the cut",
      defaultPath: `${root}/screening.mp4`,
      filters: [{ name: "MP4", extensions: ["mp4"] }],
    });
    if (typeof output !== "string") return;
    setLastOutput(null);
    await api.exportCut(output);
  };

  return (
    <div className="p-8 space-y-6 max-w-3xl">
      <h1 className="font-display text-2xl">Screening Room</h1>
      <p className="text-sm" style={{ color: "var(--at-dim)" }}>
        Stitch the cut — circled takes in story order, with any trims — into one MP4.
      </p>
      <button className="at-btn at-btn-primary text-base px-6 py-3" onClick={() => void exportCut()}>
        ▶ Export screening copy
      </button>
      {status?.phase === "working" && <div className="text-sm at-pulse" style={{ color: "var(--at-accent)" }}>{status.message}</div>}
      {status?.phase === "error" && <div className="text-sm text-red-400 whitespace-pre-wrap">{status.message}</div>}
      {lastOutput && (
        <div className="space-y-3">
          <div className="text-sm" style={{ color: "var(--at-accent)" }}>The screening copy is ready.</div>
          <video src={convertFileSrc(lastOutput)} controls className="w-full rounded bg-black aspect-video" />
        </div>
      )}
    </div>
  );
}

/* ── Studio Settings ──────────────────────────────────────────────── */

export function StudioSettings() {
  const state = useStore((s) => s.state);
  const validation = useStore((s) => s.validation);
  const refresh = useStore((s) => s.refresh);
  const [providers, setProviders] = useState<ProviderStatus[]>([]);
  const [keyInput, setKeyInput] = useState<Record<string, string>>({});
  const [models, setModels] = useState<ModelInfo[] | null>(null);
  const [ffmpeg, setFfmpeg] = useState<string | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);

  const defaults = state?.manifest?.data.defaults;

  const load = async () => {
    setProviders(await api.keyStatus());
    setFfmpeg(await api.ffmpegStatus());
  };
  useEffect(() => {
    void load();
  }, []);

  const connect = async (id: string) => {
    const key = keyInput[id]?.trim();
    if (!key) return;
    await api.keySet(id, key);
    setKeyInput({ ...keyInput, [id]: "" });
    await load();
  };

  const fetchModels = async () => {
    setError(null);
    try {
      setModels(await api.recommendedModels());
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="p-8 space-y-8 max-w-3xl">
      <h1 className="font-display text-2xl">Studio Settings</h1>

      <section className="space-y-3">
        <h2 className="font-display text-lg">Connect your studio</h2>
        <p className="text-xs" style={{ color: "var(--at-dim)" }}>
          Keys are stored in Windows Credential Manager — never in project files.
        </p>
        {providers.map((p) => (
          <div key={p.id} className="flex items-center gap-3 rounded-lg p-3" style={{ background: "var(--at-panel)", border: "1px solid var(--at-line)" }}>
            <span className="w-28 text-sm">{p.name}</span>
            {p.connected ? (
              <>
                <span className="text-xs flex-1" style={{ color: "#9bc94a" }}>✓ connected</span>
                <button className="at-btn text-xs" onClick={() => void api.keyClear(p.id).then(load)}>Disconnect</button>
              </>
            ) : (
              <>
                <input
                  className="at-input flex-1"
                  type="password"
                  placeholder="Paste API key…"
                  value={keyInput[p.id] ?? ""}
                  onChange={(e) => setKeyInput({ ...keyInput, [p.id]: e.target.value })}
                  onKeyDown={(e) => e.key === "Enter" && void connect(p.id)}
                />
                <button className="at-btn text-xs" onClick={() => void connect(p.id)}>Connect</button>
              </>
            )}
          </div>
        ))}
      </section>

      <section className="space-y-3">
        <h2 className="font-display text-lg">Default film stock</h2>
        <div className="flex gap-2 items-center">
          <input
            className="at-input flex-1"
            placeholder="owner/model or owner/model:version"
            defaultValue={defaults?.video_model ?? ""}
            onBlur={(e) => void api.setDefaults({ video_model: e.target.value || null }).then(refresh)}
          />
          <button className="at-btn text-xs" onClick={() => void fetchModels()}>What's good right now?</button>
        </div>
        {error && <div className="text-xs text-red-400">{error}</div>}
        {models && (
          <div className="space-y-1">
            {models.map((m) => (
              <button
                key={m.slug}
                className="w-full text-left text-xs p-2 rounded flex gap-2 items-baseline hover:bg-white/5"
                onClick={() => void api.setDefaults({ video_model: m.version ? `${m.slug}:${m.version}` : m.slug }).then(refresh)}
              >
                <span className="at-chip">{m.kind}</span>
                <b>{m.slug}</b>
                <span className="truncate" style={{ color: "var(--at-dim)" }}>{m.description}</span>
              </button>
            ))}
            {models.length === 0 && <div className="text-xs italic" style={{ color: "var(--at-dim)" }}>Nothing recommended right now.</div>}
          </div>
        )}
      </section>

      <section className="space-y-2">
        <h2 className="font-display text-lg">Projection booth</h2>
        {ffmpeg === undefined ? null : ffmpeg ? (
          <div className="text-xs" style={{ color: "#9bc94a" }}>✓ FFmpeg ready · <span style={{ color: "var(--at-dim)" }}>{ffmpeg}</span></div>
        ) : (
          <div className="text-xs text-amber-400">
            FFmpeg not found — install it (winget install ffmpeg) or set AUTOTEUR_FFMPEG, then reopen this panel.
          </div>
        )}
      </section>

      {validation.length > 0 && (
        <section className="space-y-2">
          <h2 className="font-display text-lg">Continuity notes</h2>
          {validation.map((v, i) => (
            <div key={i} className="text-xs" style={{ color: v.severity === "error" ? "#f87171" : "var(--at-dim)" }}>
              {v.severity === "error" ? "✗" : "•"} {v.path.split(/[\\/]/).slice(-2).join("/")}: {v.message}
            </div>
          ))}
        </section>
      )}
    </div>
  );
}
