import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Settings } from "./settings";

interface Requirements {
  os: string;
  whisper: string | null;
  model_found: boolean;
  model_path: string;
  paste_ok: boolean;
  paste_detail: string;
  whisper_hint: string;
  ollama_up: boolean;
  ollama_model_present: boolean;
  ollama_hint: string;
}

interface OnboardingStatus {
  model_found: boolean;
  model_path: string;
  model_checksum: string;
  tier: string | null;
  hotkey: string;
  onboarding_done: boolean;
}

interface BenchResult {
  tier: string;
  iters_per_sec: number;
  elapsed_ms: number;
  cores: number;
  persisted: boolean;
}

interface TestResult {
  raw: string;
  cleaned: string;
  latency_ms: number;
}

const HOTKEYS = [
  { name: "super_shift_r", label: "Super + Shift + R" },
  { name: "ctrl_shift_r", label: "Ctrl + Shift + R" },
  { name: "shift_d", label: "Shift + D" },
];

export default function OnboardingView() {
  const [step, setStep] = useState(1);
  const [status, setStatus] = useState<OnboardingStatus | null>(null);
  const [bench, setBench] = useState<BenchResult | null>(null);
  const [snippet, setSnippet] = useState("");
  const [hotkey, setHotkey] = useState("super_shift_r");
  const [testOut, setTestOut] = useState<TestResult | null>(null);
  const [busy, setBusy] = useState("");
  const [note, setNote] = useState("");
  const [confirmWipe, setConfirmWipe] = useState(false);
  const [display, setDisplay] = useState({ high_contrast: false, announce: true });
  const [reqs, setReqs] = useState<Requirements | null>(null);
  const [pct, setPct] = useState<number | null>(null);

  async function refresh() {
    try {
      const s = await invoke<OnboardingStatus>("onboarding_status");
      setStatus(s);
      setHotkey(s.hotkey || "super_shift_r");
    } catch (e) {
      setNote(`Couldn't read setup state. ${e}`);
    }
    try {
      const prefs = await invoke<Settings>("get_settings");
      setDisplay({ high_contrast: prefs.high_contrast, announce: prefs.announce });
    } catch {}
    try {
      setReqs(await invoke<Requirements>("requirements_status"));
    } catch (e) {
      setNote(`Couldn't read requirements. ${e}`);
    }
  }

  useEffect(() => {
    refresh();
    const off = listen<{ step: string; state: string; pct?: number }>(
      "susurro://onboarding",
      (e) => {
        if (e.payload?.step === "model" && typeof e.payload.pct === "number") {
          setPct(e.payload.pct);
        }
        if (e.payload?.state === "done") {
          setPct(null);
          refresh();
        }
      },
    );
    return () => {
      off.then((f) => f());
    };
  }, []);

  useEffect(() => {
    invoke<string>("hotkey_snippet", { hotkey }).then(setSnippet).catch(() => {});
  }, [hotkey]);

  async function download() {
    setBusy("model");
    setNote("");
    setPct(0);
    try {
      const path = await invoke<string>("download_model");
      setNote(`model ready: ${path}.`);
      await refresh();
    } catch (e) {
      setNote(`Couldn't download the model. ${e}`);
    }
    setBusy("");
    setPct(null);
  }

  async function runBench() {
    setBusy("bench");
    setNote("");
    try {
      setBench(await invoke<BenchResult>("run_bench"));
      await refresh();
    } catch (e) {
      setNote(`Couldn't run the benchmark. ${e}`);
    }
    setBusy("");
  }

  async function testMic() {
    setBusy("test");
    setNote("");
    setTestOut(null);
    try {
      setTestOut(await invoke<TestResult>("test_dictation"));
    } catch (e) {
      setNote(`Test failed. ${e}`);
    }
    setBusy("");
  }

  async function finish() {
    setBusy("finish");
    try {
      await invoke("finish_onboarding", { hotkey });
      await getCurrentWindow().close();
    } catch (e) {
      setNote(`Couldn't finish setup. ${e}`);
      setBusy("");
    }
  }

  async function wipe() {
    if (!confirmWipe) {
      setConfirmWipe(true);
      return;
    }
    setBusy("wipe");
    try {
      const msg = await invoke<string>("wipe_data");
      setNote(msg);
      setConfirmWipe(false);
      await refresh();
    } catch (e) {
      setNote(`Couldn't wipe data. ${e}`);
    }
    setBusy("");
  }

  return (
    <div className={`flow-onboard${display.high_contrast ? " high-contrast" : ""}`}>
      <h1 className="flow-title">Welcome to susurro,</h1>
      <div className="flow-sub">
        Setup, screen {step} of 4. No tutorial, no maze.
      </div>

      {step === 1 && (
        <div className="flow-card flow-pad flow-sec">
          <h3 className="flow-h3">1. Requirements, model, speed check</h3>
          <div className="flow-sub">
            {reqs ? (
              <>
                <div>
                  {reqs.whisper
                    ? `whisper ready: ${reqs.whisper}.`
                    : "whisper missing: dictation cannot run yet."}
                </div>
                {!reqs.whisper && reqs.whisper_hint && <div>{reqs.whisper_hint}</div>}
                <div>
                  {reqs.paste_ok ? reqs.paste_detail : `paste: ${reqs.paste_detail}`}.
                </div>
                <div>
                  {reqs.ollama_up && reqs.ollama_model_present
                    ? "cleanup model ready."
                    : `cleanup: ${reqs.ollama_hint}`}
                </div>
              </>
            ) : (
              "checking requirements..."
            )}
          </div>
          <div className="row">
            <button className="flow-mini" onClick={refresh} aria-label="recheck requirements">
              recheck
            </button>
          </div>
          <div className="flow-sub">
            {status
              ? status.model_found
                ? `model found: ${status.model_path}. Checksum ${status.model_checksum}.`
                : `model missing: ${status.model_path}. Download base.en to continue.`
              : "checking model..."}
          </div>
          <div className="row">
            <button
              className="flow-dark"
              onClick={download}
              disabled={busy === "model" || !!status?.model_found}
            >
              {busy === "model" ? "downloading..." : "download model"}
            </button>
            <button className="flow-mini" onClick={runBench} disabled={busy === "bench"}>
              {busy === "bench" ? "measuring..." : "run speed check"}
            </button>
          </div>
          {pct !== null && (
            <div className="flow-sub" aria-live="polite">
              downloading model: {pct}%.
            </div>
          )}
          {(bench || status?.tier) && (
            <div className="flow-sub">
              {bench
                ? `this machine: ${bench.tier} tier at ${bench.iters_per_sec} iters/s on ${bench.cores} cores${bench.persisted ? ", saved" : ", not saved"}.`
                : `saved tier: ${status?.tier}.`}
            </div>
          )}
          <div className="row">
            <button className="flow-dark" onClick={() => setStep(2)}>
              next
            </button>
          </div>
        </div>
      )}

      {step === 2 && (
        <div className="flow-card flow-pad flow-sec">
          <h3 className="flow-h3">2. Pick the dictation hotkey</h3>
          {HOTKEYS.map((h) => (
            <label className="flow-check" key={h.name}>
              <input
                type="radio"
                name="hotkey"
                checked={hotkey === h.name}
                onChange={() => setHotkey(h.name)}
                aria-label={h.label}
              />
              <span>{h.label}</span>
            </label>
          ))}
          <div className="flow-sub">On Hyprland, add this line to hyprland.conf:</div>
          <div className="flow-card flow-sec">
            <pre className="flow-mono" style={{ whiteSpace: "pre-wrap" }}>
              {snippet}
            </pre>
          </div>
          <div className="row">
            <button className="flow-mini" onClick={() => setStep(1)}>
              back
            </button>
            <button className="flow-dark" onClick={() => setStep(3)}>
              next
            </button>
          </div>
        </div>
      )}

      {step === 3 && (
        <div className="flow-card flow-pad flow-sec">
          <h3 className="flow-h3">3. Test dictation (6 seconds)</h3>
          <div className="flow-sub">Press the button, speak, and the transcript lands here.</div>
          <div className="row">
            <button className="flow-dark" onClick={testMic} disabled={busy === "test"}>
              {busy === "test" ? "listening..." : "speak now"}
            </button>
          </div>
          {testOut && (
            <div className="flow-card flow-sec">
              <div className="flow-mono" style={{ whiteSpace: "pre-wrap" }}>
                {testOut.cleaned || testOut.raw}
              </div>
              <div className="flow-sub">{testOut.latency_ms}ms end to end.</div>
            </div>
          )}
          <div className="row">
            <button className="flow-mini" onClick={() => setStep(2)}>
              back
            </button>
            <button className="flow-dark" onClick={() => setStep(4)}>
              next
            </button>
          </div>
        </div>
      )}

      {step === 4 && (
        <div className="flow-card flow-pad flow-sec">
          <h3 className="flow-h3">4. Done</h3>
          <div className="flow-sub">
            Model {status?.model_found ? "ready" : "still missing (dictation falls back to $SUSURRO_MODEL)"},
            speed tier {status?.tier || bench?.tier || "unset"},
            hotkey {HOTKEYS.find((h) => h.name === hotkey)?.label}.
          </div>
          <div className="row">
            <button className="flow-mini" onClick={() => setStep(3)}>
              back
            </button>
            <button className="flow-dark" onClick={finish} disabled={busy === "finish"}>
              {busy === "finish" ? "saving..." : "start dictating"}
            </button>
          </div>
        </div>
      )}

      {step === 4 && (
        <div className="flow-card flow-pad flow-sec">
          <h3 className="flow-h3">Your data stays here</h3>
          <div className="flow-sub">
            Transcripts, words, and styles never leave this machine. Erase
            them any time; settings and models survive.
          </div>
          <div className="row">
            <button
              className="flow-mini"
              onClick={wipe}
              disabled={busy === "wipe"}
              aria-label="erase dictation data"
            >
              {busy === "wipe"
                ? "erasing..."
                : confirmWipe
                  ? "click again to confirm erase"
                  : "erase my data"}
            </button>
          </div>
        </div>
      )}

      {note && <div className="flow-note" aria-live={display.announce ? "polite" : "off"}>{note}</div>}
    </div>
  );
}
