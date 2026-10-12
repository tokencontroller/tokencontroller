# The additions slot

This folder is where a private build adds its own parts to the web app, such as a page of its own. The open build's folder adds nothing, so the open web app is the same with or without the slot.

A private build copies the web app at the same commit and puts its own `additions/` folder here in place of this one. Its `index.ts` exports `additions`, and `satisfies Additions` from `src/lib/additions.ts`, which holds the types. Keep `satisfies` rather than a type annotation: it keeps the exact types of the build's routes, so links to them type-check like the app's own. Its `mocks.ts` exports `addedMocks`, typed `AddedMocks` from `src/mocks/handlers.ts`. The type check (`pnpm typecheck`) fails when the folder no longer fits the app.

`additions-check/`, next to `src/`, is a folder with one of each part, its routes written in the object as `routes: ({ app, requireRole }) => [createRoute({ getParentRoute: () => app, ... })]`. `pnpm typecheck` also checks the app with that folder in the slot, so a change to the app or to the types that a private build's folder would no longer fit fails in this repo too.

Tests in the folder go with it. `open.test.ts` checks that the open build's folder adds nothing; a private build's folder brings its own tests instead.

What the folder holds:

| Part | What it is | Where the app uses it |
|---|---|---|
| `routes` | More pages for signed-in members. It gets the app's route, as their parent, and `requireRole`, which keeps a page to a role and the roles above it. | `src/router.tsx` |
| `useMenu` | A hook that returns the menu entries to show now. Each names its menu group by title, the lowest role that sees it, and a page (`to`) or a dialog (`Panel`). A title the menu does not have starts a group after the others. Leave an entry out while the build's endpoint answers 404. | `src/components/app-shell.tsx` |
| `parts.SettingsPlan` | A line about the plan in Settings, for controllers. | `src/routes/settings.tsx` |
| `parts.PeopleNote` | A note under the header of People. | `src/routes/people.tsx` |
| `parts.OverviewNote` | A note at the end of Overview. | `src/routes/overview.tsx` |
| `parts.SignUpNote` | A note above the sign-up form. | `src/routes/sign-in.tsx` |
| `onResponse` | Gets every answer from the service, from `typed`, `call` and `download` in `src/api/client.ts`. It must not read the body or throw. | `src/api/client.ts` |
| `onErrorShown` | Gets every error message the app shows, from `ErrorBox` and `FormError`. | `src/components/ui/` |
| `addedMocks`, in `mocks.ts` | The mock build's answers for the build's own endpoints, for journeys and screenshots. It gets what the app's own mock areas get (`MockKit`): `member` for a signed-in member and the mock organization's data, `fail` for an error, and `now`. Its answers come before the app's own. Only the mock build loads the file. | `src/mocks/handlers.ts` |

The build's code may import anything from the app (`@/api/endpoints`, `@/components/ui/...`, `@/lib/queries`). Calls to the build's own endpoints go through `typed` when the build generates the API types from its own OpenAPI document (`TC_OPENAPI` in `scripts/generate-api.mjs`), or through `call` with types of its own. Keep top-level code in the folder to definitions: the app loads the slot while its own modules are still loading.
