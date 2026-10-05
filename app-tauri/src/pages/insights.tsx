import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { UsageStats } from "../settings";
import { Progress } from "@/components/ui/progress";

export default function InsightsPage() {
  const [stats, setStats] = useState<UsageStats | null>(null);
  const [note, setNote] = useState("");

  useEffect(() => {
    invoke<UsageStats>("get_stats")
      .then(setStats)
      .catch((e) => setNote(`Couldn't read stats. ${e}`));
  }, []);

  if (note) return <div className="flow-page"><div className="flow-note">{note}</div></div>;
  if (!stats) return <div className="flow-page"><div className="flow-empty">Loading usage.</div></div>;

  const maxDay = Math.max(1, ...stats.days.map((d) => d.words));
  const fp = stats.fingerprint;
  const maxWord = Math.max(1, ...fp.top_words.map((w) => w.count));
  return (
    <div className="flow-page">
      <h1 className="flow-title">Insights</h1>
      <div className="flow-grid3">
        <div className="flow-card flow-pad">
          <div className="flow-bignum">{stats.words.toLocaleString()}</div>
          <div className="flow-cap">Total words dictated</div>
        </div>
        <div className="flow-card flow-pad">
          <div className="flow-bignum">{stats.polished.toLocaleString()}</div>
          <div className="flow-cap">Fixed by susurro</div>
          <div className="flow-sub">
            {stats.dict_hits} dictionary hits across {stats.dict_phrases} phrases
          </div>
        </div>
        <div className="flow-card flow-pad">
          <div className="flow-bignum">{stats.streak_days}</div>
          <div className="flow-cap">Day streak</div>
          <div className="flow-sub">
            p50 {stats.p50_ms}ms, p95 {stats.p95_ms}ms, p99 {stats.p99_ms}ms
          </div>
        </div>
      </div>
      <div className="flow-grid2">
        <div className="flow-card flow-pad">
          <h3 className="flow-h3">Recent days</h3>
          {stats.days.map((d) => (
            <div key={d.label} className="flow-bar-row">
              <span className="flow-bar-label">{d.label}</span>
              <Progress
                value={Math.round((d.words / maxDay) * 100)}
                aria-label={`${d.label}: ${d.words} words`}
                className="flex-1"
              />
              <span className="flow-bar-num">{d.words}</span>
            </div>
          ))}
          {stats.days.length === 0 && (
            <div className="flow-sub">No dated entries yet.</div>
          )}
        </div>
        <div className="flow-card flow-pad">
          <h3 className="flow-h3">Top apps</h3>
          {stats.top_apps.map((t) => (
            <div key={t.app} className="flow-bar-row">
              <span className="flow-bar-label">{t.app}</span>
              <span className="flow-bar-num">{t.sessions} sessions</span>
            </div>
          ))}
          {stats.top_apps.length === 0 && (
            <div className="flow-sub">App tracking starts with this version.</div>
          )}
        </div>
      </div>

      <h3 className="flow-h3 flow-sec">Voice fingerprint</h3>
      <div className="flow-sub flow-sec">
        Counting over your history, on this machine. No model, and nothing leaves the box.
      </div>
      <div className="flow-grid3 flow-sec">
        <div className="flow-card flow-pad">
          <h3 className="flow-h3">Words you lean on</h3>
          {fp.top_words.map((w) => (
            <div key={w.word} className="flow-bar-row">
              <span className="flow-bar-label">{w.word}</span>
              <Progress
                value={Math.round((w.count / maxWord) * 100)}
                aria-label={`${w.word}: ${w.count} times`}
                className="flex-1"
              />
              <span className="flow-bar-num">{w.count}</span>
            </div>
          ))}
          {fp.top_words.length === 0 && (
            <div className="flow-sub">Not enough history yet. Dictate a few times.</div>
          )}
        </div>
        <div className="flow-card flow-pad">
          <h3 className="flow-h3">Catchphrase</h3>
          {fp.catchphrase ? (
            <>
              <div className="flow-quote">“{fp.catchphrase.phrase}”</div>
              <div className="flow-sub">You have said this {fp.catchphrase.count} times.</div>
            </>
          ) : (
            <div className="flow-sub">Nothing repeated yet. This appears once a phrase repeats.</div>
          )}
        </div>
        <div className="flow-card flow-pad">
          <h3 className="flow-h3">Peak hour</h3>
          {fp.peak_hour ? (
            <>
              <div className="flow-bignum">
                {String(fp.peak_hour.hour).padStart(2, "0")}:00
              </div>
              <div className="flow-cap">Most active hour</div>
              <div className="flow-sub">
                {fp.peak_hour.sessions} sessions at this hour. Times are UTC.
              </div>
            </>
          ) : (
            <div className="flow-sub">Not enough dated sessions yet.</div>
          )}
        </div>
      </div>
    </div>
  );
}
