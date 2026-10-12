// The docs pages in sidebar order, as /llms.txt and /llms-full.txt list them.
import { getCollection } from 'astro:content';
import { sidebar } from './sidebar.mjs';

export const SITE = 'https://docs.tokencontroller.com';

export type Page = { title: string; description?: string; url: string; body: string };

type Item = string | { slug?: string; link?: string; label?: string };

export async function docsInOrder(): Promise<Page[]> {
  const entries = await getCollection('docs');
  const byId = new Map(entries.map((e) => [e.id, e]));
  const order: string[] = [];
  for (const group of sidebar as { items: Item[] }[]) {
    for (const item of group.items) {
      if (typeof item === 'string') order.push(item);
      else if (item.slug) order.push(item.slug);
      else if (item.link === '/') order.push('index');
    }
  }
  for (const id of [...byId.keys()].sort()) if (!order.includes(id)) order.push(id);

  return order.flatMap((id) => {
    const entry = byId.get(id);
    if (!entry) return [];
    const path = id === 'index' ? '' : `/${id}`;
    return [{ title: entry.data.title, description: entry.data.description, url: `${SITE}${path}`, body: entry.body ?? '' }];
  });
}
