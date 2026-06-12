import { useCallback, useEffect, useMemo, useState } from 'react';
import { createPortal } from 'react-dom';
import type { InterpolationValues, MessageKey } from '../../i18n';
import {
  AdminApiError,
  PanelHeader,
  StatusLine,
  haystack,
  matchesListFilter,
} from '../../admin-ui-core';
import {
  useMarketplaceCatalogQuery,
  useInstalledMarketplaceQuery,
  useMarketplaceSourcesQuery,
  useMarketplaceOutdatedQuery,
  useMarketplaceInstall,
  useMarketplaceUninstall,
  useAddMarketplaceSource,
  useMarketplaceUpdate,
  MarketplaceError,
} from '../../hooks/queries';
import type {
  MarketplaceEntry,
  MarketplaceInstallResult,
  InstalledMarketplacePackage,
  MarketplaceSourceEntry,
} from '../../admin-types';
import { MarketplaceCard } from './MarketplaceCard';
import { MarketplaceDetailModal } from './MarketplaceDetailModal';
import { MarketplaceInstalledDetailPanel } from './MarketplaceInstalledDetailPanel';
import { MarketplaceInstalledList } from './MarketplaceInstalledList';

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

type MarketplaceTab = 'browse' | 'installed' | 'sources';

/// Top-level orchestrator for the `/admin#marketplace` panel.
///
/// Three tabs: Browse (searchable catalog with per-DCC install), Installed
/// (locally installed packages with per-package uninstall / update), and
/// Sources (manage marketplace source registries).
///
/// Browse tab includes a DCC filter chip row derived from catalog entries.
/// Cards are clickable and open a detail modal with full package metadata.
/// After a successful install, an inline notice offers "View in Skills" deep link
/// and shows "Skill reload triggered" when the backend confirms reload.
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
  const [installedDetail, setInstalledDetail] = useState<{
    pkg: InstalledMarketplacePackage;
    catalogEntry?: MarketplaceEntry | null;
  } | null>(null);
  const [dccFilter, setDccFilter] = useState<string | null>(null);
  const [forceInstall, setForceInstall] = useState(false);
  /// { name, dcc } of the most recently installed/uninstalled package for the inline notice.
  const [installNotice, setInstallNotice] = useState<{
    name: string; dcc: string; reload_required?: boolean; action: 'install' | 'uninstall' | 'update';
  } | null>(null);
  /// Source add input buffer.
  const [sourceInput, setSourceInput] = useState('');

  const catalogQuery = useMarketplaceCatalogQuery(active);
  const installedQuery = useInstalledMarketplaceQuery(active);
  const sourcesQuery = useMarketplaceSourcesQuery(active);
  const outdatedQuery = useMarketplaceOutdatedQuery(active);
  const installMut = useMarketplaceInstall();
  const uninstallMut = useMarketplaceUninstall();
  const addSourceMut = useAddMarketplaceSource();
  const updateMut = useMarketplaceUpdate();

  // ── Error mapping utils ────────────────────────────────────────────────────

  /// Map a MarketplaceError to a user-friendly message using i18n.
  const marketplaceErrorMessage = useCallback(
    (err: unknown): string => {
      if (err instanceof MarketplaceError) {
        switch (err.kind) {
          case 'already_installed':
            return t('marketplace.error.alreadyInstalled');
          case 'not_found':
            return t('marketplace.error.notFound');
          case 'hash_mismatch':
            return t('marketplace.error.hashMismatch');
          case 'missing_skill':
            return t('marketplace.error.missingSkill');
          case 'command_failed':
            return t('marketplace.error.commandFailed', { message: err.message });
          case 'admin_api_html':
            return t('marketplace.error.adminHtml');
          default:
            return t('marketplace.error.generic', { kind: err.kind, message: err.message });
        }
      }
      if (err instanceof AdminApiError) {
        return t('marketplace.error.adminHtml');
      }
      if (err instanceof Error && err.message) {
        // Network / fetch errors
        if (err.message.includes('Failed to fetch') || err.message.includes('NetworkError')) {
          return t('marketplace.error.networkError');
        }
        return err.message;
      }
      return t('marketplace.error.unknown');
    },
    [t],
  );

  const catalog = useMemo(() => catalogQuery.data ?? [], [catalogQuery.data]);
  const installed = useMemo(() => installedQuery.data ?? [], [installedQuery.data]);
  const sources = useMemo(() => sourcesQuery.data ?? [], [sourcesQuery.data]);

  // ── Outdated lookup ────────────────────────────────────────────────────────

  const outdatedByKey = useMemo(() => {
    const map = new Map<string, true>();
    if (outdatedQuery.data?.packages) {
      for (const pkg of outdatedQuery.data.packages) {
        map.set(`${pkg.name}:${pkg.dcc}`, true);
      }
    }
    return map;
  }, [outdatedQuery.data]);

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

  /// Reset DCC filter when switching tab; preserve it while typing search text.
  useEffect(() => {
    setDccFilter(null);
  }, [tab]);

  /// If a catalog refresh removes the selected DCC, clear the stale filter.
  useEffect(() => {
    if (dccFilter && !dccTypes.includes(dccFilter)) {
      setDccFilter(null);
    }
  }, [dccFilter, dccTypes]);

  // Refresh on first show.
  useEffect(() => {
    if (!active) return;
    void Promise.allSettled([
      catalogQuery.refetch(),
      installedQuery.refetch(),
      sourcesQuery.refetch(),
      outdatedQuery.refetch(),
    ]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active]);

  // Status line updates.
  useEffect(() => {
    if (!active) return;
    if (catalogQuery.data) {
      const time = new Date().toLocaleTimeString();
      const parts = [
        t('marketplace.detail.packagesFound', { count: catalog.length }),
        ` · ${t('marketplace.detail.installedCount', { count: installed.length })}`,
      ];
      if (outdatedQuery.data && outdatedQuery.data.count > 0) {
        parts.push(` · ${outdatedQuery.data.count} ${t('marketplace.card.outdated')}`);
      }
      parts.push(` · ${time}`);
      onUpdated(parts.join(''));
    }
  }, [active, catalog.length, installed.length, outdatedQuery.data, catalogQuery.data, onUpdated, t]);

  useEffect(() => {
    if (!active) return;
    const queryError =
      catalogQuery.error
      ?? installedQuery.error
      ?? sourcesQuery.error
      ?? outdatedQuery.error;
    if (queryError) {
      onError(marketplaceErrorMessage(queryError));
    }
  }, [
    active,
    catalogQuery.error,
    installedQuery.error,
    sourcesQuery.error,
    outdatedQuery.error,
    onError,
    marketplaceErrorMessage,
  ]);

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

  // ── Handlers ───────────────────────────────────────────────────────────────

  const handleInstall = useCallback(
    async (entry: MarketplaceEntry, dcc: string) => {
      const key = `${entry.name}:${dcc}`;
      setInstallingKey(key);
      setInstallNotice(null);
      try {
        const result: MarketplaceInstallResult = await installMut.mutateAsync({
          name: entry.name,
          dcc,
          force: forceInstall,
        });
        await installedQuery.refetch();
        setInstallNotice({
          name: entry.name,
          dcc,
          reload_required: result.reload_required,
          action: 'install',
        });
      } catch (err) {
        onError(marketplaceErrorMessage(err));
      } finally {
        setInstallingKey(null);
      }
    },
    [installMut, installedQuery, forceInstall, onError, marketplaceErrorMessage],
  );

  const handleUninstall = useCallback(
    async (pkg: InstalledMarketplacePackage) => {
      const key = `${pkg.name}:${pkg.dcc}`;
      setInstallingKey(key);
      setInstallNotice(null);
      try {
        const result = await uninstallMut.mutateAsync({ name: pkg.name, dcc: pkg.dcc });
        await installedQuery.refetch();
        setInstallNotice({
          name: pkg.name,
          dcc: pkg.dcc,
          reload_required: result.reload_required,
          action: 'uninstall',
        });
      } catch (err) {
        onError(marketplaceErrorMessage(err));
      } finally {
        setInstallingKey(null);
      }
    },
    [uninstallMut, installedQuery, onError, marketplaceErrorMessage],
  );

  const handleUpdate = useCallback(
    async (pkgName: string, dcc: string) => {
      const key = `${pkgName}:${dcc}`;
      setInstallingKey(key);
      setInstallNotice(null);
      try {
        const result = await updateMut.mutateAsync({ name: pkgName, dcc });
        await installedQuery.refetch();
        await outdatedQuery.refetch();
        const updatedItem = result.results?.find((r) => r.name === pkgName && r.dcc === dcc);
        setInstallNotice({
          name: pkgName,
          dcc,
          reload_required: updatedItem?.reload_required ?? false,
          action: 'update',
        });
      } catch (err) {
        onError(marketplaceErrorMessage(err));
      } finally {
        setInstallingKey(null);
      }
    },
    [updateMut, installedQuery, outdatedQuery, onError, marketplaceErrorMessage],
  );

  const handleOpenDetail = useCallback((entry: MarketplaceEntry) => {
    setDetailEntry(entry);
  }, []);

  const handleCloseDetail = useCallback(() => {
    setDetailEntry(null);
  }, []);

  const handleOpenInstalledDetail = useCallback(
    (pkg: InstalledMarketplacePackage, catalogEntry?: MarketplaceEntry | null) => {
      setInstalledDetail({ pkg, catalogEntry });
    },
    [],
  );

  const handleCloseInstalledDetail = useCallback(() => {
    setInstalledDetail(null);
  }, []);

  const handleViewInSkills = useCallback(() => {
    if (installNotice && onNavigateToSkills) {
      onNavigateToSkills(installNotice.name);
    }
  }, [installNotice, onNavigateToSkills]);

  const handleAddSource = useCallback(async () => {
    const value = sourceInput.trim();
    if (!value) return;
    try {
      await addSourceMut.mutateAsync(value);
      setSourceInput('');
    } catch (err) {
      onError(marketplaceErrorMessage(err));
    }
  }, [sourceInput, addSourceMut, onError, marketplaceErrorMessage]);

  // ── Render ─────────────────────────────────────────────────────────────────

  if (!active) return null;

  return (
    <section className="panel active marketplace-panel">
      <PanelHeader
        title={t('marketplace.title')}
        meta={t('marketplace.detail.packagesFound', { count: catalog.length })}
      />

      <StatusLine text={updatedAt || t('marketplace.detail.packagesFound', { count: catalog.length })} error={error} />

      <div className="marketplace-summary-strip" aria-label={t('marketplace.title')}>
        <div className="marketplace-summary-item">
          <span>{t('marketplace.metric.available')}</span>
          <strong>{catalog.length}</strong>
        </div>
        <div className="marketplace-summary-item">
          <span>{t('marketplace.metric.installed')}</span>
          <strong>{installed.length}</strong>
        </div>
        <div className="marketplace-summary-item">
          <span>{t('marketplace.metric.sources')}</span>
          <strong>{sources.length}</strong>
        </div>
        <div className={`marketplace-summary-item ${outdatedQuery.data?.count ? 'warn' : ''}`}>
          <span>{t('marketplace.card.outdated')}</span>
          <strong>{outdatedQuery.data?.count ?? 0}</strong>
        </div>
      </div>

      {/* ── Install / uninstall / update success notice ───────────────────── */}
      {installNotice ? (
        <div className="marketplace-install-notice" role="status">
          <span>
            {installNotice.action === 'update'
              ? t('marketplace.update.success', { name: installNotice.name, dcc: installNotice.dcc })
              : installedByKey.has(`${installNotice.name}:${installNotice.dcc}`)
                ? t('marketplace.install.success', { name: installNotice.name, dcc: installNotice.dcc })
                : t('marketplace.uninstall.success', { name: installNotice.name, dcc: installNotice.dcc })}
            {installNotice.reload_required ? (
              <span className="marketplace-reload-hint">
                {' '}{t('marketplace.install.reloadTriggered')}
              </span>
            ) : null}
          </span>
          <div className="marketplace-install-notice-actions">
            {installNotice.action !== 'uninstall' &&
             installedByKey.has(`${installNotice.name}:${installNotice.dcc}`) &&
             onNavigateToSkills ? (
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
          aria-label={t('marketplace.tab.browse')}
          type="button"
          onClick={() => setTab('browse')}
        >
          <span className="marketplace-tab-label">{t('marketplace.tab.browse')}</span>
        </button>
        <button
          className={`marketplace-tab${tab === 'installed' ? ' active' : ''}`}
          role="tab"
          aria-selected={tab === 'installed'}
          aria-label={t('marketplace.tab.installed')}
          type="button"
          onClick={() => setTab('installed')}
        >
          <span className="marketplace-tab-label">{t('marketplace.tab.installed')}</span>
          {installed.length > 0 || (outdatedQuery.data?.count ?? 0) > 0 ? (
            <span className="marketplace-tab-counts" aria-hidden="true">
              {installed.length > 0 ? (
                <span className="marketplace-tab-count">{installed.length}</span>
              ) : null}
              {(outdatedQuery.data?.count ?? 0) > 0 ? (
                <span
                  className="marketplace-tab-count marketplace-tab-count-warn"
                  title={t('marketplace.card.outdated')}
                >
                  {t('marketplace.tab.updateCount', { count: outdatedQuery.data!.count })}
                </span>
              ) : null}
            </span>
          ) : null}
        </button>
        <button
          className={`marketplace-tab marketplace-tab-sources${tab === 'sources' ? ' active' : ''}`}
          role="tab"
          aria-selected={tab === 'sources'}
          aria-label={t('marketplace.source.sectionTitle')}
          type="button"
          onClick={() => setTab('sources')}
        >
          <span className="marketplace-tab-label">{t('marketplace.source.sectionTitle')}</span>
          {sources.length > 0 ? (
            <span className="marketplace-tab-count" aria-hidden="true">{sources.length}</span>
          ) : null}
        </button>
      </div>

      {/* ── Sources tab ─────────────────────────────────────────────────── */}
      {tab === 'sources' ? (
        <div className="marketplace-content marketplace-sources-section">
          <div className="marketplace-source-add">
            <input
              className="marketplace-source-input"
              type="text"
              placeholder={t('marketplace.source.addPlaceholder')}
              value={sourceInput}
              onChange={(e) => setSourceInput(e.target.value)}
              onKeyDown={(e) => { if (e.key === 'Enter') void handleAddSource(); }}
            />
            <button
              className="marketplace-source-btn"
              type="button"
              disabled={!sourceInput.trim() || addSourceMut.isPending}
              onClick={handleAddSource}
            >
              {addSourceMut.isPending
                ? t('marketplace.source.adding')
                : t('marketplace.source.addLabel')}
            </button>
          </div>

          {sourcesQuery.isLoading ? (
            <p className="empty">{t('marketplace.status.loading')}</p>
          ) : sourcesQuery.error ? (
            <p className="empty">{t('marketplace.status.error')}: {marketplaceErrorMessage(sourcesQuery.error)}</p>
          ) : sources.length === 0 ? (
            <p className="empty">{t('marketplace.source.empty')}</p>
          ) : (
            <div className="marketplace-sources-list">
              {sources.map((source: MarketplaceSourceEntry) => (
                <div key={source.name} className="marketplace-source-row">
                  <span className="marketplace-source-name" title={source.name}>
                    {source.name}
                  </span>
                  <span className="marketplace-source-url mono-path" title={source.url}>
                    {source.url}
                  </span>
                  <span className={`source-pill source-pill-${source.origin}`}>
                    {source.origin}
                  </span>
                </div>
              ))}
            </div>
          )}
        </div>
      ) : null}

      {/* Browse tab — catalog card grid with DCC filter chips + per-DCC install/uninstall. */}
      {tab === 'browse' && (
        <div className="marketplace-content">
          <div className="marketplace-force-install">
            <label>
              <input
                type="checkbox"
                checked={forceInstall}
                onChange={(e) => setForceInstall(e.target.checked)}
              />
              {t('marketplace.card.forceInstall')}
            </label>
          </div>

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
            <p className="empty">{t('marketplace.status.error')}: {marketplaceErrorMessage(catalogQuery.error)}</p>
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
                  onUpdate={handleUpdate}
                  onOpenDetail={handleOpenDetail}
                  isOutdated={false}
                  t={t}
                />
              ))}
            </div>
          )}
        </div>
      )}

      {/* Installed tab — dense package inventory with direct update / uninstall actions. */}
      {tab === 'installed' && (
        <div className="marketplace-content">
          {installedQuery.isLoading ? (
            <p className="empty">{t('marketplace.status.loading')}</p>
          ) : installedQuery.error ? (
            <p className="empty">{t('marketplace.status.error')}: {marketplaceErrorMessage(installedQuery.error)}</p>
          ) : filteredInstalled.length === 0 ? (
            <p className="empty">
              {search.trim() ? t('marketplace.empty.search') : t('marketplace.empty.installed')}
            </p>
          ) : (
            <MarketplaceInstalledList
              packages={filteredInstalled}
              catalog={catalog}
              outdatedByKey={outdatedByKey}
              installingKey={installingKey}
              onOpenDetail={handleOpenInstalledDetail}
              onUninstall={handleUninstall}
              onUpdate={handleUpdate}
              t={t}
            />
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

      {/* Installed package detail (portal-based slide-out) */}
      <MarketplaceInstalledDetailModal
        detail={installedDetail}
        isOutdated={installedDetail ? outdatedByKey.has(`${installedDetail.pkg.name}:${installedDetail.pkg.dcc}`) : false}
        installingKey={installingKey}
        onUninstall={handleUninstall}
        onUpdate={handleUpdate}
        onClose={handleCloseInstalledDetail}
        t={t}
      />
    </section>
  );
}

/// Portal-based modal that renders the installed detail panel as a slide-in overlay.
/// Follows the same pattern as SkillDetailModal in SkillsPanel.
function MarketplaceInstalledDetailModal({
  detail,
  isOutdated,
  installingKey,
  onUninstall,
  onUpdate,
  onClose,
  t,
}: {
  detail: { pkg: InstalledMarketplacePackage; catalogEntry?: MarketplaceEntry | null } | null;
  isOutdated: boolean;
  installingKey: string | null;
  onUninstall: (pkg: InstalledMarketplacePackage) => void;
  onUpdate: (pkgName: string, dcc: string) => void;
  onClose: () => void;
  t: Translator;
}) {
  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    },
    [onClose],
  );

  useEffect(() => {
    if (!detail) return;
    document.addEventListener('keydown', handleKeyDown);
    document.body.style.overflow = 'hidden';
    return () => {
      document.removeEventListener('keydown', handleKeyDown);
      document.body.style.overflow = '';
    };
  }, [detail, handleKeyDown]);

  if (!detail) return null;

  const pkgKey = `${detail.pkg.name}:${detail.pkg.dcc}`;
  const isInstalling = installingKey === pkgKey;

  return createPortal(
    <div
      className="marketplace-installed-detail-backdrop"
      role="dialog"
      aria-modal="true"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="marketplace-installed-detail-modal">
        <button
          type="button"
          className="marketplace-installed-detail-close"
          aria-label={t('action.close')}
          onClick={onClose}
        >
          &times;
        </button>
        <MarketplaceInstalledDetailPanel
          pkg={detail.pkg}
          catalogEntry={detail.catalogEntry}
          isOutdated={isOutdated}
          installing={isInstalling}
          onUninstall={() => onUninstall(detail.pkg)}
          onUpdate={onUpdate ? () => onUpdate(detail.pkg.name, detail.pkg.dcc) : undefined}
          onClose={onClose}
          t={t}
        />
      </div>
    </div>,
    document.body,
  );
}
