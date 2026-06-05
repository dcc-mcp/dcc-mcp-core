import type { InterpolationValues, MessageKey } from '../../i18n';
import type { IntegrationEntry, IntegrationKind } from '../../admin-types';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

export type IntegrationCardProps = {
  entry: IntegrationEntry;
  onEdit: (kind: IntegrationKind) => void;
  t: Translator;
};

const SENTRY_ICON = 'M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm1 15h-2v-2h2v2zm0-4h-2V7h2v6z';
const WEBHOOK_ICON = 'M4 16l-1 1v2h2l8-8-2-2-7 7zm13-11l-2 2 1 1 2-2-1-1zm-4 4l-1 1-9 9h2l8-8-2-2 2 2z';
const OTLP_ICON = 'M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-2 15l-5-5 1.41-1.41L10 14.17l7.59-7.59L19 8l-9 9z';

const KIND_ICONS: Record<IntegrationKind, string> = {
  sentry: SENTRY_ICON,
  webhooks: WEBHOOK_ICON,
  otlp: OTLP_ICON,
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

/// A card representing one integration (Sentry, Webhooks, or OTLP).
///
/// Shows the integration icon, name, description, status badge, and an edit
/// button. For inactive integrations the edit button opens an empty-state form.
export function IntegrationCard({ entry, onEdit, t }: IntegrationCardProps) {
  const icon = KIND_ICONS[entry.kind] ?? '';
  const isSentry = entry.kind === 'sentry';
  const isPlaceholder = entry.kind === 'webhooks' || entry.kind === 'otlp';

  const nameKey = `integrations.card.${entry.kind}Name` as MessageKey;
  const descKey = `integrations.card.${entry.kind}Desc` as MessageKey;

  return (
    <article
      className={`integration-card${entry.status === 'pending_restart' ? ' pending-restart' : ''}`}
      data-kind={entry.kind}
      data-status={entry.status}
    >
      <div className="integration-card-body">
        <div className="integration-card-head">
          <svg className="integration-card-icon" viewBox="0 0 24 24" aria-hidden="true">
            <path d={icon} />
          </svg>
          <div>
            <h3 className="integration-card-name">{t(nameKey)}</h3>
            <span className={`badge ${sentryStatusTone(entry.status)}`}>
              {t(statusLocaleKey(entry.status))}
            </span>
          </div>
        </div>

        <p className="integration-card-desc">{t(descKey)}</p>

        {entry.status === 'pending_restart' && (
          <span className="badge badge-warn integration-restart-badge">
            {t('integrations.badge.pendingRestart')}
          </span>
        )}

        {/* Config summary for active/pending Sentry */}
        {isSentry && entry.status !== 'inactive' && (
          <div className="integration-config-preview">
            <span className="integration-config-field">
              <strong>{t('integrations.field.dsn')}</strong>
              {maskDsn(String(entry.config.dsn ?? ''))}
            </span>
            <span className="integration-config-field">
              <strong>{t('integrations.field.environment')}</strong>
              {String(entry.config.environment ?? t('integrations.label.notSet'))}
            </span>
          </div>
        )}

        {/* Placeholder message for Webhooks/OTLP */}
        {isPlaceholder && entry.status === 'inactive' && (
          <p className="integration-placeholder-hint">
            {t('integrations.label.notSet')}
          </p>
        )}

        <div className="integration-card-actions">
          <button
            className="refresh-btn"
            type="button"
            onClick={() => onEdit(entry.kind)}
          >
            {t('integrations.action.edit')}
          </button>
        </div>
      </div>
    </article>
  );
}

function maskDsn(dsn: string): string {
  if (!dsn) return '';
  // Mask the secret key portion: https://<key>@<host>/<project>
  // Replace the key part with asterisks
  try {
    const url = new URL(dsn);
    if (url.username) {
      url.username = '••••••••';
    }
    if (url.password) {
      url.password = '••••••••';
    }
    return url.toString();
  } catch {
    // Fallback: if DSN isn't a valid URL, mask middle portion
    if (dsn.length <= 12) return '••••••••';
    return dsn.slice(0, 4) + '••••••••' + dsn.slice(-4);
  }
}
