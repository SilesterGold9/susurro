import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Page } from "../shell";
import type { HistoryRow, UsageStats } from "../settings";

interface Group {
  label: string;
  rows: (HistoryRow & { time: string })[];
}

function dayLabel(d: Date, now: Date): string {
  const day = (x: Date) => `${x.getFullYear()}-${x.getMonth()}-${x.getDate()}`;
  if (day(d) === day(now)) return "Today";
  const y = new Date(now);
  y.setDate(y.getDate() - 1);
  if (day(d) === day(y)) return "Yesterday";
  return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

function timeLabel(d: Date): string {
  return d
    .toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })
    .toLowerCase()
    .replace(/\s/g, " ");
}

export default function HomePage({ go }: { go: (p: Page) => void }) {
  const [groups, setGroups] = useState<Group[]>([]);
  const [stats, setStats] = useState<UsageStats | null>(null);
  const [note, setNote] = useState("");

  useEffect(() => {
    invoke<HistoryRow[]>("list_history", { limit: 100 })
      .then((rows) => {
        const now = new Date();
        const map = new Map<string, Group>();
        for (const r of rows) {
          const d = new Date(r.created_at > 0 ? r.created_at * 1000 : Date.now());
          const label = dayLabel(d, now);
          const g = map.get(label) ?? { label, rows: [] };
          g.rows.push({ ...r, time: timeLabel(d) });
          map.set(label, g);
        }
        setGroups([...map.values()]);
      })
      .catch((e) => setNote(`Couldn't read history. ${e}`));
    invoke<UsageStats>("get_stats")
      .then(setStats)
      .catch(() => {});
  }, []);

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
    } catch {}
  }

  async function restore(session: string) {
    try {
      const msg = await invoke<string>("restore_session", { session });
      setNote(msg);
    } catch (e) {
      setNote(`Couldn't restore. ${e}`);
    }
  }

  return (
    <div className="flow-cols">
      <div className="flow-page">
        <h1 className="flow-greet">Welcome back, speak and it appears.</h1>
        <div className="flow-hero">
          <div className="flow-hero-text">
            <h2>Make susurro sound like you</h2>
            <p>Set up different writing styles for different apps.</p>
            <button className="flow-light" onClick={() => go("style")}>
              Start now
            </button>
          </div>
        </div>
        {note && <div className="flow-note">{note}</div>}
        {groups.map((g) => (
          <section key={g.label}>
            <h3 className="flow-day">{g.label}</h3>
            <div className="flow-card">
              {g.rows.map((r) => (
                <div key={r.session} className="flow-row">
                  <span className="flow-time">{r.time}</span>
                  <span className="flow-text">
                    {r.cleaned_text || r.raw_text}
                  </span>
                  <span className="flow-actions">
                    <button
                      className="flow-mini"
                      onClick={() => copy(r.cleaned_text || r.raw_text)}
                      aria-label="copy transcript"
                    >
                      Copy
                    </button>
                    {r.cleaned_text && r.cleaned_text !== r.raw_text && (
                      <button
                        className="flow-mini"
                        onClick={() => restore(r.session)}
                        aria-label="restore raw transcript"
                      >
                        Raw
                      </button>
                    )}
                  </span>
                </div>
              ))}
            </div>
          </section>
        ))}
        {groups.length === 0 && (
          <div className="flow-empty">
            Nothing dictated yet. Press your hotkey and speak.
          </div>
        )}
      </div>
      <aside className="flow-rail">
        <div className="flow-stat">
          <strong>{stats?.words ?? "–"}</strong> total words
        </div>
        <div className="flow-stat">
          <strong>{stats?.streak_days ?? "–"}</strong> day streak
        </div>
        <div className="flow-stat">
          <strong>{stats?.entries ?? "–"}</strong> sessions
        </div>
      </aside>
    </div>
  );
}
