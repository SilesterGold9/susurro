---
id: 2026-10-02-shadcn-foundation
date: 2026-10-02
status: binding
canon: prove-it-works
question: Hand-rolled CSS has seven button dialects; adopt shadcn or hand-roll a kit?
verdict: Adopted shadcn-style owned primitives on Tailwind v4 + Radix, themed to Susurro tokens; foundation plus Button shipped, onboarding migrated.
evidence:
  - npm install -S tailwindcss @tailwindcss/vite class-variance-authority clsx tailwind-merge @radix-ui/react-slot (18 packages, 0 vulnerabilities)
  - app-tauri/vite.config.ts (tailwindcss plugin + @ alias), tsconfig.json (@/* paths)
  - app-tauri/src/index.css (theme+utilities only, no preflight; pine/ember/cream/ink tokens)
  - app-tauri/src/lib/utils.ts (cn), app-tauri/src/components/ui/button.tsx (7 variants x 5 sizes + loading)
  - app-tauri/src/onboarding.tsx (11 raw buttons converted to Button; flow-dark/flow-mini gone from that file)
  - npm run build (tsc --noEmit + vite build, 63 modules, built in 2.95s)
filed_by: cardinal
---

Tailwind runs utilities-only so the pill/shell craft renders untouched.
`flow-dark` maps to `Button variant="ink"`, `flow-mini` to
`variant="outline" size="sm"`, busy text to the `loading` prop, and the wipe
confirm escalates to `variant="destructive"`. Tone pick cards stay custom
until the Card primitive lands. Next: Badge/Progress/Input/Select primitives,
then settings plus pages wave by wave.
