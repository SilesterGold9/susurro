import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

type PillState = "idle" | "listening" | "processing" | "done" | "error";
type ProgressTick = { stage: string; value: number };

const SLICES = 48;
const BARS = 7;
const WAVE_W = 180;
const WAVE_H = 28;
const SCALE = 2; // fixed 2x backing store, crisp on hidpi with no layout reads
const ATTACK_MS = 10;
const RELEASE_MS = 160;
const HOLD_MS = 1000;
const ACTIVE = "#d85a30";
const REST = "#8a877f";
const TEAL = "#0f6e56";

type CtxWithRoundRect = CanvasRenderingContext2D & {
  roundRect?: (x: number, y: number, w: number, h: number, r: number) => void;
};

export default function Pill() {
  const [state, setState] = useState<PillState>("idle");
  const [lastText, setLastText] = useState("");
  const [errorMsg, setErrorMsg] = useState("");
  const [settleKey, setSettleKey] = useState(0);
  const stateRef = useRef<PillState>("idle");
  const ring = useRef<number[]>(new Array(SLICES).fill(0));
  const bars = useRef<number[]>(new Array(BARS).fill(0));
  const peaks = useRef<number[]>(new Array(BARS).fill(0));
  const peakAt = useRef<number[]>(new Array(BARS).fill(0));
  const morph = useRef(0); // 0 = bars, 1 = progress fill
  const progress = useRef(0);
  const raf = useRef(0);
  const lastTs = useRef(0);
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const offs = [
      listen<string>("susurro://state", (e) => {
        const next = e.payload as PillState;
        stateRef.current = next;
        setState(next);
        if (next === "done") {
          progress.current = 1;
          setSettleKey((k) => k + 1);
        }
        if (next === "listening") progress.current = 0;
      }),
      listen<number>("susurro://level", (e) => {
        ring.current.push(Math.min(1, Math.max(0, e.payload)));
        ring.current.shift();
      }),
      listen<ProgressTick>("susurro://progress", (e) => {
        const v = e.payload;
        if (typeof v.value === "number") {
          progress.current = Math.min(1, Math.max(0, v.value));
        }
      }),
      listen<{ cleaned: string }>("susurro://result", (e) => {
        setLastText(e.payload.cleaned);
      }),
      listen<string>("susurro://error", (e) => {
        setErrorMsg(e.payload);
      }),
    ];
    return () => {
      offs.forEach((p) => p.then((off) => off()));
    };
  }, []);

  // The render loop runs only while something moves. Idle, done, and
  // error get one static frame so the pill never burns cycles at rest.
  useEffect(() => {
    if (state !== "listening" && state !== "processing") {
      cancelAnimationFrame(raf.current);
      drawStatic();
      return;
    }
    lastTs.current = 0;
    const tick = (ts: number) => {
      step(ts);
      raf.current = requestAnimationFrame(tick);
    };
    raf.current = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf.current);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state]);

  // Returning to idle clears the wave so no stale peaks linger.
  useEffect(() => {
    if (state !== "idle") return;
    ring.current = new Array(SLICES).fill(0);
    bars.current = new Array(BARS).fill(0);
    peaks.current = new Array(BARS).fill(0);
    morph.current = 0;
  }, [state]);

  // Clear stale errors when a new run starts.
  useEffect(() => {
    if (state === "listening") setErrorMsg("");
  }, [state]);

  function pooled(): number[] {
    const per = Math.floor(SLICES / BARS);
    const out = new Array(BARS).fill(0);
    for (let b = 0; b < BARS; b += 1) {
      let m = 0;
      for (let i = 0; i < per; i += 1) {
        const v = ring.current[b * per + i] ?? 0;
        if (v > m) m = v;
      }
      out[b] = m;
    }
    return out;
  }

  function step(ts: number) {
    const reduce =
      typeof window !== "undefined" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const dt = Math.min(100, Math.max(0, ts - (lastTs.current || ts)));
    lastTs.current = ts;
    const live = stateRef.current === "listening";
    const targets = live ? pooled() : new Array(BARS).fill(0);
    const up = Math.min(1, dt / ATTACK_MS);
    const down = Math.min(1, dt / RELEASE_MS);
    for (let b = 0; b < BARS; b += 1) {
      const t = targets[b];
      const d = bars.current[b];
      bars.current[b] = d + (t - d) * (t > d ? up : down);
      if (t >= peaks.current[b]) {
        peaks.current[b] = t;
        peakAt.current[b] = ts;
      } else if (ts - peakAt.current[b] > HOLD_MS) {
        peaks.current[b] += (bars.current[b] - peaks.current[b]) * down;
      }
    }
    const morphTarget = stateRef.current === "processing" ? 1 : 0;
    morph.current +=
      (morphTarget - morph.current) * (reduce ? 1 : Math.min(1, dt / 180));
    draw(ts, reduce);
  }

  function draw(ts: number, reduce: boolean) {
    const c = canvasRef.current;
    if (!c) return;
    const ctx = c.getContext("2d") as CanvasRenderingContext2D | null;
    if (!ctx) return;
    const mode = stateRef.current;
    const live = mode === "listening";
    ctx.setTransform(SCALE, 0, 0, SCALE, 0, 0);
    ctx.clearRect(0, 0, WAVE_W, WAVE_H);
    // Breathing baseline at rest: slow drift, static when reduced.
    let alpha = 1;
    if (mode === "idle" && !reduce) {
      alpha = 0.72 + 0.28 * (0.5 + 0.5 * Math.sin((ts / 1000) * 2 * Math.PI * 0.4));
    }
    ctx.globalAlpha = alpha;
    ctx.fillStyle = "rgba(128,128,128,0.35)";
    ctx.fillRect(0, Math.floor(WAVE_H / 2), WAVE_W, 1);
    // Bars ease out as the progress fill takes the same zone.
    const barAlpha = 1 - morph.current;
    if (barAlpha > 0.01) {
      ctx.globalAlpha = alpha * barAlpha;
      ctx.fillStyle = live ? ACTIVE : REST;
      const bw = WAVE_W / BARS;
      const bar = Math.max(3, bw - 3);
      const rr = (ctx as CtxWithRoundRect).roundRect;
      for (let b = 0; b < BARS; b += 1) {
        const v = bars.current[b];
        const bh = Math.max(2, v * (WAVE_H - 6));
        const x = b * bw + (bw - bar) / 2;
        const y = (WAVE_H - bh) / 2;
        if (typeof rr === "function") {
          ctx.beginPath();
          rr.call(ctx, x, y, bar, bh, bar / 2);
          ctx.fill();
        } else {
          ctx.fillRect(x, y, bar, bh);
        }
        // Peak-hold cap.
        const p = peaks.current[b];
        if (p > 0.03) {
          const py = (WAVE_H - Math.max(2, p * (WAVE_H - 6))) / 2;
          ctx.fillRect(x, py, bar, 2);
        }
      }
    }
    // Determinate fill shares the zone: no reflow, bars become progress.
    if (morph.current > 0.01) {
      ctx.globalAlpha = morph.current;
      const w = Math.max(4, progress.current * WAVE_W);
      ctx.fillStyle = TEAL;
      const rr = (ctx as CtxWithRoundRect).roundRect;
      if (typeof rr === "function") {
        ctx.beginPath();
        rr.call(ctx, 0, WAVE_H / 2 - 3, w, 6, 3);
        ctx.fill();
      } else {
        ctx.fillRect(0, WAVE_H / 2 - 3, w, 6);
      }
      // Shimmer while the value stalls, frozen when reduced.
      if (!reduce && progress.current < 1) {
        const sx = ((ts / 8) % (WAVE_W + 40)) - 20;
        ctx.globalAlpha = morph.current * 0.35;
        ctx.fillStyle = "#ffffff";
        ctx.fillRect(Math.min(sx, w - 8), WAVE_H / 2 - 3, 8, 6);
      }
    }
    ctx.globalAlpha = 1;
  }

  function drawStatic() {
    morph.current = 0;
    progress.current = 0;
    if (stateRef.current !== "listening") {
      bars.current = new Array(BARS).fill(0);
      peaks.current = new Array(BARS).fill(0);
    }
    draw(0, true);
  }

  const label =
    state === "idle"
      ? lastText
        ? "ready"
        : "press super shift d"
      : state === "listening"
        ? "listening"
        : state === "processing"
          ? "working"
          : state === "done"
            ? "pasted"
            : (errorMsg || "couldn't paste").slice(0, 64);

  return (
    <div className="pill-wrap">
      <div className={`pill state-${state}${state === "done" ? " settle" : ""}`} key={settleKey}>
        <span className="dot" aria-hidden="true" />
        <canvas
          ref={canvasRef}
          width={WAVE_W * SCALE}
          height={WAVE_H * SCALE}
          className="wave"
          aria-hidden="true"
        />
        {state === "done" && (
          <svg className="check" width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
            <path
              d="M2.5 7.5l3 3 6-7"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        )}
        <span className={`status${state === "error" ? " is-error" : ""}`} role="status">
          {label}
        </span>
      </div>
    </div>
  );
}
