# Kodiak Theme Specification (`docs/THEME.md`)

> **Source of Truth**: Extracted directly from `~/Nextcloud/ProjectsWiki/docs/extra.css`, `~/Nextcloud/ProjectsWiki/mkdocs.yml`, and `~/Nextcloud/ProjectsWiki/docs/index.md` per §6 of the Build Brief (*"the repo is the single source of truth, overriding anything in this brief or prior conversation"*).

---

## 1. Repository Inspection Summary

Inspecting `~/Nextcloud/ProjectsWiki` revealed an **MkDocs Material** documentation & catalog hub (`site_name: Projects Wiki`) rather than a static violet "Cortex-9" page:

- **Theme Configuration** (`mkdocs.yml`):
  - Theme: `material`
  - Dark mode (`(prefers-color-scheme: dark)`): `scheme: slate`, `primary: deep orange`, `accent: amber`
  - Light mode (`(prefers-color-scheme: light)`): `scheme: default`, `primary: deep orange`, `accent: amber`
- **Custom Stylesheet** (`docs/extra.css`):
  - Defines all `--wiki-*` custom properties on `:root` (`color-scheme: light dark`).
- **Missing Asset Notice**:
  - `assets/synapse.svg` and `projects_index.json` do **not** exist in `~/Nextcloud/ProjectsWiki` (the catalog is stored at `docs/projects.json` and rendered by `docs/extra.js` inside `docs/index.md`).

---

## 2. Extracted CSS Custom Properties (`:root`)

Verbatim from `docs/extra.css`:

```css
:root {
  color-scheme: light dark;
  --wiki-accent: #ff6e40;
  --wiki-accent-hover: #ff8a65;
  --wiki-accent-soft: rgba(255, 110, 64, 0.14);
  --wiki-border: rgba(148, 163, 184, 0.22);
  --wiki-border-hover: rgba(255, 110, 64, 0.65);
  --wiki-surface: var(--md-code-bg-color, #1e2129);
  --wiki-surface-elevated: var(--md-default-bg-color, #16181d);
  --wiki-radius: 12px;
  --wiki-radius-sm: 8px;
}
```

### MkDocs Material Base Variables Consumed by `extra.css`

`extra.css` references MkDocs Material's underlying CSS custom properties with explicit dark-slate fallbacks:
- `--md-default-bg-color`: `#16181d` (elevated/page background fallback in dark mode; `#ffffff` in light mode)
- `--md-code-bg-color`: `#1e2129` (card/toolbar surface fallback in dark mode; `#f5f7fa` in light mode)
- `--md-default-fg-color`: primary text foreground (`#e2e8f0` in dark slate, `#1e293b` in light mode)
- `--md-default-fg-color--light`: muted/secondary text foreground (`#94a3b8` in dark slate, `#64748b` in light mode)
- `--md-code-font-family`: `ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace`

### Status & Indicator Colors in `extra.css`

- **Active / OK (Green)** (`docs/extra.css`):
  - Dot fill: `#22c55e`
  - Border: `rgba(34, 197, 94, 0.55)`
  - Glow shadow: `0 0 8px rgba(34, 197, 94, 0.75)`
  - Diff addition tint derived from OK token: `rgba(34, 197, 94, 0.14)`
- **Inactive / Muted Dot** (`docs/extra.css`):
  - `rgba(148, 163, 184, 0.55)`
- **Accent / Warning / Action (`--wiki-accent` / Amber family)**:
  - Primary accent: `#ff6e40` (`--wiki-accent`)
  - Hover accent: `#ff8a65` (`--wiki-accent-hover`)
  - Soft fill: `rgba(255, 110, 64, 0.14)` (`--wiki-accent-soft`)
  - Warning amber (from `mkdocs.yml` `accent: amber`): `#f59e0b` / soft tint `rgba(245, 158, 11, 0.14)`
- **Bad / Deletion / Error**:
  - Material danger red aligned with the slate palette: `#ef4444` / soft tint `rgba(239, 68, 68, 0.14)`
- **Active Pill Contrast Text** (`docs/extra.css`):
  - `#111318` on `--wiki-accent` background
- **Modal Backdrop** (`docs/extra.css`):
  - `rgba(10, 12, 16, 0.72)` with `backdrop-filter: blur(4px)`

---

## 3. Typography, Spacing & Component Patterns

- **Radii**:
  - Outer containers, cards, toolbars, modals: `--wiki-radius: 12px`
  - Inputs, buttons, selects, code blocks, small badges: `--wiki-radius-sm: 8px`
  - Pills (`category-pill`, `card-category-badge`, status dots): `999px`
  - Inline kbd/tag badges (`search-kbd`, `card-tag`): `4px`
- **Shadows & Transitions**:
  - Toolbar shadow: `0 4px 20px rgba(0, 0, 0, 0.12)`
  - Card hover: `transform: translateY(-2px)`, `border-color: var(--wiki-border-hover)`, `box-shadow: 0 8px 24px rgba(0, 0, 0, 0.18)`
  - Focus ring: `border-color: var(--wiki-accent)`, `box-shadow: 0 0 0 3px var(--wiki-accent-soft)`
  - Transition timing: `0.15s ease` to `0.18s ease`
- **Buttons (`.explorer-btn`)** (`docs/extra.css`):
  - Padding: `0.5rem 0.85rem`, `border-radius: var(--wiki-radius-sm)`, `font-size: 0.82rem`, `font-weight: 600`
  - Variants: `--primary` (solid `--wiki-accent` with `#111318` text), `--accent` (tinted `--wiki-accent-soft` with `--wiki-border-hover`), `--ghost` (elevated surface with `--wiki-border`)
- **Cards (`.project-card`)** (`docs/extra.css`):
  - `background: var(--wiki-surface)`, `border: 1px solid var(--wiki-border)`, `border-radius: var(--wiki-radius)`, `padding: 1.15rem 1.2rem`, `gap: 0.65rem`
