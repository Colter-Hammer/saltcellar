# Styling

The rules for anything that draws on screen. The full system, with every token value, is [docs/spec/visual-system.md](spec/visual-system.md).

## Writing a component

- **Only tokens.** No hex, `rgb()`, `oklch()`, pixel font sizes or font names in components; use the Tailwind classes that map the tokens (`bg-card`, `text-muted-foreground`, `text-money-in`, `rounded-lg`). A new look is a token change, never a component edit.
- **Color means something.** Neutral surfaces, the accent (`primary`) for actions and "needs review", and the meaning colors only for their meaning: `money-in`, `over-budget`, `near-limit`. Never color for decoration.
- **Never color alone.** Every meaning color comes with an icon or a word.
- **Money through `Money`.** Every amount uses the `Money` component (or `formatMoney` where a component can't): tabular figures, real minus sign, `+` and green for money in, cents except on headline figures, never parentheses.
- **Group color and icon** come from the group, never picked in a component: `--cat-n` by the group's position, its Lucide icon.
- **Touch targets** at least 44px on phone; dense rows only from `lg`.
- **Motion** uses `--duration` and is off under `prefers-reduced-motion`.
- **Icons** are Lucide. Empty states are an icon and text.

## Adding a palette

1. Copy a file in `web/src/styles/palettes/` to `<id>.css` and set every color token for light and dark.
2. Add it to the registry in `web/src/theme/palettes.ts`.
3. Run `just check`: the contrast test fails on any pair below WCAG AA, any missing token, or group colors too close together.

## Changing the logo

Replace `web/src/assets/logo/mark.svg` (32×32, `currentColor`) and regenerate the fixed-color icons in `web/public/`.
