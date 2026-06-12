import {
  RiCloseLine,
  RiDeleteBinLine,
  RiRefreshLine,
} from '@remixicon/react';
import { type InterpolationValues, type MessageKey } from '../../i18n';
import type { InstalledMarketplacePackage, MarketplaceEntry } from '../../admin-types';
import { Button } from '../../components/ui/button';
import './MarketplaceInstalledDetailPanel.css';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

export type MarketplaceInstalledDetailPanelProps = {
  pkg: InstalledMarketplacePackage;
  /** Matched catalog entry (if this package is in the marketplace catalog). */
  catalogEntry?: MarketplaceEntry | null;
  isOutdated?: boolean;
  installing?: boolean;
  onUninstall: () => void;
  onUpdate?: () => void;
  onClose: () => void;
  t: Translator;
};

/// Slide-out detail panel for an installed marketplace package.
///
/// Follows the same layout/behaviour as SkillDetailPanel — right-aligned
/// slide-in with backdrop blur, sticky heading, summary grid, and action
/// buttons. Reuses existing marketplace locale keys where possible.
export function MarketplaceInstalledDetailPanel({
  pkg,
  catalogEntry,
  isOutdated,
  installing,
  onUninstall,
  onUpdate,
  onClose,
  t,
}: MarketplaceInstalledDetailPanelProps) {
  const version = pkg.version ?? catalogEntry?.version ?? t('marketplace.card.noVersion');
  const description = catalogEntry?.description || null;
  const tags = catalogEntry?.tags ?? [];
  const maintainer = catalogEntry?.maintainer ?? null;

  const installedDate = pkg.installed_at_ms
    ? new Date(pkg.installed_at_ms).toLocaleString()
    : null;

  return (
    <section className="marketplace-installed-detail-panel" aria-live="polite">
      <div className="marketplace-installed-detail-heading">
        <div>
          <h3>{pkg.name}</h3>
          <div className="marketplace-installed-detail-meta">
            <span className="source-pill">{pkg.dcc}</span>
            <span className="marketplace-installed-detail-version">
              {t('marketplace.detail.version')}: {version}
            </span>
            {maintainer ? (
              <span className="marketplace-installed-detail-version">{maintainer}</span>
            ) : null}
          </div>
        </div>
        <div className="table-actions">
          {isOutdated && onUpdate ? (
            <Button
              className="marketplace-installed-detail-action"
              type="button"
              size="sm"
              disabled={installing}
              onClick={onUpdate}
            >
              <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
              {installing ? t('marketplace.card.updating') : t('marketplace.card.update')}
            </Button>
          ) : null}
          <Button
            className="marketplace-installed-detail-action"
            type="button"
            variant="destructive"
            size="sm"
            disabled={installing}
            onClick={onUninstall}
          >
            <RiDeleteBinLine data-icon="inline-start" aria-hidden="true" />
            {installing ? t('marketplace.card.installing') : t('marketplace.card.uninstall')}
          </Button>
          <Button
            className="marketplace-installed-detail-action"
            variant="ghost"
            size="sm"
            type="button"
            onClick={onClose}
          >
            <RiCloseLine data-icon="inline-start" aria-hidden="true" />
            {t('action.close')}
          </Button>
        </div>
      </div>

      {description ? (
        <p className="marketplace-installed-detail-desc">{description}</p>
      ) : null}

      <div className="marketplace-installed-detail-summary-grid">
        <span>
          <strong>{t('marketplace.detail.installType')}</strong>
          {pkg.install_type}
        </span>
        <span>
          <strong>{t('marketplace.detail.source')}</strong>
          {pkg.source_url ? (
            <a
              href={pkg.source_url}
              target="_blank"
              rel="noopener noreferrer"
            >
              {pkg.source_name}
            </a>
          ) : (
            pkg.source_name
          )}
        </span>
        <span>
          <strong>Path</strong>
          <code className="mono-path">{pkg.path}</code>
        </span>
        <span>
          <strong>Installed</strong>
          {installedDate ?? '—'}
        </span>
      </div>

      {isOutdated ? (
        <div className="marketplace-installed-detail-outdated" role="alert">
          {t('marketplace.card.outdated')}
        </div>
      ) : null}

      {tags.length > 0 ? (
        <div className="marketplace-installed-detail-section">
          <h4>{t('marketplace.card.tags')}</h4>
          <div className="marketplace-installed-detail-chips">
            {tags.map((tag) => (
              <code key={tag} className="marketplace-card-chip marketplace-card-chip-tag">
                {tag}
              </code>
            ))}
          </div>
        </div>
      ) : null}
    </section>
  );
}
