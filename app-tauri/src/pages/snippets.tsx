import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Snippet {
  trigger: string;
  expansion: string;
}

export default function SnippetsPage() {
  const [rows, setRows] = useState<Snippet[]>([]);
  const [filter, setFilter] = useState("");
  const [trigger, setTrigger] = useState("");
  const [expansion, setExpansion] = useState("");
  const [note, setNote] = useState("");

  async function load() {
    try {
      setRows(await invoke<Snippet[]>("list_snippets"));
      setNote("");
    } catch (e) {
      setNote(`Couldn't read snippets. ${e}`);
    }
  }

  useEffect(() => {
    load();
  }, []);

  async function add() {
    if (!trigger.trim() || !expansion.trim()) return;
    try {
      await invoke("save_snippet", { trigger, expansion });
      setTrigger("");
      setExpansion("");
      await load();
    } catch (e) {
      setNote(`Couldn't add snippet. ${e}`);
    }
  }

  async function remove(t: string) {
    try {
      await invoke("remove_snippet", { trigger: t });
      await load();
    } catch (e) {
      setNote(`Couldn't remove snippet. ${e}`);
    }
  }

  const shown = rows.filter(
    (r) =>
      r.trigger.toLowerCase().includes(filter.trim().toLowerCase()) ||
      r.expansion.toLowerCase().includes(filter.trim().toLowerCase()),
  );

  return (
    <div className="flow-page">
      <div className="flow-head">
        <h1 className="flow-title">Snippets</h1>
        <div className="row">
          <input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="Search snippets"
            aria-label="search snippets"
            className="flow-search"
          />
        </div>
      </div>
      <div className="flow-hero">
        <div className="flow-hero-text">
          <h2>The stuff you should not have to re-type</h2>
          <p>
            Save shortcuts to the things you type all the time. Say the
            trigger, get the expansion.
          </p>
          <div className="row">
            <input
              value={trigger}
              onChange={(e) => setTrigger(e.target.value)}
              placeholder="say this"
              aria-label="snippet trigger"
              className="flow-search"
            />
            <input
              value={expansion}
              onChange={(e) => setExpansion(e.target.value)}
              placeholder="get this"
              aria-label="snippet expansion"
              className="flow-search"
            />
            <button className="flow-light" onClick={add}>
              Add snippet
            </button>
          </div>
        </div>
      </div>
      {note && <div className="flow-note">{note}</div>}
      <div className="flow-card">
        {shown.map((r) => (
          <div key={r.trigger} className="flow-row">
            <span className="flow-text">
              {r.trigger} <span className="flow-arrow">→</span> {r.expansion}
            </span>
            <span className="flow-actions">
              <button
                className="flow-mini"
                onClick={() => remove(r.trigger)}
                aria-label={`remove ${r.trigger}`}
              >
                Remove
              </button>
            </span>
          </div>
        ))}
        {shown.length === 0 && (
          <div className="flow-empty">
            {rows.length
              ? "No snippets match."
              : "No snippets yet. Emails, links, addresses live here."}
          </div>
        )}
      </div>
      <div className="flow-sub">
        Note: automatic expansion inside dictation is not wired yet. The
        list above is the vocabulary it will read from.
      </div>
    </div>
  );
}
