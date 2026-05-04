import { BarChart3, BookOpen, ExternalLink, FileText, KeyRound, Layers, Power, Route, Settings } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Separator } from '@/components/ui/separator';
import { Toaster } from 'sonner';
import { cn } from '@/lib/utils';
import type { AppSettings, ProxyStatus } from '@/types';

export type MainPage = 'apiPool' | 'channels' | 'tokens' | 'logs' | 'dashboard' | 'settings' | 'guide';

const NAV_ITEMS: { key: MainPage; icon: typeof Layers; labelKey: string; externalLang?: Record<string, string> }[] = [
  { key: 'apiPool', icon: Layers, labelKey: 'nav.apiPool' },
  { key: 'channels', icon: Route, labelKey: 'nav.channels' },
  { key: 'tokens', icon: KeyRound, labelKey: 'nav.tokens' },
  { key: 'logs', icon: FileText, labelKey: 'nav.logs' },
  { key: 'dashboard', icon: BarChart3, labelKey: 'nav.dashboard' },
  { key: 'settings', icon: Settings, labelKey: 'nav.settings' },
  { key: 'guide', icon: BookOpen, labelKey: 'nav.guide', externalLang: { zh: 'GUIDE_CN.md', en: 'GUIDE.md' } },
];

export interface MainShellProps {
  currentPage: MainPage;
  proxyStatus?: ProxyStatus | null;
  settings?: AppSettings | null;
  updateInfo?: { current: string; latest: string; url: string } | null;
  onUpdateDismiss?: () => void;
  onUpdateOpen?: (url: string) => void;
  onNavigate: (page: MainPage) => void;
  onOpenGuide?: (path: string) => void;
  renderPage: () => React.ReactNode;
  children?: React.ReactNode;
}

export function MainShell({
  currentPage,
  proxyStatus,
  updateInfo,
  onUpdateDismiss,
  onUpdateOpen,
  onNavigate,
  onOpenGuide,
  renderPage,
  children,
}: MainShellProps) {
  const { t, i18n } = useTranslation();

  return (
    <div className="flex h-screen flex-col bg-background">
      {updateInfo && (
        <div className="flex shrink-0 items-center justify-center gap-2 bg-primary/10 px-3 py-1.5 text-xs text-primary">
          <span>{t('update.newVersion', { version: updateInfo.latest })}</span>
          <button
            type="button"
            onClick={() => onUpdateOpen?.(updateInfo.url)}
            className="inline-flex items-center gap-1 font-medium underline-offset-2 hover:underline"
          >
            {t('update.goDownload')}
            <ExternalLink className="h-3 w-3" />
          </button>
          <button onClick={onUpdateDismiss} className="ml-1 opacity-60 hover:opacity-100">
            ✕
          </button>
        </div>
      )}
      <div className="flex min-h-0 flex-1">
        <aside className="flex w-56 flex-col border-r border-sidebar-border bg-sidebar-background">
          <div className="flex items-center gap-2 px-4 py-4">
            <Power className={cn('h-5 w-5', proxyStatus?.running ? 'text-green-500' : 'text-red-500')} />
            <span className="text-lg font-semibold">
              {proxyStatus?.running ? `API Switch: ${proxyStatus.port}` : 'API Switch'}
            </span>
          </div>

          <Separator />

          <ScrollArea className="flex-1 px-2 py-2">
            <nav className="flex flex-col gap-1">
              {NAV_ITEMS.map(({ key, icon: Icon, labelKey, externalLang }) => (
                <Button
                  key={key}
                  variant={currentPage === key ? 'secondary' : 'ghost'}
                  className={cn(
                    'justify-start gap-2 px-3',
                    currentPage === key && 'bg-sidebar-accent text-sidebar-accent-foreground',
                  )}
                  onClick={() => {
                    if (externalLang) {
                      const lang = i18n.language.startsWith('zh') ? 'zh' : 'en';
                      onOpenGuide?.(externalLang[lang]);
                    } else {
                      onNavigate(key);
                    }
                  }}
                >
                  <Icon className="h-4 w-4" />
                  {t(labelKey)}
                </Button>
              ))}
            </nav>
          </ScrollArea>

          <div className="flex justify-center pb-4">
            <a href="https://github.com/wang1970/API-Switch" target="_blank" rel="noopener noreferrer">
              <img src="/star.jpg" alt="Star on GitHub" className="cursor-pointer transition-opacity hover:opacity-80" />
            </a>
          </div>
        </aside>

        <main className="flex-1 overflow-auto">{renderPage()}</main>
        {children}
      </div>

      <Toaster position="top-center" richColors closeButton />
    </div>
  );
}
