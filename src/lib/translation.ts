/**
 * Initial translation schema surface for the Settings pilot.
 *
 * Goal: provide a minimal, exportable description of the Settings UI so
 * a future web renderer can generate a derivative view without hand-writing
 * an equivalent desktop page.
 *
 * Current assumptions:
 * - Almost everything in Settings is translatable.
 * - Only a few desktop-only capabilities are explicitly marked non-translatable.
 * - This file is intentionally additive and does not modify the existing desktop UI.
 */

import type { AppSettings } from '../types';

export type TranslationVisibility = 'visible' | 'hidden';
export type TranslationControlMode = 'full' | 'readonly' | 'disabled' | 'hidden';
export type TranslationContainerKind = 'form' | 'table' | 'dialog' | 'detail';

export interface TranslationFieldSchema {
  key: keyof AppSettings;
  labelKey: string;
  control: 'switch' | 'number' | 'text' | 'select' | 'slider';
  translationMode: TranslationControlMode;
  visibility: TranslationVisibility;
  desktopOnly?: boolean;
  note?: string;
}

export interface TranslationSectionSchema {
  id: string;
  titleKey: string;
  fields: TranslationFieldSchema[];
  visibility: TranslationVisibility;
}

export interface TranslationPageSchema {
  id: string;
  titleKey: string;
  sections: TranslationSectionSchema[];
  actions: {
    id: string;
    labelKey: string;
    translationMode: TranslationControlMode;
  }[];
}

export const SETTINGS_TRANSLATION_SCHEMA: TranslationPageSchema = {
  id: 'settings',
  titleKey: 'settings.title',
  sections: [
    {
      id: 'proxy',
      titleKey: 'settings.proxy.title',
      visibility: 'visible',
      fields: [
        {
          key: 'listen_port',
          labelKey: 'settings.proxy.port',
          control: 'number',
          translationMode: 'full',
          visibility: 'visible',
        },
        {
          key: 'proxy_enabled',
          labelKey: 'settings.proxy.enabled',
          control: 'switch',
          translationMode: 'full',
          visibility: 'visible',
        },
      ],
    },
    {
      id: 'security',
      titleKey: 'settings.security.title',
      visibility: 'visible',
      fields: [
        {
          key: 'access_key_required',
          labelKey: 'settings.security.forceKey',
          control: 'switch',
          translationMode: 'full',
          visibility: 'visible',
        },
      ],
    },
    {
      id: 'circuit',
      titleKey: 'settings.circuit.title',
      visibility: 'visible',
      fields: [
        {
          key: 'circuit_failure_threshold',
          labelKey: 'settings.circuit.threshold',
          control: 'number',
          translationMode: 'full',
          visibility: 'visible',
        },
        {
          key: 'proxy_connect_timeout_secs',
          labelKey: 'settings.circuit.connectTimeout',
          control: 'number',
          translationMode: 'full',
          visibility: 'visible',
        },
        {
          key: 'circuit_recovery_secs',
          labelKey: 'settings.circuit.recovery',
          control: 'slider',
          translationMode: 'full',
          visibility: 'visible',
        },
        {
          key: 'circuit_disable_codes',
          labelKey: 'settings.circuit.disableCodes',
          control: 'text',
          translationMode: 'full',
          visibility: 'visible',
        },
      ],
    },
    {
      id: 'general',
      titleKey: 'settings.general.title',
      visibility: 'visible',
      fields: [
        {
          key: 'locale',
          labelKey: 'settings.general.language',
          control: 'select',
          translationMode: 'full',
          visibility: 'visible',
        },
        {
          key: 'theme',
          labelKey: 'settings.general.theme',
          control: 'select',
          translationMode: 'full',
          visibility: 'visible',
        },
        {
          key: 'active_group',
          labelKey: 'settings.general.defaultGroup',
          control: 'select',
          translationMode: 'full',
          visibility: 'visible',
        },
        {
          key: 'autostart',
          labelKey: 'settings.tray.autostart',
          control: 'switch',
          translationMode: 'hidden',
          visibility: 'hidden',
          desktopOnly: true,
          note: 'Desktop-only capability; explicitly excluded from web translation.',
        },
        {
          key: 'start_minimized',
          labelKey: 'settings.tray.startMinimized',
          control: 'switch',
          translationMode: 'hidden',
          visibility: 'hidden',
          desktopOnly: true,
          note: 'Desktop-only capability; explicitly excluded from web translation.',
        },
      ],
    },
  ],
  actions: [
    {
      id: 'saveSettings',
      labelKey: 'settings.actions.save',
      translationMode: 'full',
    },
    {
      id: 'toggleProxy',
      labelKey: 'settings.actions.toggleProxy',
      translationMode: 'full',
    },
  ],
};

export function listNonTranslatableSettingsKeys() {
  return SETTINGS_TRANSLATION_SCHEMA.sections
    .flatMap((section) => section.fields)
    .filter((field) => field.translationMode !== 'full' || field.desktopOnly);
}

export function listVisibleWebSettingsKeys() {
  return SETTINGS_TRANSLATION_SCHEMA.sections
    .flatMap((section) => section.fields)
    .filter((field) => field.visibility === 'visible' && field.translationMode === 'full');
}

export interface TranslationDataFieldSchema {
  id: string;
  labelKey: string;
  translationMode: TranslationControlMode;
  note?: string;
}

export interface TranslationDataPageSchema {
  id: string;
  titleKey: string;
  columns: TranslationDataFieldSchema[];
  actions: {
    id: string;
    labelKey: string;
    translationMode: TranslationControlMode;
  }[];
}

export const CHANNELS_TRANSLATION_SCHEMA: TranslationDataPageSchema = {
  id: 'channels',
  titleKey: 'channels.title',
  columns: [
    {
      id: 'name',
      labelKey: 'channels.fields.name',
      translationMode: 'full',
    },
    {
      id: 'api_type',
      labelKey: 'channels.fields.apiType',
      translationMode: 'full',
    },
    {
      id: 'base_url',
      labelKey: 'channels.fields.baseUrl',
      translationMode: 'readonly',
      note: 'User/runtime data should display as-is; translate the label only.',
    },
    {
      id: 'notes',
      labelKey: 'channels.fields.notes',
      translationMode: 'readonly',
      note: 'User/runtime data should display as-is; translate the label only.',
    },
    {
      id: 'enabled',
      labelKey: 'channels.fields.enabled',
      translationMode: 'full',
    },
  ],
  actions: [
    {
      id: 'addChannel',
      labelKey: 'channels.actions.add',
      translationMode: 'full',
    },
    {
      id: 'editChannel',
      labelKey: 'channels.actions.edit',
      translationMode: 'full',
    },
    {
      id: 'deleteChannel',
      labelKey: 'channels.actions.delete',
      translationMode: 'full',
    },
    {
      id: 'fetchModels',
      labelKey: 'channels.actions.fetchModels',
      translationMode: 'full',
    },
    {
      id: 'selectModels',
      labelKey: 'channels.actions.selectModels',
      translationMode: 'full',
    },
    {
      id: 'speedTest',
      labelKey: 'channels.actions.speedTest',
      translationMode: 'full',
    },
  ],
};

export function listNonTranslatableChannelFields() {
  return CHANNELS_TRANSLATION_SCHEMA.columns.filter(
    (column) => column.translationMode !== 'full',
  );
}

// ============================================================================
// LOGS PAGE SCHEMA
// ============================================================================

export const LOGS_TRANSLATION_SCHEMA: TranslationDataPageSchema = {
  id: 'logs',
  titleKey: 'log.title',
  columns: [
    {
      id: 'time',
      labelKey: 'log.time',
      translationMode: 'full',
    },
    {
      id: 'channel',
      labelKey: 'log.channel',
      translationMode: 'full',
    },
    {
      id: 'token',
      labelKey: 'log.token',
      translationMode: 'full',
    },
    {
      id: 'model',
      labelKey: 'log.model',
      translationMode: 'full',
    },
    {
      id: 'duration',
      labelKey: 'log.duration',
      translationMode: 'full',
    },
    {
      id: 'promptTokens',
      labelKey: 'log.promptTokens',
      translationMode: 'full',
    },
    {
      id: 'completionTokens',
      labelKey: 'log.completionTokens',
      translationMode: 'full',
    },
    {
      id: 'status',
      labelKey: 'log.status',
      translationMode: 'full',
    },
    {
      id: 'requestedModel',
      labelKey: 'log.requestedModel',
      translationMode: 'readonly',
      note: 'Runtime data from request metadata; translate the label only.',
    },
    {
      id: 'resolvedModel',
      labelKey: 'log.resolvedModel',
      translationMode: 'readonly',
      note: 'Runtime data from request metadata; translate the label only.',
    },
    {
      id: 'attemptPath',
      labelKey: 'log.attemptPath',
      translationMode: 'readonly',
      note: 'Runtime data showing failover path; translate the label only.',
    },
    {
      id: 'streamEndReason',
      labelKey: 'log.streamEndReason',
      translationMode: 'readonly',
      note: 'Runtime data from stream completion; translate the label only.',
    },
    {
      id: 'details',
      labelKey: 'log.details',
      translationMode: 'readonly',
      note: 'User/runtime content (request/response); translate the label only.',
    },
    {
      id: 'error',
      labelKey: 'log.error',
      translationMode: 'readonly',
      note: 'Error messages from upstream; translate the label only.',
    },
  ],
  actions: [
    {
      id: 'toggleErrorsOnly',
      labelKey: 'log.failed',
      translationMode: 'full',
    },
  ],
};

export function listNonTranslatableLogFields() {
  return LOGS_TRANSLATION_SCHEMA.columns.filter(
    (column) => column.translationMode !== 'full',
  );
}

// ============================================================================
// DASHBOARD PAGE SCHEMA
// ============================================================================

export const DASHBOARD_TRANSLATION_SCHEMA: TranslationDataPageSchema = {
  id: 'dashboard',
  titleKey: 'dashboard.title',
  columns: [
    // Stat card labels
    {
      id: 'todayRequests',
      labelKey: 'dashboard.cards.todayRequests',
      translationMode: 'full',
    },
    {
      id: 'todayTokens',
      labelKey: 'dashboard.cards.todayTokens',
      translationMode: 'full',
    },
    {
      id: 'todayPromptTokens',
      labelKey: 'dashboard.cards.todayPrompt',
      translationMode: 'full',
    },
    {
      id: 'todayCompletionTokens',
      labelKey: 'dashboard.cards.todayCompletion',
      translationMode: 'full',
    },
    {
      id: 'totalRequests',
      labelKey: 'dashboard.cards.total',
      translationMode: 'full',
    },
    // Chart tab labels
    {
      id: 'consumptionChart',
      labelKey: 'dashboard.charts.consumption',
      translationMode: 'full',
    },
    {
      id: 'callTrendChart',
      labelKey: 'dashboard.charts.callTrend',
      translationMode: 'full',
    },
    {
      id: 'distributionChart',
      labelKey: 'dashboard.charts.distribution',
      translationMode: 'full',
    },
    {
      id: 'userTrendChart',
      labelKey: 'dashboard.charts.userTrend',
      translationMode: 'full',
    },
    // Filter labels
    {
      id: 'granularityHour',
      labelKey: 'dashboard.filter.hour',
      translationMode: 'full',
    },
    {
      id: 'granularityDay',
      labelKey: 'dashboard.filter.day',
      translationMode: 'full',
    },
  ],
  actions: [
    {
      id: 'setTimeRangeToday',
      labelKey: 'dashboard.filter.today',
      translationMode: 'full',
    },
    {
      id: 'setTimeRange7d',
      labelKey: 'dashboard.filter.sevenDays',
      translationMode: 'full',
    },
    {
      id: 'setTimeRange30d',
      labelKey: 'dashboard.filter.thirtyDays',
      translationMode: 'full',
    },
    {
      id: 'toggleGranularity',
      labelKey: 'dashboard.charts.consumption',
      translationMode: 'full',
    },
  ],
};

export function listNonTranslatableDashboardFields() {
  return DASHBOARD_TRANSLATION_SCHEMA.columns.filter(
    (column) => column.translationMode !== 'full',
  );
}
