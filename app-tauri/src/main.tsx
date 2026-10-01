import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import Pill from "./pill";
import OnboardingView from "./onboarding";
import Shell from "./shell";
import { ErrorBoundary } from "./errorbound";
import "./styles.css";

function route(): string {
  // Hash first: it needs no API call, so the branch is right even
  // when the window metadata cannot be read.
  const hash = window.location.hash.replace(/^#\/?/, "");
  if (hash === "pill" || hash === "onboarding" || hash === "app") {
    return hash;
  }
  try {
    const label = getCurrentWindow().label;
    if (label === "pill" || label === "onboarding") return label;
  } catch (e) {
    console.error("window identity unreadable, showing app", e);
  }
  // Unknown windows show the app, never the pill: a wrong shell is
  // debuggable, a wrong pill is a dead app.
  return "app";
}

function App() {
  const [routeName] = React.useState(route);
  if (routeName === "pill")
    return (
      <ErrorBoundary label={routeName}>
        <Pill />
      </ErrorBoundary>
    );
  if (routeName === "onboarding")
    return (
      <ErrorBoundary label={routeName}>
        <OnboardingView />
      </ErrorBoundary>
    );
  return (
    <ErrorBoundary label={routeName}>
      <Shell />
    </ErrorBoundary>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
