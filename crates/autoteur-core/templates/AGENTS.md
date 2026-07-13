# Working in this Autoteur project

This repository is an Autoteur film/series project. A human director reviews
it in the Autoteur app, which watches every file here and updates its display
live — your edits appear on their screen within a second. The director's
decisions (circled takes, reordered beats, notes) are written back into these
same files, so read them fresh before acting on stale assumptions.

## The one-page contract

1. **File order IS the order.** `[[beats]]` block order = Beat Board order,
   `[[shots]]` block order = cut order, dialogue array order = speaking
   order. There are no position fields; never add one. Reorder by moving
   whole blocks. In a series, append a new beat after the last beat of the
   SAME episode.
2. **Identity is frozen at creation.** Beat and shot `id`s are never edited
   or reused. Characters and world entries are identified by their filename
   stem; scenes by the slug part of their directory name (`012-vault-breach`
   → identity `vault-breach`; the number is sort order only, gap-numbered in
   tens). When you copy a block or directory as a template, changing the id
   is the FIRST edit you make.
3. **Never write derived facts.** Take counts, generation status, costs,
   "has selection" — the app computes these live. Authored files change only
   when a human or agent makes a creative decision.
4. **Absence means the default.** No circled take = no `selected_take` key
   (never `""` or `"none"`). A shot without `characters` inherits the scene
   cast; `characters = []` means nobody in frame. A project without
   `[[episodes]]` is a feature film.
5. **One spelling per reference.** Beat: `"beat-id"`. Character/world:
   `"file-slug"`. Scene: `"scene-slug"` (never the numbered dirname). Shot
   from outside its scene: `"scene-slug/shot-id"`. Take: `"tk_"` + 12 hex.
   Asset paths are repo-relative with forward slashes.
6. **Write UTF-8 with LF line endings, atomically or whole-file.** Never
   shell-redirect with PowerShell 5.1 (`>` writes UTF-16 and corrupts the
   file). Keep root keys (like `schema_version`) ABOVE the first `[[...]]`
   or `[...]` header — TOML puts any key after a header inside that table.
7. **Keep every key of an entity inside its own block**, and keep each
   dialogue cue on a single line:
   `{ character = "mara-chen", line = "...", delivery = "optional" }`.

## File map

| Path | What it is |
| --- | --- |
| `autoteur.toml` | Project manifest: title, format, `[defaults]` (prompt template, negative, style world-slugs) |
| `story/logline.md`, `story/treatment.md` | Prose, plain Markdown |
| `story/beats.toml` | `[[episodes]]` (series only) + ordered `[[beats]]` index cards |
| `characters/<slug>.toml` | Cast: description, `[voice]`, `[prompt]` fragments (+variants), `[visual]` reference images & adapters — `[visual]` stays LAST in the file |
| `world/<slug>.toml` | Locations, props, vehicles, style bibles (`kind = "location" \| "prop" \| "vehicle" \| "style"`) |
| `scenes/<NNN>-<slug>/scene.toml` | Synopsis, `beats = [...]`, cast, location, world, mood, director notes |
| `scenes/<NNN>-<slug>/shots.toml` | Ordered `[[shots]]`: framing, camera, action, dialogue, `duration_s`, `status`, `selected_take`, prompt overrides |
| `takes.manifest.toml` | Machine-written record of every generation. Append-only; NEVER edit existing entries |
| `timeline.toml` | Editing Room assembly: shot refs + trim in/out (takes resolve via `selected_take` live) |
| `takes/` | Gitignored content-addressed media — never commit it |

## Semantics you must respect

- **Shot ids are per-scene letters** `a`, `b`, … `z`, `aa`, … assigned in
  creation order. The next id is one past the highest letter EVER used in
  this scene — check both `shots.toml` and `takes.manifest.toml` before
  minting. Never delete a shot that has takes; set `status = "omitted"`.
- **Shot `status`** is one of `planned` (don't generate yet), `ready`
  (approved to generate), `locked` (circled take is final — do not
  regenerate or rewrite this shot), `omitted` (cut, kept for the record).
- **Prompt system.** A shot's generation prompt is composed live:
  template = shot `prompt` (if any) else `[defaults].prompt_template` else a
  built-in. Placeholders: `{style} {framing} {camera} {action} {characters}
  {location} {world} {mood} {dialogue} {extra}`. Character/world prompt
  fragments and adapter trigger tokens are injected automatically for the
  shot's effective cast and setting. A literal prompt is just a template
  with no placeholders — reference images and LoRAs STILL attach from
  `characters`/`world`, so hand-tuned text never loses a face. Braces in
  action/dialogue prose are safe; only the template is substituted.
- **Series vs feature.** Feature files simply omit `[[episodes]]` and the
  `episode` key on beats. Converting to series is purely additive.
- **`schema_version`** is the first key of every TOML file. Unknown keys at
  file root are ignored and preserved; unknown keys inside a block are
  flagged as probable misplacement.

## CLI

`autoteur` (run it from the project root):

- `autoteur validate` — parse + lint every file, report dangling references.
  Run this after any batch of edits; exit code 0 = clean.
- `autoteur status --json` — machine-readable project state.
- `autoteur generate <scene-slug>/<shot-id>` / `--scene <scene-slug>` —
  queue generation for a shot / every `ready` shot in a scene.
- `autoteur render` — assemble the timeline into an MP4 via FFmpeg.

## Etiquette

- Prefer many small, valid writes over one giant rewrite; the director sees
  changes animate in live, and a file that parses at every step never
  flashes an error banner at them.
- After the director circles takes or reorders anything, your next read of
  the files is the ground truth — don't fight their decisions, build on them.
- Commit at natural milestones with plain-language messages ("Broke scene 12
  into shots"); the director sees these as save points they can restore.
