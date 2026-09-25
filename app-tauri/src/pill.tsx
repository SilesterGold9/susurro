import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

type PillState = "idle" | "listening" | "processing" | "done" | "error";

export default function Pill() {
  const [state, setState] = useState<PillState>("idle");
  const [lastText, setLastText] = useState("");
  const [errorMsg, setErrorMsg] = useState("");
  const [settleKey, setSettleKey] = useState(0);
  const levels = useRef<number[]>(new Array(48).fill(0));
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const offs = [
      listen<string>("susurro://state", (e) => {
        setState(e.payload as PillState);
        if (e.payload === "done") setSettleKey((k) => k + 1);
      }),
      listen<number>("susurro://level", (e) => {
        levels.current.push(Math.min(1, Math.max(0, e.payload)));
        levels.current.shift();
        draw();
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

  function draw() {
    const c = canvasRef.current;
    if (!c) return;
    const ctx = c.getContext("2d");
    if (!ctx) return;
    const w = c.width;
    const h = c.height;
    ctx.clearRect(0, 0, w, h);
    const active = state === "listening";
    ctx.fillStyle = active ? "#d85a30" : "#0f6e56";
    const n = levels.current.length;
    const bw = w / n;
    levels.current.forEach((v, i) => {
      const bh = Math.max(2, v * h);
      ctx.fillRect(i * bw, (h - bh) / 2, Math.max(1, bw - 1), bh);
    });
  }

  // Clear stale errors when a new run starts.
  useEffect(() => {
    if (state === "listening") setErrorMsg("");
  }, [state]);

  const label =
    state === "idle"
      ? lastText
        ? "ready"
        : "press SUPER_SHIFT_D"
      : state === "listening"
        ? "listening"
        : state === "processing"
          ? "working"
          : state === "done"
            ? "pasted"
            : (errorMsg || "couldn't paste").slice(0, 64);

  return (
    <div className={`pill${state === "done" ? " settle" : ""}`} key={settleKey}>
      <span className={`dot ${state === "listening" ? "listening" : state === "processing" ? "processing" : ""}`} />
      <canvas ref={canvasRef} width={180} height={28} />
      <span className={`status${state === "error" ? " error" : ""}`}>
        {state === "done" ? <span className="check">pasted</span> : label}
      </span>
    </div>
  );
}
