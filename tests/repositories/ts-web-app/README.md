# ts-web-app

A deliberately small React-style web application used as a test fixture for
Context. It is shaped like a real product front end but has **no
dependencies**: React is referenced by the `.tsx` files for indexing purposes
only and is not installed, and there is no TypeScript compiler in the offline
environment, so `npm run typecheck` is a no-op.

## Layout and conventions

- Components live in `src/components/` (one component per file, PascalCase).
- Hooks live in `src/hooks/` and are named `useXxx.ts`.
- Network access goes through `src/api/client.ts`; components never call `fetch` directly.
- Pure helpers live in `src/utils/`. Logic that needs to run under Node tests is
  kept in `.mjs` twins (for example `format.ts` is mirrored by `format.mjs`) so the
  test runner does not need a build step.
- Styles are plain CSS in `src/styles/`.
- Tests live in `src/__tests__/` and use the built-in Node test runner. Each
  `*.test.mjs` file is imported from `src/__tests__/index.js`, which is the entry
  point `node --test src/__tests__/` resolves to; add new test files there.

## Commands

```sh
npm test            # node --test src/__tests__/
npm run typecheck   # no-op in this fixture
```

## Known gaps (intentional task opportunities)

- `LoginButton` has no loading state. `useAuth` already exposes `isLoading`
  and `Spinner` already exists, but the button never shows it.
- `Footer` hard-codes the copyright year.
