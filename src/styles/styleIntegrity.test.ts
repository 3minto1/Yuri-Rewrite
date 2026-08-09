// @vitest-environment node

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const aggregate = readFileSync(new URL("../styles.css", import.meta.url), "utf8");
const components = readFileSync(new URL("./components.css", import.meta.url), "utf8");
const layout = readFileSync(new URL("./layout.css", import.meta.url), "utf8");
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
});
