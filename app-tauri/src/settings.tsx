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
}

const DEFAULTS: Settings = {
  seconds: 30,
  auto_stop: true,
  sound: true,
  cleanup: "ollama",
  ollama_model: "qwen2.5:0.5b",
  whisper_model: "",
  device: "",
  socket_path: "/tmp/susurro.sock",
  update_channel: "stable",
};

export default function SettingsView() {
  const [s, setS] = useState<Settings>(DEFAULTS);
  const [saved, setSaved] = useState(false);
  const [updateNote, setUpdateNote] = useState("no updates checked yet.");
  const [doctor, setDoctor] = useState("");

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

  function set<K extends keyof Settings>(k: K, v: Settings[K]) {
    setS({ ...s, [k]: v });
  }

  return (
    <div className="settings">
      <h1 className="wordmark">Susurro</h1>
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
      <div className="update-note">{updateNote}</div>
      {doctor && (
        <div className="history">
          <pre className="mono" style={{ whiteSpace: "pre-wrap" }}>
            {doctor}
          </pre>
        </div>
      )}
    </div>
  );
}
