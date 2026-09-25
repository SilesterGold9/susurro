import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import Pill from "./pill";
import SettingsView from "./settings";
import "./styles.css";

function App() {
  const [label, setLabel] = React.useState("pill");
  React.useEffect(() => {
    try {
      setLabel(getCurrentWindow().label);
    } catch {}
  }, []);
  return label === "settings" ? <SettingsView /> : <Pill />;
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
