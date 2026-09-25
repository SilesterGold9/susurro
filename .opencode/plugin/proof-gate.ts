// proof-gate.ts — the Conclave proof gate (commitment B).
//
// v2: on session idle, scan all assistant turns for completion claims,
// reconcile against verdict records in the ledger, and flag claims
// with no evidence link and no matching verdict. The gate never blocks
// and never edits; it observes and reports.
import type { Plugin } from "@opencode-ai/plugin"
import { readdir, readFile, stat } from "fs/promises"
import { join } from "path"

// Completion claims the gate listens for.
const DONE_RE = /\b(done|complete|completed|finished|fixed|resolved|landed|shipped|closed)\b/i

// What counts as evidence for a claim: the ledger, the proof artifacts, or
// a concrete command result quoted in the turn.
const EVIDENCE_RE =
  /(evidence|verdicts?|\b\.opencode\/ledger\b|the-ledger|test|curl|diff|build (passed|succeeded)|\bexit (code )?0\b|\bpassed\b|\bverify|```)/i

// Extract verdict IDs from a verdict record's frontmatter.
const VERDICT_ID_RE = /^id:\s*(.+)$/m

// Pure and unit-testable. Returns the claim severity, if any.
export function assess(lastTurn: string): { claim: boolean; evidence: boolean; flagged: boolean } {
  const claim = DONE_RE.test(lastTurn)
  const evidence = EVIDENCE_RE.test(lastTurn)
  return { claim, evidence, flagged: claim && !evidence }
}

// Scan a session's assistant turns for completion claims.
export function scanSession(
  messages: Array<{ info?: { role?: string }; parts?: Array<{ type?: string; text?: string }> }>,
): { turnIndex: number; snippet: string; claim: boolean; evidence: boolean; flagged: boolean }[] {
  const results: {
    turnIndex: number
    snippet: string
    claim: boolean
    evidence: boolean
    flagged: boolean
  }[] = []

  for (let i = 0; i < messages.length; i++) {
    const msg = messages[i]
    if (msg.info?.role !== "assistant") continue

    const text = (msg.parts ?? [])
      .filter((part) => part.type === "text")
      .map((part) => part.text ?? "")
      .join("\n")

    if (!text) continue

    const verdict = assess(text)
    if (verdict.claim) {
      results.push({
        turnIndex: i,
        snippet: text.slice(0, 300),
        ...verdict,
      })
    }
  }

  return results
}

// Read verdict records from the ledger directory.
async function readVerdicts(
  ledgerDir: string,
): Promise<{ id: string; evidence: string[]; canon: string }[]> {
  const verdictsDir = join(ledgerDir, "verdicts")
  let entries: string[]
  try {
    entries = await readdir(verdictsDir)
  } catch {
    return []
  }

  const verdicts: { id: string; evidence: string[]; canon: string }[] = []

  for (const entry of entries) {
    if (!entry.endsWith(".md")) continue
    try {
      const content = await readFile(join(verdictsDir, entry), "utf-8")
      const idMatch = VERDICT_ID_RE.exec(content)
      if (!idMatch) continue

      const id = idMatch[1].trim()

      // Extract evidence block from frontmatter.
      const fmEnd = content.indexOf("---", 4)
      const fm = fmEnd > 0 ? content.slice(4, fmEnd) : content

      const evidenceLines: string[] = []
      let inEvidence = false
      for (const line of fm.split("\n")) {
        if (line.startsWith("evidence:")) {
          inEvidence = true
          continue
        }
        if (inEvidence && line.startsWith("  - ")) {
          evidenceLines.push(line.slice(4).trim())
        } else if (inEvidence && !line.startsWith(" ")) {
          inEvidence = false
        }
      }

      const canonMatch = fm.match(/^canon:\s*(.+)$/m)
      const canon = canonMatch ? canonMatch[1].trim() : "unknown"

      verdicts.push({ id, evidence: evidenceLines, canon })
    } catch {
      // Skip unreadable verdict files.
    }
  }

  return verdicts
}

// Check if a session claim is backed by a verdict record with evidence.
function isBackedByVerdict(
  snippet: string,
  verdicts: { id: string; evidence: string[] }[],
): boolean {
  // If the snippet references a verdict ID directly, check it.
  const idRef = snippet.match(/(?:verdict|verdicts?[/\s])(\d{4}-\d{2}-\d{2}-[\w-]+)/)
  if (idRef) {
    const v = verdicts.find((verd) => verd.id === idRef[1])
    if (v && v.evidence.length > 0) return true
  }

  // If the snippet references evidence keywords and there exists any verdict
  // with evidence, consider it backed. This is a loose check; tighter
  // matching can be added when verdict records gain richer linking.
  if (EVIDENCE_RE.test(snippet) && verdicts.some((v) => v.evidence.length > 0)) {
    return true
  }

  return false
}

function sessionIDOf(event: { properties?: Record<string, unknown> }): string | undefined {
  const p = event.properties ?? {}
  for (const key of ["sessionID", "sessionId", "id"] as const) {
    const v = p[key]
    if (typeof v === "string") return v
  }
  return undefined
}

async function findLedgerDir(startDir: string): Promise<string | null> {
  let dir = startDir
  for (let i = 0; i < 10; i++) {
    try {
      await stat(join(dir, ".opencode", "ledger", "verdicts"))
      return join(dir, ".opencode", "ledger")
    } catch {
      // Not found here, go up.
    }
    const parent = join(dir, "..")
    if (parent === dir) break
    dir = parent
  }
  return null
}

export const proofGate: Plugin = async ({ client }) => {
  const log = (level: "info" | "warn" | "error", message: string, extra?: unknown) => {
    client.app
      .log({ body: { service: "conclave-proof-gate", level, message, extra } })
      .catch(() => {})
  }

  return {
    event: async ({ event }) => {
      if (event.type !== "session.idle") return

      const sessionID = sessionIDOf(event as { properties?: Record<string, unknown> })
      if (!sessionID) {
        log("warn", "proof gate saw a session idle with no session id")
        return
      }

      try {
        const messages = await client.session.messages({ path: { id: sessionID } })

        // Scan all assistant turns for completion claims.
        const claims = scanSession(messages)
        if (claims.length === 0) return

        // Try to find the ledger for verdict reconciliation.
        const cwd = process.cwd()
        const ledgerDir = await findLedgerDir(cwd)
        const verdicts = ledgerDir ? await readVerdicts(ledgerDir) : []

        for (const claim of claims) {
          if (claim.flagged) {
            // Claim with no evidence keyword. Check if a verdict backs it.
            if (ledgerDir && isBackedByVerdict(claim.snippet, verdicts)) {
              log(
                "info",
                "completion claim without evidence keywords but backed by a verdict record",
                { sessionID, turnIndex: claim.turnIndex, snippet: claim.snippet },
              )
            } else {
              log(
                "warn",
                "completion claim with no evidence in an assistant turn",
                { sessionID, turnIndex: claim.turnIndex, snippet: claim.snippet },
              )
            }
          } else {
            log("info", "completion claim carried evidence", {
              sessionID,
              turnIndex: claim.turnIndex,
            })
          }
        }
      } catch (err) {
        // The gate observes; a read failure must never break the session.
        log("warn", "proof gate could not read the session", { error: String(err) })
      }
    },
  }
}
