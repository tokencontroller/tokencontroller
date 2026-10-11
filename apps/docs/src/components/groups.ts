// Finds the sidebar group a page belongs to, for breadcrumbs and prev/next links.
type Entry = { type: string; label: string; href?: string; isCurrent?: boolean; entries?: Entry[] };

const clean = (href: string) => href.replace(/[#?].*$/, '').replace(/(.)\/$/, '$1');

export function groupOf(sidebar: Entry[], match: { href?: string; current?: boolean }): string | undefined {
  for (const group of sidebar) {
    if (group.type !== 'group') continue;
    for (const e of group.entries ?? []) {
      if (e.type !== 'link') continue;
      if (match.current ? e.isCurrent : clean(e.href!) === clean(match.href!)) return group.label;
    }
  }
}
