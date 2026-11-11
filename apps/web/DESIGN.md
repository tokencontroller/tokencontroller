# The web app's design system

Every screen of the web app is built from the pieces on this page. A screen keeps no styles of its own: when a screen needs something that is not here, add it here first. The brand colours come from the brand guide; this page says how the web app uses them.

The look: calm cards on a warm canvas, numbers first, little text, one primary action. Modern business tools such as Stripe, Vercel and Linear work this way.

## Rules

1. **One main thing per screen.** Decide what the eye should land on first (a total, a gap, a list to fix) and make it the largest thing on the screen.
2. **One primary action per screen**, top right in the page header. Section actions sit in the card header as small secondary buttons. Row actions sit in the last column, right-aligned.
3. **Numbers lead, labels follow.** A figure is large and medium weight; its label is small and grey above it.
4. **Help text moves out of the way.** A screen has at most one short line under its title. A card has no lead sentence: an explanation goes behind the (i) hint next to the card title, or into the docs. A field keeps a hint of at most three words, such as "Empty: now".
5. **Forms that add something open in a dialog** from a button. No add-form sits open under a table.
6. **Destructive actions look destructive**: the danger button, never plain text.
7. **Colour means something.** Ink for text and the primary button, Blue for links, focus, switches and the selected item, green (posted) for what is settled, amber (exception) for what needs a look, such as a negative deviation or cost on no project. The spectrum is for tags and charts only.
8. **Word budget.** Each screen has a budget for its words outside data (titles, labels, hints, notes), set in the design diagnosis. A screen over its budget cuts text before it ships.

## Tokens

All in `src/styles.css`. Components use the Tailwind names, never raw values.

| Kind | Name | Light | Dark | Use |
|---|---|---|---|---|
| Colour | `canvas` | `#F8F7F3` | `#0F1216` | Behind the cards |
| Colour | `surface` | `#FFFFFF` | `#161A20` | Cards, sidebar, inputs, dialogs |
| Colour | `hover` | `#F7F5F0` | `#1B2028` | Row and menu hover, table header band, card footer |
| Colour | `line` / `line-strong` | `#E6E3DB` / `#C9C5BA` | `#2A303A` / `#3D4552` | Hairlines / input and button borders |
| Colour | `ink`, `ink-soft`, `muted` | brand Ink and two greys | Cream and two greys | Text |
| Colour | `accent`, `accent-tint` | brand Blue | brand Blue (dark) | Links, focus, selected item |
| Colour | `posted`, `exception` (+ `-tint`) | green, amber | lighter green, amber | Settled, needs a look |

| Type step | Size / line | Weight | Use |
|---|---|---|---|
| `text-title` | 24 / 32, tracking −0.018em | 600 | Page title, once per screen |
| `text-figure` | 28 / 36, tracking −0.02em | 500 | The number a card is about |
| `text-heading` | 15 / 22 | 600 | Card title |
| `text-body` | 14 / 20 | 400, 500 for names | Text, table cells, inputs, buttons |
| `text-small` | 13 / 20 | 400, 500 for labels | Secondary text, field labels, figure labels |
| `text-caption` | 12 / 16 | 400, 500 for headers | Table headers, notes under figures, badges |

Inter for text, IBM Plex Mono only for ids and keys. Numbers use tabular figures (`num`).

Spacing uses the 4 px steps 4, 8, 12, 16, 20, 24, 32, 40. Card padding is 20 px; cards are 24 px apart; the page has 40 px at the top on a desktop and 16 px at the sides on a phone.

Radii: 4 px (checkbox), 6 px (buttons, inputs, badges), 10 px (cards, dialogs). Shadows: `shadow-card` (a 1 px soft line under a card, none in dark mode), `shadow-chip` (buttons), `shadow-pop` (menus, dialogs, hints). Dark mode has its own shadow values with no white top edge.

## Pieces

All in `src/components/ui/`.

| Piece | Where | How to use it |
|---|---|---|
| `PageHeader` | display.tsx | `title`, at most a short `lead` (such as the range), `meta` (such as as-of), and the one primary action in `actions` |
| `Section` | display.tsx | The card. `title`, `badge` for a short status next to it (such as "built in"), `hint` for the explanation, `actions` for small buttons, `flush` when the body is a table, `footer` for a form's save button. `lead` is only for the hosted add-on's screens until they move |
| `Card` | display.tsx | A card with no title, such as a filter toolbar |
| `KpiRow` + `Kpi` | display.tsx | Two to four figures in one card, split by hairlines. The first is the `hero` (larger, wider). A `note` of at most four words |
| `Figure` | display.tsx | A figure inside a card, such as bill, cost of messages and the gap |
| `Hint` | display.tsx | The (i) that opens help text on click or tap. Use it only where the text matters; most cards need none |
| `linkClass` | display.tsx | A quiet blue link in a card header, such as "Bills" |
| `DataTable` | table.tsx | Sentence-case headers on a light band, 44 px rows, totals in a footer row at weight 500, money right-aligned. Put it in a `flush` card |
| `SettingRow` | form.tsx | Label and one line on the left, the control on the right; stacks on a phone. For settings cards |
| `Field`, `Input`, `Select`, `Textarea` | form.tsx | Label above, 36 px controls, a blue focus ring |
| `Button` | button.tsx | `primary` (Ink, once per screen or dialog), `secondary` (bordered), `ghost` (quiet links in card headers), `danger` (revoke, remove, disconnect) |
| `Segmented` | tabs.tsx | Two or three views of one list in a card header, each with its count, such as To check and Submitted |
| `Menu` | menu.tsx | The ··· row or card menu, for an action that is rare or destructive, such as removing a tag name |
| `Badge` | badge.tsx | A short status in a tinted chip with a thin ring. One per row |
| `Empty` | display.tsx | What is missing in a line, and the one action that fills it |
| `Dialog` | dialog.tsx | Every add-form. The submit is the primary button, bottom right, with Cancel beside it |
| `Note` | display.tsx | Only for something that changes what the reader does now, such as a token shown once or figures that leave something out |

## How a screen is checked

Before and after screenshots of the screen in light and dark, on a desktop and a phone; the word budget; contrast of every text colour at WCAG AA; and the journeys (`pnpm e2e`) passing.

## On a phone

Cards run to the screen's edges, so tables get the width. Columns that matter less are `wide` and hide below 768 px. A table with `stack` shows each row as a short block; empty cells are hidden and the total is one line.

## Open work

Every screen of the public web app is in the system. The hosted add-on's screens (Billing, feedback) move in the private repository; after that, `Section`'s `lead` and the old colour names `page` and `sheet` go. These shared pieces are still to make, at the latest with the first screen that needs them:

- A date range button in place of the browser's date inputs (reports, plans).
- On a phone, report filters folded into one summary row that opens them.
- On a phone, list rows (name, amount, badge) in place of stacked label and value blocks for long lists.
