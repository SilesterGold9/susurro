import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { check } from "@tauri-apps/plugin-updater";

export interface Settings {
  seconds: number;
  auto_stop: boolean;
  sound: boolean;
  cleanup: string;
  ollama_model: string;
  whisper_model: string;
  device: string;
  socket_path: string;
  update_channel: string;
  hotkey: string;
  onboarding_done: boolean;
  high_contrast: boolean;
  announce: boolean;
}

const DEFAULTS: Settings = {
  seconds: 30,
  auto_stop: true,
  sound: true,
  cleanup: "ollama",
  ollama_model: "qwen3:0.6b",
  whisper_model: "",
  device: "",
  socket_path: "/tmp/susurro.sock",
  update_channel: "stable",
  hotkey: "super_shift_r",
  onboarding_done: false,
  high_contrast: false,
  announce: true,
};

export interface HistoryRow {
  session: string;
  raw_text: string;
  cleaned_text: string | null;
  provider: string;
  latency_ms: number;
  created_at: number;
}

export interface FormatProfileRow {
  app: string;
  style: string;
  cleanup: string;
}

export interface UsageStats {
  entries: number;
  words: number;
  polished: number;
  dict_hits: number;
  dict_phrases: number;
  streak_days: number;
  top_apps: { app: string; sessions: number }[];
  p50_ms: number;
  p95_ms: number;
  p99_ms: number;
  days: { label: string; words: number }[];
}

export default function SettingsView() {
  const [s, setS] = useState<Settings>(DEFAULTS);
  const [saved, setSaved] = useState(false);
  const [updateNote, setUpdateNote] = useState("no updates checked yet.");
  const [doctor, setDoctor] = useState("");
  const [history, setHistory] = useState<HistoryRow[]>([]);
  const [showRaw, setShowRaw] = useState<Record<string, boolean>>({});
  const [historyNote, setHistoryNote] = useState("");
  const [profiles, setProfiles] = useState<FormatProfileRow[]>([]);
  const [profileApp, setProfileApp] = useState("");
  const [profileStyle, setProfileStyle] = useState("formal");
  const [profileNote, setProfileNote] = useState("");
  const [stats, setStats] = useState<UsageStats | null>(null);
  const [statsNote, setStatsNote] = useState("");

  useEffect(() => {
    invoke<Settings>("get_settings").then(setS).catch(() => {});
  }, []);

  async function save() {
    await invoke("save_settings", { settings: s });
    setSaved(true);
    setTimeout(() => setSaved(false), 1500);
  }

  async function checkUpdates() {
    setUpdateNote("checking...");
    try {
      const u = await check();
      setUpdateNote(
        u ? `update available: ${u.version}. Restart to install.` : "up to date.",
      );
    } catch (e) {
      setUpdateNote(`Couldn't reach the update server. Using local instead. ${e}`);
    }
  }

  async function runDoctor() {
    setDoctor(await invoke<string>("run_doctor"));
  }

  async function loadHistory() {
    try {
      const rows = await invoke<HistoryRow[]>("list_history", { limit: 20 });
      setHistory(rows);
      setHistoryNote(rows.length ? "" : "no history yet. Dictate something first.");
    } catch (e) {
      setHistoryNote(`Couldn't read history. ${e}`);
    }
  }

  async function restoreRaw(session: string) {
    try {
      const msg = await invoke<string>("restore_session", { session });
      setHistoryNote(msg);
    } catch (e) {
      setHistoryNote(`Couldn't restore. ${e}`);
    }
  }

  function toggleRaw(session: string) {
    setShowRaw({ ...showRaw, [session]: !showRaw[session] });
  }

  async function loadProfiles() {
    try {
      setProfiles(await invoke<FormatProfileRow[]>("list_format_profiles"));
      setProfileNote("");
    } catch (e) {
      setProfileNote(`Couldn't read profiles. ${e}`);
    }
  }

  async function saveProfile() {
    try {
      await invoke("save_format_profile", { app: profileApp, style: profileStyle });
      setProfileApp("");
      setProfileNote(`profile saved: ${profileStyle}.`);
      await loadProfiles();
    } catch (e) {
      setProfileNote(`Couldn't save profile. ${e}`);
    }
  }

  async function removeProfile(app: string) {
    try {
      await invoke("remove_format_profile", { app });
      await loadProfiles();
    } catch (e) {
      setProfileNote(`Couldn't remove profile. ${e}`);
    }
  }

  async function loadStats() {
    try {
      setStats(await invoke<UsageStats>("get_stats"));
      setStatsNote("");
    } catch (e) {
      setStatsNote(`Couldn't read stats. ${e}`);
    }
  }

  function set<K extends keyof Settings>(k: K, v: Settings[K]) {
    setS({ ...s, [k]: v });
  }

  const live = s.announce ? "polite" : "off";

  return (
    <div className={s.high_contrast ? "high-contrast" : ""}>
      <div className="flow-sub">Talk-to-text that works even when the internet doesn't.</div>

      <div className="flow-card flow-pad flow-sec">
        <h3 className="flow-h3">Recording</h3>
        <label className="flow-label">Recording window (seconds)</label>
        <input
          className="flow-input mono"
          type="number"
          value={s.seconds}
          onChange={(e) => set("seconds", Number(e.target.value))}
        />
        <label className="flow-check">
          <input
            type="checkbox"
            checked={s.auto_stop}
            onChange={(e) => set("auto_stop", e.target.checked)}
          />
          <span>Stop on end-of-speech (VAD)</span>
        </label>
        <label className="flow-check">
          <input
            type="checkbox"
            checked={s.sound}
            onChange={(e) => set("sound", e.target.checked)}
          />
          <span>Sound cues (start, stop, done, error)</span>
        </label>
        <label className="flow-label">PipeWire target (empty means default source)</label>
        <input
          className="flow-input mono"
          value={s.device}
          onChange={(e) => set("device", e.target.value)}
        />
      </div>

      <div className="flow-card flow-pad flow-sec">
        <h3 className="flow-h3">Cleanup and models</h3>
        <label className="flow-label">Cleanup</label>
        <select
          className="flow-input"
          value={s.cleanup}
          onChange={(e) => set("cleanup", e.target.value)}
        >
          <option value="none">none (raw transcript)</option>
          <option value="regex">regex fallback</option>
          <option value="ollama">ollama (local LLM)</option>
        </select>
        <label className="flow-label">Ollama model</label>
        <input
          className="flow-input mono"
          value={s.ollama_model}
          onChange={(e) => set("ollama_model", e.target.value)}
        />
        <label className="flow-label">Whisper model path (empty means $SUSURRO_MODEL)</label>
        <input
          className="flow-input mono"
          value={s.whisper_model}
          onChange={(e) => set("whisper_model", e.target.value)}
        />
        <label className="flow-label">Release channel</label>
        <select
          className="flow-input"
          value={s.update_channel}
          onChange={(e) => set("update_channel", e.target.value)}
        >
          <option value="stable">stable</option>
          <option value="beta">beta</option>
        </select>
      </div>

      <div className="flow-card flow-pad flow-sec">
        <h3 className="flow-h3">Tone follows the app</h3>
        <div className="flow-sub">Formal in docs, casual in messages. Also on the Style page.</div>
        <div className="row">
          <button className="flow-mini" onClick={loadProfiles} aria-label="load format profiles">
            load profiles
          </button>
        </div>
        <div className="row">
          <input
            className="flow-search"
            value={profileApp}
            onChange={(e) => setProfileApp(e.target.value)}
            placeholder="app pattern, e.g. docs"
            aria-label="app pattern"
          />
          <select
            className="flow-search"
            value={profileStyle}
            onChange={(e) => setProfileStyle(e.target.value)}
            aria-label="profile style"
          >
            <option value="formal">formal (polish)</option>
            <option value="casual">casual (tidy only)</option>
            <option value="verbatim">verbatim (raw)</option>
          </select>
          <button className="flow-mini" onClick={saveProfile} aria-label="save format profile">
            save profile
          </button>
        </div>
      </div>
      {profileNote && <div className="flow-note" aria-live={live}>{profileNote}</div>}
      {profiles.length > 0 && (
        <div className="flow-card flow-sec">
          {profiles.map((p) => (
            <div key={p.app} className="flow-row">
              <div className="flow-text flow-mono">
                {p.app}: {p.style} (cleanup {p.cleanup})
              </div>
              <div className="flow-actions">
                <button
                  className="flow-mini"
                  onClick={() => removeProfile(p.app)}
                  aria-label={`remove profile ${p.app}`}
                >
                  remove
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      <div className="row flow-sec">
        <button className="flow-dark" onClick={save}>
          {saved ? "saved" : "save"}
        </button>
        <button className="flow-mini" onClick={checkUpdates}>
          check for updates
        </button>
        <button className="flow-mini" onClick={runDoctor}>
          run doctor
        </button>
      </div>
      <div className="flow-note" aria-live={live}>{updateNote}</div>
      {doctor && (
        <div className="flow-card flow-pad flow-sec">
          <pre className="flow-mono">
            {doctor}
          </pre>
        </div>
      )}

      <div className="flow-card flow-pad flow-sec">
        <h3 className="flow-h3">Reading the screen</h3>
        <div className="flow-sub">The hotkey remaps live. Contrast and announcements apply on save.</div>
        <label className="flow-label">Dictation hotkey</label>
        <select
          className="flow-input"
          value={s.hotkey}
          onChange={(e) => set("hotkey", e.target.value)}
          aria-label="dictation hotkey"
        >
          <option value="super_shift_r">Super + Shift + R</option>
          <option value="ctrl_shift_r">Ctrl + Shift + R</option>
          <option value="shift_d">Shift + D</option>
        </select>
        <label className="flow-check">
          <input
            type="checkbox"
            checked={s.high_contrast}
            onChange={(e) => set("high_contrast", e.target.checked)}
            aria-label="high contrast"
          />
          <span>High contrast</span>
        </label>
        <label className="flow-check">
          <input
            type="checkbox"
            checked={s.announce}
            onChange={(e) => set("announce", e.target.checked)}
            aria-label="screen reader announcements"
          />
          <span>Screen reader announcements</span>
        </label>
      </div>

      <div className="flow-card flow-pad flow-sec">
        <h3 className="flow-h3">History</h3>
        <div className="flow-sub">Raw beside cleaned, with restore.</div>
        <div className="row">
          <button className="flow-mini" onClick={loadHistory} aria-label="load history">
            load history
          </button>
        </div>
      </div>
      {historyNote && <div className="flow-note" aria-live={live}>{historyNote}</div>}
      {history.length > 0 && (
        <div className="flow-card flow-sec">
          {history.map((h) => {
            const raw = showRaw[h.session];
            const body = raw ? h.raw_text : h.cleaned_text || h.raw_text;
            return (
              <div key={h.session} className="flow-row">
                <div className="flow-text flow-mono">
                  {body}
                </div>
                <div className="flow-sub">
                  {h.provider} | {h.latency_ms}ms | {h.session.slice(0, 8)}
                  {h.cleaned_text && h.cleaned_text !== h.raw_text
                    ? raw
                      ? " | showing raw"
                      : " | showing polished"
                    : ""}
                </div>
                <div className="flow-actions">
                  {h.cleaned_text && h.cleaned_text !== h.raw_text && (
                    <button
                      className="flow-mini"
                      onClick={() => toggleRaw(h.session)}
                      aria-label={raw ? "show polished text" : "show raw transcript"}
                    >
                      {raw ? "show polished" : "show raw"}
                    </button>
                  )}
                  <button
                    className="flow-mini"
                    onClick={() => restoreRaw(h.session)}
                    aria-label={`restore raw transcript ${h.session.slice(0, 8)}`}
                  >
                    restore raw
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      <div className="flow-card flow-pad flow-sec">
        <h3 className="flow-h3">Usage</h3>
        <div className="flow-sub">Words, streak, latency. Also on the Insights page.</div>
        <div className="row">
          <button className="flow-mini" onClick={loadStats} aria-label="load usage stats">
            load stats
          </button>
        </div>
      </div>
      {statsNote && <div className="flow-note" aria-live={live}>{statsNote}</div>}
      {stats && (
        <div className="flow-card flow-pad flow-sec">
          <div className="flow-mono">
            {stats.entries} sessions | {stats.words} words | {stats.polished} polished |{" "}
            {stats.dict_hits} dict hits ({stats.dict_phrases} phrases) | {stats.streak_days}d streak
          </div>
          <div className="flow-sub">
            latency p50 {stats.p50_ms}ms, p95 {stats.p95_ms}ms, p99 {stats.p99_ms}ms end to end.
            {stats.top_apps.length
              ? ` Top apps: ${stats.top_apps.map((t) => `${t.app} ${t.sessions}`).join(", ")}.`
              : " Top apps: unknown yet."}
          </div>
          {stats.days.map((d) => (
            <div key={d.label} className="flow-mono">
              {d.label}: {d.words} words
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
