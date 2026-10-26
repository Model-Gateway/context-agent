# Context website

The landing page for Context, built with Next.js 16 (App Router), React 19 and Tailwind CSS v4.

```bash
cd apps/web
npm install
npm run dev        # http://localhost:3000
npm run build      # production build
npm run lint
```

## Layout

- `src/app/page.tsx` composes the page from sections, with a hatched band between each.
- `src/components/sections/`: one file per section (nav, hero, showcase, features, …).
- `src/components/ui/`: the design system: the blueprint frame (`frame.tsx`: page frame,
  crosshairs, hatch bands, sections), buttons, eyebrow, photo panels and glass frames, mock windows,
  the Context logo and scroll reveal.
- `src/components/mock/`: illustrations of the app's UI used inside the sections.
- `src/lib/site.ts`: version, download and documentation links. Update it with each release.
- `src/assets/photos/`: the app's banner art, used as photographic backgrounds.

Colours, fonts, textures and animations are tokens in `src/app/globals.css`.
