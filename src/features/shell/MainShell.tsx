import { BarChart3, BookOpen, ExternalLink, FileText, KeyRound, Layers, LogOut, Power, Route, Settings } from 'lucide-react';

import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Separator } from '@/components/ui/separator';
import { Toaster } from 'sonner';
import { cn } from '@/lib/utils';
import type { AdminStatus, AppSettings, ProxyStatus } from '@/types';

export type MainPage = 'apiPool' | 'channels' | 'tokens' | 'logs' | 'dashboard' | 'translator' | 'settings' | 'guide';

const NAV_ITEMS: { key: MainPage; icon: typeof Layers; labelKey: string; externalLang?: Record<string, string> }[] = [
  { key: 'apiPool', icon: Layers, labelKey: 'nav.apiPool' },
  { key: 'channels', icon: Route, labelKey: 'nav.channels' },
  { key: 'tokens', icon: KeyRound, labelKey: 'nav.tokens' },
  { key: 'logs', icon: FileText, labelKey: 'nav.logs' },
  { key: 'dashboard', icon: BarChart3, labelKey: 'nav.dashboard' },
    { key: 'settings', icon: Settings, labelKey: 'nav.settings' },
  ];

const starImageSrc = `${import.meta.env.BASE_URL}star.jpg`;

export interface MainShellProps {
  currentPage: MainPage;
  proxyStatus?: ProxyStatus | null;
  adminStatus?: AdminStatus | null;
  settings?: AppSettings | null;
  updateInfo?: { current: string; latest: string; url: string } | null;
  onUpdateDismiss?: () => void;
  onUpdateOpen?: (url: string) => void;
  onNavigate: (page: MainPage) => void;
  onOpenGuide?: (path: string) => void;
  onLogout?: () => void;
  renderPage: () => React.ReactNode;
  children?: React.ReactNode;
}

export function MainShell({
  currentPage,
  proxyStatus,
  adminStatus,
  settings,
  updateInfo,
  onUpdateDismiss,
  onUpdateOpen,
  onNavigate,
  onOpenGuide,
  onLogout,
  renderPage,
  children,
}: MainShellProps) {
  const { t, i18n } = useTranslation();

  return (
    <div className="flex h-screen flex-col bg-background overflow-y-scroll">
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
              {NAV_ITEMS.map(({ key, icon: Icon, labelKey }) => (
                <Button
                  key={key}
                  variant={currentPage === key ? 'secondary' : 'ghost'}
                  className={cn(
                    'justify-start gap-2 px-3',
                    currentPage === key && 'bg-sidebar-accent text-sidebar-accent-foreground',
                  )}
                  onClick={() => onNavigate(key)}
                >
                  <Icon className="h-4 w-4" />
                  {t(labelKey)}
                </Button>
              ))}
              <Separator className="my-1" />
              <Button
                variant="ghost"
                className="w-full justify-start gap-2 px-3"
                onClick={() => {
                  const lang = i18n.language.startsWith('zh') ? 'zh' : 'en';
                  const guidePath = lang === 'zh' ? 'GUIDE_CN.md' : 'GUIDE.md';
                  onOpenGuide?.(guidePath);
                }}
              >
                <BookOpen className="h-4 w-4" />
                {t('nav.guide', '使用指南')}
              </Button>
              {onLogout && (
                <>
                  <Separator className="my-1" />
                  <Button variant="ghost" className="w-full justify-start gap-2 px-3 text-red-500 hover:text-red-500" onClick={onLogout}>
                    <LogOut className="h-4 w-4" />
                    {t('nav.logout', '退出登录')}
                  </Button>
                </>
              )}
            </nav>
          </ScrollArea>

          <div className="px-2 pb-4">
            <div className="flex justify-center">
              <a href="https://github.com/wang1970/API-Switch" target="_blank" rel="noopener noreferrer">
                <img src={starImageSrc} alt="Star on GitHub" className="cursor-pointer transition-opacity hover:opacity-80" />
              </a>
            </div>
            <div className="mt-2 flex items-center justify-center gap-2 text-xs text-muted-foreground">
              <span className={cn('inline-block h-4 w-4 rounded-full', proxyStatus?.running ? 'bg-green-500' : 'bg-red-500')} />
              <span className={cn('inline-block h-4 w-4 rounded-full', adminStatus?.running ? 'bg-green-500' : 'bg-red-500')} />
              <span>版本号：{settings?.app_version || '0.0.0'}</span>
            </div>
          </div>
        </aside>

        <main className="flex-1 overflow-auto">{renderPage()}</main>
        {children}
      </div>

      <Toaster position="top-center" richColors closeButton />
    </div>
  );
}
