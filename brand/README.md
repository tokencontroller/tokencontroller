# Brand

The Token Controller mark is a ring of seven token chips around a check: every token, accounted for.
This folder is the one home for the logo files, the brand colors and the look and feel. The website, the docs, the web app and Stripe all take their values from here.

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

## Look and feel

Token Controller is serious about money and playful in how it looks. Four rules carry the brand:

- **Colorful.** The seven-color spectrum is the brand. Use it where color carries meaning: the logo, charts, tags, small badges. Everything else stays calm (cream and white surfaces, Ink text) so the colors stand out.
- **Crisp.** Edges are sharp. No soft blur, no hazy outer glows. Light comes from straight above: a bright rim on the top edge, a slightly darker crease on the bottom edge, a thin dark line just inside each outline.
- **Tactile, not glossy.** Surfaces feel like real chips you could pick up: a gentle top-to-bottom gradient and a short, tight shadow under them. No glass, no reflections, no heavy 3D.
- **Plain shapes.** Rounded rectangles and rings with even corner radii. No squircles, no mascots, no stock illustration.

## Files

Every file is built by `source/build.py` (run `python3 brand/source/build.py`; needs `shapely`, `pillow`, `playwright`). Never edit an SVG or PNG by hand.

The edge strength depends on size. Large versions (128 px and up) use a medium edge with a faint colored glow. Small versions (64 px and below) use a stronger edge and no glow, so the mark stays sharp in a browser tab or a header.

| File | What it is | Where it goes |
| --- | --- | --- |
| `logo-light-small.svg` | Mark for light backgrounds, strong edge, tight margin | Headers, nav bars, anything 64 px and below |
| `logo-dark-small.svg` | Same, cream check, for dark backgrounds | Dark headers, dark mode, 64 px and below |
| `logo-light.svg` | Mark for light backgrounds, medium edge | Hero images, slides, anything 128 px and up |
| `logo-dark.svg` | Same, cream check | Large mark on dark backgrounds |
| `favicon.svg` | Small mark; the check turns cream when the browser is in dark mode | Browser tab |
| `favicon.ico` | Small mark at 16, 32 and 48 px | Browser tab fallback |
| `app-icon.svg` | App icon: the mark on a light rounded tile, medium edge | 128 px and up |
| `app-icon-small.svg` | App icon, strong edge | 64 px and below |
| `app-icon-square.svg` | App icon without the tile, filling the square | iOS home screen, Android, Stripe icon |
| `apple-touch-icon.png` | `app-icon-square` at 180 px | iOS home screen |
| `AppIcon.iconset/` | macOS icon set (`iconutil -c icns brand/AppIcon.iconset`) | A desktop app, if one ever exists |
| `png/` | PNG exports of all of the above | Places that do not take SVG |
| `stripe/` | `stripe-icon.png` and `stripe-logo.png` | Stripe dashboard, Settings, Branding |

On a web page, link the SVG with `<img src="/logo-light-small.svg">` rather than pasting its markup: the files use filters and ids that clash when two copies sit on one page.

## Stripe

Stripe dashboard, Settings, Branding:

- Brand color: `#1C2030`
- Accent color: `#2563CF`
- Icon: `stripe/stripe-icon.png`
- Logo: `stripe/stripe-logo.png`
