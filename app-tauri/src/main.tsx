import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import Pill from "./pill";
import OnboardingView from "./onboarding";
import Shell from "./shell";
import "./styles.css";

function App() {
  const [label, setLabel] = React.useState("pill");
  React.useEffect(() => {
    try {
      setLabel(getCurrentWindow().label);
    } catch {}
  }, []);
  if (label === "pill") return <Pill />;
  if (label === "onboarding") return <OnboardingView />;
  return <Shell />;
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
