import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { FormatProfileRow } from "../settings";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";

const STYLES = [
  { name: "formal", title: "Formal.", desc: "Caps plus punctuation, full polish." },
  { name: "casual", title: "Casual", desc: "Caps plus light tidy, never rewritten." },
  { name: "verbatim", title: "Verbatim", desc: "Raw transcript, untouched." },
];

export default function StylePage() {
  const [rows, setRows] = useState<FormatProfileRow[]>([]);
  const [app, setApp] = useState("");
  const [style, setStyle] = useState("formal");
  const [note, setNote] = useState("");

  async function load() {
    try {
      setRows(await invoke<FormatProfileRow[]>("list_format_profiles"));
      setNote("");
    } catch (e) {
      setNote(`Couldn't read profiles. ${e}`);
    }
  }

  useEffect(() => {
    load();
  }, []);

  async function save() {
    if (!app.trim()) return;
    try {
      await invoke("save_format_profile", { app, style });
      setApp("");
      await load();
    } catch (e) {
      setNote(`Couldn't save profile. ${e}`);
    }
  }

  async function remove(a: string) {
    try {
      await invoke("remove_format_profile", { app: a });
      await load();
    } catch (e) {
      setNote(`Couldn't remove profile. ${e}`);
    }
  }

  return (
    <div className="flow-page">
      <h1 className="flow-title">How do you write your emails?</h1>
      <p className="flow-sub">
        Pick a tone, name the apps it applies to. Formal in docs, casual in
        messages, verbatim where the transcript must stay untouched.
      </p>
      <div className="flow-grid3">
        {STYLES.map((s) => (
          <Card key={s.name} asChild selected={style === s.name}>
            <button
              onClick={() => setStyle(s.name)}
              aria-pressed={style === s.name}
              className="w-full cursor-pointer text-left"
            >
              <div className="flow-serif">{s.title}</div>
              <div className="flow-sub">{s.desc}</div>
            </button>
          </Card>
        ))}
      </div>
      <div className="row">
        <Input
          value={app}
          onChange={(e) => setApp(e.target.value)}
          placeholder="app pattern, e.g. docs"
          aria-label="app pattern"
        />
        <Button variant="ink" onClick={save}>
          Save profile
        </Button>
      </div>
      {note && <div className="flow-note">{note}</div>}
      <div className="flow-card">
        {rows.map((r) => (
          <div key={r.app} className="flow-row">
            <span className="flow-text">
              {r.app} <span className="flow-arrow">→</span> {r.style}
            </span>
            <span className="flow-actions">
              <Button
                variant="outline"
                size="xs"
                onClick={() => remove(r.app)}
                aria-label={`remove ${r.app}`}
              >
                Remove
              </Button>
            </span>
          </div>
        ))}
        {rows.length === 0 && (
          <div className="flow-empty">
            No profiles yet. Everything uses the cleanup setting.
          </div>
        )}
      </div>
    </div>
  );
}
