import { useState } from 'react';
import './MarketplaceCard.css';
import type { InterpolationValues, MessageKey } from '../../i18n';
import type { MarketplaceEntry, InstalledMarketplacePackage } from '../../admin-types';
import { resolveDccIcon } from '../../platform';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

export type MarketplaceCardProps = {
  entry: MarketplaceEntry;
  /** Which DCCs of this entry are currently installed. Map from dcc → InstalledMarketplacePackage. */
  installedDccs: Map<string, InstalledMarketplacePackage>;
  /** "name:dcc" key of the package currently being installed/uninstalled, or null. */
  installingKey: string | null;
  onInstall: (entry: MarketplaceEntry, dcc: string) => void;
  onUninstall: (pkg: InstalledMarketplacePackage) => void;
  onUpdate?: (pkgName: string, dcc: string) => void;
  onOpenDetail: (entry: MarketplaceEntry) => void;
  /** Whether this installed package has a newer version available. */
  isOutdated?: boolean;
  t: Translator;
};

function installingKeyName(k: string | null): string | null {
  if (!k) return null;
  const idx = k.lastIndexOf(':');
  return idx === -1 ? k : k.slice(0, idx);
}

/// Marketplace catalog card — one per entry in the browse or installed grid.
export function MarketplaceCard({
  entry,
  installedDccs,
  installingKey,
  onInstall,
  onUninstall,
  onUpdate,
  onOpenDetail,
  isOutdated,
  t,
}: MarketplaceCardProps) {
  const version = entry.version ?? t('marketplace.card.noVersion');
  const maintainer = entry.maintainer ?? undefined;
  const isInstalling = installingKeyName(installingKey) === entry.name;
  const [iconFailed, setIconFailed] = useState(false);
  const [showcaseFailed, setShowcaseFailed] = useState(false);
  const dccIcon = entry.dcc.length === 1 ? resolveDccIcon(entry.dcc[0]) : null;
  const icon = entry.icon && !iconFailed ? entry.icon : dccIcon;
  const showcase = entry.showcase && !showcaseFailed ? entry.showcase : null;

  return (
    <article
      className={`marketplace-card${isOutdated ? ' marketplace-card-outdated' : ''}`}
      data-name={entry.name}
    >
      <button
        type="button"
        className={`marketplace-card-media marketplace-card-media-trigger${showcase ? ' has-showcase' : ' is-fallback'}`}
        aria-label={`${t('marketplace.card.detail')}: ${entry.name}`}
        onClick={() => onOpenDetail(entry)}
      >
        {showcase ? (
          <img
            className="marketplace-card-showcase"
            src={showcase}
            alt=""
            aria-hidden
            loading="lazy"
            decoding="async"
            referrerPolicy="no-referrer"
            onError={() => setShowcaseFailed(true)}
          />
        ) : (
          <div className="marketplace-card-media-fallback" aria-hidden="true">
            <span className="marketplace-card-media-grid" />
            {icon ? (
              <img
                className="marketplace-card-media-icon"
                src={icon}
                alt=""
                onError={() => setIconFailed(true)}
              />
            ) : (
              <span className="marketplace-card-media-monogram">
                {entry.name.charAt(0).toUpperCase()}
              </span>
            )}
          </div>
        )}
        <div className="marketplace-card-media-shade" aria-hidden="true" />
        <div className="marketplace-card-media-meta">
          <span className="marketplace-card-version">v{version}</span>
          {installedDccs.size > 0 ? (
            <span className="marketplace-card-state marketplace-card-state-installed">
              {t('marketplace.tab.installed')} {installedDccs.size}/{entry.dcc.length}
            </span>
          ) : null}
          {isOutdated ? (
            <span className="marketplace-card-state marketplace-card-state-outdated">
              {t('marketplace.card.outdated')}
            </span>
          ) : null}
        </div>
      </button>

      <div className="marketplace-card-body">
        <div className="marketplace-card-head">
          {icon ? (
            <img
              className="marketplace-card-icon"
              src={icon}
              alt=""
              aria-hidden
              onError={() => setIconFailed(true)}
            />
          ) : (
            <span className="marketplace-card-icon-fallback">
              {entry.name.charAt(0).toUpperCase()}
            </span>
          )}
          <div className="marketplace-card-title-group">
            <h3 className="marketplace-card-name" title={entry.name}>
              {entry.name}
            </h3>
            {maintainer ? (
              <span className="marketplace-card-publisher">{maintainer}</span>
            ) : null}
          </div>
        </div>

        <p className="marketplace-card-desc" title={entry.description}>
          {entry.description || t('marketplace.card.noDescription')}
        </p>

        {entry.dcc.length > 0 ? (
          <div className="marketplace-card-dcc-list">
            <span className="marketplace-card-tags-label">{t('marketplace.card.dccLabel')}:</span>
            <div className="marketplace-card-chips">
              {entry.dcc.map((dcc) => {
                const pkg = installedDccs.get(dcc);
                const key = `${entry.name}:${dcc}`;
                if (pkg) {
                  return (
                    <button
                      key={dcc}
                      type="button"
                      className="marketplace-card-chip marketplace-card-chip-installed"
                      disabled={installingKey === key}
                      title={t('marketplace.card.uninstall')}
                      onClick={(e) => { e.stopPropagation(); onUninstall(pkg); }}
                    >
                      <span className="marketplace-card-chip-check">✓</span>
                      {dcc}
                    </button>
                  );
                }
                return (
                  <button
                    key={dcc}
                    type="button"
                    className="marketplace-card-chip marketplace-card-chip-action"
                    disabled={isInstalling}
                    title={t('marketplace.card.installFor', { dcc })}
                    onClick={(e) => { e.stopPropagation(); onInstall(entry, dcc); }}
                  >
                    {installingKey === key
                      ? t('marketplace.card.installing')
                      : t('marketplace.card.installDcc', { dcc })}
                  </button>
                );
              })}
            </div>
          </div>
        ) : null}

        {/* Outdated update button — shown on installed tab for outdated packages */}
        {isOutdated && onUpdate && entry.dcc.length > 0 ? (
          <div className="marketplace-card-update-row">
            {entry.dcc.map((dcc) => {
              const pkg = installedDccs.get(dcc);
              if (!pkg) return null;
              const key = `${entry.name}:${dcc}`;
              return (
                <button
                  key={dcc}
                  type="button"
                  className="marketplace-card-chip marketplace-card-chip-update"
                  disabled={installingKey === key}
                  onClick={(e) => { e.stopPropagation(); onUpdate(entry.name, dcc); }}
                >
                  {installingKey === key
                    ? t('marketplace.card.updating')
                    : t('marketplace.card.update')}
                </button>
              );
            })}
          </div>
        ) : null}

        {entry.tags.length > 0 ? (
          <div className="marketplace-card-tags">
            <span className="marketplace-card-tags-label">{t('marketplace.card.tags')}:</span>
            <div className="marketplace-card-chips">
              {entry.tags.slice(0, 3).map((tag) => (
                <code key={tag} className="marketplace-card-chip marketplace-card-chip-tag">
                  {tag}
                </code>
              ))}
              {entry.tags.length > 3 ? (
                <code className="marketplace-card-chip marketplace-card-chip-tag">
                  +{entry.tags.length - 3}
                </code>
              ) : null}
            </div>
          </div>
        ) : null}
      </div>
    </article>
  );
}
