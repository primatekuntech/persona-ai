# Design system — modern & minimalist

A calm, document-forward interface. High contrast text, generous whitespace, one accent colour, no gradients, no visual noise. Inspired by Linear, Notion, Vercel dashboards, and Readwise. The product is about **the user's own words**; the chrome must get out of the way.

## Core principles

1. **Content over chrome.** Largest visual weight goes to uploaded/generated text. UI elements recede.
2. **One accent colour.** Used for interactive states and primary CTAs. Nothing else.
3. **Monochrome hierarchy.** 4–5 shades of neutral do almost all the work.
4. **No decoration.** No drop shadows except sparingly on elevated surfaces. No gradients. No illustrations. No emoji in UI copy.
5. **Density by context.** Lists and tables can be tight; reading surfaces (chat, documents) are airy.
6. **Typography is the design.** Use weight, size, and tracking to build hierarchy before reaching for colour or dividers.

## Colour

Use CSS variables; support light and dark mode. Default is light.

### Light mode

| Token | Value | Use |
|-------|-------|-----|
| `--bg` | `#FFFFFF` | Page background |
| `--bg-elevated` | `#FAFAFA` | Cards, inputs, hover rows |
| `--bg-subtle` | `#F4F4F5` | Section backgrounds, tag chips |
| `--border` | `#E4E4E7` | Dividers, input borders |
| `--text` | `#0A0A0A` | Body, headings |
| `--text-muted` | `#52525B` | Secondary text, labels |
| `--text-subtle` | `#A1A1AA` | Placeholder, timestamps |
| `--accent` | `#18181B` | Primary button bg, focus ring (neutral-near-black) |
| `--accent-fg` | `#FAFAFA` | Text on accent |
| `--danger` | `#DC2626` | Destructive actions |
| `--warning` | `#D97706` | Non-blocking warnings |
| `--success` | `#16A34A` | Completed states |

### Dark mode

| Token | Value |
|-------|-------|
| `--bg` | `#0A0A0A` |
| `--bg-elevated` | `#141414` |
| `--bg-subtle` | `#1C1C1F` |
| `--border` | `#27272A` |
| `--text` | `#FAFAFA` |
| `--text-muted` | `#A1A1AA` |
| `--text-subtle` | `#71717A` |
| `--accent` | `#FAFAFA` |
| `--accent-fg` | `#0A0A0A` |

The accent is **near-black in light, near-white in dark** — the highest-contrast neutral. This is the Vercel/Linear approach and keeps the palette truly minimalist. If the user later wants a brand hue (e.g. a deep indigo), it drops into `--accent` alone.

## Typography

- **Primary:** Inter (variable), preloaded `woff2`. Fallback `ui-sans-serif, system-ui, -apple-system, sans-serif`.
- **Monospace:** JetBrains Mono or `ui-monospace`. Used only for code blocks and inline `tokens`.
- **Reading surfaces:** use the same Inter; do not introduce a serif for "body text" — we're in an app, not a blog, and one family reads more modern.

### Scale (tailwind-friendly)

| Token | Size / line-height | Use |
|-------|--------------------|-----|
| `text-xs` | 12 / 16 | Timestamps, metadata |
| `text-sm` | 14 / 20 | Secondary text, labels, table cells |
| `text-base` | 15 / 24 | Body, chat bubbles |
| `text-lg` | 17 / 26 | Section headings |
| `text-xl` | 20 / 28 | Page titles (in cards) |
| `text-2xl` | 24 / 32 | Dashboard numbers |
| `text-3xl` | 30 / 36 | Page titles (top-level) |

Body is **15px**, not 16, to match the product's information-dense feel without feeling cramped.

### Weights

- 400 — body
- 500 — UI labels, secondary emphasis
- 600 — headings, button text
- 700 — dashboard numbers only

Avoid 700 on prose.

## Spacing

Use a 4px base. Tailwind default scale. Prefer multiples of 4 and 8.

- Component internal padding: `px-4 py-2` (buttons), `p-4` (cards), `p-6` (dialogs).
- Section spacing: `space-y-6` or `space-y-8`.
- Page gutter: `px-6` on mobile, `px-10` on desktop inside a max-width container.
- Max reading width: `max-w-2xl` (672px) for chat and document views. No full-bleed text.

## Layout

### App shell (desktop ≥ 768 px)

```
┌────────────────────────────────────────────────────────────────────┐
│  Top bar (h-14)                                                    │
│   • left: logo / current persona name                              │
│   • right: user menu                                               │
├───────────────────┬────────────────────────────────────────────────┤
│  Sidebar (w-64)   │  Main content (max-w-4xl, centered)            │
│   • Personas      │                                                │
│     ─ Persona A   │                                                │
│     ─ Persona B   │                                                │
│   • Admin (if)    │                                                │
│                   │                                                │
│                   │                                                │
└───────────────────┴────────────────────────────────────────────────┘
```

On `/personas/:id/*`, the sidebar switches to a persona nav: Dashboard, Documents, Chat, Eras, Settings.

### Mobile shell (< 768 px)

- Sidebar collapses to a hamburger button in the top bar. Tapping opens a `Sheet` (full-height drawer from left, 80 % viewport width). Same nav content.
- Persona switcher becomes a full-width `Command` modal triggered from the top bar title.
- Main content uses full viewport width with `px-4` gutter.
- Chat input sticks to the visual viewport (not layout viewport) so the iOS keyboard does not hide it — use `100svh` for the chat layout height and `env(safe-area-inset-bottom)` for input padding.
- Multi-line chat input grows to max 4 lines on mobile (8 on desktop).
- Drag-and-drop upload targets become a primary "Upload" button triggering the OS file picker.
- Tables (admin, documents list) reflow as stacked cards on narrow viewports.

### Breakpoints

- `sm` 640 — large phones (no layout change, just text sizes relax).
- `md` 768 — sidebar returns.
- `lg` 1024 — content max-width expands.
- `xl` 1280 — unchanged from `lg` except spacing.

No desktop-only features. If a feature is not usable on mobile, it is not a v1 feature.

### Auth pages

Centered card, `max-w-sm`, no sidebar, no top bar. Logo above the card.

### Chat

No sidebar clutter in the thread area. Messages in a centered column `max-w-2xl`. Input pinned to bottom, grows to max 8 lines (4 on mobile) then scrolls. Era selector as a pill in the thread header.

## Components

Start from **shadcn/ui** (copy-paste, not a dependency). Required components for v1:

- `Button` (variants: primary, secondary, ghost, destructive)
- `Input`, `Textarea`, `Select`, `Switch`, `Checkbox`
- `Dialog`, `Sheet`, `DropdownMenu`, `Command` (Cmd+K palette)
- `Card`, `Separator`, `Badge`, `Tooltip`
- `Toast`
- `Table` (for admin user list)
- `ScrollArea`
- `Tabs`

Custom components layered on top:

- `PersonaSwitcher` — combobox in top bar.
- `DocumentRow` — title, source, era chip, status dot, menu.
- `ChatMessage` — role-styled bubble with optional "sources" disclosure.
- `Dropzone` — thin wrapper around react-dropzone, matches input style.
- `ProgressRow` — shows ingestion status with phase label.
- `StatCard` — dashboard number + label + subtle trend line.
- `StyleProfileCard` — renders a section of the profile (vocabulary, rhythm, etc).

## Interaction & motion

- **Transitions 120–180ms.** Longer feels sluggish, shorter feels abrupt.
- **Ease-out** for enter, **ease-in** for exit.
- **No parallax, no bounce, no spring** unless the user hovers a draggable.
- Focus rings: 2px `--accent` ring with 2px offset. Always visible on keyboard focus.
- Skeleton loaders only for content that takes > 300ms. Otherwise nothing.
- Streaming tokens appear without motion; cursor indicator is a subtle blinking caret.

## Iconography

**Lucide** icons only. Stroke 1.5. Size defaults `h-4 w-4` (inline) or `h-5 w-5` (nav). Never decorative — always paired with a label or tooltip.

## States

| State | Style |
|-------|-------|
| Hover | `bg-elevated` on rows; `opacity-90` on solid buttons |
| Focus | 2px accent ring |
| Disabled | `opacity-50 cursor-not-allowed` |
| Loading | Button: spinner + dim label; Page: skeletons after 300ms |
| Empty | Short centered message + one action, no illustrations |
| Error | Inline text in `--danger`; toast for transient |

## Accessibility

- All interactive elements keyboard-reachable.
- Colour contrast: body ≥ 7:1 (AAA); secondary ≥ 4.5:1.
- Do not encode meaning in colour alone — status dots always pair with a label or tooltip.
- Forms use `<label>` wired to inputs; errors announced via `aria-live="polite"`.
- Cmd+K palette navigable by arrow keys + enter.
- `axe-core` runs in CI against the built frontend (see [`05-engineering-practices.md`](05-engineering-practices.md#accessibility--internationalisation)). Violations fail the build.
- Skip-to-content link as the first focusable element on every page.

## Keyboard shortcuts

Global:

| Shortcut | Action |
|----------|--------|
| `Cmd/Ctrl + K` | Open command palette |
| `Cmd/Ctrl + P` | Open persona switcher |
| `Cmd/Ctrl + ,` | Open settings |
| `Cmd/Ctrl + /` | Show keyboard shortcuts overlay |
| `Esc` | Close topmost dialog / popover |
| `g` then `p` | Go to personas |
| `g` then `a` | Go to admin (if admin) |

Chat:

| Shortcut | Action |
|----------|--------|
| `Cmd/Ctrl + N` | New chat session |
| `Cmd/Ctrl + Enter` | Send message (alternative to plain Enter) |
| `Shift + Enter` | Newline in input |
| `Cmd/Ctrl + ↑` | Edit last sent message (v2) |

Upload:

| Shortcut | Action |
|----------|--------|
| `Cmd/Ctrl + U` | Open upload dialog |

A shortcut cheat-sheet is available at `Cmd+/`. Shortcuts are documented in `/settings/about`.

## Dark mode

Toggle in user menu. Persist in `localStorage` + respect `prefers-color-scheme` on first visit. All tokens defined in both schemes; components use tokens only, never hex.

## Things we will NOT do

- No dashboards full of charts (one small sparkline is plenty per metric).
- No skeuomorphic "paper" effects on documents.
- No confetti, no celebrations, no streaks.
- No onboarding tours — the UI should be self-evident.
- No hero marketing sections inside the app.

## Reference look

- Linear (spacing, typography, restraint).
- Vercel dashboard (neutral palette, mono accent).
- Readwise Reader (reading surface, chrome that disappears).
- Notion (density toggles, command palette).

## Tailwind setup

- `tailwind.config.js`: custom tokens (`theme.extend.colors`) mapped to CSS variables; `fontFamily.sans = ['Inter', ...]`.
- `@tailwindcss/typography` for rendered markdown in chat replies; override `--tw-prose-*` to match the design tokens.
- Shadcn CLI initialised with CSS variables mode (not class-based).
