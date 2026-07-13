# Proposal 0001 — File formats & live-sync (Autoteur v0.1)

Status: **awaiting review** (2026-07-13). Covers `beats.toml`, `scene.toml`, `shots.toml`, `characters/<slug>.toml`, and the live-sync event flow. Produced by a 4-designer / 4-critic panel synthesis; every example below parses under strict TOML 1.0.

## Design principles (apply to every file)

1. **File order IS the order.** `[[beats]]` order = board order, `[[shots]]` order = cut order, dialogue array order = speaking order. No position/index fields anywhere, ever. Append = add a block at the right place; reorder = move a whole block; concurrent inserts surface as honest, human-resolvable git conflicts around whole blocks instead of silently-mergeable duplicate integers.
2. **The filename is the identity.** Characters and world entries: identity = filename stem (`mara-chen`). Scenes: directory is `<NNN>-<slug>` where NNN is *sort order only* (gap-numbered by 10, numerically sorted) and the slug is the frozen identity. Copying a directory or file as a template automatically mints a new identity — the most common agent mistake (copy + forget to change an inner id) becomes structurally impossible for scenes/characters/world. Beats and shots, which live inside shared files, carry an explicit frozen `id`.
3. **Authored vs derived — the litmus test:** *could the app ever compute this value and disagree with the file?* If yes, it is derived (take counts, generating/queued, "circled" badges, costs, coverage rollups, resolved prompts) and is **never written into authored files**. This is the write-loop firewall. Authored files change only on explicit human/agent gestures.
4. **Absence means the default.** No selection = `selected_take` key absent (never `""`/`"none"`). Omitted `characters` on a shot = inherit the scene cast; `[]` = explicitly nobody. No `[[episodes]]` = feature film. TOML has no null; key presence is the null.
5. **One spelling per reference.** beat → `"<beat-id>"`; character/world → `"<file-slug>"`; scene → `"<scene-slug>"` (never the NNN-prefixed dirname); shot from outside its scene → `"<scene-slug>/<shot-id>"`; take → `"tk_<12-hex>"`. Asset paths are repo-root-relative with forward slashes. Everything is greppable; the validator is an exact-match pass that reports dangling refs as warnings, never crashes.
6. **References point child → parent, one direction only.** Scenes declare `beats = [...]`; beats never list scenes (beats.toml is the merge-hottest file — adding a scene must not touch it). Reverse maps (beat coverage badges) are derived live.
7. **TOML subset discipline** (documented in every file header): root keys before the first `[[...]]` block; prose in `'''` literal strings (no escaping); dialogue as single-line inline tables; no dotted-key nesting; table arrays only at the end of a file. These are exactly the constructs LLMs emit wrong — banned by convention, linted by the validator (a known shot key found on a dialogue table, or any key after the last table array, raises a notice instead of silently landing in the wrong table).
8. **Files carry their own spec.** Header comments state the ordering, id, and placement rules — an agent with plain Read/Edit needs no external docs. GUI writes go through `toml_edit` surgical edits that preserve comments, key order, and unknown keys byte-for-byte, so a GUI drag produces the same minimal diff an agent's Edit would.

## story/beats.toml

```toml
# story/beats.toml - one [[beats]] block per index card on the Beat Board.
# Card order = block order in this file. Reorder by moving whole blocks.
# Series: the board groups cards by episode; append a new beat after the
# last beat of the SAME episode. Root keys (schema_version) must appear
# above the first [[...]] block - never at the end of the file.
schema_version = 1

# Series only - a feature film omits [[episodes]] and every `episode` key
# below; nothing else changes (feature -> series is purely additive).
# Episode order = block order. `color` tints that episode's cards.
[[episodes]]
id = "e01"                 # frozen at creation
title = "Cold Signal"
color = "sky"              # rose|amber|lime|teal|sky|violet|slate|sand

[[episodes]]
id = "e02"
title = "The Auditor"
color = "violet"

[[beats]]
id = "cold-open-heist"     # kebab-case, unique in this file, FROZEN at
                           # creation - scenes reference beats by id
title = "Cold open: the vault job goes sideways"
episode = "e01"            # series only; omit in a feature
act = 1                    # optional integer, scoped to the episode/film
summary = '''
Mara's crew breaches the Halcyon vault and finds it already empty -
except for a device that has just started counting down.
'''

[[beats]]
id = "midpoint-betrayal"
title = "June sold the route"
episode = "e01"
act = 2
color = "amber"            # optional manual tint; delete the line to revert
                           # to episode color (series) / act palette (feature)
summary = '''
Mara finds the buyer's mark in June's ledger. The rescue was a lure.
'''
notes = "Tone pivot: the comedy drains out over three scenes."

[[beats]]
id = "e02-fallout"
title = "Fallout in the safehouse"
episode = "e02"
act = 1
summary = '''
Nobody says the word "betrayal". Everyone thinks it.
'''
```

Corkboard color precedence: `beat.color` (manual gesture, one-line add/delete) > `episode.color` > built-in per-act palette > neutral. Tokens, not hex, so files stay theme-neutral and "color the betrayal beat amber" is directly writable by an agent. Series boards group by episode lane (episode block order), card order = file order *within* the episode; a cross-lane drag writes the `episode` field in addition to the block move. View state (zoom, color-by toggle) lives in gitignored `.autoteur/`, never here.

## scenes/NNN-slug/scene.toml

```toml
# scenes/012-vault-breach/scene.toml
# The directory name is <NNN>-<slug>. NNN is sort order ONLY - renumber
# freely (gap-number in steps of 10; the app sorts numerically). The slug
# IS the scene's identity: takes.manifest.toml and timeline.toml reference
# shots as "<scene-slug>/<shot-id>". Renaming the slug is an identity
# change - use `autoteur mv scene` so references are rewritten with it.
schema_version = 1

title = "Breaching the Halcyon vault"

# Beats this scene realizes (ids from story/beats.toml). May be empty -
# unmapped scenes show in a Beat Board side tray. Beats never list scenes;
# the reverse map is derived live.
beats = ["cold-open-heist"]

# Scene cast and world (slugs = filenames under characters/ and world/).
# Shots inherit these unless they override (see shots.toml).
characters = ["mara-chen", "june-park"]
location = "halcyon-vault"          # world entry with kind = "location"
world = ["crew-drill-rig"]          # props / vehicles / extra style bibles

int_ext = "INT"                     # optional: "INT" | "EXT" | "INT/EXT"
time = "NIGHT"                      # optional free text
mood = "airless, blue-lit, held breath before the alarm"

synopsis = '''
Night. Mara and June work the vault door in near silence. The drill
bites; the alarm trips forty seconds early. June freezes a half-second
too long, and Mara sees it.
'''

director_notes = '''
Whisper volume until the alarm - it should feel physically loud by
contrast. Hold on June when it trips; the audience suspects her first.
'''
```

No in-file `id` — the directory slug is the single source of scene identity, so the copy-a-scene-as-template move can never create duplicate identities. No scene `status` in v0.1 (every candidate value was a derived rollup of its shots).

## scenes/NNN-slug/shots.toml

```toml
# scenes/012-vault-breach/shots.toml
# Shot order = block order; reorder by moving whole [[shots]] blocks.
# Shot ids are per-scene letters ("a", "b", ... then "aa"), FROZEN at
# creation and never reused - when adding a shot, take the next letter
# after the highest ever used (check takes.manifest.toml too). Delete
# nothing that has takes; set status = "omitted" instead.
# Global reference form: "vault-breach/c" (scene slug + shot id).
# Keep every key of a shot inside its own [[shots]] block, and keep each
# dialogue cue on a single line.
schema_version = 1

[[shots]]
id = "a"
framing = "wide"           # wide | medium | close-up | extreme-close-up |
                           # two-shot | ots | pov | insert | aerial
                           # (free text; the UI autocompletes these and the
                           # prompt resolver expands them to full phrases)
camera = "slow push-in from the breached blast door"     # optional move
action = '''
The antechamber lies bare under strip-light. Empty racks recede into
darkness. Center floor: the drill rig throws the only warm light.
'''
duration_s = 6.0           # target seconds; actual trim lives in timeline.toml
status = "locked"          # planned | ready | locked | omitted
                           # Director/agent INTENT only. Take counts,
                           # generating/queued, and "circled" badges are
                           # derived from takes.manifest.toml + the job
                           # queue and are never written into this file.
selected_take = "tk_3f9c2a8b41de"    # the circled take (id from
                           # takes.manifest.toml). OMIT the line when
                           # nothing is circled - never write "" or "none".
# `characters` omitted -> this shot inherits the scene cast.

[[shots]]
id = "b"
framing = "two-shot"
action = "Mara sweeps the racks with a hand lamp while June hangs back in the doorway."
characters = ["mara-chen", "june-park"]   # explicit = EXACTLY these are in
                           # frame (drives image/LoRA/fragment injection);
                           # [] = nobody in frame
duration_s = 9.5
status = "ready"
# Dialogue: ordered cues, one single-line inline table per cue. `character`
# is a characters/ slug (off-screen and V.O. speakers are legal - they get
# voice injection but visual injection follows `characters` above).
# `delivery` is the optional parenthetical.
dialogue = [
  { character = "mara-chen", line = "That's forty seconds early." },
  { character = "june-park", line = "Sensor ghost. It happens.", delivery = "too calm" },
  { character = "mara-chen", line = "Not in this building." },
]
notes = "The beat lives in June's stillness - if takes read panicked, regenerate."

[[shots]]
id = "c"
framing = "insert"
action = "The drill bit, forgotten, still spinning down against the floor plate."
characters = []            # nobody in frame - no character injection
world = ["crew-drill-rig"] # override: replaces the scene's location+world
                           # for this shot (omit to inherit both)
duration_s = 2.0
status = "ready"
# Optional custom template - replaces the project default template from
# autoteur.toml. Placeholders keep injection alive; a fully literal prompt
# is just a template with zero placeholders. Either way, reference images
# and LoRAs of characters[]/world[] STILL attach - the prompt controls
# text, never identity.
prompt = '''
{style}
Macro insert, 35mm: {action}
{world}
'''
prompt_extra = "shallow depth of field, oily metal sheen, fine film grain"
negative_extra = "blurry digits, text artifacts"    # optional; appended to
                           # the composed negative prompt
```

**Prompt resolution** (pure function, computed live, shown in the compose flap, snapshotted per-take into `takes.manifest.toml`, never written back):

1. Template = shot `prompt` if present, else `autoteur.toml [defaults].prompt_template`.
2. Placeholders: `{style}` (project style bible — world slugs listed in `autoteur.toml [defaults].style`, plus scene/shot world entries with `kind = "style"`), `{framing}` (vocab tokens expanded: `close-up` → "close-up shot", etc.), `{camera}`, `{action}`, `{characters}` (each effective cast member's `[prompt].fragment` — or pinned variant — plus LoRA trigger tokens, in list order), `{location}`, `{world}`, `{mood}`, `{dialogue}` (cues as `Name: "line"`), `{extra}` (= `prompt_extra`). Empty slots collapse; no conditionals in v0.1.
3. Negative prompt = project default negative + character/world `negative` fragments + shot `negative_extra`.
4. Reference images / LoRAs / embeddings attach as generation **inputs** based on effective `characters`/`world` — always, even under a fully literal `prompt`. Text and identity are separate channels.
5. Inheritance: omitted `characters` → scene cast; omitted `world` → scene location + world; explicit shot `world` → replaces both (set shot-level fields to re-add). Explicit lists never merge — what you write is exactly what injects.

Shot `status` values are all decisions, never observations: `planned` (don't generate yet), `ready` (approved to generate), `locked` (circled take is final — tools must not regenerate), `omitted` (cut, kept for the record). Unknown values render as a validation notice, not data loss.

## characters/slug.toml

```toml
# characters/mara-chen.toml - the filename stem IS the character's id
# ("mara-chen"), used in scene casts, shot characters[], and dialogue cues.
# Rename via `autoteur mv character` (rewrites references project-wide).
# New top-level fields go directly below schema_version - NEVER at the end
# of the file (they would land inside the last table).
schema_version = 1

name = "Mara Chen"
aliases = ["The Locksmith"]    # optional display names; not valid as references

description = '''
Late 30s safecracker; ex-structural engineer. Talks like she is rationing
words. Loyal until the math says otherwise - and she always does the math.
'''

# Voice profile - used by the dialogue/audio pipeline, never the image prompt.
[voice]
provider = "elevenlabs"        # optional; falls back to the project default
voice_id = "pNInz6obpgDQGcFmaJgB"
style = "low, dry, measured; slight rasp; never raises her voice"
reference_audio = "characters/refs/mara-chen/voice-sample.wav"   # optional

# Prompt fragments - injected wherever {characters} resolves in a shot that
# casts her. Visual description only, comma-phrase style.
[prompt]
fragment = "Mara Chen, East Asian woman in her late 30s, sharp jaw, short black hair with a gray streak, faded burn scar on left forearm, dark utility jacket"
negative = "youthful glamour makeup, long hair, jewelry"     # optional

# Named variants (optional). A shot pins one by writing "mara-chen:storm-gear"
# in its characters[]; the variant REPLACES `fragment` for that shot.
# Reference images and adapters still apply.
[prompt.variants]
storm-gear = "Mara Chen, late 30s, rain-plastered black hair under a dripping hood, black storm poncho over a dark utility jacket"

# Visual identity - reference images and adapters attach automatically as
# generation INPUTS to every shot that casts her; they never appear in the
# prompt text. This section stays LAST in the file: the table arrays below
# would swallow any key appended after them.
[visual]
reference_images = [
  "characters/refs/mara-chen/front.png",           # first image = primary
  "characters/refs/mara-chen/profile-left.png",    # repo-relative, forward slashes
]

# Optional LoRA / embedding stack, applied in listed order.
[[visual.adapters]]
kind = "lora"                  # "lora" | "embedding"
source = "civitai:123456"      # provider ref, URL, or repo-relative path
                               # (large weight files stay OUT of git - use
                               # provider refs, or gitignored assets/)
weight = 0.85
trigger = "m4rachen woman"     # trigger token, auto-prepended to her fragment
```

Section order is deliberate: prose first, `[visual]` with its table arrays **last**, because a key appended after a table array lands inside it. The validator additionally warns when a known top-level key is found inside a leaf table. `world/<slug>.toml` follows the same shape with `kind = "location" | "prop" | "vehicle" | "style"` and no `[voice]`.

## Live-sync: file change → UI update (one page)

**Watch → settle.** One recursive `notify` watcher (ReadDirectoryChangesW) on the project root. Hard-ignored: `takes/`, `.git/`, `.autoteur/`, our temp files (`.at-tmp-*`), editor droppings (`*.swp`, `~*`, `.#*`), non-`.toml`/`.md` files — but **directory events are exempt from the extension filter** (a scene-dir rename must remap every tracked child path, then integrity-sweep the subtree). Debounce **per-path**: 150 ms quiet window, 500 ms max-latency cap — a hot file still flushes twice a second, and an agent hammering `shots.toml` can't starve `beats.toml`. Coalescing folds create/modify/rename bursts (editor save-via-rename) into one settled event.

**Classify → parse → validate.** Path routes through a static table to a domain parser (`story/beats.toml`→Beats, `scenes/*/shots.toml`→Shots, …). Parse with `toml_edit` (comment/format-preserving). A validation pass runs between parse and diff: duplicate/missing ids, dangling references (unknown character slug, unknown beat), and misplaced known keys mark the file **stale exactly like a syntax error** — the UI keeps showing the last-good state with an amber banner ("`shots.toml` has two shots named 'c'"), plus a copyable **"fix it for me"** prompt (path + error + last-good content) to hand to the agent. Parse failures get an 800 ms grace period before the banner shows — a mid-write flush usually parses clean on the next settle. The failure path sniffs UTF-16 BOMs so a PowerShell-5.1 `>` redirect produces "this file is UTF-16 — rewrite as UTF-8," not a baffling syntax error.

**Diff → granular deltas.** The domain differ compares the fresh doc against canonical in-memory `ProjectState`, keyed by stable ids, order read from array position: `BeatAdded/Updated/Moved/Removed`, `ShotUpdated`, `SelectedTakeChanged`, `TakeAdded`, `SceneMoved`, … Granular deltas (not whole-state broadcasts) give targeted animation, preserved scroll/selection, and feed sentences. **Removals are quarantined**: an entity must be missing across two consecutive clean parses (~500 ms hold) before `Removed` emits — a truncated-but-valid TOML prefix from a non-atomic agent write must not vaporize five shot cards for 300 ms. Adds/updates apply immediately.

**Emit → apply.** One Tauri channel `project://delta`, envelope `{rev, origin: "local"|"external"|"startup", deltas}`, `rev` strictly monotonic. The frontend store applies typed reducers to normalized maps + order arrays. Rev gap → request a **rev-stamped snapshot**; deltas arriving during the fetch are buffered and replayed if newer than the snapshot's rev (no silent regression). A cheap canonical-state hash check piggybacks on refocus/resume, so drift is caught even without a rev gap.

**GUI write path (the race that three independent reviews flagged).** The GUI never serializes from possibly-stale memory. Every gesture is a typed command (`circle_take`, `move_beat`, …) executed as **read-modify-write against current disk**: re-read the file bytes; if a watcher event for this path is pending, flush it first (parse → diff → apply external deltas); apply the surgical `toml_edit` mutation to the fresh doc; write `.at-tmp-*` in the same dir, fsync, `rename` over the target (atomic on NTFS; retries with jittered backoff up to ~2 s for AV/editor/OneDrive sharing violations). On final failure the canonical mutation **rolls back** with a corrective delta and an actionable toast — the UI never claims an edit that isn't on disk. A blake3 write-journal (path → content hash, 2 s expiry) tags the echo event's origin as `local`; **journal hits never skip the differ** (a true echo diffs to zero deltas anyway), so a stale journal entry can never swallow a genuine agent write.

**Origin drives feel.** `external` deltas animate (card glides onto the board, take fades into Dailies) and feed the Activity panel — delta → sentence via entity names, coalesced by (verb, kind, parent) in a 5 s window: "3 new takes for Shot 12B". `local` deltas apply silently (the UI is already optimistic). `startup` renders cold. The feed is a session-only ring buffer — derived state, never written to project files; durable history is `git log`.

**Startup & recovery.** Watcher starts first, then full scan → snapshot (dedup by hash). One idempotent mtime+size integrity sweep serves power-resume, window refocus, and notify's `Rescan` (buffer overflow); diff-based application makes redundant sweeps emit nothing. Project roots under OneDrive/Dropbox are detected at open with a one-time "git is your sync" warning.

**Dirty buffers — there are exactly two, both specified.** (1) The Markdown editor: 1 s idle + 5 s max-interval autosave; on external write while dirty, line-level diff3 (base advances on every reconciled save) auto-merges or shows a non-modal *Take theirs / Keep mine / View both* banner; the buffer is never clobbered. (2) TOML form fields: commit on blur/Enter or 500 ms typing settle (one write, one journal entry per settle — not per keystroke); per-field dirty flags drive an inline "changed on disk" chip on true conflicts; untouched fields merge silently.

```
agent               OS           debounce        parse+validate      diff            Tauri         React store
  | write beats.toml |               |                  |               |               |               |
  |----------------->| RDCW events   |                  |               |               |               |
  |                  |-- coalesce -->| settle 150ms     |               |               |               |
  |                  |               |-- flush -------->| toml_edit OK  |               |               |
  |                  |               |                  | ids valid --->| BeatAdded     |               |
  |                  |               |                  | journal miss  | (external) -->| project://delta
  |                  |               |                  |               |               |-- reducer --->|
  |                  |               |                  |               |               |  card glides  |
  |                  |               |                  |               |               |  onto board   |
```

## Adjacent contracts these schemas depend on (summary)

- **takes.manifest.toml** (committed, append-only, written only by the generation pipeline): `[[takes]]` blocks with `id = "tk_<first 12 hex of BLAKE3 of output media>"`, `shot = "<scene-slug>/<shot-id>"`, provider, model+version, full inputs, seed, cost, timestamps, resolved-prompt snapshot, output hashes. `.gitattributes` sets `takes.manifest.toml merge=union` (EOF appends of content-addressed blocks merge cleanly). Bit-identical regenerations dedupe into one take with multiple generation records. Missing local media (fresh clone) renders as a grayed tile with "fetch or regenerate."
- **timeline.toml**: entries reference `"<scene-slug>/<shot-id>"` and resolve `selected_take` live (one source of truth with Dailies); trim in/out stored per entry, clamped to the current take's duration, with a "stale trim" badge when the circled take changes. Series form: `[[sequences]]` keyed by episode, feature = single implicit sequence.
- **CLI ops** that keep references intact: `autoteur mv scene|character|world|shot` (rewrites refs in the same commit), `autoteur renumber` (compacts NNN prefixes as a dedicated commit), `autoteur validate` (the same validator the GUI runs).
- **AGENTS.md** (generated at init) documents all of the above plus: write files atomically or whole-file, UTF-8 only, never reuse ids, append beats within their episode.

## Decision points for review

1. **Shot ids = per-scene letters** (`a`, `b`, … → displayed "12B"), matching production convention, vs descriptive slugs (`drill-bite-cu`). Letters chosen: directors already speak "shot 12B", ids stay one character, and the letter-reuse-after-delete hazard is closed by the high-water-mark rule.
2. **Scene identity = directory slug** (no in-file id). Copy-as-template can't duplicate identity; the cost is that renaming a scene's slug rewrites references (CLI-assisted, validator-guarded). Alternative: an in-file immutable id survives slug renames but reintroduces the duplicate-id-on-copy class.
3. **Inheritance semantics**: omitted shot `characters`/`world` inherit from the scene; explicit values replace. Alternative: always-explicit lists (no inheritance) — safer against a director editing the scene cast and silently changing old shots, but every shot carries boilerplate and cast edits require touching every shot.
