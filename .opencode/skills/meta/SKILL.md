---
name: meta
description: "The memory of the Conclave: the-file (reopen the case file to resume a session), case-notes (log lessons and verdicts into the ledger), the-ledger (record evidence for every done claim). Use whenever a session in a Conclave-adapted project begins, whenever a lesson or a ruling occurs, and whenever a task is about to be declared done. Read the ledger before ruling, write to it before claiming. Only in projects with a ledger at .opencode/ledger."
---

# Meta

The Conclave's memory is the ledger itself, never a transcript scrape. What
the ledger records is durable and queryable; what it omits never happened.

## the-file

On resume, reopen the case file at `.opencode/ledger/the-file.md`. It holds
the open questions, the pending rulings, and the next actions of the last
session. Orient from it before doing anything; append to it before leaving.

## case-notes

When a lesson surfaces or a question is settled, file a case note in
`.opencode/ledger/case-notes.md` before moving on. One entry, dated, named
assertion with what the evidence showed. A lesson that is not filed is a
lesson paid for twice.

## the-ledger

A verdict without evidence is opinion. When a task is claimed done, record
in the ledger what was run and what it returned: the command, the output, the
diff, the test name. The proof gate inspects these records; a done claim with
no evidence link is flagged. Evidence over assertion.