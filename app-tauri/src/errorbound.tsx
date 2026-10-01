import React from "react";

interface State {
  error: string | null;
}

export class ErrorBoundary extends React.Component<
  { label: string; children: React.ReactNode },
  State
> {
  state: State = { error: null };

  static getDerivedStateFromError(e: unknown): State {
    return {
      error: e instanceof Error ? `${e.name}: ${e.message}` : String(e),
    };
  }

  render() {
    if (this.state.error) {
      return (
        <div
          style={{
            background: "#f6f4ed",
            color: "#1b1a17",
            minHeight: "100vh",
            padding: 28,
            fontFamily: "monospace",
            fontSize: 13,
          }}
        >
          <div>window: {this.props.label}</div>
          <div>susurro hit a render error:</div>
          <pre style={{ whiteSpace: "pre-wrap" }}>{this.state.error}</pre>
        </div>
      );
    }
    return this.props.children;
  }
}
