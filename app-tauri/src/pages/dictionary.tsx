import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export default function DictionaryPage() {
  const [words, setWords] = useState<string[]>([]);
  const [filter, setFilter] = useState("");
  const [draft, setDraft] = useState("");
  const [note, setNote] = useState("");

  async function load() {
    try {
      setWords(await invoke<string[]>("list_dictionary"));
      setNote("");
    } catch (e) {
      setNote(`Couldn't read dictionary. ${e}`);
    }
  }

  useEffect(() => {
    load();
  }, []);

  async function add() {
    if (!draft.trim()) return;
    try {
      await invoke("save_word", { phrase: draft });
      setDraft("");
      await load();
    } catch (e) {
      setNote(`Couldn't add word. ${e}`);
    }
  }

  async function remove(phrase: string) {
    try {
      await invoke("remove_word", { phrase });
      await load();
    } catch (e) {
      setNote(`Couldn't remove word. ${e}`);
    }
  }

  const shown = words.filter((w) =>
    w.toLowerCase().includes(filter.trim().toLowerCase()),
  );

  return (
    <div className="flow-page">
      <div className="flow-head">
        <h1 className="flow-title">Dictionary</h1>
        <div className="row">
          <input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="Search words"
            aria-label="search dictionary"
            className="flow-search"
          />
        </div>
      </div>
      <div className="flow-hero flow-hero-dict">
        <div className="flow-hero-text">
          <h2>Susurro spells the way you do</h2>
          <p>Add personal terms, company jargon, and client names.</p>
          <div className="row">
            <input
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              placeholder="Add new word"
              aria-label="new dictionary word"
              className="flow-search"
            />
            <button className="flow-light" onClick={add}>
              Add word
            </button>
          </div>
        </div>
      </div>
      {note && <div className="flow-note">{note}</div>}
      <div className="flow-card">
        {shown.map((w) => (
          <div key={w} className="flow-row">
            <span className="flow-text">{w}</span>
            <span className="flow-actions">
              <button
                className="flow-mini"
                onClick={() => remove(w)}
                aria-label={`remove ${w}`}
              >
                Remove
              </button>
            </span>
          </div>
        ))}
        {shown.length === 0 && (
          <div className="flow-empty">
            {words.length ? "No words match." : "No words yet. Add the names whisper mangles."}
          </div>
        )}
      </div>
    </div>
  );
}
