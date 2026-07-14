# Autoteur

**The director's chair. Your AI agent is the crew.**

Autoteur is an open-source desktop app for directing AI-generated films and TV series. You run a coding agent (Claude Code, or any agent) in a terminal beside it: the agent writes treatments, breaks scenes into shots, and queues generations by editing plain files and calling a CLI. Autoteur watches the project and updates live — new beats glide onto the board, new takes fade into Dailies, within a second of the agent writing them. You review, circle takes, reorder, and annotate visually; your choices are written back to the same files, so the crew sees the director's decisions on its next turn.

An Autoteur project is a plain git repository. Story lives in human-readable TOML and Markdown; generated media lives in a content-addressed `takes/` store with a committed manifest, so any take can be re-printed from the negative. Everything the GUI does is files + CLI underneath — the app is a live, bidirectional lens, never a walled garden.

## Status

Early development, moving fast. Working today: the complete headless loop (`autoteur` CLI: create → beats → scenes → shots → prompt resolution with character/world injection → Replicate generation → circled takes → FFmpeg export) and the Tauri desktop app with all nine rooms — Writers' Room, Beat Board, Casting, Locations & Props, Shot List, Dailies, Editing Room, Screening Room, Studio Settings — wired to the live-sync engine: file changes appear on screen within a second, and every GUI gesture is a surgical file edit. Format spec: [docs/proposals/0001-file-formats-and-live-sync.md](docs/proposals/0001-file-formats-and-live-sync.md).

Run the app from `apps/desktop` with `npm install && npm run tauri dev`.

## Layout

- `crates/autoteur-core` — domain types, TOML parsing/surgical editing, prompt resolution, validation
- `crates/autoteur-cli` — the `autoteur` command-line tool
- `apps/desktop` — Tauri 2 desktop app (React + TypeScript + Tailwind)

## License

Dual-licensed under MIT or Apache-2.0, at your option.
