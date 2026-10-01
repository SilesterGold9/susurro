import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import Pill from "./pill";
import OnboardingView from "./onboarding";
import Shell from "./shell";
import { ErrorBoundary } from "./errorbound";
import "./styles.css";

function App() {
  const [label, setLabel] = React.useState("pill");
  React.useEffect(() => {
    try {
      const w = getCurrentWindow();
      setLabel(w.label);
    } catch (e) {
      console.error("window label unreadable, falling back to pill", e);
    }
  }, []);
  if (label === "pill")
    return (
      <ErrorBoundary label={label}>
        <Pill />
      </ErrorBoundary>
    );
  if (label === "onboarding")
    return (
      <ErrorBoundary label={label}>
        <OnboardingView />
      </ErrorBoundary>
    );
  return (
    <ErrorBoundary label={label}>
      <Shell />
    </ErrorBoundary>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
