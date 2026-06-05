import { useCallback, useEffect, useMemo, useState } from 'react';
import type { InterpolationValues, MessageKey } from '../../i18n';
import type { IntegrationEntry, IntegrationKind, EnvLockedField } from '../../admin-types';

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
  type: 'text' | 'number' | 'password';
  validate?: (value: string) => string | null;
};

const SENTRY_FIELDS: FieldDef[] = [
  {
    key: 'dsn',
    labelKey: 'integrations.field.dsn',
    placeholderKey: 'integrations.placeholder.dsn',
    helpKey: 'integrations.help.dsn',
    type: 'password',
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
    key: 'config_path',
    labelKey: 'integrations.field.configPath',
    placeholderKey: 'integrations.placeholder.configPath',
    type: 'text',
  },
];

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
  otlp: OTLP_FIELDS,
};

/// Edit form for a single integration.
///
/// Renders fields based on the integration kind. Sentry fields include DSN
/// (masked when env-locked), environment, release, sample_rate, each with
/// env-lock UI. Webhooks and OTLP show simpler placeholder forms.
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
      values[field.key] = String(entry.config[field.key] ?? '');
    }
    setFormValues(values);
    setFieldErrors({});
  }, [entry, fields]);

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
      const envLock = envLockedByKey.get(field.key);
      if (envLock?.locked) continue; // skip env-locked fields
      if (field.validate) {
        const error = field.validate(formValues[field.key] ?? '');
        if (error) errors[field.key] = error;
      }
    }
    setFieldErrors(errors);
    return Object.keys(errors).length === 0;
  }, [fields, envLockedByKey, formValues]);

  const handleSubmit = useCallback(
    async (e: React.FormEvent) => {
      e.preventDefault();
      if (!validate()) return;

      const config: Record<string, unknown> = {};
      for (const [key, value] of Object.entries(formValues)) {
        const field = fields.find((f) => f.key === key);
        if (field?.type === 'number') {
          const num = parseFloat(value);
          config[key] = Number.isFinite(num) ? num : null;
        } else {
          config[key] = value || null;
        }
      }
      await onSave(entry.kind, config);
    },
    [formValues, fields, entry.kind, validate, onSave],
  );

  const nameKey = `integrations.card.${entry.kind}Name` as MessageKey;

  return (
    <div className="integration-edit-overlay">
      <form className="integration-edit-form" onSubmit={handleSubmit}>
        <div className="integration-edit-head">
          <h3>{t(nameKey)}</h3>
          <span className={`badge ${entry.status === 'active' ? 'badge-ok' : entry.status === 'pending_restart' ? 'badge-warn' : 'badge-muted'}`}>
            {t(statusLocaleKey(entry.status))}
          </span>
        </div>

        <div className="integration-edit-fields">
          {fields.map((field) => {
            const envLock = envLockedByKey.get(field.key);
            const locked = envLock?.locked ?? false;
            const value = formValues[field.key] ?? '';
            const error = fieldErrors[field.key];
            const isDsn = field.key === 'dsn';
            const dsnMasked = isDsn && locked && value;

            return (
              <div
                key={field.key}
                className={`integration-edit-field${locked ? ' env-locked' : ''}${error ? ' has-error' : ''}${dsnMasked ? ' dsn-masked' : ''}`}
              >
                <label htmlFor={`integration-${entry.kind}-${field.key}`}>
                  {t(field.labelKey)}
                  {locked && (
                    <span className="integration-env-lock-tag" title={`${t('integrations.label.envVar')}: ${envLock!.env_var}`}>
                      🔒 {t('integrations.label.envLocked')}
                    </span>
                  )}
                </label>
                <input
                  id={`integration-${entry.kind}-${field.key}`}
                  type={field.type === 'password' ? 'password' : 'text'}
                  value={dsnMasked ? '••••••••' : value}
                  placeholder={locked ? t('integrations.label.envLocked') : t(field.placeholderKey)}
                  disabled={locked || saving}
                  onChange={(e) => handleChange(field.key, e.target.value)}
                  autoComplete="off"
                />
                {locked && envLock && (
                  <p className="integration-env-hint">
                    {t('integrations.label.envVar')}: <code>{envLock.env_var}</code>
                  </p>
                )}
                {field.helpKey && !dsnMasked && (
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
          <button
            className="refresh-btn"
            type="submit"
            disabled={saving}
          >
            {saving ? t('integrations.action.saving') : t('integrations.action.save')}
          </button>
          <button
            className="linkish"
            type="button"
            onClick={onCancel}
            disabled={saving}
          >
            {t('integrations.action.cancel')}
          </button>
        </div>
      </form>
    </div>
  );
}
