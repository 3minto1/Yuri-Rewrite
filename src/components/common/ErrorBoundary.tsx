import { Component, type ErrorInfo, type ReactNode } from "react";
import { invokeCommand } from "../../tauriApi";

type ErrorBoundaryProps = { children: ReactNode };
type ErrorBoundaryState = { error: Error | null };

export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    void invokeCommand("record_frontend_error", {
      message: error.message || "Unknown render error",
      stack: error.stack ?? null,
      componentStack: info.componentStack ?? null
    }).catch(() => {
      console.error("Failed to record frontend error", error, info.componentStack);
    });
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <main className="fatal-error-page" role="alert">
        <section>
          <h1>界面出现异常</h1>
          <p>错误信息已尝试写入本地日志。请重新启动应用；如果问题持续存在，请附上 frontend-errors.log。</p>
          <button onClick={() => window.location.reload()}>重新加载</button>
        </section>
      </main>
    );
  }
}
