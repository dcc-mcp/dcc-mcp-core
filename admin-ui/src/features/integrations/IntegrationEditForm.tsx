import {
  RiCloseLine,
  RiSaveLine,
} from '@remixicon/react';
import { useCallback, useEffect, useMemo, useState } from 'react';
import type { InterpolationValues, MessageKey } from '../../i18n';
import type { IntegrationEntry, IntegrationKind, EnvLockedField } from '../../admin-types';
import { Button } from '../../components/ui/button';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

export type IntegrationEditFormProps = {
  entry: IntegrationEntry;
  saving: boolean;
  onSave: (kind: IntegrationKind, config: Record<string, unknown>) => Promise<void>;
  onCancel: () => void;
  t: Translator;
};

/// Per-field configuration mapping for supported integration kinds.
type FieldDef = {
  key: string;
  labelKey: MessageKey;
  placeholderKey: MessageKey;
  helpKey?: MessageKey;
  type: 'text' | 'number' | 'password' | 'list' | 'textarea';
  secret?: boolean;
  validate?: (value: string) => string | null;
};

const SENTRY_FIELDS: FieldDef[] = [
  {
    key: 'dsn',
    labelKey: 'integrations.field.dsn',
    placeholderKey: 'integrations.placeholder.dsn',
    helpKey: 'integrations.help.dsn',
    type: 'password',
    secret: true,
    validate: (value) => {
      if (!value) return null; // empty = disabled
      // Must be a URL-like DSN
      if (!value.startsWith('http') || !value.includes('@') || !value.includes('/')) {
        return 'integrations.error.invalidDsn';
      }
      return null;
    },
  },
  {
    key: 'environment',
    labelKey: 'integrations.field.environment',
    placeholderKey: 'integrations.placeholder.environment',
    type: 'text',
  },
  {
    key: 'release',
    labelKey: 'integrations.field.release',
    placeholderKey: 'integrations.placeholder.release',
    type: 'text',
  },
  {
    key: 'sample_rate',
    labelKey: 'integrations.field.sampleRate',
    placeholderKey: 'integrations.placeholder.sampleRate',
    helpKey: 'integrations.help.sampleRate',
    type: 'number',
  },
];

const WEBHOOKS_FIELDS: FieldDef[] = [
  {
    key: 'config_text',
    labelKey: 'integrations.field.webhooksYaml',
    placeholderKey: 'integrations.placeholder.webhooksYaml',
    helpKey: 'integrations.help.webhooksYaml',
    type: 'textarea',
  },
];

const WECOM_FIELDS: FieldDef[] = [
  {
    key: 'webhook_url',
    labelKey: 'integrations.field.wecomWebhookUrl',
    placeholderKey: 'integrations.placeholder.wecomWebhookUrl',
    helpKey: 'integrations.help.wecomWebhookUrl',
    type: 'password',
    secret: true,
    validate: (value) => {
      if (!value) return null;
      if (!value.startsWith('http')) {
        return 'integrations.error.invalidWebhookUrl';
      }
      return null;
    },
  },
  {
    key: 'event_types',
    labelKey: 'integrations.field.eventTypes',
    placeholderKey: 'integrations.placeholder.eventTypes',
    helpKey: 'integrations.help.eventTypes',
    type: 'list',
  },
  {
    key: 'template',
    labelKey: 'integrations.field.messageTemplate',
    placeholderKey: 'integrations.placeholder.messageTemplate',
    helpKey: 'integrations.help.messageTemplate',
    type: 'textarea',
  },
];

const WECOM_TEMPLATE_VARIABLES = [
  '$event',
  '$event-id',
  '$dcc-type',
  '$instance-id',
  '$tool-slug',
  '$skill-name',
  '$url',
] as const;

function statusLocaleKey(status: string): MessageKey {
  switch (status) {
    case 'active': return 'integrations.status.active';
    case 'inactive': return 'integrations.status.inactive';
    case 'pending_restart': return 'integrations.status.pendingRestart';
    default: return 'integrations.status.inactive';
  }
}

const OTLP_FIELDS: FieldDef[] = [
  {
    key: 'endpoint',
    labelKey: 'integrations.field.endpoint',
    placeholderKey: 'integrations.placeholder.endpoint',
    helpKey: 'integrations.help.endpoint',
    type: 'text',
  },
  {
    key: 'service_name',
    labelKey: 'integrations.field.serviceName',
    placeholderKey: 'integrations.placeholder.serviceName',
    type: 'text',
  },
  {
    key: 'headers',
    labelKey: 'integrations.field.headers',
    placeholderKey: 'integrations.placeholder.headers',
    type: 'text',
  },
];

const KIND_FIELDS: Record<IntegrationKind, FieldDef[]> = {
  sentry: SENTRY_FIELDS,
  webhooks: WEBHOOKS_FIELDS,
  wecom: WECOM_FIELDS,
  otlp: OTLP_FIELDS,
};

/// Edit form for a single integration.
///
/// Renders fields based on the integration kind. Secret fields can preserve
/// runtime env values while staging manual overrides for the next restart.
export function IntegrationEditForm({
  entry,
  saving,
  onSave,
  onCancel,
  t,
}: IntegrationEditFormProps) {
  const fields = useMemo(() => KIND_FIELDS[entry.kind] ?? [], [entry.kind]);

  const [formValues, setFormValues] = useState<Record<string, string>>({});
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});

  // Build env-locked field lookup: key → EnvLockedField
  const envLockedByKey = useMemo(() => {
    const map = new Map<string, EnvLockedField>();
    for (const f of entry.env_locked_fields) {
      map.set(f.key, f);
    }
    return map;
  }, [entry.env_locked_fields]);

  // Initialize form values from entry config.
  useEffect(() => {
    const values: Record<string, string> = {};
    for (const field of fields) {
      const envLock = envLockedByKey.get(field.key);
      const rawValue = formStringValue(entry.config[field.key]);
      const isMaskedSecret = field.secret && rawValue.includes('********');
      values[field.key] = (envLock?.locked && field.secret) || isMaskedSecret ? '' : rawValue;
    }
    setFormValues(values);
    setFieldErrors({});
  }, [entry, fields, envLockedByKey]);

  const handleChange = useCallback(
    (key: string, value: string) => {
      setFormValues((prev) => ({ ...prev, [key]: value }));
      // Clear error on change
      setFieldErrors((prev) => {
        if (!prev[key]) return prev;
        const next = { ...prev };
        delete next[key];
        return next;
      });
    },
    [],
  );

  const validate = useCallback((): boolean => {
    const errors: Record<string, string> = {};
    for (const field of fields) {
      if (field.validate) {
        const error = field.validate(formValues[field.key] ?? '');
        if (error) errors[field.key] = error;
      }
    }
    setFieldErrors(errors);
    return Object.keys(errors).length === 0;
  }, [fields, formValues]);

  const insertTemplateVariable = useCallback((token: string) => {
    setFormValues((prev) => {
      const current = prev.template ?? '';
      const separator = current.length > 0 && !/[\s\n]$/.test(current) ? ' ' : '';
      return { ...prev, template: `${current}${separator}${token}` };
    });
    setFieldErrors((prev) => {
      if (!prev.template) return prev;
      const next = { ...prev };
      delete next.template;
      return next;
    });
  }, []);

  const handleSubmit = useCallback(
    async (e: React.FormEvent) => {
      e.preventDefault();
      if (!validate()) return;

      const config: Record<string, unknown> = {};
      for (const [key, value] of Object.entries(formValues)) {
        const field = fields.find((f) => f.key === key);
        const envLock = envLockedByKey.get(key);
        const hasMaskedCurrent = field?.secret && String(entry.config[key] ?? '').includes('********');
        if (field?.secret && (envLock?.locked || hasMaskedCurrent) && !value.trim()) {
          continue;
        }
        if (field?.type === 'number') {
          const num = parseFloat(value);
          config[key] = Number.isFinite(num) ? num : null;
        } else if (field?.type === 'list') {
          const values = splitListValue(value);
          config[key] = values.length > 0 ? values : null;
        } else {
          config[key] = value || null;
        }
      }
      await onSave(entry.kind, config);
    },
    [formValues, fields, envLockedByKey, entry.kind, validate, onSave],
  );

  const nameKey = `integrations.card.${entry.kind}Name` as MessageKey;
  const configWritePath = formStringValue(entry.config.write_config_path);
  const configPath = formStringValue(entry.config.config_path);
  const configDestination = configWritePath || configPath || defaultConfigPathForKind(entry.kind, t);

  return (
    <div className="integration-edit-panel">
      <form className="integration-edit-form" onSubmit={handleSubmit}>
        <div className="integration-edit-head">
          <h3>{t(nameKey)}</h3>
          <span className={`badge ${entry.status === 'active' ? 'badge-ok' : entry.status === 'pending_restart' ? 'badge-warn' : 'badge-muted'}`}>
            {t(statusLocaleKey(entry.status))}
          </span>
        </div>

        {configDestination ? (
          <div className="integration-config-path-note">
            <span>{t('integrations.field.configPath')}</span>
            <code>{configDestination}</code>
          </div>
        ) : null}

        <div className="integration-edit-fields">
          {fields.map((field) => {
            const envLock = envLockedByKey.get(field.key);
            const locked = envLock?.locked ?? false;
            const value = formValues[field.key] ?? '';
            const error = fieldErrors[field.key];
            const maskedSecret = field.secret && String(entry.config[field.key] ?? '').includes('********');
            const secretOverride = field.secret && (locked || maskedSecret);

            return (
              <div
                key={field.key}
                className={`integration-edit-field${field.type === 'textarea' ? ' is-textarea' : ''}${locked ? ' env-locked' : ''}${error ? ' has-error' : ''}${secretOverride ? ' secret-override' : ''}`}
                data-disabled={saving ? true : undefined}
                data-invalid={error ? true : undefined}
              >
                <label htmlFor={`integration-${entry.kind}-${field.key}`}>
                  {t(field.labelKey)}
                  {locked && (
                    <span className="integration-env-lock-tag" title={`${t('integrations.label.envVar')}: ${envLock!.env_var}`}>
                      {t('integrations.label.envLocked')}
                    </span>
                  )}
                </label>
                {field.type === 'textarea' ? (
                  <>
                    <textarea
                      id={`integration-${entry.kind}-${field.key}`}
                      value={value}
                      placeholder={t(field.placeholderKey)}
                      disabled={saving}
                      aria-invalid={error ? true : undefined}
                      onChange={(e) => handleChange(field.key, e.target.value)}
                      rows={5}
                    />
                    {entry.kind === 'wecom' && field.key === 'template' ? (
                      <div
                        className="integration-template-token-strip"
                        aria-label={t('integrations.field.templateVariables')}
                      >
                        <span>{t('integrations.field.templateVariables')}</span>
                        <div className="integration-template-token-list">
                          {WECOM_TEMPLATE_VARIABLES.map((token) => (
                            <Button
                              key={token}
                              className="integration-template-token"
                              data-template-token={token}
                              type="button"
                              variant="outline"
                              size="sm"
                              disabled={saving}
                              onClick={() => insertTemplateVariable(token)}
                            >
                              {token}
                            </Button>
                          ))}
                        </div>
                      </div>
                    ) : null}
                  </>
                ) : (
                  <input
                    id={`integration-${entry.kind}-${field.key}`}
                    type={field.type === 'password' ? 'password' : field.type === 'number' ? 'number' : 'text'}
                    value={value}
                    placeholder={secretOverride ? t('integrations.placeholder.secretOverride') : t(field.placeholderKey)}
                    disabled={saving}
                    aria-invalid={error ? true : undefined}
                    onChange={(e) => handleChange(field.key, e.target.value)}
                    autoComplete="off"
                  />
                )}
                {locked && envLock && (
                  <p className="integration-env-hint">
                    {t('integrations.help.envOverride', { envVar: envLock.env_var })}
                  </p>
                )}
                {field.helpKey && (
                  <p className="integration-field-help">{t(field.helpKey)}</p>
                )}
                {error && (
                  <p className="integration-field-error">{t(error as MessageKey)}</p>
                )}
              </div>
            );
          })}
        </div>

        <div className="integration-edit-actions">
          <Button
            type="submit"
            size="sm"
            disabled={saving}
          >
            <RiSaveLine data-icon="inline-start" aria-hidden="true" />
            {saving ? t('integrations.action.saving') : t('integrations.action.save')}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            type="button"
            onClick={onCancel}
            disabled={saving}
          >
            <RiCloseLine data-icon="inline-start" aria-hidden="true" />
            {t('integrations.action.cancel')}
          </Button>
        </div>
      </form>
    </div>
  );
}

function formStringValue(value: unknown): string {
  if (Array.isArray(value)) {
    return value.map((item) => String(item)).join(', ');
  }
  return String(value ?? '');
}

function defaultConfigPathForKind(kind: IntegrationKind, t: Translator): string {
  switch (kind) {
    case 'sentry':
      return t('integrations.defaultPath.sentry');
    case 'webhooks':
    case 'wecom':
      return t('integrations.defaultPath.webhooks');
    case 'otlp':
      return t('integrations.defaultPath.otlp');
    default:
      return '';
  }
}

function splitListValue(value: string): string[] {
  return value
    .split(/[,\n]/)
    .map((item) => item.trim())
    .filter(Boolean);
}
