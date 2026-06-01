# AGENTS.md

## Project Overview

Yuri Rewrite is a Windows-first local desktop app for importing TXT novels, analyzing chapters with user-configured AI models, and rewriting content into dual-female-lead/yuri versions.

The app is built with:

- Frontend: React + TypeScript + Vite
- Desktop shell: Tauri v2
- Backend: Rust Tauri commands
- Local storage: SQLite via `rusqlite`
- AI providers: OpenAI-compatible chat completions and Gemini

The app is local-first. Novel text, analysis, rewrite output, model profiles, logs, and settings are stored on the user's machine.

## Repository Layout

- `src/`: React frontend.
  - `App.tsx`: Main UI, navigation, model config, logs page, settings page, compare page.
  - `styles.css`: App styling.
  - `main.tsx`: React entrypoint.
- `src-tauri/`: Rust/Tauri backend.
  - `src/lib.rs`: Tauri commands, SQLite schema/migrations, model calls, import/export logic.
  - `src/main.rs`: Tauri entrypoint.
  - `capabilities/default.json`: Tauri ACL permissions.
  - `tauri.conf.json`: Tauri config.
- `scripts/package-portable.ps1`: Builds a portable Windows zip from the release executable.
- `portable/`: Generated local release artifacts. Ignored by git.

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

## Implementation Notes

- Keep the app local-first. Do not add cloud sync, accounts, or remote storage unless explicitly requested.
- Do not commit generated build outputs such as `dist/`, `node_modules/`, `portable/`, or `src-tauri/target/`.
- Do commit lockfiles and source-managed Tauri schema/capability files.
- API keys must never be logged. The backend stores API keys through Windows secure storage when possible and falls back to the local SQLite profile record.
- AI logs may include model outputs and raw provider responses, but must not include API keys or Authorization headers.
- TXT import reads local files through Tauri commands, not browser-only file APIs.
- Export path is configurable through app settings; if unset, export files go to the app data `exports` directory.

## Backend Guidelines

- Add new app capabilities as Tauri commands in `src-tauri/src/lib.rs`.
- When adding persistent data, update `init_db` and include lightweight migration logic via `ensure_column` or explicit migration statements.
- Keep destructive operations scoped:
  - Deleting a novel must delete chapters, canon assets, jobs, and related AI logs.
  - Deleting a model profile must delete its stored API key and related logs.
- For AI provider errors, preserve the provider response body in the user-facing error where practical.
- For successful AI calls, store:
  - `content`: extracted output text
  - `reasoning`: model thinking/reasoning content if returned
  - `raw_response`: raw provider JSON

## Frontend Guidelines

- The UI is an operational desktop tool, not a landing page.
- Keep navigation predictable:
  - Left brand button returns to the main workspace.
  - Top menu contains cross-workspace views such as Compare.
  - Left sidebar contains import, novel list, model selector, logs, and settings.
- Keep long text in scrollable regions with stable dimensions.
- Use icon buttons for menus and actions where possible.
- Do not put large original/rewrite text panes inside small workspace cards; use the Compare page for full side-by-side reading.
- Notifications should auto-dismiss after 5 seconds.

## Verification Checklist

Before handing off changes:

1. Run `npm run build`.
2. Run `npm run tauri:build`.
3. Run `npm run package:portable`.
4. Confirm `git status -sb` is clean after committing/pushing when requested.

## Release Notes

The current release target is Windows x64 portable zip. The project also produces an NSIS installer during `tauri build`, but the intended user distribution artifact is the portable zip.

The app is currently unsigned, so Windows SmartScreen may warn users about an unknown publisher.
