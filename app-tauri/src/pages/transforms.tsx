import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

interface HistoryRow {
  session: string;
  raw_text: string;
  cleaned_text: string | null;
  provider: string;
  latency_ms: number;
  created_at: number;
}

interface TransformSpec {
  name: string;
  label: string;
  blurb: string;
  rewrites: boolean;
  tier: string;
}

/** Menu order matches the CLI and core. Kept here rather than fetched
 *  so the buttons render before the first history read lands. */
const TRANSFORMS: TransformSpec[] = [
  {
    name: "tidy",
    label: "Tidy",
    blurb: "Punctuation and capitalisation. On device, nothing discarded.",
    rewrites: false,
    tier: "onnx",
  },
  {
    name: "organize",
    label: "Organize",
    blurb: "Turn rambling notes into ordered points. Rewrites the text.",
    rewrites: true,
    tier: "ollama",
  },
  {
    name: "shorten",
    label: "Shorten",
    blurb: "Cut length, keep every point. Rewrites the text.",
    rewrites: true,
    tier: "ollama",
  },
  {
    name: "formalize",
    label: "Formalize",
    blurb: "Raise the register. Rewrites the text.",
    rewrites: true,
    tier: "ollama",
  },
];

interface Preview {
  session: string;
  name: string;
  text: string;
  outcome: string;
}

function whenLabel(unix: number): string {
  if (!unix) return "undated";
  return new Date(unix * 1000).toLocaleString();
}

export default function TransformsPage() {
  const [rows, setRows] = useState<HistoryRow[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [preview, setPreview] = useState<Preview | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [note, setNote] = useState("");

  async function refresh() {
    try {
      const list = await invoke<HistoryRow[]>("transform_candidates", { limit: 20 });
      setRows(list);
      if (!selected && list.length > 0) setSelected(list[0].session);
      setNote("");
    } catch (e) {
      setNote(`Couldn't read history. ${e}`);
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function run(name: string, apply: boolean) {
    if (!selected) return;
    setBusy(name);
    setNote("");
    try {
      const cmd = apply ? "apply_transform" : "preview_transform";
      const result = await invoke<Preview>(cmd, { session: selected, name });
      setPreview(result);
      if (apply) await refresh();
    } catch (e) {
      setPreview(null);
      setNote(String(e));
    } finally {
      setBusy(null);
    }
  }

  if (!rows) {
    return (
      <div className="flow-page">
        <h1 className="flow-title">Transforms</h1>
        <div className="flow-note">{note || "Loading your dictations."}</div>
      </div>
    );
  }

  const entry = rows.find((r) => r.session === selected) ?? null;

  return (
    <div className="flow-page">
      <h1 className="flow-title">Transforms</h1>
      <div className="flow-sub">
        Say it once, shape it after. Every transform previews before it replaces anything, and the
        raw transcript stays one restore away.
      </div>

      {rows.length === 0 && (
        <div className="flow-empty">
          Nothing to transform yet. Dictate something, then come back.
        </div>
      )}

      {rows.length > 0 && (
        <>
          <div className="flow-card flow-pad flow-sec">
            <h3 className="flow-h3">Recent dictations</h3>
            {rows.map((r) => (
              <button
                key={r.session}
                type="button"
                className="flow-pick"
                aria-pressed={r.session === selected}
                onClick={() => {
                  setSelected(r.session);
                  setPreview(null);
                  setNote("");
                }}
              >
                <span className="flow-pick-when">{whenLabel(r.created_at)}</span>
                <span className="flow-pick-text">{r.raw_text}</span>
                {r.cleaned_text && r.cleaned_text !== r.raw_text && (
                  <Badge variant="outline">transformed</Badge>
                )}
              </button>
            ))}
          </div>

          {entry && (
            <div className="flow-card flow-pad flow-sec">
              <h3 className="flow-h3">Pick a shape</h3>
              {TRANSFORMS.map((t) => (
                <div key={t.name} className="flow-row">
                  <div className="flow-text">
                    <span>{t.label}</span>{" "}
                    <Badge variant={t.rewrites ? "degraded" : "local"}>
                      {t.rewrites ? "rewrites text" : "on device"}
                    </Badge>
                    <div className="flow-sub">
                      {t.blurb} Runs on the {t.tier} tier.
                    </div>
                  </div>
                  <div className="row">
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={busy !== null}
                      onClick={() => run(t.name, false)}
                    >
                      {busy === t.name ? "working" : "preview"}
                    </Button>
                    <Button
                      size="sm"
                      disabled={busy !== null || preview?.name !== t.name}
                      onClick={() => run(t.name, true)}
                    >
                      use this
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          )}

          {preview && (
            <div className="flow-card flow-pad flow-sec">
              <h3 className="flow-h3">Preview</h3>
              <div className="row">
                <Badge
                  variant={
                    preview.outcome === "tidied"
                      ? "local"
                      : preview.outcome === "rewritten"
                        ? "cloud"
                        : "idle"
                  }
                >
                  {preview.outcome === "tidied"
                    ? "tidied on device"
                    : preview.outcome === "rewritten"
                      ? "rewritten by a model"
                      : "unchanged"}
                </Badge>
              </div>
              <div className="flow-quote">{preview.text}</div>
              {preview.outcome === "rewritten" && (
                <div className="flow-sub">
                  A model rewrote this. The words are not the ones you said, so check it before you
                  keep it.
                </div>
              )}
              {entry && (
                <div className="flow-sub">
                  Original stays recoverable from History for this session.
                </div>
              )}
            </div>
          )}

          {note && <div className="flow-note">{note}</div>}
        </>
      )}
    </div>
  );
}
