import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Settings } from "./settings";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Progress } from "@/components/ui/progress";

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

const INTENTS = [
  { name: "docs", label: "Documents", desc: "Formal tone for docs and writing." },
  { name: "messages", label: "Messages", desc: "Casual tone for chat and mail." },
  { name: "both", label: "Both", desc: "Each app sounds like itself." },
];

const TONES = [
  { name: "formal", title: "Formal.", desc: "Caps plus punctuation." },
  { name: "casual", title: "Casual", desc: "Light tidy, never rewritten." },
  { name: "verbatim", title: "Verbatim", desc: "Raw transcript, untouched." },
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
  const [wipeOpen, setWipeOpen] = useState(false);
  const [intent, setIntent] = useState("both");
  const [tone, setTone] = useState("formal");
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
    // Base streams in the background from first paint: the user
    // answers intent, tone, and hotkey while bytes arrive. The tiny
    // model is already on board, so nothing here blocks on network.
    invoke<string>("start_model_prefetch").catch(() => {});
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

  async function applyTone(name: string) {
    setTone(name);
    setNote("");
    try {
      const msg = await invoke<string>("apply_intent", { intent, style: name });
      setNote(msg);
    } catch (e) {
      setNote(`Couldn't apply tone. ${e}`);
    }
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
    setBusy("wipe");
    try {
      const msg = await invoke<string>("wipe_data");
      setNote(msg);
      setWipeOpen(false);
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
        Setup, screen {step} of 6. No tutorial, no maze.
      </div>
      <div className="flow-dots" aria-hidden="true">
        {[1, 2, 3, 4, 5, 6].map((n) => (
          <span key={n} className={n <= step ? "on" : ""} />
        ))}
      </div>

      {step === 1 && (
        <div className="flow-card flow-pad flow-sec">
          <h3 className="flow-h3">1. What brings you here?</h3>
          <div className="flow-sub">
            This seeds your tone profiles, so the answer changes how
            susurro sounds from the first dictation.
          </div>
          {INTENTS.map((i) => (
            <label className="flow-check" key={i.name}>
              <input
                type="radio"
                name="intent"
                checked={intent === i.name}
                onChange={() => setIntent(i.name)}
                aria-label={i.label}
              />
              <span>
                <strong>{i.label}.</strong> {i.desc}
              </span>
            </label>
          ))}
          <div className="row">
            <Button variant="ink" onClick={() => setStep(2)}>
              next
            </Button>
          </div>
        </div>
      )}

      {step === 2 && (
        <div className="flow-card flow-pad flow-sec">
          <h3 className="flow-h3">2. Requirements, model, speed check</h3>
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
            <Button variant="outline" size="sm" onClick={refresh} aria-label="recheck requirements">
              recheck
            </Button>
          </div>
          <div className="flow-sub">
            {status
              ? status.model_found
                ? `model found: ${status.model_path}. Checksum ${status.model_checksum}.`
                : `model still arriving: the built-in tiny already dictates, base.en streams in the background.`
              : "checking model..."}
          </div>
          <div className="row">
            <Button
              variant="ink"
              onClick={download}
              loading={busy === "model"}
              disabled={!!status?.model_found}
            >
              {busy === "model" ? "downloading..." : "fetch base now"}
            </Button>
            <Button variant="outline" size="sm" onClick={runBench} loading={busy === "bench"}>
              {busy === "bench" ? "measuring..." : "run speed check"}
            </Button>
          </div>
          {pct !== null && (
            <div className="flow-bar-row" aria-live="polite">
              <span className="flow-bar-label">downloading model</span>
              <Progress value={pct} aria-label={`downloading model: ${pct} percent`} className="flex-1" />
              <span className="flow-bar-num">{pct}%</span>
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
            <Button variant="ink" onClick={() => setStep(3)}>
              next
            </Button>
          </div>
        </div>
      )}

      {step === 3 && (
        <div className="flow-card flow-pad flow-sec">
          <h3 className="flow-h3">3. Pick the dictation hotkey</h3>
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
            <Button variant="outline" size="sm" onClick={() => setStep(2)}>
              back
            </Button>
            <Button variant="ink" onClick={() => setStep(4)}>
              next
            </Button>
          </div>
        </div>
      )}

      {step === 4 && (
        <div className="flow-card flow-pad flow-sec">
          <h3 className="flow-h3">4. How should you sound?</h3>
          <div className="flow-sub">
            Pick a tone. It writes profiles for{" "}
            {intent === "both" ? "docs and messages" : intent} right now.
          </div>
          <div className="flow-grid3">
            {TONES.map((t) => (
              <Card key={t.name} asChild selected={tone === t.name}>
                <button
                  onClick={() => applyTone(t.name)}
                  aria-pressed={tone === t.name}
                  className="w-full cursor-pointer text-left"
                >
                  <div className="flow-serif">{t.title}</div>
                  <div className="flow-sub">{t.desc}</div>
                </button>
              </Card>
            ))}
          </div>
          <div className="row">
            <Button variant="outline" size="sm" onClick={() => setStep(3)}>
              back
            </Button>
            <Button variant="ink" onClick={() => setStep(5)}>
              next
            </Button>
          </div>
        </div>
      )}

      {step === 5 && (
        <div className="flow-card flow-pad flow-sec">
          <h3 className="flow-h3">5. Test dictation (6 seconds)</h3>
          <div className="flow-sub">
            Your mic is the only permission this needs. Press the button,
            speak, and the transcript lands here.
          </div>
          <div className="row">
            <Button variant="ink" onClick={testMic} loading={busy === "test"}>
              {busy === "test" ? "listening..." : "speak now"}
            </Button>
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
            <Button variant="outline" size="sm" onClick={() => setStep(4)}>
              back
            </Button>
            <Button variant="ink" onClick={() => setStep(6)}>
              next
            </Button>
          </div>
        </div>
      )}

      {step === 6 && (
        <div className="flow-card flow-pad flow-sec">
          <h3 className="flow-h3">6. Done</h3>
          <div className="flow-sub">
            Model {status?.model_found ? "ready" : "arriving (built-in tiny dictates now, base.en follows automatically)"},
            speed tier {status?.tier || bench?.tier || "measuring in the background"},
            hotkey {HOTKEYS.find((h) => h.name === hotkey)?.label},
            writing {tone} for {intent}.
          </div>
          <div className="row">
            <Button variant="outline" size="sm" onClick={() => setStep(5)}>
              back
            </Button>
            <Button variant="ink" onClick={finish} loading={busy === "finish"}>
              {busy === "finish" ? "saving..." : "start dictating"}
            </Button>
          </div>
        </div>
      )}

      {step === 6 && (
        <div className="flow-card flow-pad flow-sec">
          <h3 className="flow-h3">Your data stays here</h3>
          <div className="flow-sub">
            Transcripts, words, and styles never leave this machine. Erase
            them any time; settings and models survive.
          </div>
          <div className="row">
            <Dialog open={wipeOpen} onOpenChange={setWipeOpen}>
              <DialogTrigger asChild>
                <Button variant="outline" size="sm" aria-label="erase dictation data">
                  erase my data
                </Button>
              </DialogTrigger>
              <DialogContent>
                <DialogTitle>Erase dictation data?</DialogTitle>
                <DialogDescription>
                  Transcripts, words, and styles go. Settings and models survive.
                </DialogDescription>
                <div className="row justify-end">
                  <DialogClose asChild>
                    <Button variant="outline" size="sm">
                      keep it
                    </Button>
                  </DialogClose>
                  <Button
                    variant="destructive"
                    size="sm"
                    onClick={wipe}
                    loading={busy === "wipe"}
                  >
                    {busy === "wipe" ? "erasing..." : "erase everything"}
                  </Button>
                </div>
              </DialogContent>
            </Dialog>
          </div>
        </div>
      )}

      {note && <div className="flow-note" aria-live={display.announce ? "polite" : "off"}>{note}</div>}
    </div>
  );
}
