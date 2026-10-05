import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

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
          <Input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="Search words"
            aria-label="search dictionary"
          />
        </div>
      </div>
      <div className="flow-hero flow-hero-dict">
        <div className="flow-hero-text">
          <h2>Susurro spells the way you do</h2>
          <p>Add personal terms, company jargon, and client names.</p>
          <div className="row">
            <Input
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              placeholder="Add new word"
              aria-label="new dictionary word"
            />
            <Button variant="paper" onClick={add}>
              Add word
            </Button>
          </div>
        </div>
      </div>
      {note && <div className="flow-note">{note}</div>}
      <div className="flow-card">
        {shown.map((w) => (
          <div key={w} className="flow-row">
            <span className="flow-text">{w}</span>
            <span className="flow-actions">
              <Button
                variant="outline"
                size="xs"
                onClick={() => remove(w)}
                aria-label={`remove ${w}`}
              >
                Remove
              </Button>
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
