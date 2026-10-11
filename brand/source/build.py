"""Builds every brand file in brand/ from exact geometry.

    python3 brand/source/build.py          # writes brand/*.svg, brand/png, brand/AppIcon.iconset, ...

Needs: pip install shapely pillow playwright  (and a Chromium for PNG export; set CHROMIUM=/path if
Playwright cannot find one).

The mark: seven rounded token chips of uneven length in a ring, rainbow in order from the top,
with a check in the middle. The look: one light from straight above. Every shape gets a crisp
edge (a darker, more saturated line just inside the outline and a thin bright line just outside),
a bright top rim, a dark bottom crease and a faint inside border. Large sizes (128 px and up) use
the MEDIUM edge with a small colored glow; small sizes use the stronger SMALL edge without glow,
so the mark stays sharp in a browser tab.
"""
import asyncio, colorsys, math, os, shutil
from PIL import Image
from shapely.geometry import LineString

OUT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))  # brand/

# ---- geometry (units: the mark spans about 83 units) ----
PARTS = [10, 16, 8, 13, 11, 18, 9]           # chip lengths: uneven on purpose, longest under 3x the shortest
R, W, RAD, GAP, START = 34, 13, 3.2, 5, -90  # ring radius, chip width, corner radius, gap in degrees, first chip at the top
INK = '#1C2030'
CREAM = '#F6F3EC'
SPECTRUM = ['#EE4545', '#F77A22', '#F7C125', '#3FBF68', '#22B5AE', '#2F80E8', '#7E57E8']
TILE = 'M285 100H739C841 100 924 183 924 285V739C924 841 841 924 739 924H285C183 924 100 841 100 739V285C100 183 183 100 285 100Z'
MEDIUM = dict(edge=1.3, halo=1.4, glow=0.45)  # 128 px and up
SMALL = dict(edge=2.0, halo=2.0, glow=0.0)    # 64 px and below

def f(v): return f'{v:.3f}'.rstrip('0').rstrip('.')
def polar(cx, cy, rho, a): a = math.radians(a); return (cx + rho*math.cos(a), cy + rho*math.sin(a))

def chip(cx, cy, a0, a1, s):
    """One rounded ring segment with exact arcs; s = canvas px per unit."""
    r, w, rad = R*s, W*s, RAD*s
    ro, ri, Ro, Ri = r + w/2 - rad, r - w/2 + rad, r + w/2, r - w/2
    ang = lambda rho: math.degrees(math.asin(rad/rho))
    o0, o1, i0, i1 = a0 + ang(ro), a1 - ang(ro), a0 + ang(ri), a1 - ang(ri)
    def side(rho, a, aside, sign):
        c = polar(cx, cy, rho, a); t = math.radians(aside)
        return (c[0] - sign*rad*math.sin(t), c[1] + sign*rad*math.cos(t))
    p = [polar(cx, cy, Ro, o0), polar(cx, cy, Ro, o1), side(ro, o1, a1, 1), side(ri, i1, a1, 1),
         polar(cx, cy, Ri, i1), polar(cx, cy, Ri, i0), side(ri, i0, a0, -1), side(ro, o0, a0, -1)]
    pt = lambda q: f'{f(q[0])} {f(q[1])}'
    return (f'M{pt(p[0])}A{f(Ro)} {f(Ro)} 0 0 1 {pt(p[1])}A{f(rad)} {f(rad)} 0 0 1 {pt(p[2])}'
            f'L{pt(p[3])}A{f(rad)} {f(rad)} 0 0 1 {pt(p[4])}A{f(Ri)} {f(Ri)} 0 0 0 {pt(p[5])}'
            f'A{f(rad)} {f(rad)} 0 0 1 {pt(p[6])}L{pt(p[7])}A{f(rad)} {f(rad)} 0 0 1 {pt(p[0])}Z')

def chips(cx, cy, s):
    span = 360 - GAP*len(PARTS); a = START; out = []
    for n in PARTS:
        b = a + span*n/sum(PARTS); out.append(chip(cx, cy, a, b, s)); a = b + GAP
    return out

def check(cx, cy, s):
    """The check as a filled outline (round caps and join), so it can take the same edge treatment."""
    k = 26*s
    line = LineString([(cx - k*0.5, cy + k*0.02), (cx - k*0.12, cy + k*0.4), (cx + k*0.5, cy - k*0.38)])
    poly = line.buffer(7*s/2, quad_segs=24, cap_style='round', join_style='round')
    return 'M' + 'L'.join(f'{x:.2f} {y:.2f}' for x, y in poly.exterior.coords) + 'Z'

# ---- color helpers ----
def adjust(c, sat=1.0, light=0.0):
    h, l, s = colorsys.rgb_to_hls(*(int(c[i:i+2], 16)/255 for i in (1, 3, 5)))
    r = colorsys.hls_to_rgb(h, min(1, max(0, l + light)), min(1, max(0, s*sat)))
    return '#' + ''.join(f'{round(x*255):02X}' for x in r)

# ---- lighting ----
class Doc:
    def __init__(self, k): self.defs, self.k, self.n = [], k, 0
    def id(self, p): self.n += 1; return f'{p}{self.n}'

def shape(doc, d, fill, u, top, bottom, tint, edge=None, halo=None, glow=None, border=(0.5, 0.18)):
    """Returns (under, over) markup. u = one 'recipe pixel' at this canvas size."""
    k, D = doc.k, doc.defs; under, over = [], []
    if glow and k['glow']:  # small glow in the shape's own color, light bouncing below it
        g = doc.id('gl'); D.append(f'<filter id="{g}" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="{4*u:.2f}"/></filter>')
        under.append(f'<path d="{d}" fill="{glow[0]}" opacity="{glow[1]*k["glow"]:.2f}" filter="url(#{g})" transform="translate(0 {2*u:.2f})"/>')
    if halo:  # bright line just outside: the light side of a sharpened edge
        under.append(f'<path d="{d}" fill="none" stroke="{halo[0]}" stroke-opacity="{halo[1]}" stroke-width="{2*k["halo"]*u:.2f}" stroke-linejoin="round"/>')
    ew = k['edge']*u if edge else 0
    over.append(f'<path d="{d}" fill="{fill}"/>')
    for name, color, op, dy in (('mt', '#FFFFFF', top, ew + 1.6*u), ('mb', tint, bottom, -(ew + 1.6*u))):
        mid = doc.id(name)  # shape minus itself shifted: a crisp rim on top, a crease at the bottom
        D.append(f'<mask id="{mid}" maskUnits="userSpaceOnUse" x="0" y="0" width="100000" height="100000"><path d="{d}" fill="#fff"/>'
                 f'<path d="{d}" fill="#000" transform="translate(0 {dy:.2f})"/></mask>')
        over.append(f'<path d="{d}" fill="{color}" opacity="{op}" mask="url(#{mid})"/>')
    g, c = doc.id('bg'), doc.id('cl')  # faint inside border, light on top, dark at the bottom
    D.append(f'<linearGradient id="{g}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#FFFFFF" stop-opacity="{border[0]}"/>'
             f'<stop offset="0.5" stop-color="#FFFFFF" stop-opacity="0"/><stop offset="1" stop-color="{tint}" stop-opacity="{border[1]}"/></linearGradient>')
    D.append(f'<clipPath id="{c}"><path d="{d}"/></clipPath>')
    over.append(f'<path d="{d}" fill="none" stroke="url(#{g})" stroke-width="{1.4*u:.2f}" clip-path="url(#{c})"/>')
    if edge:  # darker, more saturated line just inside: the dark side of a sharpened edge
        over.append(f'<path d="{d}" fill="none" stroke="{edge}" stroke-width="{2*ew:.2f}" clip-path="url(#{c})"/>')
    return under, over

def mark(doc, cx, cy, s, dark=False):
    u = s*2.5/8.2; under, over = [], []
    for i, d in enumerate(chips(cx, cy, s)):
        c = SPECTRUM[i]; g = doc.id('f')
        doc.defs.append(f'<linearGradient id="{g}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{adjust(c, 1.05, 0.10)}"/>'
                        f'<stop offset="1" stop-color="{adjust(c, 1.1, -0.03)}"/></linearGradient>')
        a, b = shape(doc, d, f'url(#{g})', u, 0.6, 0.28, adjust(c, 1.2, -0.3),
                     edge=adjust(c, 1.1, 0.10) if dark else adjust(c, 1.25, -0.08),
                     halo=('#000000', 0.5) if dark else ('#FFFFFF', 0.95), glow=(c, 0.6 if dark else 0.45))
        under += a; over += b
    g = doc.id('f'); top, bottom = ('#FFFFFF', '#E4E0D6') if dark else ('#3B4058', INK)
    doc.defs.append(f'<linearGradient id="{g}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{top}"/><stop offset="1" stop-color="{bottom}"/></linearGradient>')
    a, b = shape(doc, check(cx, cy, s), f'url(#{g})', u, 0.9 if dark else 0.35, 0.15 if dark else 0.35, '#000000',
                 edge='#FFFFFF' if dark else '#06070D', halo=('#000000', 0.5) if dark else ('#FFFFFF', 0.95),
                 glow=None if dark else (INK, 0.25))
    under += a; over += b
    return ''.join(under + over)

def svg(size, doc, body, style=''):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}" width="{size}" height="{size}">{style}'
            f'<defs>{"".join(doc.defs)}</defs>{body}</svg>\n')

TILE_FILL = '<linearGradient id="tile" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#FFFFFF"/><stop offset="1" stop-color="#ECEEF2"/></linearGradient>'

def app_icon(k):
    doc = Doc(k); doc.defs.append(TILE_FILL)
    doc.defs.append('<filter id="sh" x="-30%" y="-30%" width="160%" height="160%">'  # contact shadow + wide ambient shadow
                    '<feDropShadow dx="0" dy="4" stdDeviation="5" flood-color="#1C2030" flood-opacity="0.16"/>'
                    '<feDropShadow dx="0" dy="28" stdDeviation="34" flood-color="#2A3050" flood-opacity="0.14"/></filter>')
    a, b = shape(doc, TILE, 'url(#tile)', 2.5, 0.9, 0.10, INK, edge='#D3D7DF', border=(0.9, 0.10))
    body = f'<path d="{TILE}" fill="url(#tile)" filter="url(#sh)"/>' + ''.join(b)
    body += f'<path d="{TILE}" fill="none" stroke="{INK}" stroke-opacity="0.16" stroke-width="1.5"/>'
    return svg(1024, doc, body + mark(doc, 512, 512, 8.2))

def app_icon_square(k):
    """Full-bleed square for iOS, Android and Stripe, which add their own rounding."""
    doc = Doc(k); doc.defs.append(TILE_FILL)
    return svg(1024, doc, '<rect width="1024" height="1024" fill="url(#tile)"/>' + mark(doc, 512, 512, 8.2*1.12))

def logo(k, dark, margin):
    doc = Doc(k); s = 100/((R + W/2)*2 + margin)
    return svg(100, doc, mark(doc, 50, 50, s, dark))

def favicon():
    """Small logo; the check turns cream when the browser is in dark mode."""
    doc = Doc(SMALL); s = 100/((R + W/2)*2 + 4)
    light, dark = mark(doc, 50, 50, s), mark(doc, 50, 50, s, dark=True)
    css = '<style>.d{display:none}@media (prefers-color-scheme:dark){.l{display:none}.d{display:inline}}</style>'
    return svg(100, doc, f'<g class="l">{light}</g><g class="d">{dark}</g>', css)

SOURCES = {
    'app-icon.svg': lambda: app_icon(MEDIUM), 'app-icon-small.svg': lambda: app_icon(SMALL),
    'app-icon-square.svg': lambda: app_icon_square(MEDIUM),
    'logo-light.svg': lambda: logo(MEDIUM, False, 12), 'logo-dark.svg': lambda: logo(MEDIUM, True, 12),
    'logo-light-small.svg': lambda: logo(SMALL, False, 4), 'logo-dark-small.svg': lambda: logo(SMALL, True, 4),
    'favicon.svg': favicon,
}
PNGS = ([(f'app-icon{"-small" if z <= 64 else ""}.svg', z, f'png/app-icon-{z}.png') for z in (16, 32, 64, 128, 256, 512, 1024)]
        + [('app-icon-square.svg', z, f'png/app-icon-square-{z}.png') for z in (180, 192, 512, 1024)]
        + [('logo-light-small.svg', z, f'png/logo-light-{z}.png') for z in (16, 32, 48, 64)]
        + [(f'logo-{t}.svg', z, f'png/logo-{t}-{z}.png') for t in ('light', 'dark') for z in (512, 1024)])

async def render():
    from playwright.async_api import async_playwright
    async with async_playwright() as p:
        kw = {'executable_path': os.environ['CHROMIUM']} if os.environ.get('CHROMIUM') else {}
        browser = await p.chromium.launch(**kw); page = await browser.new_page()
        html = f'{OUT}/.render.html'  # an <img> scales the SVG to the viewport; file:// needs a real page
        for src, z, dst in PNGS:
            await page.set_viewport_size({'width': z, 'height': z})
            open(html, 'w').write(f'<body style="margin:0"><img src="{src}" width="{z}" height="{z}" style="display:block">')
            await page.goto(f'file://{html}'); await page.wait_for_timeout(50)
            await page.screenshot(path=f'{OUT}/{dst}', omit_background=True)
        await browser.close()
        os.remove(html)

def main():
    for d in ('png', 'AppIcon.iconset', 'stripe'): os.makedirs(f'{OUT}/{d}', exist_ok=True)
    for name, make in SOURCES.items(): open(f'{OUT}/{name}', 'w').write(make())
    asyncio.run(render())
    ico = [Image.open(f'{OUT}/png/logo-light-{z}.png') for z in (16, 32, 48)]
    ico[2].save(f'{OUT}/favicon.ico', sizes=[(16, 16), (32, 32), (48, 48)], append_images=ico[:2])
    shutil.copy(f'{OUT}/png/app-icon-square-180.png', f'{OUT}/apple-touch-icon.png')
    for z in (16, 32, 128, 256, 512):  # macOS: iconutil -c icns brand/AppIcon.iconset
        shutil.copy(f'{OUT}/png/app-icon-{z}.png', f'{OUT}/AppIcon.iconset/icon_{z}x{z}.png')
        shutil.copy(f'{OUT}/png/app-icon-{z*2}.png', f'{OUT}/AppIcon.iconset/icon_{z}x{z}@2x.png')
    Image.open(f'{OUT}/png/app-icon-square-512.png').convert('RGB').save(f'{OUT}/stripe/stripe-icon.png')
    logo_png = Image.open(f'{OUT}/png/logo-light-1024.png').convert('RGBA')
    white = Image.new('RGBA', logo_png.size, (255, 255, 255, 255)); white.alpha_composite(logo_png)
    white.convert('RGB').save(f'{OUT}/stripe/stripe-logo.png')

if __name__ == '__main__':
    main()
