import React, { type ComponentType, type ErrorInfo } from "react";
import { AlertTriangle, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useTranslation } from "react-i18next";

interface ErrorBoundaryProps {
  children: React.ReactNode;
  fallback?: React.ReactNode;
}

interface ErrorBoundaryState {
  hasError: boolean;
  error: Error | null;
  errorInfo: ErrorInfo | null;
}

export class ErrorBoundary extends React.Component<ErrorBoundaryProps, ErrorBoundaryState> {
  constructor(props: ErrorBoundaryProps) {
    super(props);
    this.state = { hasError: false, error: null, errorInfo: null };
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { hasError: true, error, errorInfo: null };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo): void {
    console.error("ErrorBoundary caught an error:", error, errorInfo);
  }

  handleTryAgain = (): void => {
    this.setState({ hasError: false, error: null, errorInfo: null });
  };

  render(): React.ReactNode {
    if (this.state.hasError && this.state.error) {
      if (this.props.fallback) {
        return this.props.fallback;
      }

      return (
        <div className="flex min-h-[400px] flex-col items-center justify-center gap-4 rounded-lg border bg-background p-6 text-center">
          <div className="flex h-12 w-12 items-center justify-center rounded-full bg-destructive/10 text-destructive">
            <AlertTriangle className="h-6 w-6" />
          </div>
          <div className="space-y-2">
            <h2 className="text-lg font-semibold">Something went wrong</h2>
            <p className="max-w-md text-sm text-muted-foreground">
              An error occurred while rendering this page. You can try to reload it.
            </p>
          </div>
          <div className="max-w-md overflow-auto rounded border bg-muted p-3 text-left text-xs text-muted-foreground">
            <pre className="whitespace-pre-wrap">{this.state.error.message}</pre>
          </div>
          <Button onClick={this.handleTryAgain} className="gap-2">
            <RefreshCw className="h-4 w-4" />
            Try Again
          </Button>
        </div>
      );
    }

    return this.props.children;
  }
}

// Helper component for the fallback UI (can be used with Suspense)
export function ErrorFallback({ error, resetErrorBoundary }: { error: Error | null; resetErrorBoundary: () => void }) {
  const { t } = useTranslation();

  return (
    <div className="flex min-h-[400px] flex-col items-center justify-center gap-4 rounded-lg border bg-background p-6 text-center">
      <div className="flex h-12 w-12 items-center justify-center rounded-full bg-destructive/10 text-destructive">
        <AlertTriangle className="h-6 w-6" />
      </div>
      <div className="space-y-2">
        <h2 className="text-lg font-semibold">{t("error.somethingWentWrong", "Something went wrong")}</h2>
        <p className="max-w-md text-sm text-muted-foreground">
          {t("error.tryReload", "An error occurred while rendering this page. You can try to reload it.")}
        </p>
      </div>
      {error && (
        <div className="max-w-md overflow-auto rounded border bg-muted p-3 text-left text-xs text-muted-foreground">
          <pre className="whitespace-pre-wrap">{error.message}</pre>
        </div>
      )}
      <Button onClick={resetErrorBoundary} className="gap-2">
        <RefreshCw className="h-4 w-4" />
        {t("common.tryAgain", "Try Again")}
      </Button>
    </div>
  );
}
