import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

interface AssetCopy {
  kind: "Store" | "Bundled";
  path: string;
  bytes_ok: boolean;
}

interface AssetHealth {
  name: string;
  version: string;
  copies: AssetCopy[];
}

interface ConvergenceAsset {
  name: string;
  version: string;
  state: "ready" | "waiting" | "missing";
  next_retry_secs: number;
}

interface ConvergenceAttempt {
  category: string;
  remedy: string;
  attempts: number;
  next_retry_secs: number;
}

interface Convergence {
  assets: ConvergenceAsset[];
  converged: boolean;
  next_retry_secs: number;
  attempts: Record<string, ConvergenceAttempt>;
  models_dir: string | null;
}

interface SystemStatus {
  os: string;
  whisper: string | null;
  model_found: boolean;
  model_path: string;
  paste_ok: boolean;
  paste_detail: string;
  whisper_hint: string;
  ollama_up: boolean;
  ollama_model_present: boolean;
  ollama_hint: string;
  engine: { kind: string; version: string };
  models: AssetHealth[];
  resolved_path: string;
  tier: string | null;
  prefetch_running: boolean;
  convergence: Convergence;
}

/** Backoff waits read better as a duration than as a second count. */
function formatDelay(secs: number): string {
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.round(secs / 60)}m`;
  return `${Math.round(secs / 3600)}h`;
}

export default function SystemPage() {
  const [status, setStatus] = useState<SystemStatus | null>(null);
  const [note, setNote] = useState("");

  async function refresh() {
    try {
      setStatus(await invoke<SystemStatus>("system_status"));
      setNote("");
    } catch (e) {
      setNote(`Couldn't read system status. ${e}`);
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  if (note) return <div className="flow-page"><div className="flow-note">{note}</div></div>;
  if (!status) return <div className="flow-page"><div className="flow-empty">Reading system.</div></div>;
  const conv = status.convergence;
  const failures = Object.entries(conv.attempts);

  return (
    <div className="flow-page">
      <div className="flow-head">
        <h1 className="flow-title">System</h1>
        <div className="row">
          <Button variant="outline" size="sm" onClick={refresh} aria-label="refresh system status">
            recheck
          </Button>
        </div>
      </div>

      <div className="flow-grid2">
        <div className="flow-card flow-pad">
          <h3 className="flow-h3">Engine</h3>
          <div className="row">
            <Badge variant="local">{status.engine.kind}</Badge>
            <span className="flow-mono">whisper.cpp {status.engine.version}</span>
          </div>
          <div className="flow-sub">Linked in-process. No binary to install, no PATH to set.</div>
          <h3 className="flow-h3">Dictation model</h3>
          <div className="flow-mono">{status.resolved_path}</div>
          <div className="row">
            <Badge variant={status.model_found ? "local" : "degraded"}>
              {status.model_found ? "ready" : "missing"}
            </Badge>
            {status.tier && <Badge variant="idle">tier {status.tier}</Badge>}
            <Badge variant={status.prefetch_running ? "cloud" : "idle"}>
              {status.prefetch_running ? "base fetching" : "fetch idle"}
            </Badge>
            <Badge variant={conv.converged ? "local" : "degraded"}>
              {conv.converged
                ? "converged"
                : conv.next_retry_secs > 0
                  ? `retry in ${formatDelay(conv.next_retry_secs)}`
                  : "needs a fix"}
            </Badge>
          </div>
        </div>

        <div className="flow-card flow-pad">
          <h3 className="flow-h3">Environment</h3>
          <div className="flow-sub">OS: <span className="flow-mono">{status.os}</span></div>
          <div className="row">
            <Badge variant={status.paste_ok ? "local" : "degraded"}>
              {status.paste_ok ? "paste ready" : "paste limited"}
            </Badge>
            <span className="flow-sub">{status.paste_detail}</span>
          </div>
          <div className="row">
            <Badge variant={status.ollama_up && status.ollama_model_present ? "local" : "idle"}>
              {status.ollama_up && status.ollama_model_present ? "cleanup model ready" : "cleanup fallback"}
            </Badge>
            {!status.ollama_up || !status.ollama_model_present ? (
              <span className="flow-sub">{status.ollama_hint}</span>
            ) : null}
          </div>
          {status.whisper_hint && <div className="flow-sub">{status.whisper_hint}</div>}
        </div>
      </div>

      <div className="flow-card flow-pad flow-sec">
        <h3 className="flow-h3">Model assets</h3>
        <div className="flow-sub">Every manifest asset, every copy on disk. Size is the cheap integrity signal; content verifies on fetch and model-check.</div>
      </div>
      <div className="flow-card flow-pad flow-sec">
        <h3 className="flow-h3">Convergence</h3>
        <div className="flow-sub">
          The loop that closes asset gaps on boot. It retries a missing asset on a backoff
          timer, and stops when a failure needs you instead of the clock.
        </div>
        {conv.assets.map((a) => (
          <div key={a.name} className="flow-row">
            <div className="flow-text">
              <span className="flow-mono">{a.name}</span>{" "}
              <span className="flow-sub">v{a.version}</span>
              <div className="row">
                <Badge
                  variant={
                    a.state === "ready" ? "local" : a.state === "waiting" ? "cloud" : "degraded"
                  }
                >
                  {a.state}
                </Badge>
                {a.next_retry_secs > 0 && (
                  <span className="flow-sub">next in {formatDelay(a.next_retry_secs)}</span>
                )}
              </div>
            </div>
          </div>
        ))}
        {failures.map(([name, f]) => (
          <div key={name} className="flow-row">
            <div className="flow-text">
              <span className="flow-mono">{name}</span>{" "}
              <span className="flow-sub">{f.category}</span>
              <div className="row">
                <Badge variant="outline">attempt {f.attempts}</Badge>
                <span className="flow-sub">next in {formatDelay(f.next_retry_secs)}</span>
              </div>
              <div className="flow-sub">{f.remedy}</div>
            </div>
          </div>
        ))}
        {failures.length === 0 && conv.converged && (
          <div className="flow-sub">Nothing pending. Every asset is on disk and verified.</div>
        )}
      </div>
      <div className="flow-card flow-sec">
        {status.models.map((m) => (
          <div key={m.name} className="flow-row">
            <div className="flow-text">
              <span className="flow-mono">{m.name}</span>{" "}
              <span className="flow-sub">v{m.version}</span>
              {m.copies.map((c) => (
                <div key={c.path} className="row">
                  <Badge variant={c.kind === "Bundled" ? "idle" : "outline"}>
                    {c.kind === "Bundled" ? "bundled" : "store"}
                  </Badge>
                  <span className="flow-mono">{c.path}</span>
                  <Badge variant={c.bytes_ok ? "local" : "degraded"}>
                    {c.bytes_ok ? "size ok" : "size mismatch"}
                  </Badge>
                  {c.path === status.resolved_path && (
                    <Badge variant="local">active</Badge>
                  )}
                </div>
              ))}
              {m.copies.length === 0 && (
                <div className="flow-sub">no copy on this machine.</div>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
