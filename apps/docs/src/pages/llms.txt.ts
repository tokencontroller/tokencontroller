// /llms.txt: the index of the docs for AI assistants (llmstxt.org), in sidebar order.
// The full text of every page is at /llms-full.txt.
import type { APIRoute } from 'astro';
import { docsInOrder, SITE } from '../llms';

export const GET: APIRoute = async () => {
  const pages = await docsInOrder();
  const lines = [
    '# Token Controller',
    '',
    '> Cost accounting and management control for AI agents: token entries from agents are matched to provider bills, tagged with the work and the people that caused them, and compared with token budgets.',
    '',
    `The full text of every page is at ${SITE}/llms-full.txt.`,
    '',
    '## Docs',
    '',
    ...pages.map((p) => `- [${p.title}](${p.url})${p.description ? `: ${p.description}` : ''}`),
    `- [Glossary](${SITE}/glossary): the precise meaning of every term the docs use.`,
    '',
  ];
  return new Response(lines.join('\n'), { headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
};
