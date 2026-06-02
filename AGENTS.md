# AGENTS.md

## Project Overview

Yuri Rewrite is a Windows-first local desktop app for importing TXT novels, analyzing them with user-configured AI models, and rewriting content into dual-female-lead / yuri versions.

The app is local-first. Novel text, chapter batches, analysis output, rewrite drafts, canon assets, logs, model profiles, novel settings, and export settings are stored on the user's machine.

## Tech Stack

- Frontend: React + TypeScript + Vite
- Desktop shell: Tauri v2
- Backend: Rust Tauri commands
- Local storage: SQLite through `rusqlite`
- UI icons: `lucide-react`
- AI providers: OpenAI-compatible chat completions and Gemini

## Repository Layout

- `src/`: React frontend.
  - `App.tsx`: Main UI, navigation, model configuration, settings dialogs, logs page, compare page, workspace interactions.
  - `styles.css`: Application styling.
  - `main.tsx`: React entrypoint.
- `src-tauri/`: Rust / Tauri backend.
  - `src/lib.rs`: Tauri commands, SQLite schema and migrations, import/export logic, chapter splitting, model calls, AI logs.
  - `src/main.rs`: Tauri entrypoint. Release builds use `windows_subsystem = "windows"` to hide the console window.
  - `capabilities/default.json`: Tauri ACL permissions.
  - `tauri.conf.json`: Tauri app and bundle configuration.
- `scripts/package-portable.ps1`: Builds a portable Windows zip from the release executable.
- `portable/`: Generated local release artifacts. Ignored by git.
- `dist/`, `node_modules/`, `src-tauri/target/`: Generated artifacts. Ignored by git.

## Common Commands

Install dependencies:

```powershell
npm install
```

Run development app:

```powershell
npm run tauri:dev
```

Build frontend only:

```powershell
npm run build
```

Build Windows release:

```powershell
npm run tauri:build
```

Generate portable zip:

```powershell
npm run package:portable
```

The portable zip is generated at:

```text
portable/YuriRewrite-v0.1.0-windows-x64.zip
```

## Product Behavior

- TXT import should happen through Tauri backend commands, not browser-only file APIs.
- Chapter recognition should handle common Chinese web novel heading formats. When chapters cannot be detected, split the text by length.
- Chapter-based batching is fixed at 30 chapters per batch.
- Non-chapter batching is fixed at 100,000 characters per batch.
- Analysis and rewrite operate only on the selected batch.
- Rewrite should only process chapters in the selected batch that have completed analysis.
- After rewrite completes, the UI should navigate to the Compare page.
- Notifications should auto-dismiss after 5 seconds.

## Novel Settings

Each novel has its own settings record:

- `protagonist_name`: required before rewrite.
- `additional_feminize_names`: optional names to feminize if they appear in the text.
- `bust`: only `平胸` or `巨乳`.
- `body_type`: only `萝莉`, `御姐`, or `少女`.
- `advanced_settings`: free-form user instructions.

Important behavior:

- Settings are bound to `novel_id`.
- Deleting a novel must delete its settings.
- The Settings button is disabled when no novel is selected.
- If rewrite is clicked while required settings are missing, open the settings dialog.
- Do not automatically open the settings dialog after importing a novel.

## Prompting Rules

- Analysis prompts should analyze the original novel only. Do not inject basic or advanced rewrite settings into analysis prompts.
- Rewrite prompts must include basic settings and advanced settings.
- Rewrite rules must instruct the model to feminize the protagonist's name:
  - Prefer homophones or near-homophones.
  - Preserve the surname.
  - Replace masculine given-name characters with feminine alternatives.
  - Examples: `萧炎 -> 萧妍`, `李火旺 -> 李火婉`.
- Optional names should only be feminized if they appear in the processed text.
- Analysis should extract or maintain a name-feminization mapping when applicable.
- Rewrite should reuse the mapping consistently across the novel.

## Backend Guidelines

- Add app capabilities as Tauri commands in `src-tauri/src/lib.rs`.
- When adding persistent data, update `init_db` and include lightweight migration logic through `ensure_column` or explicit migration statements.
- Keep destructive operations scoped:
  - Deleting a novel must delete chapters, chapter batches, internal batch TXT files, novel settings, canon assets, jobs, rewrites, and related AI logs.
  - Deleting a model profile must delete its stored API key and related logs.
- API keys must never be logged.
- AI logs may include model outputs and raw provider responses, but must not include API keys or Authorization headers.
- For AI provider errors, preserve the provider response body in the user-facing error where practical.
- For successful AI calls, store:
  - `content`: extracted output text.
  - `reasoning`: model thinking / reasoning content if returned.
  - `raw_response`: raw provider JSON.

## Frontend Guidelines

- The UI is an operational desktop tool, not a landing page.
- Keep navigation predictable:
  - The left brand button returns to the main workspace.
  - The top menu contains cross-workspace views such as Compare and Settings.
  - The left sidebar contains import, novel list, model selector, logs, and application settings.
- Use modal dialogs for novel settings.
- Keep long text in scrollable regions with stable dimensions.
- Use the Compare page for large original/rewrite text panes.
- Do not put large original/rewrite panes inside small workspace cards.
- Use icon buttons for compact actions where possible.
- Make selected chapter titles visible; status text must not overlap the title.

## Build and Release Notes

- Do not commit generated build outputs such as `dist/`, `node_modules/`, `portable/`, or `src-tauri/target/`.
- Commit lockfiles and source-managed Tauri schema/capability files.
- Release target is Windows x64 portable zip.
- `tauri build` also creates installer artifacts, but the intended distribution artifact is the portable zip.
- The app is currently unsigned, so Windows SmartScreen may warn users about an unknown publisher.

## Verification Checklist

Before handing off functional changes:

1. Run `npm run build`.
2. Run `cargo check` or relevant Rust tests when backend code changed.
3. Run `npm run tauri:build` for release-impacting changes.
4. Run `npm run package:portable` when producing a user-distributable build.
5. Confirm `git status -sb` before committing or pushing.
