---
id: 2026-10-02-shadcn-wave2
date: 2026-10-02
status: binding
canon: prove-it-works
question: Can all screens move off the hand-rolled button/input dialects without breaking the build?
verdict: Yes. Badge, Progress, Input, Label, and Radix Select primitives shipped; settings plus all six pages plus help plus onboarding migrated; dead CSS purged.
evidence:
  - npm install -S @radix-ui/react-select @radix-ui/react-progress @radix-ui/react-label (0 vulnerabilities)
  - src/components/ui/badge.tsx (local/cloud/degraded/idle per plan-doc badge semantics), progress.tsx, input.tsx (mono prop), label.tsx, select.tsx
  - settings.tsx rewritten on primitives (4 native selects to Radix, all buttons/inputs converted)
  - pages/home, dictionary, snippets, style, help, insights, onboarding.tsx converted (flow-light to paper, flow-mini to outline xs/sm, bars to Progress)
  - styles.css purged: button.primary/ghost, .flow-light/dark/mini/search/input/label, .field, .badge, .history, .settings, .flow-bar-track/fill, HC orphans; stray purple #6d5bd0 replaced with teal
  - Select-String confirms zero TSX references to removed classes (flow-pick/flow-check kept intentionally)
  - npm run build (tsc --noEmit + vite build, 135 modules, CSS 35.07 to 32.25 kB, 4.26s)
filed_by: cardinal
---

High-contrast propagates into the new primitives via `@theme inline` var
references in index.css. Remaining custom CSS is pill craft, shell chrome,
and flow-pick tone cards (Card primitive is the next wave if wanted).
