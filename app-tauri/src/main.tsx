import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import Pill from "./pill";
import SettingsView from "./settings";
import OnboardingView from "./onboarding";
import "./styles.css";

function App() {
  const [label, setLabel] = React.useState("pill");
  React.useEffect(() => {
    try {
      setLabel(getCurrentWindow().label);
    } catch {}
  }, []);
  if (label === "settings") return <SettingsView />;
  if (label === "onboarding") return <OnboardingView />;
  return <Pill />;
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
