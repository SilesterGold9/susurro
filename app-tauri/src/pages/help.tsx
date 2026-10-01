import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export default function HelpPage() {
  const [doctor, setDoctor] = useState("");
  const [busy, setBusy] = useState(false);

  async function run() {
    setBusy(true);
    try {
      setDoctor(await invoke<string>("run_doctor"));
    } catch (e) {
      setDoctor(`Couldn't run doctor. ${e}`);
    }
    setBusy(false);
  }

  async function replay() {
    try {
      await invoke("show_onboarding");
    } catch (e) {
      setDoctor(`Couldn't reopen setup. ${e}`);
    }
  }

  return (
    <div className="flow-page">
      <h1 className="flow-title">Help</h1>
      <div className="flow-card flow-pad">
        <h3 className="flow-h3">Dictate</h3>
        <div className="flow-sub">
          Press your dictation hotkey, speak, and the transcript lands at
          the cursor. Say exactly scratch that to undo the last session.
        </div>
        <h3 className="flow-h3">First aid</h3>
        <div className="flow-sub">
          No text appears: check that whisper-cli is installed, a model is
          downloaded, and on Linux that ydotoold runs. Then run doctor.
        </div>
        <div className="row">
          <button className="flow-dark" onClick={run} disabled={busy}>
            {busy ? "checking..." : "Run doctor"}
          </button>
          <button className="flow-mini" onClick={replay} aria-label="replay setup">
            Replay setup
          </button>
        </div>
      </div>
      {doctor && (
        <div className="flow-card flow-pad">
          <pre className="flow-mono">{doctor}</pre>
        </div>
      )}
    </div>
  );
}
