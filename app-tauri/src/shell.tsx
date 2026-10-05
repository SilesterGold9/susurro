import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import SettingsView from "./settings";
import HomePage from "./pages/home";
import InsightsPage from "./pages/insights";
import DictionaryPage from "./pages/dictionary";
import SnippetsPage from "./pages/snippets";
import StylePage from "./pages/style";
import { TransformsPage, ScratchpadPage } from "./pages/placeholders";
import SystemPage from "./pages/system";
import HelpPage from "./pages/help";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";

export type Page =
  | "home"
  | "insights"
  | "dictionary"
  | "snippets"
  | "style"
  | "transforms"
  | "scratchpad"
  | "system"
  | "settings"
  | "help";

const NAV: { id: Page; label: string; glyph: string }[] = [
  { id: "home", label: "Home", glyph: "⌂" },
  { id: "insights", label: "Insights", glyph: "◔" },
  { id: "dictionary", label: "Dictionary", glyph: "▤" },
  { id: "snippets", label: "Snippets", glyph: "✂" },
  { id: "style", label: "Style", glyph: "Tt" },
  { id: "transforms", label: "Transforms", glyph: "⇄" },
  { id: "scratchpad", label: "Scratchpad", glyph: "✎" },
  { id: "system", label: "System", glyph: "◉" },
];

const FOOT: { id: Page; label: string; glyph: string }[] = [
  { id: "settings", label: "Settings", glyph: "⚙" },
  { id: "help", label: "Help", glyph: "?" },
];

export default function Shell() {
  const [page, setPage] = useState<Page>("home");
  const [hc, setHc] = useState(false);
  useEffect(() => {
    invoke<{ high_contrast: boolean }>("get_settings")
      .then((s) => setHc(!!s?.high_contrast))
      .catch(() => {});
  }, []);
  return (
    <TooltipProvider delayDuration={300}>
    <div className={`flow${hc ? " high-contrast" : ""}`}>
      <aside className="flow-side">
        <div className="flow-brand" aria-label="susurro,">
          <span className="word">susurro</span>
          <span className="comma" aria-hidden="true">,</span>
        </div>
        <nav aria-label="primary">
          {NAV.map((n) => (
            <Tooltip key={n.id}>
              <TooltipTrigger asChild>
                <button
                  className={`flow-nav${page === n.id ? " active" : ""}`}
                  onClick={() => setPage(n.id)}
                  aria-current={page === n.id ? "page" : undefined}
                  aria-label={n.label}
                >
                  <span className="glyph" aria-hidden="true">
                    {n.glyph}
                  </span>
                  <span className="label">{n.label}</span>
                </button>
              </TooltipTrigger>
              <TooltipContent side="right">{n.label}</TooltipContent>
            </Tooltip>
          ))}
        </nav>
        <div className="flow-foot">
          {FOOT.map((n) => (
            <Tooltip key={n.id}>
              <TooltipTrigger asChild>
                <button
                  className={`flow-nav${page === n.id ? " active" : ""}`}
                  onClick={() => setPage(n.id)}
                  aria-current={page === n.id ? "page" : undefined}
                  aria-label={n.label}
                >
                  <span className="glyph" aria-hidden="true">
                    {n.glyph}
                  </span>
                  <span className="label">{n.label}</span>
                </button>
              </TooltipTrigger>
              <TooltipContent side="right">{n.label}</TooltipContent>
            </Tooltip>
          ))}
        </div>
      </aside>
      <main className="flow-main">
        {page === "home" && <HomePage go={setPage} />}
        {page === "insights" && <InsightsPage />}
        {page === "dictionary" && <DictionaryPage />}
        {page === "snippets" && <SnippetsPage />}
        {page === "style" && <StylePage />}
        {page === "transforms" && <TransformsPage />}
        {page === "scratchpad" && <ScratchpadPage />}
        {page === "system" && <SystemPage />}
        {page === "settings" && (
          <div className="flow-page">
            <h1 className="flow-title">Settings</h1>
            <SettingsView />
          </div>
        )}
        {page === "help" && <HelpPage />}
      </main>
    </div>
    </TooltipProvider>
  );
}
