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
}

export interface FormatProfileRow {
  app: string;
  style: string;
  cleanup: string;
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

  function set<K extends keyof Settings>(k: K, v: Settings[K]) {
    setS({ ...s, [k]: v });
  }

  const live = s.announce ? "polite" : "off";

  return (
    <div className={`settings${s.high_contrast ? " high-contrast" : ""}`}>
      {/* Split keeps the coral comma styled while screen readers
          announce susurro with its comma exactly once. */}
      <h1 className="brand-wordmark" aria-label="susurro,">
        susurro<span className="comma" aria-hidden="true">,</span>
      </h1>
      <div className="sub">Talk-to-text that works even when the internet doesn't.</div>

      <div className="field">
        <label>Recording window (seconds)</label>
        <input
          type="number"
          value={s.seconds}
          onChange={(e) => set("seconds", Number(e.target.value))}
        />
      </div>
      <div className="field row">
        <input
          type="checkbox"
          checked={s.auto_stop}
          onChange={(e) => set("auto_stop", e.target.checked)}
        />
        <span>Stop on end-of-speech (VAD)</span>
      </div>
      <div className="field row">
        <input
          type="checkbox"
          checked={s.sound}
          onChange={(e) => set("sound", e.target.checked)}
        />
        <span>Sound cues (start, stop, done, error)</span>
      </div>
      <div className="field">
        <label>Cleanup</label>
        <select value={s.cleanup} onChange={(e) => set("cleanup", e.target.value)}>
          <option value="none">none (raw transcript)</option>
          <option value="regex">regex fallback</option>
          <option value="ollama">ollama (local LLM)</option>
        </select>
      </div>
      <div className="field">
        <label>Ollama model</label>
        <input
          value={s.ollama_model}
          onChange={(e) => set("ollama_model", e.target.value)}
        />
      </div>
      <div className="field">
        <label>Whisper model path (empty means $SUSURRO_MODEL)</label>
        <input
          value={s.whisper_model}
          onChange={(e) => set("whisper_model", e.target.value)}
        />
      </div>
      <div className="field">
        <label>PipeWire target (empty means default source)</label>
        <input value={s.device} onChange={(e) => set("device", e.target.value)} />
      </div>
      <div className="field">
        <label>Release channel</label>
        <select
          value={s.update_channel}
          onChange={(e) => set("update_channel", e.target.value)}
        >
          <option value="stable">stable</option>
          <option value="beta">beta</option>
        </select>
      </div>

      <div className="field">
        <label>Tone follows the app (formal in docs, casual in messages)</label>
        <div className="row">
          <button className="ghost" onClick={loadProfiles} aria-label="load format profiles">
            load profiles
          </button>
        </div>
        <div className="row">
          <input
            value={profileApp}
            onChange={(e) => setProfileApp(e.target.value)}
            placeholder="app pattern, e.g. docs"
            aria-label="app pattern"
          />
          <select
            value={profileStyle}
            onChange={(e) => setProfileStyle(e.target.value)}
            aria-label="profile style"
          >
            <option value="formal">formal (polish)</option>
            <option value="casual">casual (tidy only)</option>
            <option value="verbatim">verbatim (raw)</option>
          </select>
          <button className="ghost" onClick={saveProfile} aria-label="save format profile">
            save profile
          </button>
        </div>
      </div>
      {profileNote && <div className="update-note" aria-live={live}>{profileNote}</div>}
      {profiles.length > 0 && (
        <div className="history">
          {profiles.map((p) => (
            <div key={p.app} className="history-row">
              <div className="mono">
                {p.app}: {p.style} (cleanup {p.cleanup})
              </div>
              <div className="row">
                <button
                  className="ghost"
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

      <div className="row">
        <button className="primary" onClick={save}>
          {saved ? "saved" : "save"}
        </button>
        <button className="ghost" onClick={checkUpdates}>
          check for updates
        </button>
        <button className="ghost" onClick={runDoctor}>
          run doctor
        </button>
      </div>
      <div className="update-note" aria-live={live}>{updateNote}</div>
      {doctor && (
        <div className="history">
          <pre className="mono" style={{ whiteSpace: "pre-wrap" }}>
            {doctor}
          </pre>
        </div>
      )}

      <div className="field">
        <label>Reading the screen</label>
        <div className="sub">The hotkey remaps live. Contrast and announcements apply on save.</div>
        <div className="field">
          <label>Dictation hotkey</label>
          <select
            value={s.hotkey}
            onChange={(e) => set("hotkey", e.target.value)}
            aria-label="dictation hotkey"
          >
            <option value="super_shift_r">Super + Shift + R</option>
            <option value="ctrl_shift_r">Ctrl + Shift + R</option>
            <option value="shift_d">Shift + D</option>
          </select>
        </div>
        <div className="field row">
          <input
            type="checkbox"
            checked={s.high_contrast}
            onChange={(e) => set("high_contrast", e.target.checked)}
            aria-label="high contrast"
          />
          <span>High contrast</span>
        </div>
        <div className="field row">
          <input
            type="checkbox"
            checked={s.announce}
            onChange={(e) => set("announce", e.target.checked)}
            aria-label="screen reader announcements"
          />
          <span>Screen reader announcements</span>
        </div>
      </div>

      <div className="field">
        <label>History (raw beside cleaned)</label>
        <div className="row">
          <button className="ghost" onClick={loadHistory} aria-label="load history">
            load history
          </button>
        </div>
      </div>
      {historyNote && <div className="update-note" aria-live={live}>{historyNote}</div>}
      {history.length > 0 && (
        <div className="history">
          {history.map((h) => {
            const raw = showRaw[h.session];
            const body = raw ? h.raw_text : h.cleaned_text || h.raw_text;
            return (
              <div key={h.session} className="history-row">
                <div className="mono" style={{ whiteSpace: "pre-wrap" }}>
                  {body}
                </div>
                <div className="sub">
                  {h.provider} | {h.latency_ms}ms | {h.session.slice(0, 8)}
                  {h.cleaned_text && h.cleaned_text !== h.raw_text
                    ? raw
                      ? " | showing raw"
                      : " | showing polished"
                    : ""}
                </div>
                <div className="row">
                  {h.cleaned_text && h.cleaned_text !== h.raw_text && (
                    <button
                      className="ghost"
                      onClick={() => toggleRaw(h.session)}
                      aria-label={raw ? "show polished text" : "show raw transcript"}
                    >
                      {raw ? "show polished" : "show raw"}
                    </button>
                  )}
                  <button
                    className="ghost"
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
    </div>
  );
}
