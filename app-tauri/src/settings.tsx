import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { check } from "@tauri-apps/plugin-updater";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

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
        <Label htmlFor="set-seconds">Recording window (seconds)</Label>
        <Input
          id="set-seconds"
          mono
          type="number"
          value={s.seconds}
          onChange={(e) => set("seconds", Number(e.target.value))}
        />
        <div className="my-2.5 flex items-center gap-2.5">
          <Switch
            id="set-vad"
            checked={s.auto_stop}
            onCheckedChange={(v) => set("auto_stop", v)}
          />
          <Label htmlFor="set-vad" className="mb-0">Stop on end-of-speech (VAD)</Label>
        </div>
        <div className="my-2.5 flex items-center gap-2.5">
          <Switch
            id="set-sound"
            checked={s.sound}
            onCheckedChange={(v) => set("sound", v)}
          />
          <Label htmlFor="set-sound" className="mb-0">Sound cues (start, stop, done, error)</Label>
        </div>
        <Label htmlFor="set-device">PipeWire target (empty means default source)</Label>
        <Input
          id="set-device"
          mono
          value={s.device}
          onChange={(e) => set("device", e.target.value)}
        />
      </div>

      <div className="flow-card flow-pad flow-sec">
        <h3 className="flow-h3">Cleanup and models</h3>
        <Label>Cleanup</Label>
        <Select value={s.cleanup} onValueChange={(v) => set("cleanup", v)}>
          <SelectTrigger aria-label="cleanup">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="none">none (raw transcript)</SelectItem>
            <SelectItem value="regex">regex fallback</SelectItem>
            <SelectItem value="ollama">ollama (local LLM)</SelectItem>
          </SelectContent>
        </Select>
        <Label htmlFor="set-ollama">Ollama model</Label>
        <Input
          id="set-ollama"
          mono
          value={s.ollama_model}
          onChange={(e) => set("ollama_model", e.target.value)}
        />
        <Label htmlFor="set-model">Whisper model path (empty means $SUSURRO_MODEL)</Label>
        <Input
          id="set-model"
          mono
          value={s.whisper_model}
          onChange={(e) => set("whisper_model", e.target.value)}
        />
        <Label>Release channel</Label>
        <Select value={s.update_channel} onValueChange={(v) => set("update_channel", v)}>
          <SelectTrigger aria-label="release channel">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="stable">stable</SelectItem>
            <SelectItem value="beta">beta</SelectItem>
          </SelectContent>
        </Select>
      </div>

      <div className="flow-card flow-pad flow-sec">
        <h3 className="flow-h3">Tone follows the app</h3>
        <div className="flow-sub">Formal in docs, casual in messages. Also on the Style page.</div>
        <div className="row">
          <Button variant="outline" size="sm" onClick={loadProfiles} aria-label="load format profiles">
            load profiles
          </Button>
        </div>
        <div className="row">
          <Input
            value={profileApp}
            onChange={(e) => setProfileApp(e.target.value)}
            placeholder="app pattern, e.g. docs"
            aria-label="app pattern"
          />
          <Select value={profileStyle} onValueChange={setProfileStyle}>
            <SelectTrigger aria-label="profile style">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="formal">formal (polish)</SelectItem>
              <SelectItem value="casual">casual (tidy only)</SelectItem>
              <SelectItem value="verbatim">verbatim (raw)</SelectItem>
            </SelectContent>
          </Select>
          <Button variant="outline" size="sm" onClick={saveProfile} aria-label="save format profile">
            save profile
          </Button>
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
                <Button
                  variant="outline"
                  size="xs"
                  onClick={() => removeProfile(p.app)}
                  aria-label={`remove profile ${p.app}`}
                >
                  remove
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}

      <div className="row flow-sec">
        <Button variant="ink" onClick={save}>
          {saved ? "saved" : "save"}
        </Button>
        <Button variant="outline" size="sm" onClick={checkUpdates}>
          check for updates
        </Button>
        <Button variant="outline" size="sm" onClick={runDoctor}>
          run doctor
        </Button>
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
        <Label>Dictation hotkey</Label>
        <Select value={s.hotkey} onValueChange={(v) => set("hotkey", v)}>
          <SelectTrigger aria-label="dictation hotkey">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="super_shift_r">Super + Shift + R</SelectItem>
            <SelectItem value="ctrl_shift_r">Ctrl + Shift + R</SelectItem>
            <SelectItem value="shift_d">Shift + D</SelectItem>
          </SelectContent>
        </Select>
        <div className="my-2.5 flex items-center gap-2.5">
          <Switch
            id="set-hc"
            checked={s.high_contrast}
            onCheckedChange={(v) => set("high_contrast", v)}
            aria-label="high contrast"
          />
          <Label htmlFor="set-hc" className="mb-0">High contrast</Label>
        </div>
        <div className="my-2.5 flex items-center gap-2.5">
          <Switch
            id="set-announce"
            checked={s.announce}
            onCheckedChange={(v) => set("announce", v)}
            aria-label="screen reader announcements"
          />
          <Label htmlFor="set-announce" className="mb-0">Screen reader announcements</Label>
        </div>
      </div>

      <div className="flow-card flow-pad flow-sec">
        <h3 className="flow-h3">History</h3>
        <div className="flow-sub">Raw beside cleaned, with restore.</div>
        <div className="row">
          <Button variant="outline" size="sm" onClick={loadHistory} aria-label="load history">
            load history
          </Button>
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
                    <Button
                      variant="outline"
                      size="xs"
                      onClick={() => toggleRaw(h.session)}
                      aria-label={raw ? "show polished text" : "show raw transcript"}
                    >
                      {raw ? "show polished" : "show raw"}
                    </Button>
                  )}
                  <Button
                    variant="outline"
                    size="xs"
                    onClick={() => restoreRaw(h.session)}
                    aria-label={`restore raw transcript ${h.session.slice(0, 8)}`}
                  >
                    restore raw
                  </Button>
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
          <Button variant="outline" size="sm" onClick={loadStats} aria-label="load usage stats">
            load stats
          </Button>
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
