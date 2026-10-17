import { Component, type ErrorInfo, type ReactNode } from "react";
import { withTranslation, type WithTranslation } from "react-i18next";

type Props = WithTranslation & { children: ReactNode };
type State = { error: Error | null };

// Catches a render crash in any view so the app shows an error and keeps the
// nav alive instead of going white. Reset when the user switches views.
class ErrorBoundaryInner extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("sigrun: view crashed", error, info);
  }

  render() {
    if (this.state.error) {
      const { t } = this.props;
      return (
        <div role="alert" className="flex flex-col gap-2 rounded border border-danger p-4">
          <p className="font-semibold text-danger">{t("common.error")}</p>
          <p className="break-all font-mono text-xs text-muted">{this.state.error.message}</p>
          <button
            className="self-start rounded border border-border px-2 py-1"
            onClick={() => this.setState({ error: null })}
          >
            {t("common.close")}
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

export default withTranslation()(ErrorBoundaryInner);
