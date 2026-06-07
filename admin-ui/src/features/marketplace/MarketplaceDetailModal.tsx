import { useCallback, useEffect } from 'react';
import { createPortal } from 'react-dom';
import type { InterpolationValues, MessageKey } from '../../i18n';
import type { MarketplaceEntry } from '../../admin-types';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

export type MarketplaceDetailModalProps = {
  entry: MarketplaceEntry | null;
  coreVersion?: string | null;
  onClose: () => void;
  t: Translator;
};

/// Portal-based modal showing full package details.
/// Shows description, version, tags, dcc, maintainer, url,
/// min_core_version, source, install type.
/// Displays a compatibility warning when min_core_version > current core version.
export function MarketplaceDetailModal({
  entry,
  coreVersion,
  onClose,
  t,
}: MarketplaceDetailModalProps) {
  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    },
    [onClose],
  );

  useEffect(() => {
    if (!entry) return;
    document.addEventListener('keydown', handleKeyDown);
    document.body.style.overflow = 'hidden';
    return () => {
      document.removeEventListener('keydown', handleKeyDown);
      document.body.style.overflow = '';
    };
  }, [entry, handleKeyDown]);

  if (!entry) return null;

  const version = entry.version ?? t('marketplace.card.noVersion');
  const maintainer = entry.maintainer ?? undefined;
  const installType = entry.install?.type ?? null;
  const sourceLabel = entry.source_name ?? null;
  const sourceUrl = entry.source_url ?? null;
  const hasUrl = Boolean(entry.url);

  // Compatibility check: warn if min_core_version > current core version
  const compatWarning =
    entry.min_core_version && coreVersion
      ? entry.min_core_version.localeCompare(coreVersion, undefined, { numeric: true }) > 0
      : false;

  return createPortal(
    <div
      className="marketplace-detail-backdrop"
      role="dialog"
      aria-modal="true"
      aria-label={t('marketplace.detail.title')}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="marketplace-detail-modal">
        <button
          type="button"
          className="marketplace-detail-close"
          aria-label={t('marketplace.detail.close')}
          onClick={onClose}
        >
          &times;
        </button>

        <div className="marketplace-detail-header">
          {entry.icon ? (
            <img
              className="marketplace-detail-icon"
              src={entry.icon}
              alt={entry.name}
            />
          ) : (
            <span className="marketplace-detail-icon-fallback">
              {entry.name.charAt(0).toUpperCase()}
            </span>
          )}
          <h2 className="marketplace-detail-name">{entry.name}</h2>
        </div>

        {compatWarning ? (
          <div className="marketplace-detail-warning" role="alert">
            {t('marketplace.detail.compatibilityWarning', {
              min: entry.min_core_version ?? '?',
              current: coreVersion ?? '?',
            })}
          </div>
        ) : null}

        <div className="marketplace-detail-grid">
          {entry.description ? (
            <div className="marketplace-detail-section marketplace-detail-desc">
              <h4>{t('marketplace.detail.description')}</h4>
              <p>{entry.description}</p>
            </div>
          ) : null}

          <div className="marketplace-detail-kv">
            <div className="marketplace-detail-kv-item">
              <span className="marketplace-detail-kv-label">{t('marketplace.detail.version')}</span>
              <span className="marketplace-detail-kv-value">{version}</span>
            </div>

            {entry.min_core_version ? (
              <div className="marketplace-detail-kv-item">
                <span className="marketplace-detail-kv-label">{t('marketplace.detail.minCoreVersion')}</span>
                <span className={`marketplace-detail-kv-value${compatWarning ? ' marketplace-detail-kv-warn' : ''}`}>
                  {entry.min_core_version}
                </span>
              </div>
            ) : null}

            {maintainer ? (
              <div className="marketplace-detail-kv-item">
                <span className="marketplace-detail-kv-label">{t('marketplace.detail.maintainer')}</span>
                <span className="marketplace-detail-kv-value">{maintainer}</span>
              </div>
            ) : (
              <div className="marketplace-detail-kv-item">
                <span className="marketplace-detail-kv-label">{t('marketplace.detail.maintainer')}</span>
                <span className="marketplace-detail-kv-value muted">{t('marketplace.detail.noMaintainer')}</span>
              </div>
            )}

            {installType ? (
              <div className="marketplace-detail-kv-item">
                <span className="marketplace-detail-kv-label">{t('marketplace.detail.installType')}</span>
                <span className="marketplace-detail-kv-value marketplace-detail-mono">{installType}</span>
              </div>
            ) : null}

            {sourceLabel ? (
              <div className="marketplace-detail-kv-item">
                <span className="marketplace-detail-kv-label">{t('marketplace.detail.source')}</span>
                <span className="marketplace-detail-kv-value">
                  {sourceUrl ? (
                    <a
                      href={sourceUrl}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="marketplace-detail-link"
                    >
                      {sourceLabel}
                    </a>
                  ) : (
                    sourceLabel
                  )}
                </span>
              </div>
            ) : null}

            {hasUrl ? (
              <div className="marketplace-detail-kv-item">
                <span className="marketplace-detail-kv-label">{t('marketplace.detail.url')}</span>
                <span className="marketplace-detail-kv-value">
                  <a
                    href={entry.url!}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="marketplace-detail-link"
                  >
                    {t('marketplace.detail.visitProject')}
                  </a>
                </span>
              </div>
            ) : null}
          </div>

          {entry.dcc.length > 0 ? (
            <div className="marketplace-detail-section">
              <h4>{t('marketplace.detail.dcc')}</h4>
              <div className="marketplace-detail-chip-row">
                {entry.dcc.map((dcc) => (
                  <span key={dcc} className="marketplace-card-chip marketplace-card-chip-tag">
                    {dcc}
                  </span>
                ))}
              </div>
            </div>
          ) : null}

          {entry.tags.length > 0 ? (
            <div className="marketplace-detail-section">
              <h4>{t('marketplace.detail.tags')}</h4>
              <div className="marketplace-detail-chip-row">
                {entry.tags.map((tag) => (
                  <code key={tag} className="marketplace-card-chip marketplace-card-chip-tag">
                    {tag}
                  </code>
                ))}
              </div>
            </div>
          ) : null}
        </div>
      </div>
    </div>,
    document.body,
  );
}
