import pathlib
import re,html
src=open(pathlib.Path(__file__).resolve().parent/'glossary.md').read()
lines=src.split('\n')
out=['    <p class="lead">The precise meaning of every term in Token Controller. The product and these docs use these words, and only these.</p>\n']
i=0; cur=None
def slug(t): return re.sub(r'[^a-z0-9]+','-',t.lower()).strip('-')
body=src.split('## Language',1)[1]
for block in re.split(r'\n(?=### )',body):
    block=block.strip()
    if not block.startswith('### '): continue
    head,rest=block.split('\n',1)
    h=head[4:].strip()
    out.append(f'\n    <h2 id="{slug(h)}">{html.escape(h)}</h2>\n    <table>\n      <thead><tr><th>Term</th><th>Meaning</th></tr></thead>\n      <tbody>')
    for entry in re.split(r'\n\s*\n',rest.strip()):
        m=re.match(r'\*\*(.+?)\*\*(?:, \*\*(.+?)\*\*)?:\s*\n(.*)',entry,re.S)
        if not m: continue
        term=m.group(1)+(', '+m.group(2) if m.group(2) else '')
        txt=m.group(3).strip()
        avoid=''
        am=re.search(r'\n?_Avoid_:\s*(.*)$',txt)
        if am: avoid=am.group(1).strip(); txt=txt[:am.start()].strip()
        d=html.escape(' '.join(txt.split()))
        d=re.sub(r'`([^`]+)`',r'<code>\1</code>',d)
        if avoid: d+=f'<br><span style="color:var(--muted);font-size:.85em">Not: {html.escape(avoid)}</span>'
        out.append(f'        <tr><td>{html.escape(term)}</td><td>{d}</td></tr>')
    out.append('      </tbody>\n    </table>')
open(pathlib.Path(__file__).resolve().parent/'pages'/'glossary.html','w').write('\n'.join(out)+'\n')
print(len(out))
