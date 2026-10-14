// The docs at docs.tokencontroller.com: Astro with Starlight (docs/adr/0012).
// Pages are Markdown in src/content/docs; the glossary page is rendered from glossary.md.
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import { sidebar } from './src/sidebar.mjs';
import { satteri } from '@astrojs/markdown-satteri';
import codeTheme from './src/code-theme.json' with { type: 'json' };

export default defineConfig({
  site: 'https://docs.tokencontroller.com',
  // URLs have no trailing slash, as Cloudflare serves them (html_handling: drop-trailing-slash).
  trailingSlash: 'never',
  // "## The tag map {#map}" keeps a fixed anchor, so links like /account/tags#map survive rewording.
  markdown: { processor: satteri({ features: { headingAttributes: true, smartPunctuation: false } }) },
  integrations: [
    starlight({
      title: 'Token Controller Docs',
      titleDelimiter: '—',
      description: 'Docs for Token Controller: cost accounting and management control for AI agents.',
      favicon: '/favicon.svg',
      head: [
        { tag: 'link', attrs: { rel: 'icon', href: '/favicon.ico', sizes: '32x32' } },
        { tag: 'link', attrs: { rel: 'apple-touch-icon', href: '/apple-touch-icon.png' } },
        // Cloudflare Web Analytics: no cookies, same token as tokencontroller.com and the status page.
        {
          tag: 'script',
          attrs: {
            defer: true,
            src: 'https://static.cloudflareinsights.com/beacon.min.js',
            'data-cf-beacon': '{"token": "98702d468430482ca764f71c675e91b1"}',
          },
        },
      ],
      customCss: ['./src/styles/fonts.css', './src/styles/docs.css'],
      sidebar,
      // Prev/next links follow the sidebar order.
      pagination: true,
      lastUpdated: true,
      editLink: { baseUrl: 'https://github.com/tokencontroller/tokencontroller/blob/main/apps/docs/' },
      markdown: { headingLinks: false },
      expressiveCode: {
        themes: [codeTheme],
        useStarlightUiThemeColors: false,
        defaultProps: { frame: 'none' },
        styleOverrides: {
          borderColor: 'var(--line)',
          borderRadius: '6px',
          codeBackground: 'var(--bg-soft)',
          codeFontFamily: 'var(--mono)',
          codeFontSize: '0.82rem',
          codeLineHeight: '1.7',
          codePaddingBlock: '16px',
          codePaddingInline: '18px',
          frames: { shadowColor: 'transparent' },
        },
      },
      components: {
        Header: './src/components/Header.astro',
        Sidebar: './src/components/Sidebar.astro',
        PageTitle: './src/components/PageTitle.astro',
        Footer: './src/components/Footer.astro',
        ThemeProvider: './src/components/ThemeProvider.astro',
        ThemeSelect: './src/components/Empty.astro',
        MobileMenuFooter: './src/components/Empty.astro',
      },
    }),
  ],
});
