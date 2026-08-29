// @vitest-environment node

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const aggregate = readFileSync(new URL("../styles.css", import.meta.url), "utf8");
const base = readFileSync(new URL("./base.css", import.meta.url), "utf8");
const components = readFileSync(new URL("./components.css", import.meta.url), "utf8");
const layout = readFileSync(new URL("./layout.css", import.meta.url), "utf8");
const motion = readFileSync(new URL("./motion.css", import.meta.url), "utf8");
const responsive = readFileSync(new URL("./responsive.css", import.meta.url), "utf8");
const tokens = readFileSync(new URL("./tokens.css", import.meta.url), "utf8");
const views = readFileSync(new URL("./views.css", import.meta.url), "utf8");

describe("style layer integrity", () => {
  it("loads responsive overrides after component and view defaults", () => {
    const componentsIndex = aggregate.indexOf('./styles/components.css');
    const viewsIndex = aggregate.indexOf('./styles/views.css');
    const responsiveIndex = aggregate.indexOf('./styles/responsive.css');

    expect(componentsIndex).toBeGreaterThanOrEqual(0);
    expect(viewsIndex).toBeGreaterThan(componentsIndex);
    expect(responsiveIndex).toBeGreaterThan(viewsIndex);
  });

  it("keeps dedicated theme-aware foreground tokens for sidebar controls", () => {
    expect(tokens).toContain("--color-sidebar-text:");
    expect(tokens).toContain("--color-sidebar-muted:");
    expect(layout).toMatch(/\.nav-button,\s*\.novel-item,\s*\.menu-trigger\s*\{[^}]*color: var\(--color-sidebar-text\);/s);
    expect(layout).toMatch(/\.sidebar-collapse-toggle\s*\{[^}]*color: var\(--color-sidebar-text\);/s);
    expect(layout).toMatch(/\.sidebar-novel-filter input\s*\{[^}]*color: var\(--color-sidebar-text\);/s);
    expect(components).toMatch(/\.menu-trigger\s*\{[^}]*color: var\(--color-sidebar-text\);/s);
  });

  it("keeps compact compare and A/B rules in the final responsive layer", () => {
    expect(layout).not.toContain("@media (max-width");
    expect(responsive).toContain("@media (max-width: 1120px)");
    expect(responsive).toMatch(/\.compare-word-count\s*\{[^}]*min-width: 0;/s);
    expect(responsive).toMatch(/\.rewrite-ab-model-progress\s*\{[^}]*grid-template-columns:/s);
    expect(views).toMatch(/\.estimate-grid\s*\{[^}]*grid-template-columns: repeat\(3,/s);
  });

  it("defines every shared color token used by the style layers", () => {
    const styles = [tokens, base, layout, components, views, responsive, motion].join("\n");
    const definitions = new Set(Array.from(styles.matchAll(/(--color-[a-z0-9-]+)\s*:/g), (match) => match[1]));
    const usages = new Set(Array.from(styles.matchAll(/var\((--color-[a-z0-9-]+)/g), (match) => match[1]));
    const missing = [...usages].filter((token) => !definitions.has(token));

    expect(missing).toEqual([]);
    expect(tokens).toContain("--color-brand: #a14e2c;");
    expect(tokens).toContain("--color-brand-2: #64765a;");
    expect(tokens).toContain("--color-token-input: #5c7355;");
    expect(tokens).toContain("--color-token-output: #a2762a;");
  });

  it("keeps danger and diff semantics independent from the brand palette", () => {
    expect(components).toMatch(/\.status-danger\s*\{[^}]*--status-color: var\(--color-danger\);/s);
    expect(views).toMatch(/\.log-item\.error\s*\{[^}]*border-color: var\(--color-danger\);/s);
    expect(views).toMatch(/\.log-day-tabs button\.active\s*\{[^}]*border-color: color-mix\(in srgb, var\(--color-brand\)/s);
    expect(views).toMatch(/\.diff-removed\s*\{[^}]*background: var\(--color-danger-soft\);/s);
    expect(views).toMatch(/\.diff-added\s*\{[^}]*background: var\(--color-success-soft\);/s);
  });

  it("uses the Windows UI font stacks and supported numeric weights", () => {
    const styles = [tokens, base, layout, components, views, responsive, motion].join("\n");
    const numericWeights = Array.from(styles.matchAll(/font-weight\s*:\s*(\d+)/g), (match) => Number(match[1]));
    const fontFamilies = Array.from(styles.matchAll(/font-family\s*:\s*([^;]+);/g), (match) => match[1].trim());

    expect(tokens).toContain(
      '--font-ui: "Segoe UI Variable Text", "Segoe UI", "Microsoft YaHei UI",',
    );
    expect(tokens).toMatch(/--font-serif:[^;]+serif;/);
    expect(numericWeights.every((weight) => [400, 500, 600, 700].includes(weight))).toBe(true);
    expect(fontFamilies.every((family) => family === "var(--font-ui)" || family === "var(--font-serif)" || family === "inherit")).toBe(true);
    expect(styles).not.toMatch(new RegExp(`\\b${["Int", "er"].join("")}\\b`));
  });

  it("stays flat: no gradient decorations anywhere in the style layers", () => {
    const styles = [tokens, base, layout, components, views, responsive, motion].join("\n");

    expect(styles).not.toContain("linear-gradient");
    expect(styles).not.toContain("radial-gradient");
  });

  it("keeps the activity rail tooltips dark ink with surface-bound status dots", () => {
    expect(layout).toMatch(/\.activity-tooltip\s*\{[^}]*background: var\(--color-ink\);/s);
    expect(layout).toMatch(/\.activity-progress-dot,\s*\.activity-update-dot\s*\{[^}]*border: 2px solid var\(--color-sidebar-surface\);/s);
  });

  it("keeps quick start on a plain surface without decorative artwork", () => {
    expect(components).toMatch(/\.quickstart-dialog\s*\{[^}]*background: var\(--color-surface-raised\);/s);
    expect(components).not.toContain("quick-start-bg.png");
  });

  it("keeps the task estimate compact with readable values", () => {
    expect(layout).toMatch(/\.task-center-scroll\s*\{[^}]*display: flex;[^}]*flex-direction: column;/s);
    expect(layout).toMatch(/\.task-center-estimate\s*\{[^}]*flex: 0 0 auto;[^}]*min-height: 0;/s);
    expect(layout).toMatch(/\.task-center-estimate \.estimate-grid\s*\{[^}]*display: flex;[^}]*flex-direction: column;/s);
    expect(layout).toMatch(/\.task-center-estimate \.estimate-item\s*\{[^}]*display: flex;[^}]*justify-content: space-between;/s);
    expect(layout).toMatch(/\.task-center-estimate \.estimate-item\s*\{[^}]*min-height: 38px;/s);
    expect(layout).toMatch(/\.task-center-estimate \.estimate-item strong\s*\{[^}]*font-variant-numeric: tabular-nums;/s);
    expect(layout).not.toMatch(/\.task-center-estimate\s*\{[^}]*min-height: 100%;/s);
    expect(layout).not.toMatch(/\.task-center-estimate \.estimate-grid\s*\{[^}]*grid-template-rows:/s);
  });

  it("keeps chapter rows as single-line index entries with dot-only state", () => {
    expect(layout).toMatch(/\.workspace-chapter-surface \.chapter-item,\s*\.chapter-item\s*\{[^}]*grid-template-columns: auto minmax\(0, 1fr\) auto;/s);
    expect(layout).toMatch(/\.chapter-state-dot\s*\{[^}]*border-radius: 50%;/s);
    expect(components).not.toMatch(/\.chapter-status-row/);
    expect(views).toMatch(/\.chapter-title\s*\{[^}]*text-overflow: ellipsis;/s);
  });

  it("keeps the activity rail as a flat 48px strip with a left edge active indicator", () => {
    expect(layout).toMatch(/\.app-shell\s*\{[^}]*grid-template-columns: 48px 232px minmax\(0, 1fr\);/s);
    expect(layout).toMatch(/\.activity-rail-button\.active::before\s*\{[^}]*height: 100%;[^}]*width: 2px;/s);
    expect(layout).toMatch(/\.activity-brand\s*\{[^}]*background: transparent;[^}]*color: var\(--color-sidebar-text\);/s);
    expect(layout).toMatch(/\.activity-rail-primary::before,\s*\.activity-rail-secondary::before\s*\{[^}]*background: var\(--color-sidebar-border\);/s);
    expect(layout).toMatch(/\.activity-rail-button\.active\s*\{[^}]*color: var\(--color-sidebar-text\);/s);
  });

  it("keeps a persistent workspace status bar", () => {
    expect(layout).toMatch(/\.workspace-status-bar\s*\{[^}]*border-top: 1px solid var\(--color-border\);/s);
    expect(layout).toMatch(/\.workspace-status-state::before\s*\{[^}]*border-radius: 50%;/s);
    expect(layout).toMatch(/\.workspace-status-state\.tone-progress::before\s*\{[^}]*background: var\(--color-progress\);/s);
  });

  it("keeps the workspace content on a raised content layer above the chrome", () => {
    expect(tokens).toMatch(/--color-content:\s*#fffcf5;/);
    expect(tokens).toMatch(/:root\[data-theme="dark"\]\s*\{[^}]*--color-content:\s*#2a251c;/s);
    expect(layout).toMatch(/\.workspace-chapter-surface\s*\{[^}]*background: var\(--color-content\);/s);
  });

  it("keeps destructive dialogs readable in both themes", () => {
    expect(tokens).toContain("--color-danger-contrast: #ffffff;");
    expect(tokens).toMatch(/:root\[data-theme="dark"\]\s*\{[^}]*--color-danger-contrast: #2b120c;/s);
    expect(components).toMatch(/\.dialog-danger\s*\{[^}]*color: var\(--color-danger-contrast\);/s);
  });

  it("keeps status badges quiet with color only in the dot and abnormal text", () => {
    expect(components).toMatch(/\.status-badge\s*\{[^}]*background: transparent;[^}]*border: 0;/s);
    expect(components).toMatch(/\.status-badge-dot\s*\{[^}]*background: var\(--status-color, var\(--color-text-muted\)\);/s);
    expect(components).toMatch(/\.status-badge\.status-danger,[^{]*\{[^}]*color: var\(--status-color\);/s);
  });
});
