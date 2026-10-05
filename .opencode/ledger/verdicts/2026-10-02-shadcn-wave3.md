---
id: 2026-10-02-shadcn-wave3
date: 2026-10-02
status: binding
canon: prove-it-works
question: What finishes the component system: Card, Switch, Dialog, Tooltip with real callers?
verdict: All four shipped with migrations. Tone cards to Card, settings checkboxes to Switch, wipe two-click to Dialog confirm, sidebar nav gains Tooltips. Last custom interactive CSS (.flow-pick) deleted.
evidence:
  - npm install -S @radix-ui/react-switch @radix-ui/react-dialog @radix-ui/react-tooltip (0 vulnerabilities)
  - src/components/ui/card.tsx (selected ring, asChild), switch.tsx, dialog.tsx, tooltip.tsx
  - pages/style.tsx + onboarding.tsx tone cards to Card asChild with aria-pressed (teal ring replaces stray purple)
  - settings.tsx 4 checkboxes to Switch rows with Label htmlFor
  - onboarding.tsx wipe two-click to Dialog (title, description, keep/erase, destructive confirm, closes on success)
  - shell.tsx nav wrapped in TooltipProvider with side=right labels plus aria-labels
  - styles.css .flow-pick rules deleted; Select-String confirms zero TSX references
  - npm run build (tsc --noEmit + vite build, 142 modules, 4.69s)
filed_by: cardinal
---

Remaining custom CSS is layout and craft only: pill physics, shell chrome,
heroes, flow rows/cards/check(radio) shells. Radios stay native by choice.
The interactive layer is now one language: Button, Badge, Card, Dialog,
Input, Label, Progress, Select, Switch, Tooltip.
