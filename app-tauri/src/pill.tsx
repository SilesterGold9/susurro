import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Blobatar } from "@blobatar/react";
import { useGaze } from "@blobatar/react/gaze";
import { idle, thinking } from "blobatar/expression";
import "blobatar/motion.css";
import "blobatar/gaze.css";
import "blobatar/motion.css";
import "blobatar/gaze.css";

type PillState = "idle" | "listening" | "processing" | "done" | "error";
type ProgressTick = { stage: string; value: number };

const SLICES = 48;
const BARS = 7;
const WAVE_W = 200;
const WAVE_H = 30;
const SCALE = 2; // fixed 2x backing store, crisp on hidpi with no layout reads
const ATTACK_MS = 10;
const RELEASE_MS = 160;
const HOLD_MS = 1000;
const ACTIVE = "#d85a30";
const REST = "#8a877f";
const TEAL = "#0f6e56";

// Blobatar voice: one face per app, always the same face for the same
// app. Tone pinned to the pale neutral swatch so identity reads in
// silhouette, never in saturated color: teal means working and coral
// means failed, and the avatar must never borrow either signal.
const AVATAR_TRAITS = { tone: [0.25] };

function formatElapsed(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const mm = String(Math.floor(s / 60)).padStart(2, "0");
  const ss = String(s % 60).padStart(2, "0");
  return `${mm}:${ss}`;
}

// Odometer digit: a 0-9 strip three cycles tall, translated so the
// target glyph lands in the 1em window. Position only ever moves
// forward, so 9 to 0 rolls through the seam instead of rewinding;
// a synchronous snap folds it back mid-strip, invisible because
// glyph N renders identically to glyph N plus or minus 10. Static
// characters (the colon) render as plain text, never in a strip.
const GLYPHS = "012345678901234567890123456789";

function RollDigit({ value, frozen }: { value: string; frozen: boolean }) {
  const n = value >= "0" && value <= "9" ? value.charCodeAt(0) - 48 : -1;
  const pos = useRef(10);
  const colRef = useRef<HTMLSpanElement>(null);
  // Blur clears on transition end, not on a timer: the end event is
  // the roll actually finishing. Frozen (not recording) clears
  // outright, which covers remounts and throttled pages where no end
  // event ever arrives.
  useLayoutEffect(() => {
    const el = colRef.current;
    if (!el) return;
    const reduce =
      typeof window !== "undefined" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const onEnd = (e: TransitionEvent) => {
      if (e.propertyName === "transform") el.style.filter = "";
    };
    el.addEventListener("transitionend", onEnd);
    return () => el.removeEventListener("transitionend", onEnd);
  }, []);
  useLayoutEffect(() => {
    const el = colRef.current;
    if (!el) return;
    if (frozen) {
      el.style.filter = "";
      return;
    }
    if (n < 0) return;
    const cur = ((pos.current % 10) + 10) % 10;
    if (cur === n) return;
    const next = pos.current + ((n - cur + 10) % 10);
    if (next >= 25) {
      pos.current -= 10;
      el.style.transition = "none";
      el.style.transform = `translateY(${-pos.current}em)`;
      void el.offsetWidth;
      el.style.transition = "";
      pos.current = next - 10;
    } else {
      pos.current = next;
    }
    el.style.transform = `translateY(${-pos.current}em)`;
    const reduce =
      typeof window !== "undefined" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    el.style.filter = reduce ? "" : "blur(1px)";
  }, [value, frozen]);
  if (n < 0) return <span className="rdigit-static">{value}</span>;
  return (
    <span className="rdigit">
      <span ref={colRef} className="rdigit-col" style={{ transform: `translateY(${-pos.current}em)` }}>
        {GLYPHS.split("").map((g, i) => (
          <span key={i} className="rdigit-glyph">
            {g}
          </span>
        ))}
      </span>
    </span>
  );
}

function RollTimer({ elapsed, frozen }: { elapsed: number; frozen: boolean }) {
  const text = formatElapsed(elapsed);
  return (
    <span className="timer" aria-hidden="true">
      {text.split("").map((ch, i) => (
        <RollDigit key={i} value={ch} frozen={frozen} />
      ))}
    </span>
  );
}

type CtxWithRoundRect = CanvasRenderingContext2D & {
  roundRect?: (x: number, y: number, w: number, h: number, r: number) => void;
};

export default function Pill() {
  const [state, setState] = useState<PillState>("idle");
  const [lastText, setLastText] = useState("");
  const [errorMsg, setErrorMsg] = useState("");
  const [settleKey, setSettleKey] = useState(0);
  const [contextApp, setContextApp] = useState<string | null>(null);
  const [elapsed, setElapsed] = useState(0);
  const stateRef = useRef<PillState>("idle");
  const startedAt = useRef(0);
  const timerId = useRef<number>(0);
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
      listen<{ app: string | null }>("susurro://context", (e) => {
        setContextApp(e.payload?.app ?? null);
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

  // Transient state flash: the resting row is avatar, wave, and
  // timer only. Errors and the first-run hint stick until the next
  // run because they carry instructions; done shows no words at
  // all, the check plus settle already said it.
  type Flash = { text: string; key: number };
  const [flash, setFlash] = useState<Flash | null>(null);
  useEffect(() => {
    if (state === "listening" || state === "processing" || state === "done") {
      setFlash(null);
      return;
    }
    if (state === "error") {
      setFlash({ text: (errorMsg || "couldn't paste").slice(0, 64), key: Date.now() });
      return;
    }
    if (!lastText) {
      setFlash({ text: "press super shift d", key: 0 });
    } else {
      setFlash(null);
    }
  }, [state, errorMsg, lastText]);

  // Dictation timer: runs while recording only, so the number is
  // the take length. Freezes the moment capture ends; processing
  // and done hold the total, a new run resets it.
  useEffect(() => {
    if (state === "listening") {
      startedAt.current = Date.now();
      setElapsed(0);
      if (timerId.current) return;
      timerId.current = window.setInterval(() => {
        setElapsed(Date.now() - startedAt.current);
      }, 250);
      return () => {
        window.clearInterval(timerId.current);
        timerId.current = 0;
      };
    }
    if (timerId.current) {
      window.clearInterval(timerId.current);
      timerId.current = 0;
    }
    if (state === "idle") setElapsed(0);
    return undefined;
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
    // Slow merge: bars ease into the fill over ~420ms so the states
    // blend instead of cutting.
    morph.current +=
      (morphTarget - morph.current) * (reduce ? 1 : Math.min(1, dt / 420));
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
      // Traveling glow while the value stalls: energy sweeping the
      // fill instead of a hard shimmer box. Frozen when reduced.
      if (!reduce && progress.current < 1) {
        const span = WAVE_W + 80;
        const t = (ts / 1400) % 2;
        const gx = (t < 1 ? t : 2 - t) * span - 40;
        const cy = WAVE_H / 2;
        const grad = ctx.createRadialGradient(gx, cy, 0, gx, cy, 20);
        grad.addColorStop(0, "rgba(255,255,255,0.5)");
        grad.addColorStop(1, "rgba(255,255,255,0)");
        ctx.globalAlpha = morph.current;
        ctx.fillStyle = grad;
        ctx.fillRect(Math.max(0, gx - 20), cy - 8, 40, 16);
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

  // The resting row is avatar, wave, and timer only. State words
  // appear as a transient flash over the wave and leave; nothing
  // reflows between states.
  const avatarName = contextApp ?? "susurro";
  const appInitial = (contextApp?.trim().match(/[A-Za-z0-9]/)?.[0] ?? "?").toUpperCase();
  const showThought = contextApp !== null && (state === "listening" || state === "processing");

  // Eyes follow the pointer: the blobatar gaze layer aims the eyes at
  // the cursor and eases home when it leaves. Settles to zero frames
  // under a still pointer; detaches under reduced motion by itself.
  const { ref: gazeRef } = useGaze({ travel: 3, lookAt: "pointer" });

  // Pill drag, Hyprland-native first. startDragging silently no-ops
  // here because the compositor drops client move requests without an
  // input serial, so: mousedown resolves our window address plus its
  // top-left and validates the move path, mousemove deltas accumulate
  // into absolute targets flushed per frame, mouseup ends. Anything
  // failing falls back to the platform drag for X11 and Windows.
  type DragAnchor = { address: string; x: number; y: number };
  const dragRef = useRef<{
    address: string;
    baseX: number;
    baseY: number;
    lastX: number;
    lastY: number;
    dx: number;
    dy: number;
    raf: number;
  } | null>(null);

  function flushDrag() {
    const d = dragRef.current;
    if (!d) return;
    d.raf = 0;
    void invoke("pill_drag_move", {
      address: d.address,
      x: Math.round(d.baseX + d.dx),
      y: Math.round(d.baseY + d.dy),
    }).catch(() => {});
  }

  function endDrag() {
    window.removeEventListener("mousemove", onDragMove);
    window.removeEventListener("mouseup", endDrag);
    const d = dragRef.current;
    dragRef.current = null;
    if (!d) return;
    if (d.raf) {
      window.cancelAnimationFrame(d.raf);
      d.raf = 0;
      flushDrag();
    }
  }

  function onDragMove(e: MouseEvent) {
    const d = dragRef.current;
    if (!d) return;
    d.dx += e.clientX - d.lastX;
    d.dy += e.clientY - d.lastY;
    d.lastX = e.clientX;
    d.lastY = e.clientY;
    if (!d.raf) d.raf = window.requestAnimationFrame(flushDrag);
  }

  function onDragStart(e: React.MouseEvent) {
    const startX = e.clientX;
    const startY = e.clientY;
    void invoke<DragAnchor>("pill_drag_start")
      .then((anchor) => {
        dragRef.current = {
          address: anchor.address,
          baseX: anchor.x,
          baseY: anchor.y,
          lastX: startX,
          lastY: startY,
          dx: 0,
          dy: 0,
          raf: 0,
        };
        window.addEventListener("mousemove", onDragMove);
        window.addEventListener("mouseup", endDrag);
      })
      .catch(() => {
        void getCurrentWindow().startDragging().catch(() => {});
      });
  }

  return (
    <div className="pill-wrap">
      <div
        className={`pill state-${state}${state === "done" ? " settle" : ""}`}
        key={settleKey}
        onMouseDown={onDragStart}
      >
        <span
          className="avatar"
          title={contextApp ? `dictating into ${contextApp}` : "dictation target unknown"}
          aria-hidden="true"
        >
          <Blobatar
            ref={gazeRef}
            name={avatarName}
            size={30}
            traits={AVATAR_TRAITS}
            expression={state === "processing" ? thinking : idle}
            animate="always"
          />
          {showThought && (
            <span className="thought" aria-hidden="true">
              {appInitial}
            </span>
          )}
        </span>
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
        <RollTimer elapsed={elapsed} frozen={state !== "listening"} />
        {flash && (
          <span
            key={flash.key}
            className={`flash${state === "error" ? " is-error" : ""}`}
            role="status"
          >
            {flash.text}
          </span>
        )}
      </div>
    </div>
  );
}
