import { Component, type ReactNode } from "react";
import { RotateCcw, TriangleAlert } from "lucide-react";

/** Keeps one broken view from blanking the whole app, and offers a recovery path. */
export default class ErrorBoundary extends Component<{ children: ReactNode; resetKey?: string }, { error: Error | null }> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidUpdate(prev: { resetKey?: string }) {
    if (prev.resetKey !== this.props.resetKey && this.state.error) this.setState({ error: null });
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="error-screen card">
        <div className="card-head">
          <div className="card-icon"><TriangleAlert size={17} /></div>
          <div className="grow">
            <h2>This view hit a problem</h2>
            <p>Your data is safe. You can retry, or switch to another page.</p>
          </div>
        </div>
        <pre className="code-block mono">{String(this.state.error.message || this.state.error)}</pre>
        <div className="row end" style={{ marginTop: 14 }}>
          <button className="btn primary" onClick={() => this.setState({ error: null })}><RotateCcw size={15} /> Try again</button>
        </div>
      </div>
    );
  }
}
