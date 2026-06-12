import {
  RiErrorWarningLine,
  RiPencilLine,
  RiPulseLine,
  RiWechatLine,
  RiWebhookLine,
} from '@remixicon/react';
import type { InterpolationValues, MessageKey } from '../../i18n';
import type { IntegrationEntry, IntegrationKind } from '../../admin-types';
import { Button } from '../../components/ui/button';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

export type IntegrationCardProps = {
  entry: IntegrationEntry;
  active?: boolean;
  onEdit: (kind: IntegrationKind) => void;
  t: Translator;
};

type IntegrationIcon = typeof RiErrorWarningLine;

const KIND_ICONS: Record<IntegrationKind, IntegrationIcon> = {
  sentry: RiErrorWarningLine,
  webhooks: RiWebhookLine,
  wecom: RiWechatLine,
  otlp: RiPulseLine,
};

type ConfigPreviewRow = {
  key: string;
  label: string;
  value: string;
  muted?: boolean;
};

function statusLocaleKey(status: string): MessageKey {
  switch (status) {
    case 'active': return 'integrations.status.active';
    case 'inactive': return 'integrations.status.inactive';
    case 'pending_restart': return 'integrations.status.pendingRestart';
    default: return 'integrations.status.inactive';
  }
}

function sentryStatusTone(status: string): string {
  switch (status) {
    case 'active': return 'badge-ok';
    case 'pending_restart': return 'badge-warn';
    default: return 'badge-muted';
  }
}

/// A list row representing one integration (Sentry, Webhooks, or OTLP).
///
/// Shows the integration icon, name, description, status badge, and an edit
/// button. For inactive integrations the edit button opens an empty-state form.
export function IntegrationCard({ entry, active = false, onEdit, t }: IntegrationCardProps) {
  const Icon = KIND_ICONS[entry.kind] ?? RiPulseLine;
  const configRows = getConfigRows(entry, t);

  const nameKey = `integrations.card.${entry.kind}Name` as MessageKey;
  const descKey = `integrations.card.${entry.kind}Desc` as MessageKey;

  return (
    <article
      className={`integration-card${entry.status === 'pending_restart' ? ' pending-restart' : ''}${active ? ' is-editing' : ''}`}
      data-kind={entry.kind}
      data-status={entry.status}
      data-editing={active ? true : undefined}
    >
      <div className="integration-card-body">
        <div className="integration-card-head">
          <span className="integration-card-icon-wrap" aria-hidden="true">
            <Icon className="integration-card-icon" />
          </span>
          <div>
            <h3 className="integration-card-name">{t(nameKey)}</h3>
            <span className={`badge ${sentryStatusTone(entry.status)}`}>
              {t(statusLocaleKey(entry.status))}
            </span>
          </div>
        </div>

        <p className="integration-card-desc">{t(descKey)}</p>

        <div className="integration-config-preview">
          {configRows.map((row) => (
            <span className="integration-config-field" key={row.key}>
              <strong>{row.label}</strong>
              <code className={`integration-config-value${row.muted ? ' muted' : ''}`}>
                {row.value}
              </code>
            </span>
          ))}
        </div>

        <div className="integration-card-actions">
          <Button
            className="integration-action-button"
            variant={active ? 'secondary' : 'outline'}
            size="sm"
            type="button"
            aria-expanded={active}
            onClick={() => onEdit(entry.kind)}
          >
            <RiPencilLine data-icon="inline-start" aria-hidden="true" />
            {t('integrations.action.edit')}
          </Button>
        </div>
      </div>
    </article>
  );
}

function getConfigRows(entry: IntegrationEntry, t: Translator): ConfigPreviewRow[] {
  const notSet = t('integrations.label.notSet');
  if (entry.kind === 'sentry') {
    const environment = configValue(entry.config.environment, notSet);
    if (entry.status === 'inactive') {
      return [{
        key: 'dsn',
        label: t('integrations.field.dsn'),
        value: notSet,
        muted: true,
      }];
    }
    return [
      {
        key: 'dsn',
        label: t('integrations.field.dsn'),
        value: maskDsn(String(entry.config.dsn ?? '')),
      },
      {
        key: 'environment',
        label: t('integrations.field.environment'),
        value: environment.value,
        muted: environment.muted,
      },
    ];
  }

  if (entry.kind === 'webhooks') {
    const path = configValue(entry.config.config_path, defaultConfigPathForKind('webhooks', t));
    const count = entry.config.webhook_count;
    return [
      {
        key: 'config_path',
        label: t('integrations.field.configPath'),
        value: path.value,
        muted: path.muted,
      },
      ...(count == null ? [] : [{
        key: 'webhook_count',
        label: t('integrations.label.status'),
        value: String(count),
      }]),
    ];
  }

  if (entry.kind === 'wecom') {
    const webhookUrl = configValue(entry.config.webhook_url, notSet);
    const eventTypes = listConfigValue(entry.config.event_types, notSet);
    return [
      {
        key: 'webhook_url',
        label: t('integrations.field.wecomWebhookUrl'),
        value: webhookUrl.muted ? webhookUrl.value : maskSecretUrl(webhookUrl.value),
        muted: webhookUrl.muted,
      },
      {
        key: 'event_types',
        label: t('integrations.field.eventTypes'),
        value: eventTypes.value,
        muted: eventTypes.muted,
      },
    ];
  }

  const endpoint = configValue(entry.config.endpoint, notSet);
  return [
    {
      key: 'endpoint',
      label: t('integrations.field.endpoint'),
      value: endpoint.value,
      muted: endpoint.muted,
    },
    ...(entry.config.service_name == null ? [] : [{
      key: 'service_name',
      label: t('integrations.field.serviceName'),
      value: String(entry.config.service_name),
    }]),
  ];
}

function defaultConfigPathForKind(kind: IntegrationKind, t: Translator): string {
  switch (kind) {
    case 'sentry':
      return t('integrations.defaultPath.sentry');
    case 'webhooks':
      return t('integrations.defaultPath.webhooks');
    case 'wecom':
      return t('integrations.defaultPath.wecom');
    case 'otlp':
      return t('integrations.defaultPath.otlp');
    default:
      return t('integrations.label.notSet');
  }
}

function configValue(value: unknown, notSet: string): { value: string; muted: boolean } {
  const text = value == null ? '' : String(value).trim();
  return text ? { value: text, muted: false } : { value: notSet, muted: true };
}

function listConfigValue(value: unknown, notSet: string): { value: string; muted: boolean } {
  if (Array.isArray(value)) {
    const text = value.map((item) => String(item).trim()).filter(Boolean).join(', ');
    return text ? { value: text, muted: false } : { value: notSet, muted: true };
  }
  return configValue(value, notSet);
}

function maskSecretUrl(value: string): string {
  if (!value) return '';
  try {
    const url = new URL(value);
    if (url.username) {
      url.username = '********';
    }
    if (url.password) {
      url.password = '********';
    }
    for (const key of ['key', 'token', 'secret', 'access_token']) {
      if (url.searchParams.has(key)) {
        url.searchParams.set(key, '********');
      }
    }
    return url.toString();
  } catch {
    if (value.length <= 12) return '********';
    return `${value.slice(0, 4)}********${value.slice(-4)}`;
  }
}

function maskDsn(dsn: string): string {
  if (!dsn) return '';
  try {
    const url = new URL(dsn);
    if (url.username) {
      const auth = url.password ? '********:********' : '********';
      return `${url.protocol}//${auth}@${url.host}${url.pathname}${url.search}${url.hash}`;
    }
    return `${url.protocol}//${url.host}${url.pathname}${url.search}${url.hash}`;
  } catch {
    if (dsn.length <= 12) return '********';
    return `${dsn.slice(0, 4)}********${dsn.slice(-4)}`;
  }
}
