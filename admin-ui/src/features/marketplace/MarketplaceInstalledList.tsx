import {
  RiDeleteBinLine,
  RiInformationLine,
  RiRefreshLine,
} from '@remixicon/react';
import { useMemo } from 'react';
import type { InterpolationValues, MessageKey } from '../../i18n';
import type { InstalledMarketplacePackage, MarketplaceEntry } from '../../admin-types';
import { Button } from '../../components/ui/button';
import './MarketplaceInstalledList.css';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

export type MarketplaceInstalledListProps = {
  packages: InstalledMarketplacePackage[];
  catalog: MarketplaceEntry[];
  outdatedByKey: Map<string, true>;
  installingKey: string | null;
  onOpenDetail: (pkg: InstalledMarketplacePackage, catalogEntry?: MarketplaceEntry | null) => void;
  onUninstall: (pkg: InstalledMarketplacePackage) => void;
  onUpdate: (pkgName: string, dcc: string) => void;
  t: Translator;
};

/// Dense operator list for installed marketplace packages.
///
/// The browse view stays card-based, but installed packages behave more like
/// inventory: one row per installed `{package, dcc}` pair with direct update,
/// detail, and uninstall actions.
export function MarketplaceInstalledList({
  packages,
  catalog,
  outdatedByKey,
  installingKey,
  onOpenDetail,
  onUninstall,
  onUpdate,
  t,
}: MarketplaceInstalledListProps) {
  const catalogByName = useMemo(() => {
    const map = new Map<string, MarketplaceEntry>();
    for (const entry of catalog) {
      map.set(entry.name, entry);
    }
    return map;
  }, [catalog]);

  return (
    <div className="marketplace-installed-list" role="list">
      <div className="marketplace-installed-list-header" aria-hidden="true">
        <span>{t('marketplace.installed.package')}</span>
        <span>{t('marketplace.installed.version')}</span>
        <span>{t('marketplace.installed.source')}</span>
        <span>{t('marketplace.installed.installedAt')}</span>
        <span>{t('marketplace.installed.actions')}</span>
      </div>
      {packages.map((pkg) => {
        const pkgKey = `${pkg.name}:${pkg.dcc}`;
        const catalogEntry = catalogByName.get(pkg.name);
        const isOutdated = outdatedByKey.has(pkgKey);
        const installing = installingKey === pkgKey;
        const version = pkg.version ?? catalogEntry?.version ?? t('marketplace.card.noVersion');
        const installedAt = pkg.installed_at_ms
          ? new Date(pkg.installed_at_ms).toLocaleString()
          : t('marketplace.detail.noMaintainer');

        return (
          <article
            key={pkgKey}
            className={`marketplace-installed-row${isOutdated ? ' is-outdated' : ''}`}
            data-name={pkg.name}
            data-dcc={pkg.dcc}
            role="listitem"
          >
            <div className="marketplace-installed-package-cell">
              <button
                type="button"
                className="marketplace-installed-title"
                onClick={() => onOpenDetail(pkg, catalogEntry)}
              >
                {pkg.name}
              </button>
              <div className="marketplace-installed-subline">
                <span className="source-pill marketplace-installed-dcc">{pkg.dcc}</span>
                <code className="marketplace-installed-path" title={pkg.path}>
                  {pkg.path}
                </code>
              </div>
            </div>

            <div className="marketplace-installed-version-cell">
              <span className="marketplace-installed-version">{version}</span>
              {isOutdated ? (
                <span className="marketplace-installed-update-badge">
                  {t('marketplace.installed.updateAvailable')}
                </span>
              ) : null}
            </div>

            <div className="marketplace-installed-source-cell">
              {pkg.source_url ? (
                <a href={pkg.source_url} target="_blank" rel="noopener noreferrer">
                  {pkg.source_name}
                </a>
              ) : (
                <span>{pkg.source_name}</span>
              )}
              <small>{pkg.install_type}</small>
            </div>

            <time className="marketplace-installed-time-cell">
              {installedAt}
            </time>

            <div className="marketplace-installed-actions">
              {isOutdated ? (
                <Button
                  type="button"
                  size="sm"
                  className="marketplace-installed-action is-primary"
                  disabled={installing}
                  onClick={() => onUpdate(pkg.name, pkg.dcc)}
                >
                  <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
                  {installing ? t('marketplace.card.updating') : t('marketplace.card.update')}
                </Button>
              ) : null}
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="marketplace-installed-action"
                onClick={() => onOpenDetail(pkg, catalogEntry)}
              >
                <RiInformationLine data-icon="inline-start" aria-hidden="true" />
                {t('marketplace.card.detail')}
              </Button>
              <Button
                type="button"
                variant="destructive"
                size="sm"
                className="marketplace-installed-action is-danger"
                disabled={installing}
                onClick={() => onUninstall(pkg)}
              >
                <RiDeleteBinLine data-icon="inline-start" aria-hidden="true" />
                {installing ? t('marketplace.card.uninstalling') : t('marketplace.card.uninstall')}
              </Button>
            </div>
          </article>
        );
      })}
    </div>
  );
}
