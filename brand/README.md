# Brand

The Token Controller mark is a ring of seven token chips around a check: every token, accounted for.
This folder is the one home for the logo files and the brand colors. The website, the docs, the web app and Stripe all take their values from here.

## Colors

| Role | Name | Light background | Dark background | Use it for |
| --- | --- | --- | --- | --- |
| Main brand color | Ink | `#1C2030` | `#F6F3EC` (Cream) | The check in the logo, primary buttons, headings, dark brand surfaces |
| Accent color | Blue | `#2563CF` | `#5B9BF0` | Links, focus rings, the selected item in a list or menu, small highlights |
| Accent tint | Blue tint | `#E7EEFA` | `#16263F` | Background of a selected item, info notes |

Both accent values pass WCAG AA for normal text on white (5.6:1) and on a near-black page (6.6:1 on `#0F1216`).

### Logo spectrum

The seven chips, in order from the top, clockwise. Use these for the logo and for charts only, never for text.

| Red | Orange | Yellow | Green | Teal | Blue | Violet |
| --- | --- | --- | --- | --- | --- | --- |
| `#EE4545` | `#F77A22` | `#F7C125` | `#3FBF68` | `#22B5AE` | `#2F80E8` | `#7E57E8` |

## Files

| File | What it is | Where it goes |
| --- | --- | --- |
| `logo-light.svg` | Plain mark, dark check, for light backgrounds | Website and docs header, anything below 64 px |
| `logo-dark.svg` | Plain mark, cream check, for dark backgrounds | Dark headers, dark mode |
| `favicon.svg` | Plain mark; the check turns cream when the browser is in dark mode | Browser tab |
| `favicon.ico` | Plain mark at 16, 32 and 48 px | Browser tab fallback |
| `app-icon.svg` | App icon: the mark on a light rounded tile with soft shadows | Desktop app, store listings, large sizes (64 px and up) |
| `app-icon-square.svg` | App icon without the tile, filling the square | iOS home screen, Android, Stripe icon |
| `apple-touch-icon.png` | `app-icon-square` at 180 px | iOS home screen |
| `png/` | 512 and 1024 px PNG exports of the above | Places that do not take SVG |

Small sizes (favicons, anything below 64 px) use the plain mark. The tile is for 64 px and up.

## Stripe

Stripe dashboard, Settings, Branding:

- Brand color: `#1C2030`
- Accent color: `#2563CF`
- Icon: `png/app-icon-square-512.png`
- Logo: `png/logo-light-512.png`
