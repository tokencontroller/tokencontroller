# The docs move to Astro with Starlight

The docs under `apps/docs/` are hand-built HTML pages today. They move to one Astro project with Starlight: pages become Markdown with frontmatter that Astro checks at build time, the sidebar and search come from Starlight, and the glossary stays `glossary.md` and is rendered into the site. The marketing site in the private repo can later use the same Astro toolchain. The docs keep their URLs; the deploy stays Cloudflare Workers Builds.

## Why

Markdown in the repo is what agents and the maintainer edit best, and a broken page fails the build instead of shipping. Astro runs on Vite, the same toolchain as the web app, so there is no second build system. The alternatives considered add a framework with no other use here.
