import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { UsageStats } from "../settings";

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
              <span className="flow-bar-track">
                <span
                  className="flow-bar-fill"
                  style={{ width: `${Math.round((d.words / maxDay) * 100)}%` }}
                />
              </span>
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
    </div>
  );
}
