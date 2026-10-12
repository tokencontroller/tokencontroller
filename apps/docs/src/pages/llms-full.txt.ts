// /llms-full.txt: every docs page as Markdown in one file, in sidebar order, then the glossary.
// AI assistants read it, and our support answers and site assistant use it as their only
// source. Built from the same Markdown as the pages, so it never drifts.
import type { APIRoute } from 'astro';
import { docsInOrder, SITE } from '../llms';
import glossary from '../../glossary.md?raw';

export const GET: APIRoute = async () => {
  const pages = await docsInOrder();
  const parts = [
    '# Token Controller Docs',
    '',
    '> Cost accounting and management control for AI agents. These are the complete docs of docs.tokencontroller.com, one section per page.',
    '',
  ];
  for (const page of pages) {
    parts.push(`---\n\n# ${page.title}\n\nURL: ${page.url}\n`);
    if (page.description) parts.push(`> ${page.description}\n`);
    parts.push(page.body.trim(), '');
  }
  parts.push(`---\n\n# Glossary\n\nURL: ${SITE}/glossary\n`, glossary.trim(), '');
  return new Response(parts.join('\n'), { headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
};
