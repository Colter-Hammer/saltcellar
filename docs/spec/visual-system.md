# Visual system

How Saltcellar looks: palettes, light and dark, type, money, density, group colors and icons, the logo, motion and contrast. It replaces the grayscale tokens milestone 1 shipped with. Every later screen (Budget, Overview, reports) follows these rules without another styling pass. The short version for anyone writing a component is [docs/styling.md](../styling.md).

The look must stay cheap to change. **Tokens are the mechanism:** components never hard-code a color, size, radius, shadow or font. A new look is a token or palette-file change, never a component edit.

## Feel

Calm and data-first. Mostly neutral surfaces with one accent per palette. Color is kept for meaning (money in, over budget, near limit, needs review), never decoration. Soft corners, light shadows. Numbers lead: tabular figures, larger or bolder than their labels.

## Palettes and modes

Four palettes ship, each with a light and a dark variant: **Slate / teal**, **Sage**, **Deep blue** (the default), **Plum**. Palettes vary the accent and the tint of the neutrals; meaning colors are the same in every palette so they read the same everywhere.

- **Palette is a per-login setting, stored on the server** (follows the login across devices, like display locale): `users.palette`, `NULL` = the default. The server stores any id matching `^[a-z0-9-]{1,32}$` and doesn't know the list; the frontend shows the default for an id it doesn't know *(agent: so adding a palette never touches the server)*. It is saved on its own, `PUT /api/v1/profile/palette`, so picking one doesn't save the rest of the profile form *(agent)*.
- **Mode is Light, Dark or System**, default System, stored per device and per login in `localStorage` *(agent: key `saltcellar.mode.<username>`, the name the session gives the login)*.
- **Where:** Settings › Profile gets an **Appearance** part: the palettes as swatches (name, accent, a light and dark chip, drawn by the palette's own CSS so no color value lives outside its file *(agent)*) and a Light / Dark / System choice. Changing either applies at once, no save button *(agent)*.
- **No flash** *(agent)*: an inline script in `index.html` applies the last palette and mode (cached in `localStorage` *(agent: `saltcellar.palette`, `saltcellar.mode`)*) before first paint; the profile's palette replaces it when it loads. The login and setup screens use the cached palette, or the default.

### Theme mechanism

- `web/src/styles/tokens.css` holds everything that isn't a color (font, spacing, density, radius, shadow, motion) and the `@theme inline` block that maps every token into Tailwind.
- Each palette is **one CSS file**, `web/src/styles/palettes/<id>.css`, setting the same color custom properties under `[data-palette='<id>']` and `[data-palette='<id>'].dark`. Values are hex *(agent: the gamut-mapped sRGB the OKLCH design values land on, and what the contrast check reads)*.
- `web/src/theme/palettes.ts` is the **registry**: id, label and swatch per palette, the default id (`deep-blue`), and the CSS imports.
- `<html>` carries `data-palette="<id>"`, the `.dark` class in dark mode, and `color-scheme` to match.
- Components and (from milestone 4) the ECharts theme only read tokens. The ECharts theme re-reads the custom properties when the palette or mode changes.
- **Adding a palette** = one CSS file + one registry entry, passing the contrast check. Nothing else changes.
- Token names follow shadcn-vue's, so copied-in components work unchanged. The grayscale `:root` values and `--chart-1` … `--chart-5` go.

## Color tokens

Aliases, the same in every palette and mode: `--card-foreground`, `--popover-foreground`, `--secondary-foreground` and `--accent-foreground` = `--foreground`; `--popover` = `--card`; `--secondary` and `--accent` = `--muted` (shadcn's "accent" is the hover surface; the palette's accent color is `--primary`); `--input` = `--border`; `--ring` and `--needs-review` = `--primary`; `--destructive` = `--over-budget`; `--destructive-foreground` = `--primary-foreground` *(agent: text on a destructive button)*; `--chart-in` = `--money-in`.

### Meaning colors (every palette)

| Token | Light | Dark | Used for |
|---|---|---|---|
| `--money-in` | `#137738` | `#64cf80` | Positive amounts, income |
| `--over-budget` | `#be2323` | `#fa706a` | Over a target; errors (`--destructive`) |
| `--near-limit` | `#a0600b` | `#edb345` | From 90% of a target *(default, set in milestone 4)* |
| `--chart-out` | `#c86556` | `#dd7767` | "Out" in in-vs-out charts only |

- **Spending is plain foreground.** No alarm color by default.
- **Needs review is the palette accent.**
- `--chart-out` is its own token, distinct from `--over-budget`, so the two can diverge.
- **Color is never the only signal:** every meaning color comes with an icon or a word.

### Per-palette colors

| Token | Slate / teal light | Slate / teal dark | Sage light | Sage dark | Deep blue light | Deep blue dark | Plum light | Plum dark |
|---|---|---|---|---|---|---|---|---|
| `--background` | `#f8fafd` | `#0d1013` | `#f8fbf7` | `#0d110c` | `#f7faff` | `#0c1016` | `#fcf9fd` | `#120e13` |
| `--card` | `#ffffff` | `#14171b` | `#ffffff` | `#151814` | `#ffffff` | `#13171e` | `#ffffff` | `#1a151b` |
| `--muted` | `#ecf1f5` | `#1f2225` | `#edf2ec` | `#1f231e` | `#ebf1fa` | `#1e2228` | `#f4eef5` | `#242025` |
| `--border` | `#dadee3` | `#2d3134` | `#dbe0d9` | `#2e322d` | `#d9dee8` | `#2c3138` | `#e2dce3` | `#332e34` |
| `--foreground` | `#12171b` | `#eceff2` | `#121811` | `#ecefec` | `#101620` | `#ebeff4` | `#1a131b` | `#f1edf1` |
| `--muted-foreground` | `#575e67` | `#9fa5ac` | `#586156` | `#a0a79e` | `#545e6e` | `#9da5b2` | `#645966` | `#aaa1ac` |
| `--primary` | `#007272` | `#64c3c3` | `#5c6232` | `#afb688` | `#1f53b8` | `#77a6ff` | `#823b7d` | `#d892d1` |
| `--primary-foreground` | `#fafafa` | `#0d1013` | `#fafafa` | `#0d110c` | `#fafafa` | `#0c1016` | `#fafafa` | `#120e13` |
| `--cat-1` | `#00858a` | `#24c1c9` | `#008a5a` | `#5dc291` | `#4573c0` | `#7eabf6` | `#9558a3` | `#cc91da` |
| `--cat-2` | `#c76a99` | `#c26c96` | `#a676c8` | `#a276c2` | `#cb733e` | `#c67544` | `#939421` | `#91922f` |
| `--cat-3` | `#478638` | `#81bd74` | `#857500` | `#bdac4a` | `#00858a` | `#24c1c9` | `#4573c0` | `#7eabf6` |
| `--cat-4` | `#8d7ed7` | `#8c7ed0` | `#4992d9` | `#4e90d2` | `#c76a99` | `#c26c96` | `#cb733e` | `#c67544` |
| `--cat-5` | `#986b00` | `#d2a249` | `#b25634` | `#eb9070` | `#478638` | `#81bd74` | `#00858a` | `#24c1c9` |
| `--cat-6` | `#009bcc` | `#1f99c7` | `#00a39d` | `#00a19b` | `#8d7ed7` | `#8c7ed0` | `#c76a99` | `#c26c96` |
| `--cat-7` | `#b45054` | `#ed8b8b` | `#a5538c` | `#dd8cc3` | `#986b00` | `#d2a249` | `#478638` | `#81bd74` |
| `--cat-8` | `#00a682` | `#1ea381` | `#739d41` | `#749a48` | `#009bcc` | `#1f99c7` | `#8d7ed7` | `#8c7ed0` |
| `--cat-9` | `#9558a3` | `#cc91da` | `#6769bf` | `#9ca2f5` | `#b45054` | `#ed8b8b` | `#986b00` | `#d2a249` |
| `--cat-10` | `#939421` | `#91922f` | `#bf7e1f` | `#ba7e2d` | `#00a682` | `#1ea381` | `#009bcc` | `#1f99c7` |
| `--cat-11` | `#4573c0` | `#7eabf6` | `#00829e` | `#39bcdf` | `#9558a3` | `#cc91da` | `#b45054` | `#ed8b8b` |
| `--cat-12` | `#cb733e` | `#c67544` | `#cf697b` | `#c96b7b` | `#939421` | `#91922f` | `#00a682` | `#1ea381` |

Design values, for making a new palette: surfaces and text are OKLCH lightness steps tinted by the palette's neutral hue (light: background 0.985, card 1, muted 0.955, border 0.9, foreground 0.2, muted text 0.48; dark: 0.17, 0.205, 0.25, 0.31, 0.95, 0.72). The `--cat-n` sequence is 12 hues stepping 150° round the wheel from a per-palette offset, lightness alternating (light 0.56 / 0.645, chroma 0.13; dark 0.74 / 0.64, chroma 0.12), so neighbours differ a lot. Sage's accent is olive (hue 115), kept clear of money-in green *(agent)*.

## Contrast

Every palette passes WCAG AA in both modes, checked by a test, not by eye *(agent: a Vitest test in `just check` that reads every palette file)*:

- 4.5:1 for foreground and muted text on background, card and muted; accent text on background and card; text on an accent button; each meaning color as text on background and card.
- 3:1 for `--chart-out` and every `--cat-n` on card.
- The closest two `--cat-n` in a palette and mode are at least 0.09 apart in OKLab *(agent: the shipped palettes' closest pair is 0.10)*.
- A palette file missing any token fails too.

## Type

- **Inter** (variable), bundled with the app from `@fontsource-variable/inter`, no external CDN. One font for every palette, set by the single `--font-sans` token. A font choice in Settings is not v1.
- Sizes are Tailwind's scale. Body text `text-sm` on desktop, `text-base` on phone; inputs at least 16px on phone so iOS doesn't zoom *(agent)*.
- **Numbers lead.** Money and counts use tabular figures. Amounts in rows are `font-medium` beside `font-normal` labels; headline figures (Overview cards, budget totals) are `text-3xl font-semibold` with a `text-xs` muted label above *(agent)*.

## Money

Every amount goes through `formatMoney` and shows through one `Money` component *(agent: `web/src/components/Money.vue`, taking minor units)*, which adds tabular figures, weight and color. Symbol and separators come from the login's display locale.

- **Cents always** in lists, tables and forms.
- **Whole units on headline figures** (Overview cards, budget totals), rounded half away from zero; the exact amount shows on hover and on tap *(agent: as the `title` and a tap-to-toggle)*.
- **Out:** `−$42.10`, plain foreground, with a real minus sign (U+2212), never a hyphen.
- **In:** `+$1,200.00`, `--money-in`.
- **Never parentheses**, whatever the locale's accounting style.
- Balances and totals that aren't a transaction's amount carry no plus sign; a negative one still gets the real minus.

## Density and spacing

One fixed density in v1, no toggle, all from spacing tokens so a toggle can come later *(agent: `--row-touch` 2.75rem, `--row-dense` 2.25rem, `--page-x` 1rem phone / 1.5rem desktop)*.

- **Phone is roomy:** touch targets at least 44px, one row per transaction.
- **Desktop tables are tighter:** dense rows from the `lg` breakpoint.

## Shape, shadow and motion

- Radius stays `--radius: 0.625rem` with the `sm` … `xl` steps.
- Shadows are light: cards get `--shadow-card` in light mode and none in dark, where the border carries the edge *(agent; used as `shadow-(--shadow-card)`)*.
- **Motion** is short and subtle: `--duration` 150ms with an ease-out curve, for hover, open and close only *(agent)*. Under `prefers-reduced-motion` the durations are zero.

## Group color and icon

Each category group has a color and a Lucide icon; its categories show the group's.

- **Color** is auto-assigned by the group's position, never picked: the n-th group (all groups, hidden ones included, in the Categories screen's order, `sort_order` then `id` *(agent: so hiding a group doesn't recolor the rest)*) takes `--cat-n`. Past 12 the sequence cycles with a lightness shift (−0.08 in light mode, +0.08 in dark, through CSS relative colors *(agent)*). Reordering groups changes their colors.
- Shown in charts, and as a dot beside the group or category name wherever one is listed (Categories, transaction rows, category pickers).
- **Icon:** `category_groups.icon`, a Lucide icon name; `NULL` shows `folder` *(agent)*. Chosen from a curated set of about 40 icons when creating or editing a group *(agent: a fixed list in the frontend, so the bundle doesn't carry all of Lucide)*. Starter groups ship with:

| Group | Icon *(agent)* |
|---|---|
| Housing | `house` |
| Food | `utensils` |
| Transportation | `car` |
| Health & Fitness | `heart-pulse` |
| Insurance | `shield` |
| Kids | `baby` |
| Shopping | `shopping-bag` |
| Fun | `party-popper` |
| Giving | `hand-heart` |
| Pets | `paw-print` |
| Financial | `landmark` |
| Income | `wallet` |

Groups the Actual importer creates get `NULL`.

## Icons and empty states

- Interface icons are Lucide (`@lucide/vue`), sized by the text they sit beside.
- Empty states are an icon and text, no illustrations *(agent)*.

## Budget bars (milestone 4)

Under target the bar is the palette accent; from 90% of the target (the default) it is `--near-limit`; over the target it is `--over-budget`. Each state also has an icon and words ("Near limit", "$12 over").

## Logo

- **The mark** is the footed bowl: a bowl on a foot with a mound of salt, a single 32×32 SVG drawn in `currentColor` at `web/src/assets/logo/mark.svg`. It is one swappable asset: changing the logo is a file swap, not a component edit. Two alternative sketches (a lidded cellar, a dish with an S) are kept in `docs/assets/logo/` *(agent)*.
- **The wordmark** is "Saltcellar" set in Inter semibold beside the mark, as text, not outlined *(agent)*.
- **In the app** (login, setup, sidebar, phone header) the mark takes the palette accent.
- **Favicon and PWA icon** are one fixed tile whatever the palette: the mark in `#f4f1ea` on `#1d3a40`, rounded. `web/public/` holds `favicon.svg`, `apple-touch-icon.png` (180) and `icon-192.png`, `icon-512.png`, `icon-maskable-512.png` *(agent)*; the manifest that uses them comes with the PWA in milestone 6.

## Later, not v1

A font choice in Settings, and a set of token overrides an operator can supply at deploy time. The token layer is shaped so either is a small addition.
