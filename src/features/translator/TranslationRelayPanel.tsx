import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Languages, RefreshCw } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useApiAdapter } from "@/lib/useApiAdapter";
import type { TranslationRelayPayload } from "@/types";

const POLL_INTERVAL_MS = 3_000;

function getErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }
  return fallback;
}

function formatUpdatedAt(updatedAt: number): string {
  return new Date(updatedAt).toLocaleString();
}

function StatusMessage({ payload }: { payload: TranslationRelayPayload }) {
  const { t } = useTranslation();
  if (!payload.success) {
    return (
      <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
        {t('translator.failed')}{payload.error ? `: ${payload.error}` : ''}
      </div>
    );
  }

  return (
    <div className="rounded-md border border-green-500/30 bg-green-500/10 px-3 py-2 text-sm text-green-700 dark:text-green-400">
      {t('translator.relayPanel.success')}
    </div>
  );
}

export function TranslationRelayPanel() {
  const api = useApiAdapter();
  const { t } = useTranslation();
  const [text, setText] = useState("");
  const [sourceLang, setSourceLang] = useState("");
  const [targetLang, setTargetLang] = useState("zh");
  const [submitting, setSubmitting] = useState(false);
  const [result, setResult] = useState<TranslationRelayPayload | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function handleSubmit(event: React.FormEvent) {
    event.preventDefault();
    const trimmedText = text.trim();
    if (!trimmedText || submitting) return;

    setSubmitting(true);
    setError(null);
    setResult(null);

    try {
      const payload = await api.translation.translateAndRelay({
        text: trimmedText,
        sourceLang: sourceLang.trim() || undefined,
        targetLang: targetLang.trim() || "zh",
      });
      setResult(payload);
    } catch (err) {
      setError(getErrorMessage(err, t('translator.relayPanel.failure')));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="p-6">
      <div className="mb-6 flex items-center gap-3">
        <Languages className="h-5 w-5 text-primary" />
        <div>
          <h1 className="text-xl font-semibold">{t('translator.relayPanel.title')}</h1>
          <p className="mt-1 text-sm text-muted-foreground">{t('translator.relayPanel.description')}</p>
        </div>
      </div>

      <div className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_minmax(20rem,28rem)]">
        <Card>
          <CardHeader>
            <CardTitle>{t('translator.relayPanel.submitTitle')}</CardTitle>
            <CardDescription>{t('translator.relayPanel.submitDescription')}</CardDescription>
          </CardHeader>
          <CardContent>
            <form onSubmit={handleSubmit} className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="translation-source-text">{t('translator.sourceText')}</Label>
                <textarea
                  id="translation-source-text"
                  value={text}
                  onChange={(event) => setText(event.target.value)}
                  placeholder={t('translator.sourcePlaceholder')}
                  className="min-h-40 w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-sm transition-colors placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
                  disabled={submitting}
                />
              </div>

              <div className="grid gap-4 md:grid-cols-2">
                <div className="space-y-2">
                  <Label htmlFor="translation-source-lang">{t('translator.sourceLang')}</Label>
                  <Input
                    id="translation-source-lang"
                    value={sourceLang}
                    onChange={(event) => setSourceLang(event.target.value)}
                    placeholder={t('translator.autoDetect')}
                    disabled={submitting}
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="translation-target-lang">{t('translator.targetLang')}</Label>
                  <Input
                    id="translation-target-lang"
                    value={targetLang}
                    onChange={(event) => setTargetLang(event.target.value)}
                    placeholder={t("translator.defaultTargetLang")}
                    disabled={submitting}
                  />
                </div>
              </div>

              {error ? (
                <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                  {error}
                </div>
              ) : null}

              <Button type="submit" disabled={!text.trim() || submitting} className="gap-2">
                {submitting ? <RefreshCw className="h-4 w-4 animate-spin" /> : <Languages className="h-4 w-4" />}
                {submitting ? t('translator.translating') : t('translator.translateAndRelay')}
              </Button>
            </form>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>{t('translator.relayPanel.resultTitle')}</CardTitle>
            <CardDescription>{t('translator.relayPanel.resultDescription')}</CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            {result ? (
              <>
                <StatusMessage payload={result} />
                <div className="space-y-2">
                  <div className="text-sm font-medium">{t('translator.translated')}</div>
                  <div className="min-h-24 whitespace-pre-wrap rounded-md bg-muted/50 p-3 text-sm">
                    {result.translatedText || "-"}
                  </div>
                </div>
                <div className="text-xs text-muted-foreground">{t('translator.updatedAt')}: {formatUpdatedAt(result.updatedAt)}</div>
              </>
            ) : (
<div className="flex h-40 items-center justify-center rounded-md border border-dashed text-sm text-muted-foreground">
                {t("translator.notSubmittedYet", { submitTitle: t("translator.relayPanel.submitTitle") })}
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

export function TranslationRelayView() {
  const api = useApiAdapter();
  const { t } = useTranslation();
  const [latest, setLatest] = useState<TranslationRelayPayload | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchLatest = useCallback(async () => {
    try {
      const payload = await api.translation.getLatest();
      setLatest(payload);
      setError(null);
    } catch (err) {
      setError(getErrorMessage(err, t('translator.fetchFailed')));
    } finally {
      setLoading(false);
    }
  }, [api.translation]);

  useEffect(() => {
    fetchLatest();
    const id = window.setInterval(fetchLatest, POLL_INTERVAL_MS);
    return () => window.clearInterval(id);
  }, [fetchLatest]);

  return (
    <div className="p-6">
      <div className="mb-6 flex items-center gap-3">
        <Languages className="h-5 w-5 text-primary" />
        <div>
          <h1 className="text-xl font-semibold">{t('translator.viewPanel.title')}</h1>
          <p className="mt-1 text-sm text-muted-foreground">{t('translator.pollingDescription')}</p>
        </div>
      </div>

      <Card>
        <CardHeader>
          <div className="flex items-start justify-between gap-4">
            <div>
              <CardTitle>{t('translator.viewPanel.latestTitle')}</CardTitle>
              <CardDescription>{t('translator.viewPanel.latestDescription')}</CardDescription>
            </div>
            <Button variant="outline" size="sm" className="gap-2" onClick={fetchLatest} disabled={loading}>
              <RefreshCw className={loading ? "h-4 w-4 animate-spin" : "h-4 w-4"} />
              {t('translator.refresh')}
            </Button>
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          {error ? (
            <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
              {error}
            </div>
          ) : null}

          {!latest && !loading ? (
            <div className="flex h-48 items-center justify-center rounded-md border border-dashed text-sm text-muted-foreground">
              {t('translator.viewPanel.empty')}
            </div>
          ) : null}

          {latest ? (
            <div className="space-y-4">
              <StatusMessage payload={latest} />
              <div className="grid gap-4 lg:grid-cols-2">
                <div className="space-y-2">
                  <div className="text-sm font-medium">{t('translator.original')}</div>
                  <div className="min-h-36 whitespace-pre-wrap rounded-md bg-muted/50 p-3 text-sm">
                    {latest.sourceText || "-"}
                  </div>
                </div>
                <div className="space-y-2">
                  <div className="text-sm font-medium">{t('translator.translated')}</div>
                  <div className="min-h-36 whitespace-pre-wrap rounded-md bg-muted/50 p-3 text-sm">
                    {latest.translatedText || "-"}
                  </div>
                </div>
              </div>
              <div className="flex flex-wrap gap-x-4 gap-y-2 text-xs text-muted-foreground">
                <span>{t('translator.sourceLang')}: {latest.sourceLang || "auto"}</span>
                <span>{t('translator.targetLang')}: {latest.targetLang || "zh"}</span>
                <span>{t('translator.updatedAt')}: {formatUpdatedAt(latest.updatedAt)}</span>
              </div>
            </div>
          ) : null}
        </CardContent>
      </Card>
    </div>
  );
}
