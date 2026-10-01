import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import SettingsView from "./settings";
import HomePage from "./pages/home";
import InsightsPage from "./pages/insights";
import DictionaryPage from "./pages/dictionary";
import SnippetsPage from "./pages/snippets";
import StylePage from "./pages/style";
import { TransformsPage, ScratchpadPage } from "./pages/placeholders";
import HelpPage from "./pages/help";

export type Page =
  | "home"
  | "insights"
  | "dictionary"
  | "snippets"
  | "style"
  | "transforms"
  | "scratchpad"
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
    <div className={`flow${hc ? " high-contrast" : ""}`}>
      <aside className="flow-side">
        <div className="flow-brand" aria-label="susurro,">
          susurro<span className="comma" aria-hidden="true">,</span>
        </div>
        <nav aria-label="primary">
          {NAV.map((n) => (
            <button
              key={n.id}
              className={`flow-nav${page === n.id ? " active" : ""}`}
              onClick={() => setPage(n.id)}
              aria-current={page === n.id ? "page" : undefined}
            >
              <span className="glyph" aria-hidden="true">
                {n.glyph}
              </span>
              {n.label}
            </button>
          ))}
        </nav>
        <div className="flow-foot">
          {FOOT.map((n) => (
            <button
              key={n.id}
              className={`flow-nav${page === n.id ? " active" : ""}`}
              onClick={() => setPage(n.id)}
              aria-current={page === n.id ? "page" : undefined}
            >
              <span className="glyph" aria-hidden="true">
                {n.glyph}
              </span>
              {n.label}
            </button>
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
        {page === "settings" && (
          <div className="flow-page">
            <h1 className="flow-title">Settings</h1>
            <SettingsView />
          </div>
        )}
        {page === "help" && <HelpPage />}
      </main>
    </div>
  );
}
