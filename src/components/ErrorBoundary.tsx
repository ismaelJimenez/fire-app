import { Component, type ErrorInfo, type ReactNode } from "react";

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

/**
 * Catches render-time errors anywhere below it so a single broken component
 * shows a recoverable message instead of unmounting the whole app.
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    // Surface the details for debugging; a real deployment could ship these
    // to a logging backend.
    console.error("Unhandled render error:", error, info.componentStack);
  }

  handleReset = () => this.setState({ error: null });

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;

    return (
      <div className="error-boundary" role="alert">
        <div className="big">💥</div>
        <h1>Something went wrong</h1>
        <p className="muted">
          {error.message || "An unexpected error occurred."}
        </p>
        <div style={{ display: "flex", gap: 10, justifyContent: "center" }}>
          <button className="btn primary" onClick={this.handleReset}>
            Try again
          </button>
          <button className="btn" onClick={() => window.location.reload()}>
            Reload app
          </button>
        </div>
      </div>
    );
  }
}
