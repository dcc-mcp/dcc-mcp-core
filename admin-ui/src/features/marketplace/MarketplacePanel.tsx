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

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

export type MarketplacePanelProps = {
  active: boolean;
  search: string;
  updatedAt: string;
  error?: string;
  onUpdated: (text: string) => void;
  onError: (error: unknown) => void;
  onCountsChange?: (counts: { total: number; installed: number }) => void;
  t: Translator;
};

type MarketplaceTab = 'browse' | 'installed';

/// Top-level orchestrator for the `/admin#marketplace` panel.
///
/// Two tabs: Browse (searchable catalog with per-DCC install) and Installed
/// (locally installed packages with per-package uninstall). Both tabs use
/// `name:dcc` as the canonical unique key so multi-DCC entries don't leak
/// installed state across DCC types.
export function MarketplacePanel({
  active,
  search,
  updatedAt,
  error,
  onUpdated,
  onError,
  onCountsChange,
  t,
}: MarketplacePanelProps) {
  const [tab, setTab] = useState<MarketplaceTab>('browse');
  const [installingKey, setInstallingKey] = useState<string | null>(null);

  const catalogQuery = useMarketplaceCatalogQuery(active);
  const installedQuery = useInstalledMarketplaceQuery(active);
  const installMut = useMarketplaceInstall();
  const uninstallMut = useMarketplaceUninstall();

  const catalog = useMemo(() => catalogQuery.data ?? [], [catalogQuery.data]);
  const installed = useMemo(() => installedQuery.data ?? [], [installedQuery.data]);

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

  // Filter catalog by search.
  const filteredCatalog = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return catalog;
    return catalog.filter((entry) =>
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
  }, [catalog, search]);

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
      try {
        await installMut.mutateAsync({ name: entry.name, dcc });
        await installedQuery.refetch();
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
      try {
        await uninstallMut.mutateAsync({ name: pkg.name, dcc: pkg.dcc });
        await installedQuery.refetch();
      } catch (err) {
        onError(err);
      } finally {
        setInstallingKey(null);
      }
    },
    [uninstallMut, installedQuery, onError],
  );

  return (
    <section className={`panel${active ? ' active' : ''} marketplace-panel`}>
      <PanelHeader
        title={t('marketplace.title')}
        meta={t('marketplace.detail.packagesFound', { count: catalog.length })}
      />

      <StatusLine text={updatedAt || t('marketplace.detail.packagesFound', { count: catalog.length })} error={error} />

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

      {/* Browse tab — catalog card grid with per-DCC install/uninstall. */}
      {tab === 'browse' && (
        <div className="marketplace-content">
          {catalogQuery.isLoading ? (
            <p className="empty">Loading…</p>
          ) : catalogQuery.error ? (
            <p className="empty">Error: {catalogQuery.error.message}</p>
          ) : filteredCatalog.length === 0 ? (
            <p className="empty">
              {search.trim() ? t('marketplace.empty.search') : t('marketplace.empty.none')}
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
            <p className="empty">Loading…</p>
          ) : installedQuery.error ? (
            <p className="empty">Error: {installedQuery.error.message}</p>
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
                    t={t}
                  />
                );
              })}
            </div>
          )}
        </div>
      )}
    </section>
  );
}
