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

  it("uses dedicated foreground tokens for dark sidebar controls", () => {
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
    expect(tokens).toContain("--color-brand: #1f4b7a;");
    expect(tokens).toContain("--color-brand-2: #4779a8;");
    expect(tokens).toContain("--color-token-input: #2f6ea5;");
    expect(tokens).toContain("--color-token-output: #b56314;");
  });

  it("keeps danger and diff semantics independent from the brand palette", () => {
    expect(components).toMatch(/\.status-danger\s*\{[^}]*--status-color: var\(--color-danger\);/s);
    expect(views).toMatch(/\.log-item\.error\s*\{[^}]*border-color: var\(--color-danger\);/s);
    expect(views).toMatch(/\.log-day-tabs button\.active\s*\{[^}]*border-color: var\(--color-brand\);/s);
    expect(views).toMatch(/\.diff-removed\s*\{[^}]*background: var\(--color-danger-soft\);/s);
    expect(views).toMatch(/\.diff-added\s*\{[^}]*background: var\(--color-success-soft\);/s);
  });

  it("uses the Windows UI font stack and supported numeric weights", () => {
    const styles = [tokens, base, layout, components, views, responsive, motion].join("\n");
    const numericWeights = Array.from(styles.matchAll(/font-weight\s*:\s*(\d+)/g), (match) => Number(match[1]));
    const fontFamilies = Array.from(styles.matchAll(/font-family\s*:\s*([^;]+);/g), (match) => match[1].trim());

    expect(tokens).toContain(
      '--font-ui: "Segoe UI Variable Text", "Segoe UI", "Microsoft YaHei UI",',
    );
    expect(numericWeights.every((weight) => [400, 500, 600, 700].includes(weight))).toBe(true);
    expect(fontFamilies.every((family) => family === "var(--font-ui)" || family === "inherit")).toBe(true);
    expect(styles).not.toMatch(new RegExp(`\\b${["Int", "er"].join("")}\\b`));
  });

  it("does not reintroduce obsolete accent or plum activity colors", () => {
    const styles = [tokens, base, layout, components, views, responsive, motion].join("\n");
    const obsoleteAccent = ["accent", "rose"].join("-");
    const obsoleteTooltip = ["#211c", "26"].join("");
    const obsoleteDotBorder = ["#211a", "29"].join("");

    expect(styles).not.toContain(obsoleteAccent);
    expect(styles).not.toContain(obsoleteTooltip);
    expect(styles).not.toContain(obsoleteDotBorder);
    expect(layout).toMatch(/\.activity-tooltip\{[^}]*background:var\(--color-ink-muted\);/s);
    expect(layout).toMatch(/\.activity-progress-dot,[^{]+\{[^}]*border:2px solid var\(--color-ink\);/s);
  });

  it("keeps quick start on a plain surface without decorative artwork", () => {
    expect(components).toMatch(/\.quickstart-dialog\s*\{[^}]*background: var\(--color-surface-raised\);/s);
    expect(components).not.toContain("quick-start-bg.png");
  });

  it("keeps the task estimate compact with readable values", () => {
    expect(layout).toMatch(/\.task-center-scroll\{[^}]*display:flex;[^}]*flex-direction:column;/s);
    expect(layout).toMatch(/\.task-center-estimate\{[^}]*flex:0 0 auto;[^}]*min-height:0/s);
    expect(layout).toMatch(/\.task-center-estimate \.estimate-grid\{[^}]*display:flex;[^}]*flex-direction:column;/s);
    expect(layout).toMatch(/\.task-center-estimate \.estimate-item\{[^}]*min-height:52px;/s);
    expect(layout).toMatch(/\.task-center-estimate \.estimate-item strong\{[^}]*font-size:14px;/s);
    expect(layout).not.toMatch(/\.task-center-estimate\{[^}]*min-height:100%/s);
    expect(layout).not.toMatch(/\.task-center-estimate \.estimate-grid\{[^}]*grid-template-rows:/s);
  });
});
