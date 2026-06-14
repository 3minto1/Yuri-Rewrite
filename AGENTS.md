# AGENTS.md

## Project Overview

Yuri Rewrite is a Windows-first, local-first Tauri desktop application. It imports TXT novels, analyzes original canon with user-configured AI models, and rewrites eligible chapters into dual-female-lead / yuri versions.

The user owns the AI account and API key. Novel content, SQLite data, internal batch files, rewrite drafts, canon assets, logs, settings, and exports are stored locally. Online model calls send only the content required for the selected operation to the configured provider.

## Tech Stack

- Frontend: React 18, TypeScript, Vite, Zustand, Vitest, React Testing Library
- Desktop: Tauri v2
- Backend: Rust
- Storage: SQLite through `rusqlite`
- Credentials: Windows Credential Manager through `keyring`, with an explicit SQLite fallback
- UI icons: `lucide-react`
- AI providers: OpenAI-compatible chat completions and Gemini

## Current Repository Layout

- `src/App.tsx`: top-level navigation, orchestration, and page composition.
- `src/components/Workspace/`: workspace panels such as chapter, batch, model, and task views.
- `src/components/Settings/`: application, novel, and model settings views.
- `src/components/Compare/`: compare page, global search, diff worker, and highlighting.
- `src/components/common/`: shared modal, error boundary, and layout components.
- `src/hooks/`: novel, model-profile, and task-state hooks.
- `src/store/appStore.ts`: non-persistent Zustand runtime state.
- `src/types/`: shared frontend domain types.
- `src/tauriApi.ts`: strongly typed Tauri command boundary. Keep command names and argument mappings centralized here.
- `src-tauri/src/commands/`: Tauri commands grouped by domain.
- `src-tauri/src/ai/`: provider calls, prompts, response parsing, and shared AI behavior.
- `src-tauri/src/db/`: SQLite schema and migrations.
- `src-tauri/src/text/`: encoding detection and chapter splitting.
- `src-tauri/src/credentials.rs`: system credential and database-fallback behavior.
- `src-tauri/src/task_control.rs`: active-task locking and cancellation.
- `src-tauri/src/lib.rs`: application setup, shared orchestration, and `generate_handler!` registration.
- `scripts/package-portable.ps1`: portable Windows packaging.
- `clean-debug-cache.ps1`: guarded cleanup for Rust debug/dev artifacts only.

## Windows Shell Rules

Commands that may print Chinese text must set UTF-8 output first:

```powershell
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8
chcp 65001 | Out-Null
```

Use explicit UTF-8 encoding when reading or writing text files that may contain Chinese, for example `Get-Content -Encoding UTF8`. Use `apply_patch` for source-controlled manual edits.

## Common Commands

```powershell
npm install
npm run tauri:dev
npm test
npm run build
cargo test --manifest-path .\src-tauri\Cargo.toml
cargo clippy --manifest-path .\src-tauri\Cargo.toml --all-targets --all-features -- -D warnings
npm run tauri:build
npm run package:portable
```

Safe debug-cache cleanup:

```powershell
powershell -ExecutionPolicy Bypass -File .\clean-debug-cache.ps1 -DryRun
powershell -ExecutionPolicy Bypass -File .\clean-debug-cache.ps1
```

The cleanup script must remain scoped to `src-tauri/target/debug` and Cargo's dev profile. It must not remove source files, release artifacts, user data, `node_modules`, or portable packages.

## Architecture Boundaries

- Keep Tauri command names, serialized fields, event names, and argument shapes backward compatible unless a change is explicitly requested.
- Add new frontend command calls to `src/tauriApi.ts`; do not scatter raw `invoke` strings through components.
- Keep API keys and unsaved secret form values in local component state, never Zustand.
- Zustand stores runtime UI/domain state only and is not a second persistence layer. SQLite and Tauri remain the source of truth.
- Prefer domain modules under `commands`, `ai`, `db`, and `text`. Do not move unrelated logic back into a monolithic `lib.rs` or `App.tsx`.
- Preserve existing prompts and parsing behavior during structural refactors. Move one domain at a time and test immediately.
- Use `pub(crate)` for internal Rust APIs unless a wider public surface is required.

## Core Product Invariants

### Import and Chapters

- TXT import goes through Tauri backend commands, not browser-only file APIs.
- Recognize common Chinese web-novel heading units including `章`, `节`, `回`, `卷`, `部`, `篇`, `集`, `幕`, `话`, `夜`, `案`, `场`, `弹`, `折`, and `更`.
- Loose numbered headings are fallback-only. Use them only when standard headings are absent and candidate numbers are sequential.
- If no chapters can be detected, split by text length.
- Chapter-based batches contain 30 chapters. Non-chapter batches contain at most 100,000 characters.

### Analysis and Rewrite

- Analysis requires a novel-settings record but analyzes the original novel only. Do not inject yuri instructions, feminization settings, body settings, or advanced rewrite instructions into analysis prompts.
- Analysis produces compact original-canon assets: outline, characters, original genders, pronouns, aliases, relationships, titles, locations, foreshadowing, terms, and name-mapping candidates.
- Rewrite processes only the selected batch and only chapters eligible after analysis.
- Rewrite prompts include global core settings before normal rules, then novel settings, advanced settings, compact canon, and stable name mappings.
- Forced protagonist naming has highest priority. Otherwise use one consistent feminine mapping across shards and batches.
- Do not alter non-target characters' gender, pronouns, titles, seniority, relationships, or social roles.
- Remove masculine residue from the target protagonist while preserving plot continuity and established appearance details.
- Strict mode preserves plot and avoids unnecessary embellishment. Creative mode may reinforce female identity, appearance, expression, and dual-female-lead interaction without breaking continuity.

### Stable Chapter Parsing

- Batch and shard rewrite output uses stable chapter start/end markers and must preserve chapter order and count.
- Parse only output that can be mapped reliably to the requested chapters.
- Missing markers, truncated output, extra unrelated output, or ambiguous marker-free output must trigger a bounded retry, smaller subdivision, or a clear failure. Never write corrupted text into chapters.
- Provider `content_filter` responses are provider safety errors, not marker errors.
- MiMo-specific prompt sanitization may soften direct body-type wording, but must not weaken prompts for other providers.

### Optional Dual-Expert Review

- Review is disabled by default because it substantially increases request count and wait time.
- When enabled, the rewrite model produces a full shard and the review model returns JSON approval/issues.
- Rejected drafts may be regenerated twice. Each regeneration must return the full shard with the original stable markers and must be reviewed again.
- If the third decision still fails, append a per-novel warning, save the second regenerated draft, and continue later shards instead of failing the whole batch.
- Logs must distinguish draft generation, review decisions, rejection rewrites, final review, and fallback warning paths.

### Task Lifecycle

- Validate model, API key, settings, batch, and output directory before registering an active task.
- Use one cleanup guard so success, failure, cancellation, pause, or early return always releases the task lock.
- Reject duplicate active tasks for the same novel.
- Reject deletion of a novel or model used by an active task.
- Progress events remain `job-progress` and must be filtered by `novel_id` and the current task ID in the frontend.
- Disable novel/model switching, import, deletion, and relevant settings changes while the active task makes those operations unsafe.
- Parallel shard failure must cancel and await sibling requests so quota is not consumed in the background.
- Full one-click runs batches in order: analyze, rewrite, export the batch, then continue. It supports pause, continue, and terminate. Continue restarts from the first unfinished batch and reruns that batch's analysis.

### Provider Calls and Logs

- HTTP clients use a connection timeout and a bounded request timeout. Timeout errors must be explicit.
- Remove unsupported thinking parameters and retry only for HTTP 400/422 responses that clearly identify parameter incompatibility. Do not duplicate 401, 403, 429, or 5xx requests.
- Gemini reasoning consists of all `thought: true` text parts; final content consists of all other text parts. Do not assume `parts[0]` is the answer.
- Preserve provider response bodies in user-facing errors where practical.
- Successful AI logs store extracted content, reasoning, raw provider JSON, input/output character counts, duration, review state, and thinking mode.
- Never log API keys, Authorization headers, or credential-store contents.

### Settings and Export

- Novel settings are keyed by `novel_id`; the protagonist name is required before analysis or rewrite.
- Do not automatically open novel settings after import. Open them when a required operation is attempted without valid settings.
- Application settings include export directory, global core prompt, review configuration, and shared analysis/rewrite concurrency.
- Allowed concurrency values remain `10`, `6`, `3`, and `1`, with `6` as the default unless the product behavior is intentionally changed.
- Export TXT only. Include only chapters with completed rewrite status and non-empty rewrite text. Never fall back to the original text.
- After normal rewrite completion, navigate to Compare. Preserve the selected batch after refreshes.

## Credentials and Destructive Operations

- Prefer Windows Credential Manager for API keys.
- Use SQLite plaintext fallback only when system credential storage fails, and expose `api_key_storage` as `system`, `database_fallback`, or `none`.
- After a verified system-store write, clear the corresponding SQLite fallback. Retry migration of old fallback keys at startup.
- Model deletion separately reports database deletion and credential deletion failures; never silently ignore credential cleanup failure.
- Novel deletion requires a confirmation dialog describing all affected files and records.
- Deleting a novel must remove chapters, batches, internal batch TXT files, settings, canon assets, jobs, rewrites, review warnings, and related AI logs.
- Move batch directories to the temporary recycle area before committing database deletion. Startup cleanup handles leftovers.

## Frontend Behavior and Performance

- The UI is an operational desktop tool. Keep navigation and existing control placement predictable.
- Esc and visible Back buttons return non-workspace pages to the workspace without closing unrelated settings dialogs.
- Notifications auto-dismiss after five seconds unless a specific dialog requires persistence.
- First launch shows quick-start once; Help reopens the same content.
- Keep model configuration, chapters, canon assets, and other long content in independent, stable scroll regions.
- Use the Compare page for full original/rewrite text. Do not place large text panes in workspace cards.
- Compare search is plain-text, cross-chapter, searches original then rewrite, supports next/previous navigation, and excludes empty rewrite placeholders.
- Compare diff is current-chapter-only and defaults on. Search highlighting has higher visual priority than diff highlighting.
- Diff state is bound to chapter ID and text versions. Never apply stale ranges or stale Worker results to a new chapter.
- Keep the 12-entry in-memory LRU diff cache and cancellation of obsolete per-calculation Workers unless replaced by an equally interruptible design.
- Prefer CSS Custom Highlight API so each text pane remains a single text node. Preserve the memoized linear-scan fallback for older WebView2 versions.
- Mixed diff has a time budget and degrades to line mode for excessive cost/ranges, then to plain mode if necessary. Responsiveness is more important than forcing fine-grained highlights.
- Do not add full-text virtualization or `content-visibility` to visible compare panes without proving text selection, search positioning, and scroll height remain correct.

## Documentation Rules

- Keep `README.md` beginner-oriented: installation, prerequisites, first successful workflow, troubleshooting, privacy, and developer commands should be easy to find.
- Do not put a fixed application version number in `README.md`; releases change independently and stale version text creates unnecessary maintenance.
- Document user-visible behavior accurately. Do not promise provider availability, fixed model IDs, or pricing.

## Editing and Git Safety

- Default to ASCII in source files unless Chinese content or an existing Unicode context requires otherwise.
- Add comments only when they explain non-obvious behavior.
- Keep changes scoped and preserve unrelated user modifications in a dirty worktree.
- Never use destructive Git commands such as `git reset --hard` or `git checkout --` without an explicit request.
- Do not commit generated outputs: `dist/`, `node_modules/`, `portable/`, or `src-tauri/target/`.
- Commit lockfiles and source-managed Tauri schemas/capabilities when they intentionally change.

## Verification

Match verification effort to the change, but complete the relevant checks before handoff:

1. Frontend or shared changes: `npm test` and `npm run build`.
2. Backend changes: `cargo test --manifest-path .\src-tauri\Cargo.toml`.
3. Rust control-flow or release changes: strict Clippy.
4. Release-impacting changes: `npm run tauri:build`.
5. User-distributable builds: delete old local portable zips, then run `npm run package:portable`.
6. All commits: `git diff --check` and `git status -sb`.

Do not update versions, generate releases, upload portable packages, or create GitHub Releases unless the user explicitly asks for that release step. The intended release asset is the Windows x64 portable zip; installer artifacts are incidental.
