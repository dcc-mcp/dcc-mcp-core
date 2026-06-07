import { useCallback, useEffect, useMemo, useState } from 'react';
import type { InterpolationValues, MessageKey } from '../../i18n';
import {
  PanelHeader,
  StatusLine,
  haystack,
  matchesListFilter,
} from '../../admin-ui-core';
import {
  useMarketplaceCatalogQuery,
  useInstalledMarketplaceQuery,
  useMarketplaceInstall,
  useMarketplaceUninstall,
} from '../../hooks/queries';
import type { MarketplaceEntry, InstalledMarketplacePackage } from '../../admin-types';
import { MarketplaceCard } from './MarketplaceCard';
import { MarketplaceDetailModal } from './MarketplaceDetailModal';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

export type MarketplacePanelProps = {
  active: boolean;
  search: string;
  updatedAt: string;
  error?: string;
  onUpdated: (text: string) => void;
  onError: (error: unknown) => void;
  onCountsChange?: (counts: { total: number; installed: number }) => void;
  /** Current dcc-mcp-core version (from /health) for compatibility warning. */
  coreVersion?: string | null;
  /** Navigate to Skills panel and highlight the given skill name. */
  onNavigateToSkills?: (skillName: string) => void;
  t: Translator;
};

type MarketplaceTab = 'browse' | 'installed';

/// Top-level orchestrator for the `/admin#marketplace` panel.
///
/// Two tabs: Browse (searchable catalog with per-DCC install) and Installed
/// (locally installed packages with per-package uninstall). Both tabs use
/// `name:dcc` as the canonical unique key so multi-DCC entries don't leak
/// installed state across DCC types.
///
/// Browse tab now includes a DCC filter chip row derived from catalog entries.
/// Cards are clickable and open a detail modal with full package metadata.
/// After a successful install, an inline notice offers "View in Skills" deep link.
export function MarketplacePanel({
  active,
  search,
  updatedAt,
  error,
  onUpdated,
  onError,
  onCountsChange,
  coreVersion,
  onNavigateToSkills,
  t,
}: MarketplacePanelProps) {
  const [tab, setTab] = useState<MarketplaceTab>('browse');
  const [installingKey, setInstallingKey] = useState<string | null>(null);
  const [detailEntry, setDetailEntry] = useState<MarketplaceEntry | null>(null);
  const [dccFilter, setDccFilter] = useState<string | null>(null);
  /// { name, dcc } of the most recently installed package for the inline notice.
  const [installNotice, setInstallNotice] = useState<{ name: string; dcc: string } | null>(null);

  const catalogQuery = useMarketplaceCatalogQuery(active);
  const installedQuery = useInstalledMarketplaceQuery(active);
  const installMut = useMarketplaceInstall();
  const uninstallMut = useMarketplaceUninstall();

  const catalog = useMemo(() => catalogQuery.data ?? [], [catalogQuery.data]);
  const installed = useMemo(() => installedQuery.data ?? [], [installedQuery.data]);

  /// Derive unique DCC types from the catalog for the filter chip row.
  const dccTypes = useMemo(() => {
    const types = new Set<string>();
    for (const entry of catalog) {
      for (const dcc of entry.dcc) {
        types.add(dcc);
      }
    }
    return Array.from(types).sort((a, b) => a.localeCompare(b));
  }, [catalog]);

  /// Reset DCC filter when tab, search, or catalog changes.
  useEffect(() => {
    setDccFilter(null);
  }, [tab, search]);

  // Refresh on first show.
  useEffect(() => {
    if (!active) return;
    void Promise.allSettled([
      catalogQuery.refetch(),
      installedQuery.refetch(),
    ]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active]);

  // Status line updates.
  useEffect(() => {
    if (!active) return;
    if (catalogQuery.data) {
      const time = new Date().toLocaleTimeString();
      onUpdated(
        t('marketplace.detail.packagesFound', { count: catalog.length }) +
          ` · ${t('marketplace.detail.installedCount', { count: installed.length })} · ${time}`,
      );
    }
  }, [active, catalog.length, installed.length, catalogQuery.data, onUpdated, t]);

  useEffect(() => {
    if (catalogQuery.error) onError(catalogQuery.error);
    if (installedQuery.error) onError(installedQuery.error);
  }, [catalogQuery.error, installedQuery.error, onError]);

  // Report counts to parent.
  useEffect(() => {
    onCountsChange?.({ total: catalog.length, installed: installed.length });
  }, [catalog.length, installed.length, onCountsChange]);

  // Canonical lookup: "name:dcc" → InstalledMarketplacePackage.
  const installedByKey = useMemo(() => {
    const map = new Map<string, InstalledMarketplacePackage>();
    for (const pkg of installed) {
      map.set(`${pkg.name}:${pkg.dcc}`, pkg);
    }
    return map;
  }, [installed]);

  // Group installed packages by entry name — returns a Map<dcc, InstalledMarketplacePackage>
  // for a given entry name.
  const installedDccsForEntry = useCallback(
    (entryName: string) => {
      const map = new Map<string, InstalledMarketplacePackage>();
      for (const [key, pkg] of installedByKey) {
        if (key.startsWith(`${entryName}:`)) {
          map.set(pkg.dcc, pkg);
        }
      }
      return map;
    },
    [installedByKey],
  );

  /// Filter catalog by search AND DCC chip.
  const filteredCatalog = useMemo(() => {
    const q = search.trim().toLowerCase();
    let result = catalog;
    if (dccFilter) {
      result = result.filter((entry) => entry.dcc.includes(dccFilter));
    }
    if (q) {
      result = result.filter((entry) =>
        matchesListFilter(
          q,
          haystack(
            entry.name,
            entry.description,
            ...entry.dcc,
            ...entry.tags,
            entry.maintainer ?? '',
            entry.version ?? '',
          ),
        ),
      );
    }
    return result;
  }, [catalog, search, dccFilter]);

  // Filter installed packages by search.
  const filteredInstalled = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return installed;
    return installed.filter((pkg) =>
      matchesListFilter(
        q,
        haystack(
          pkg.name,
          pkg.dcc,
          pkg.version ?? '',
          pkg.install_type,
          pkg.path,
        ),
      ),
    );
  }, [installed, search]);

  const handleInstall = useCallback(
    async (entry: MarketplaceEntry, dcc: string) => {
      const key = `${entry.name}:${dcc}`;
      setInstallingKey(key);
      setInstallNotice(null);
      try {
        await installMut.mutateAsync({ name: entry.name, dcc });
        await installedQuery.refetch();
        setInstallNotice({ name: entry.name, dcc });
      } catch (err) {
        onError(err);
      } finally {
        setInstallingKey(null);
      }
    },
    [installMut, installedQuery, onError],
  );

  const handleUninstall = useCallback(
    async (pkg: InstalledMarketplacePackage) => {
      const key = `${pkg.name}:${pkg.dcc}`;
      setInstallingKey(key);
      setInstallNotice(null);
      try {
        await uninstallMut.mutateAsync({ name: pkg.name, dcc: pkg.dcc });
        await installedQuery.refetch();
        setInstallNotice({ name: pkg.name, dcc: pkg.dcc });
      } catch (err) {
        onError(err);
      } finally {
        setInstallingKey(null);
      }
    },
    [uninstallMut, installedQuery, onError],
  );

  const handleOpenDetail = useCallback((entry: MarketplaceEntry) => {
    setDetailEntry(entry);
  }, []);

  const handleCloseDetail = useCallback(() => {
    setDetailEntry(null);
  }, []);

  const handleViewInSkills = useCallback(() => {
    if (installNotice && onNavigateToSkills) {
      onNavigateToSkills(installNotice.name);
    }
  }, [installNotice, onNavigateToSkills]);

  return (
    <section className={`panel${active ? ' active' : ''} marketplace-panel`}>
      <PanelHeader
        title={t('marketplace.title')}
        meta={t('marketplace.detail.packagesFound', { count: catalog.length })}
      />

      <StatusLine text={updatedAt || t('marketplace.detail.packagesFound', { count: catalog.length })} error={error} />

      {/* ── Install / uninstall success notice ──────────────────────────── */}
      {installNotice ? (
        <div className="marketplace-install-notice" role="status">
          <span>
            {installedByKey.has(`${installNotice.name}:${installNotice.dcc}`)
              ? t('marketplace.install.success', { name: installNotice.name, dcc: installNotice.dcc })
              : t('marketplace.uninstall.success', { name: installNotice.name, dcc: installNotice.dcc })}
          </span>
          <div style={{ display: 'flex', gap: '0.5rem', alignItems: 'center' }}>
            {installedByKey.has(`${installNotice.name}:${installNotice.dcc}`) && onNavigateToSkills ? (
              <button
                type="button"
                className="marketplace-install-notice-link"
                onClick={handleViewInSkills}
              >
                {t('marketplace.install.viewInSkills')}
              </button>
            ) : null}
            <button
              type="button"
              className="marketplace-install-notice-close"
              aria-label={t('action.close')}
              onClick={() => setInstallNotice(null)}
            >
              &times;
            </button>
          </div>
        </div>
      ) : null}

      <div className="marketplace-tabs" role="tablist" aria-label={t('marketplace.title')}>
        <button
          className={`marketplace-tab${tab === 'browse' ? ' active' : ''}`}
          role="tab"
          aria-selected={tab === 'browse'}
          type="button"
          onClick={() => setTab('browse')}
        >
          {t('marketplace.tab.browse')}
        </button>
        <button
          className={`marketplace-tab${tab === 'installed' ? ' active' : ''}`}
          role="tab"
          aria-selected={tab === 'installed'}
          type="button"
          onClick={() => setTab('installed')}
        >
          {t('marketplace.tab.installed')}
          {installed.length > 0 ? (
            <span className="marketplace-tab-count">{installed.length}</span>
          ) : null}
        </button>
      </div>

      {/* Browse tab — catalog card grid with DCC filter chips + per-DCC install/uninstall. */}
      {tab === 'browse' && (
        <div className="marketplace-content">
          {/* DCC filter chip row */}
          {dccTypes.length > 1 ? (
            <div className="marketplace-dcc-filter" role="group" aria-label={t('marketplace.filter.dccLabel')}>
              <span className="marketplace-dcc-filter-label">{t('marketplace.filter.dccLabel')}</span>
              <button
                type="button"
                className={`marketplace-dcc-chip${dccFilter === null ? ' active' : ''}`}
                onClick={() => setDccFilter(null)}
              >
                {t('marketplace.filter.dccAll')}
              </button>
              {dccTypes.map((dcc) => (
                <button
                  key={dcc}
                  type="button"
                  className={`marketplace-dcc-chip${dccFilter === dcc ? ' active' : ''}`}
                  onClick={() => setDccFilter(dccFilter === dcc ? null : dcc)}
                >
                  {dcc}
                </button>
              ))}
            </div>
          ) : null}

          {catalogQuery.isLoading ? (
            <p className="empty">{t('marketplace.status.loading')}</p>
          ) : catalogQuery.error ? (
            <p className="empty">{t('marketplace.status.error')}: {catalogQuery.error.message}</p>
          ) : filteredCatalog.length === 0 ? (
            <p className="empty">
              {search.trim() || dccFilter ? t('marketplace.empty.search') : t('marketplace.empty.none')}
            </p>
          ) : (
            <div className="marketplace-grid">
              {filteredCatalog.map((entry) => (
                <MarketplaceCard
                  key={entry.name}
                  entry={entry}
                  installedDccs={installedDccsForEntry(entry.name)}
                  installingKey={installingKey}
                  onInstall={handleInstall}
                  onUninstall={handleUninstall}
                  onOpenDetail={handleOpenDetail}
                  t={t}
                />
              ))}
            </div>
          )}
        </div>
      )}

      {/* Installed tab — per-package cards with uninstall via installedDccs. */}
      {tab === 'installed' && (
        <div className="marketplace-content">
          {installedQuery.isLoading ? (
            <p className="empty">{t('marketplace.status.loading')}</p>
          ) : installedQuery.error ? (
            <p className="empty">{t('marketplace.status.error')}: {installedQuery.error.message}</p>
          ) : filteredInstalled.length === 0 ? (
            <p className="empty">
              {search.trim() ? t('marketplace.empty.search') : t('marketplace.empty.installed')}
            </p>
          ) : (
            <div className="marketplace-grid">
              {filteredInstalled.map((pkg) => {
                const catalogEntry = catalog.find((e) => e.name === pkg.name);
                const displayEntry: MarketplaceEntry = catalogEntry ?? {
                  name: pkg.name,
                  description: '',
                  dcc: [pkg.dcc],
                  tags: [],
                  version: pkg.version ?? undefined,
                  maintainer: null,
                  url: null,
                  min_core_version: null,
                  source_name: pkg.source_name,
                  source_url: pkg.source_url,
                  install: {
                    type: pkg.install_type,
                    url: pkg.install_url ?? null,
                    ref: pkg.install_ref ?? null,
                  },
                };
                // For installed tab, the only installed DCC is `pkg.dcc`.
                const dccMap = new Map<string, InstalledMarketplacePackage>();
                dccMap.set(pkg.dcc, pkg);
                return (
                  <MarketplaceCard
                    key={`${pkg.name}:${pkg.dcc}`}
                    entry={displayEntry}
                    installedDccs={dccMap}
                    installingKey={installingKey}
                    onInstall={handleInstall}
                    onUninstall={handleUninstall}
                    onOpenDetail={handleOpenDetail}
                    t={t}
                  />
                );
              })}
            </div>
          )}
        </div>
      )}

      {/* Detail modal (portal-based overlay) */}
      <MarketplaceDetailModal
        entry={detailEntry}
        coreVersion={coreVersion}
        onClose={handleCloseDetail}
        t={t}
      />
    </section>
  );
}
