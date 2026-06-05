# AGENTS.md

## Project Overview

Yuri Rewrite is a Windows-first local desktop app for importing TXT novels, analyzing them with user-configured AI models, and rewriting content into dual-female-lead / yuri versions.

The app is local-first. Novel text, chapter batches, internal batch TXT files, analysis output, rewrite drafts, canon assets, logs, model profiles, novel settings, app settings, and export output are stored on the user's machine.

## Tech Stack

- Frontend: React + TypeScript + Vite
- Desktop shell: Tauri v2
- Backend: Rust Tauri commands
- Local storage: SQLite through `rusqlite`
- UI icons: `lucide-react`
- AI providers: OpenAI-compatible chat completions and Gemini

## Repository Layout

- `src/`: React frontend.
  - `App.tsx`: Main UI, navigation, model configuration, settings dialogs, logs page, compare page, workspace interactions, one-click controls.
  - `styles.css`: Application styling.
  - `main.tsx`: React entrypoint.
- `src-tauri/`: Rust / Tauri backend.
  - `src/lib.rs`: Tauri commands, SQLite schema and migrations, import/export logic, chapter splitting, model calls, batch analysis/rewrite, AI logs.
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
portable/YuriRewrite-v{version}-windows-x64.zip
```

## Product Behavior

- TXT import should happen through Tauri backend commands, not browser-only file APIs.
- Chapter recognition should handle common Chinese web novel heading formats. When chapters cannot be detected, split the text by length.
- Chapter-based batching is fixed at 30 chapters per batch.
- Non-chapter batching is fixed at 100,000 characters per batch.
- Analysis requires the novel settings record to exist, but the analysis prompt must still analyze the original novel only.
- Analysis works at batch level and may split a batch into parallel shards. It should produce or merge compact consistency assets instead of chapter-by-chapter bulky JSON.
- Rewrite operates only on the selected batch and only processes chapters in that batch that have completed analysis.
- Rewrite works at batch or shard level with stable machine chapter markers, then parses model output back into per-chapter `rewrite_text` for the Compare page.
- Export supports TXT only and must include only chapters with `rewrite_status = 'completed'` and non-empty `rewrite_text`; never fall back to original text.
- Current-batch one-click runs analysis then rewrite for the selected batch.
- Full one-click runs batches in order: analyze batch, rewrite batch, export `{novel_title}_第N批.txt`, then continue. At the end it exports the full rewritten TXT and keeps all per-batch files.
- Full one-click supports pause, continue, and terminate. Pause/terminate requests abort in-flight AI work where possible. Continue restarts from the first unfinished batch; an unfinished batch is rerun from analysis.
- After normal rewrite completes, the UI should navigate to the Compare page.
- The selected batch should remain selected after analysis/rewrite refreshes; do not unexpectedly jump back to the first batch.
- Esc and visible Back buttons should return non-workspace pages to the workspace without disrupting open settings dialogs.
- Notifications should auto-dismiss after 5 seconds unless a specific dialog behavior is required.

## Novel Settings

Each novel has its own settings record:

- `protagonist_name`: required before analysis and rewrite.
- `rewritten_protagonist_name`: optional forced rewritten protagonist name. If filled, rewrite must use it consistently.
- `additional_feminize_names`: optional names to feminize if they appear in processed text.
- `bust`: only `平胸` or `巨乳`.
- `body_type`: only `萝莉`, `御姐`, or `少女`.
- `rewrite_mode`: only `strict` or `creative`.
- `advanced_settings`: free-form user instructions.

Important behavior:

- Settings are bound to `novel_id`.
- Deleting a novel must delete its settings.
- The Settings button is disabled when no novel is selected.
- If analysis, rewrite, current-batch one-click, or full one-click is clicked while required settings are missing, open the settings dialog.
- Do not automatically open the settings dialog after importing a novel.
- Strict mode should preserve the original plot and avoid unnecessary embellishment while still completing required feminization.
- Creative mode should more actively reinforce the protagonist's female identity, appearance details, expressions, and dual-female-lead interactions, while keeping plot continuity and character consistency.

## App Settings

- `export_dir`: output folder for exported TXT files and one-click batch files.
- `review_enabled`: optional AI review pass after rewrite. Default is off because it increases rewrite time.
- `rewrite_parallelism`: shared concurrency setting for analysis and rewrite. Allowed values are `10`, `6`, `3`, and `1`; default is `6`.
- Higher concurrency can reduce wall-clock time, but it increases request count, failure probability, and may slightly increase token usage. Keep prompts and parsing robust across shard boundaries.

## Model Profiles

- Model profiles support OpenAI-compatible chat completions and Gemini.
- API keys must be stored locally and never logged.
- Saving a new model profile must not discard existing profiles.
- The UI can suggest model IDs when Base URL or model name indicates common providers, including DeepSeek, OpenAI, Kimi / Moonshot, MiniMax, Xiaomi MiMo, SiliconFlow, and Claude-compatible endpoints.
- `thinking_mode` supports `auto`, `off`, and `on`. Provider compatibility varies; unsupported thinking parameters should be retried without those parameters when practical.
- DeepSeek analysis should use official JSON output when applicable.
- Provider `content_filter` responses should be shown as safety/provider interception errors, not as marker parsing errors.
- MiMo-family models need softer prompt wording for direct body-type terms that may trigger filtering. This sanitization is MiMo-specific and should not weaken non-MiMo prompts.

## Prompting Rules

- Analysis prompts should analyze the original novel only. Do not inject yuri rewrite instructions, feminization instructions, basic rewrite settings, or advanced rewrite settings into analysis prompts.
- Analysis should extract compact original-canon assets: outline, characters, original genders, pronouns, names and aliases, relationships, titles, locations, foreshadowing, terms, and name mapping candidates when available.
- Rewrite prompts must include basic settings, advanced settings, compact consistency assets, and name mapping rules.
- Name mapping has highest priority:
  - If `rewritten_protagonist_name` is filled, force the protagonist to that name everywhere, including titles and body text.
  - Otherwise, feminize the protagonist's name consistently, preferably with homophones or near-homophones while preserving the surname.
  - Replace masculine given-name characters with feminine alternatives where appropriate.
  - Examples: `萧炎 -> 萧妍`, `李火旺 -> 李火婉`.
- Optional names should only be feminized if they appear in the processed text.
- Reuse the same name mapping across shards and batches. Never let different shards invent different rewritten names for the same person.
- Preserve non-target characters' original gender, pronouns, titles, seniority, relationships, and social role unless they are explicitly listed for feminization. A male supporting character must not drift into female pronouns in later chapters.
- The protagonist's male-coded descriptions must be rewritten into female-coded descriptions so that a new reader cannot tell the protagonist was originally male.
- Appearance details must remain consistent across chapters. If the protagonist is established as black-haired, do not change her to blond or red-haired in later chapters unless the original plot explicitly changes it.
- Rewrite must preserve chapter order, chapter count, stable start/end markers, and per-chapter boundaries. If AI output is missing markers, includes extra unrelated content after markers, or cannot be parsed reliably, retry or fail clearly instead of writing corrupted chapters.
- Review prompts, when enabled, should check and repair protagonist name, optional name mappings, pronouns, titles, masculine residue, appearance consistency, logic breaks, and chapter boundaries.

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
  - input character count.
  - output character count.
  - AI call duration.
  - whether review was enabled for that call.
  - thinking mode used for that call.

## Frontend Guidelines

- The UI is an operational desktop tool, not a landing page.
- Keep navigation predictable:
  - The left brand button returns to the main workspace.
  - The top menu contains cross-workspace views such as Compare and Settings.
  - The left sidebar contains import, novel list, model selector, logs, and application settings.
- Use modal dialogs for novel settings.
- Keep long text in scrollable regions with stable dimensions.
- Use the Compare page for large original/rewrite text panes and the TXT export entry.
- Do not put large original/rewrite panes inside small workspace cards.
- Use icon buttons for compact actions where possible.
- Make selected chapter titles visible; status text must not overlap the title.
- One-click pause, continue, and terminate controls should be visible only while a full one-click run is active or paused.
- Settings page should focus on actual controls; do not add decorative subtitles such as "配置导出目录".

## Build and Release Notes

- Do not commit generated build outputs such as `dist/`, `node_modules/`, `portable/`, or `src-tauri/target/`.
- Commit lockfiles and source-managed Tauri schema/capability files.
- Release target is Windows x64 portable zip.
- `tauri build` also creates installer artifacts, but the intended distribution artifact is the portable zip.
- The app is currently unsigned, so Windows SmartScreen may warn users about an unknown publisher.
- Do not update version numbers, create releases, or push generated portable packages unless the user explicitly asks for that release/upload step.
- When generating a new local portable package, delete old local portable zips first.

## Verification Checklist

Before handing off functional changes:

1. Run `npm run build`.
2. Run `cargo check` or relevant Rust tests when backend code changed.
3. Run `npm run tauri:build` for release-impacting changes.
4. Run `npm run package:portable` when producing a user-distributable build.
5. Confirm `git status -sb` before committing or pushing.
