import { useState, useEffect, useCallback, useMemo } from 'react';
import { locales } from './locales';

type Lang = 'en' | 'zh';

export default function App() {
  const [lang, setLang] = useState<Lang>(() => {
    const navLang = navigator.language;
    return navLang.startsWith('zh') ? 'zh' : 'en';
  });

  const [wsUrl, setWsUrl] = useState(() => {
    return localStorage.getItem('dcc_mcp_ws_url') || 'ws://127.0.0.1:9765/admin/api/marketplace/ws';
  });

  const [wsState, setWsState] = useState<'disconnected' | 'connecting' | 'connected' | 'error'>('disconnected');
  const [wsError, setWsError] = useState<string | null>(null);
  const [client, setClient] = useState<any>(null);

  // Marketplace state
  const [tab, setTab] = useState<'browse' | 'installed' | 'sources'>('browse');
  const [search, setSearch] = useState('');
  const [catalog, setCatalog] = useState<any[]>([]);
  const [installed, setInstalled] = useState<any[]>([]);
  const [sources, setSources] = useState<any[]>([]);
  const [outdated, setOutdated] = useState<any>({ count: 0, packages: [] });
  const [loading, setLoading] = useState(false);
  const [forceInstall, setForceInstall] = useState(false);
  const [installingKey, setInstallingKey] = useState<string | null>(null);
  const [installNotice, setInstallNotice] = useState<{
    name: string;
    dcc: string;
    reload_required?: boolean;
    action: 'install' | 'uninstall' | 'update';
  } | null>(null);

  // Modals
  const [detailEntry, setDetailEntry] = useState<any | null>(null);
  const [installedDetail, setInstalledDetail] = useState<any | null>(null);

  // Source add input
  const [sourceInput, setSourceInput] = useState('');

  // DCC Filter
  const [dccFilter, setDccFilter] = useState<string | null>(null);

  const t = useCallback((key: string, values: any = {}) => {
    const dict = (locales as any)[lang];
    let str = dict[key] || (locales.en as any)[key] || key;
    for (const [k, v] of Object.entries(values)) {
      str = str.replace(`{${k}}`, String(v));
    }
    return str;
  }, [lang]);

  // WebSocket Client implementation
  const connectWs = useCallback((urlToConnect: string) => {
    setWsState('connecting');
    setWsError(null);

    const ws = new WebSocket(urlToConnect);
    const pendingRequests = new Map<string, { resolve: (val: any) => void; reject: (err: any) => void }>();

    const wsClient = {
      send: (action: string, payload: any = {}) => {
        return new Promise((resolve, reject) => {
          if (ws.readyState !== WebSocket.OPEN) {
            reject(new Error('WebSocket is not connected'));
            return;
          }
          const id = Math.random().toString(36).substring(2, 15);
          pendingRequests.set(id, { resolve, reject });
          ws.send(JSON.stringify({ id, action, payload }));
        });
      }
    };

    ws.onopen = () => {
      setWsState('connected');
      localStorage.setItem('dcc_mcp_ws_url', urlToConnect);
      setClient(wsClient);
    };

    ws.onclose = () => {
      setWsState('disconnected');
      setClient(null);
      for (const pending of pendingRequests.values()) {
        pending.reject(new Error('Connection closed'));
      }
      pendingRequests.clear();
    };

    ws.onerror = () => {
      setWsState('error');
      setWsError('WebSocket error');
    };

    ws.onmessage = (event) => {
      try {
        const res = JSON.parse(event.data);
        const pending = pendingRequests.get(res.id);
        if (pending) {
          pendingRequests.delete(res.id);
          if (res.status === 'success') {
            pending.resolve(res.payload);
          } else {
            pending.reject(res.error);
          }
        }
      } catch (err) {
        console.error('Failed to parse message:', err);
      }
    };
  }, []);

  // Connect on mount if we have a URL
  useEffect(() => {
    if (wsUrl) {
      connectWs(wsUrl);
    }
  }, []);

  // Fetch data when connected
  const fetchData = useCallback(async () => {
    if (!client) return;
    setLoading(true);
    try {
      const [catRes, instRes, srcRes, outRes] = await Promise.all([
        client.send('catalog'),
        client.send('installed'),
        client.send('sources'),
        client.send('outdated', {})
      ]);

      setCatalog(catRes.entries || []);
      setInstalled(instRes.packages || []);
      setSources(srcRes.sources || []);
      setOutdated(outRes || { count: 0, packages: [] });
    } catch (err) {
      console.error('Failed to fetch marketplace data:', err);
    } finally {
      setLoading(false);
    }
  }, [client]);

  useEffect(() => {
    if (wsState === 'connected' && client) {
      fetchData();
    }
  }, [wsState, client, fetchData]);

  // DCC types in catalog
  const dccTypes = useMemo(() => {
    const types = new Set<string>();
    for (const entry of catalog) {
      for (const dcc of entry.dcc) {
        types.add(dcc);
      }
    }
    return Array.from(types).sort((a, b) => a.localeCompare(b));
  }, [catalog]);

  // Filtered catalog
  const filteredCatalog = useMemo(() => {
    const q = search.trim().toLowerCase();
    let result = catalog;
    if (dccFilter) {
      result = result.filter((entry) => entry.dcc.includes(dccFilter));
    }
    if (q) {
      result = result.filter((entry) =>
        entry.name.toLowerCase().includes(q) ||
        (entry.description && entry.description.toLowerCase().includes(q))
      );
    }
    return result;
  }, [catalog, search, dccFilter]);

  // Filtered installed
  const filteredInstalled = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return installed;
    return installed.filter((pkg) =>
      pkg.name.toLowerCase().includes(q) ||
      pkg.dcc.toLowerCase().includes(q)
    );
  }, [installed, search]);

  // Actions
  const handleInstall = useCallback(async (entry: any, dcc: string) => {
    if (!client) return;
    const key = `${entry.name}:${dcc}`;
    setInstallingKey(key);
    setInstallNotice(null);
    try {
      const result = await client.send('install', {
        name: entry.name,
        dcc,
        force: forceInstall
      });
      await fetchData();
      setInstallNotice({
        name: entry.name,
        dcc,
        reload_required: result.reload_required,
        action: 'install'
      });
    } catch (err: any) {
      alert(t('install.error', { message: err.message || err }));
    } finally {
      setInstallingKey(null);
    }
  }, [client, forceInstall, fetchData, t]);

  const handleUninstall = useCallback(async (pkg: any) => {
    if (!client) return;
    const key = `${pkg.name}:${pkg.dcc}`;
    setInstallingKey(key);
    setInstallNotice(null);
    try {
      const result = await client.send('uninstall', {
        name: pkg.name,
        dcc: pkg.dcc
      });
      await fetchData();
      setInstallNotice({
        name: pkg.name,
        dcc: pkg.dcc,
        reload_required: result.reload_required,
        action: 'uninstall'
      });
      setInstalledDetail(null);
    } catch (err: any) {
      alert(t('uninstall.error', { message: err.message || err }));
    } finally {
      setInstallingKey(null);
    }
  }, [client, fetchData, t]);

  const handleUpdate = useCallback(async (pkgName: string, dcc: string) => {
    if (!client) return;
    const key = `${pkgName}:${dcc}`;
    setInstallingKey(key);
    setInstallNotice(null);
    try {
      const result = await client.send('update', {
        name: pkgName,
        dcc
      });
      await fetchData();
      const updatedItem = result.results?.find((r: any) => r.name === pkgName && r.dcc === dcc);
      setInstallNotice({
        name: pkgName,
        dcc,
        reload_required: updatedItem?.reload_required || false,
        action: 'update'
      });
      setInstalledDetail(null);
    } catch (err: any) {
      alert(t('update.error', { message: err.message || err }));
    } finally {
      setInstallingKey(null);
    }
  }, [client, fetchData, t]);

  const handleAddSource = useCallback(async () => {
    if (!client || !sourceInput.trim()) return;
    try {
      await client.send('add_source', {
        source: sourceInput.trim()
      });
      setSourceInput('');
      await fetchData();
    } catch (err: any) {
      alert(t('source.addFailed') + ': ' + (err.message || err));
    }
  }, [client, sourceInput, fetchData, t]);

  const getInstalledPackage = useCallback((name: string, dcc: string) => {
    return installed.find((pkg) => pkg.name === name && pkg.dcc === dcc);
  }, [installed]);

  const isPackageOutdated = useCallback((name: string, dcc: string) => {
    return outdated.packages?.some((pkg: any) => pkg.name === name && pkg.dcc === dcc);
  }, [outdated]);

  if (wsState !== 'connected') {
    return (
      <div className="min-h-screen bg-slate-50 flex flex-col justify-center py-12 sm:px-6 lg:px-8">
        <div className="sm:mx-auto sm:w-full sm:max-w-md">
          <div className="flex justify-end px-4">
            <button
              onClick={() => setLang(lang === 'en' ? 'zh' : 'en')}
              className="text-sm font-semibold text-blue-600 hover:text-blue-500"
            >
              {lang === 'en' ? '中文' : 'English'}
            </button>
          </div>
          <div className="setup-card">
            <h2 className="text-center text-2xl font-bold tracking-tight text-gray-900 mb-6">
              {t('connection.setup')}
            </h2>
            <div className="space-y-4">
              <div>
                <label className="block text-sm font-medium text-gray-700">
                  {t('connection.url')}
                </label>
                <input
                  type="text"
                  value={wsUrl}
                  onChange={(e) => setWsUrl(e.target.value)}
                  className="input-field"
                  placeholder="ws://127.0.0.1:9765/admin/api/marketplace/ws"
                />
              </div>

              {wsState === 'error' && (
                <div className="rounded-md bg-red-50 p-4 mb-4">
                  <div className="text-sm text-red-700">
                    {t('connection.error', { error: wsError })}
                  </div>
                </div>
              )}

              <button
                onClick={() => connectWs(wsUrl)}
                disabled={wsState === 'connecting'}
                className="btn-primary"
              >
                {wsState === 'connecting' ? t('connection.connecting') : t('connection.connect')}
              </button>
            </div>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="marketplace-container">
      {/* Header */}
      <header className="header">
        <div>
          <h1 className="title">{t('title')}</h1>
          <p className="text-sm text-slate-500 mt-1">{t('description')}</p>
        </div>
        <div className="flex items-center gap-4">
          <button
            onClick={() => setLang(lang === 'en' ? 'zh' : 'en')}
            className="text-sm font-semibold text-blue-600 hover:text-blue-500"
          >
            {lang === 'en' ? '中文' : 'English'}
          </button>
          <div className="connection-status">
            <span className="status-dot status-connected"></span>
            <span className="text-slate-600">{t('connection.connected')}</span>
            <button
              onClick={() => {
                if (client) {
                  localStorage.removeItem('dcc_mcp_ws_url');
                  window.location.reload();
                }
              }}
              className="text-xs text-red-600 hover:text-red-500 font-semibold ml-2"
            >
              {t('connection.disconnected')}
            </button>
          </div>
        </div>
      </header>

      {/* Summary Strip */}
      <div className="marketplace-summary-strip">
        <div className="marketplace-summary-item">
          <span>{t('metric.available')}</span>
          <strong>{catalog.length}</strong>
        </div>
        <div className="marketplace-summary-item">
          <span>{t('metric.installed')}</span>
          <strong>{installed.length}</strong>
        </div>
        <div className="marketplace-summary-item">
          <span>{t('metric.sources')}</span>
          <strong>{sources.length}</strong>
        </div>
        <div className="marketplace-summary-item">
          <span>{t('card.outdated')}</span>
          <strong className={outdated.count > 0 ? 'text-amber-600' : ''}>{outdated.count}</strong>
        </div>
      </div>

      {/* Install/Uninstall Notices */}
      {installNotice && (
        <div className="marketplace-install-notice" role="status">
          <span>
            {installNotice.action === 'update'
              ? t('update.success', { name: installNotice.name, dcc: installNotice.dcc })
              : installNotice.action === 'install'
                ? t('install.success', { name: installNotice.name, dcc: installNotice.dcc })
                : t('uninstall.success', { name: installNotice.name, dcc: installNotice.dcc })}
            {installNotice.reload_required && (
              <span className="ml-2 font-semibold text-amber-700">
                ({t('install.reloadTriggered')})
              </span>
            )}
          </span>
          <button
            type="button"
            className="marketplace-install-notice-close"
            onClick={() => setInstallNotice(null)}
          >
            &times;
          </button>
        </div>
      )}

      {/* Tabs */}
      <div className="marketplace-tabs" role="tablist">
        <button
          className={`marketplace-tab ${tab === 'browse' ? 'active' : ''}`}
          onClick={() => setTab('browse')}
        >
          {t('tab.browse')}
        </button>
        <button
          className={`marketplace-tab ${tab === 'installed' ? 'active' : ''}`}
          onClick={() => setTab('installed')}
        >
          {t('tab.installed')}
          {installed.length > 0 && (
            <span className="ml-1.5 bg-slate-100 text-slate-700 text-xs px-1.5 py-0.5 rounded-full font-bold">
              {installed.length}
            </span>
          )}
        </button>
        <button
          className={`marketplace-tab ${tab === 'sources' ? 'active' : ''}`}
          onClick={() => setTab('sources')}
        >
          {t('source.sectionTitle')}
          {sources.length > 0 && (
            <span className="ml-1.5 bg-slate-100 text-slate-700 text-xs px-1.5 py-0.5 rounded-full font-bold">
              {sources.length}
            </span>
          )}
        </button>
      </div>

      {/* Search and Filters */}
      {tab !== 'sources' && (
        <div className="flex flex-col sm:flex-row gap-4 mb-6 items-center justify-between">
          <input
            type="text"
            placeholder={t('source.addPlaceholder')}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="w-full sm:max-w-md px-4 py-2 border border-slate-200 rounded-md text-sm outline-none focus:border-blue-500"
            style={{ margin: 0 }}
          />
          {tab === 'browse' && (
            <div className="flex items-center gap-4">
              <label className="flex items-center gap-2 text-sm text-slate-600 font-medium cursor-pointer">
                <input
                  type="checkbox"
                  checked={forceInstall}
                  onChange={(e) => setForceInstall(e.target.checked)}
                  className="rounded border-slate-300 text-blue-600 focus:ring-blue-500"
                />
                {t('card.forceInstall')}
              </label>
            </div>
          )}
        </div>
      )}

      {/* DCC Filter Chips */}
      {tab === 'browse' && dccTypes.length > 0 && (
        <div className="flex flex-wrap gap-2 mb-6 items-center">
          <span className="text-sm font-semibold text-slate-500">{t('filter.dccLabel')}</span>
          <button
            onClick={() => setDccFilter(null)}
            className={`px-3 py-1 rounded-full text-xs font-semibold ${!dccFilter ? 'bg-blue-600 text-white' : 'bg-slate-100 text-slate-600 hover:bg-slate-200'}`}
          >
            {t('filter.dccAll')}
          </button>
          {dccTypes.map((dcc) => (
            <button
              key={dcc}
              onClick={() => setDccFilter(dccFilter === dcc ? null : dcc)}
              className={`px-3 py-1 rounded-full text-xs font-semibold ${dccFilter === dcc ? 'bg-blue-600 text-white' : 'bg-slate-100 text-slate-600 hover:bg-slate-200'}`}
            >
              {dcc}
            </button>
          ))}
        </div>
      )}

      {/* Content */}
      {loading ? (
        <div className="text-center py-12 text-slate-500 font-medium">{t('status.loading')}</div>
      ) : (
        <>
          {/* Browse Tab */}
          {tab === 'browse' && (
            filteredCatalog.length === 0 ? (
              <div className="text-center py-12 text-slate-500 font-medium">
                {search.trim() || dccFilter ? t('empty.search') : t('empty.none')}
              </div>
            ) : (
              <div className="marketplace-grid">
                {filteredCatalog.map((entry) => (
                  <div key={entry.name} className="marketplace-card" onClick={() => setDetailEntry(entry)}>
                    <div>
                      <div className="marketplace-card-head">
                        {entry.icon ? (
                          <img className="marketplace-card-icon" src={entry.icon} alt={entry.name} />
                        ) : (
                          <span className="marketplace-card-icon-fallback">{entry.name.charAt(0).toUpperCase()}</span>
                        )}
                        <h3 className="marketplace-card-name">{entry.name}</h3>
                      </div>
                      <p className="marketplace-card-desc">{entry.description || t('card.noDescription')}</p>
                      <div className="marketplace-card-meta">
                        <span>{t('card.version', { version: entry.version || t('card.noVersion') })}</span>
                        {entry.maintainer && <span>{t('card.author', { author: entry.maintainer })}</span>}
                      </div>
                    </div>
                    <div>
                      <div className="marketplace-card-chips mb-4">
                        {entry.dcc.map((dcc: string) => (
                          <span key={dcc} className="marketplace-card-chip">{dcc}</span>
                        ))}
                      </div>
                      <div className="marketplace-card-actions">
                        {entry.dcc.map((dcc: string) => {
                          const installedPkg = getInstalledPackage(entry.name, dcc);
                          const isInstalled = !!installedPkg;
                          const isOutdated = isInstalled && isPackageOutdated(entry.name, dcc);
                          const isInstalling = installingKey === `${entry.name}:${dcc}`;

                          return (
                            <button
                              key={dcc}
                              disabled={isInstalling || (isInstalled && !isOutdated && !forceInstall)}
                              onClick={(e) => {
                                e.stopPropagation();
                                if (isOutdated) {
                                  handleUpdate(entry.name, dcc);
                                } else if (isInstalled) {
                                  handleUninstall(installedPkg);
                                } else {
                                  handleInstall(entry, dcc);
                                }
                              }}
                              className={`btn-action text-xs ${isInstalled ? (isOutdated ? 'btn-action-primary' : 'bg-green-50 border-green-200 text-green-700') : 'btn-action-primary'}`}
                            >
                              {isInstalling
                                ? t('card.installing')
                                : isInstalled
                                  ? isOutdated
                                    ? t('card.update')
                                    : t('card.installed')
                                  : t('card.installFor', { dcc })}
                            </button>
                          );
                        })}
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            )
          )}

          {/* Installed Tab */}
          {tab === 'installed' && (
            filteredInstalled.length === 0 ? (
              <div className="text-center py-12 text-slate-500 font-medium">
                {search.trim() ? t('empty.search') : t('empty.installed')}
              </div>
            ) : (
              <div className="bg-white border border-slate-200 rounded-lg overflow-hidden">
                <table className="min-w-full divide-y divide-slate-200">
                  <thead className="bg-slate-50">
                    <tr>
                      <th className="px-6 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wider">{t('installed.package')}</th>
                      <th className="px-6 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wider">{t('card.dccLabel')}</th>
                      <th className="px-6 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wider">{t('installed.version')}</th>
                      <th className="px-6 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wider">{t('installed.source')}</th>
                      <th className="px-6 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wider">{t('installed.actions')}</th>
                    </tr>
                  </thead>
                  <tbody className="bg-white divide-y divide-slate-200">
                    {filteredInstalled.map((pkg) => {
                      const isOutdated = isPackageOutdated(pkg.name, pkg.dcc);
                      const isInstalling = installingKey === `${pkg.name}:${pkg.dcc}`;

                      return (
                        <tr key={`${pkg.name}:${pkg.dcc}`} className="hover:bg-slate-50 cursor-pointer" onClick={() => setInstalledDetail(pkg)}>
                          <td className="px-6 py-4 whitespace-nowrap">
                            <div className="font-semibold text-slate-900">{pkg.name}</div>
                          </td>
                          <td className="px-6 py-4 whitespace-nowrap">
                            <span className="px-2.5 py-0.5 rounded-full text-xs font-semibold bg-blue-50 text-blue-700">{pkg.dcc}</span>
                          </td>
                          <td className="px-6 py-4 whitespace-nowrap">
                            <div className="text-sm text-slate-500">{pkg.version || t('card.noVersion')}</div>
                          </td>
                          <td className="px-6 py-4 whitespace-nowrap">
                            <div className="text-sm text-slate-500">{pkg.source_name}</div>
                          </td>
                          <td className="px-6 py-4 whitespace-nowrap text-sm font-medium">
                            <div className="flex gap-2" onClick={(e) => e.stopPropagation()}>
                              {isOutdated && (
                                <button
                                  onClick={() => handleUpdate(pkg.name, pkg.dcc)}
                                  disabled={isInstalling}
                                  className="text-blue-600 hover:text-blue-900 font-semibold"
                                >
                                  {isInstalling ? t('card.updating') : t('card.update')}
                                </button>
                              )}
                              <button
                                onClick={() => handleUninstall(pkg)}
                                disabled={isInstalling}
                                className="text-red-600 hover:text-red-900 font-semibold"
                              >
                                {isInstalling ? t('card.uninstalling') : t('card.uninstall')}
                              </button>
                            </div>
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            )
          )}

          {/* Sources Tab */}
          {tab === 'sources' && (
            <div className="space-y-6">
              <div className="bg-white border border-slate-200 rounded-lg p-6">
                <h3 className="text-lg font-bold text-slate-900 mb-4">{t('source.addTitle')}</h3>
                <div className="flex flex-col sm:flex-row gap-4">
                  <input
                    type="text"
                    placeholder={t('source.addPlaceholder')}
                    value={sourceInput}
                    onChange={(e) => setSourceInput(e.target.value)}
                    className="flex-1 px-4 py-2 border border-slate-200 rounded-md text-sm outline-none focus:border-blue-500"
                    style={{ margin: 0 }}
                  />
                  <button
                    onClick={handleAddSource}
                    disabled={!sourceInput.trim()}
                    className="btn-primary sm:w-auto"
                  >
                    {t('source.addLabel')}
                  </button>
                </div>
              </div>

              {sources.length === 0 ? (
                <div className="text-center py-12 text-slate-500 font-medium">{t('source.empty')}</div>
              ) : (
                <div className="bg-white border border-slate-200 rounded-lg overflow-hidden">
                  <table className="min-w-full divide-y divide-slate-200">
                    <thead className="bg-slate-50">
                      <tr>
                        <th className="px-6 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wider">{t('source.name')}</th>
                        <th className="px-6 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wider">{t('source.url')}</th>
                        <th className="px-6 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wider">{t('source.origin')}</th>
                      </tr>
                    </thead>
                    <tbody className="bg-white divide-y divide-slate-200">
                      {sources.map((source) => (
                        <tr key={source.name} className="hover:bg-slate-50">
                          <td className="px-6 py-4 whitespace-nowrap font-semibold text-slate-900">{source.name}</td>
                          <td className="px-6 py-4 whitespace-nowrap text-sm text-slate-500">{source.url}</td>
                          <td className="px-6 py-4 whitespace-nowrap">
                            <span className="px-2.5 py-0.5 rounded-full text-xs font-semibold bg-slate-100 text-slate-700">{source.origin}</span>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </div>
          )}
        </>
      )}

      {/* Detail Modal */}
      {detailEntry && (
        <div className="modal-backdrop" onClick={() => setDetailEntry(null)}>
          <div className="modal-content" onClick={(e) => e.stopPropagation()}>
            <button className="modal-close" onClick={() => setDetailEntry(null)}>&times;</button>
            <h2 className="text-2xl font-bold text-slate-900 mb-4">{detailEntry.name}</h2>
            <div className="space-y-4">
              <div>
                <h4 className="text-xs font-semibold text-slate-400 uppercase tracking-wider">{t('detail.description')}</h4>
                <p className="text-slate-700 mt-1">{detailEntry.description || t('card.noDescription')}</p>
              </div>
              <div className="grid grid-cols-2 gap-4">
                <div>
                  <h4 className="text-xs font-semibold text-slate-400 uppercase tracking-wider">{t('detail.version')}</h4>
                  <p className="text-slate-700 mt-1">{detailEntry.version || t('card.noVersion')}</p>
                </div>
                <div>
                  <h4 className="text-xs font-semibold text-slate-400 uppercase tracking-wider">{t('detail.maintainer')}</h4>
                  <p className="text-slate-700 mt-1">{detailEntry.maintainer || t('detail.noMaintainer')}</p>
                </div>
              </div>
              {detailEntry.url && (
                <div>
                  <h4 className="text-xs font-semibold text-slate-400 uppercase tracking-wider">{t('detail.url')}</h4>
                  <a href={detailEntry.url} target="_blank" rel="noopener noreferrer" className="text-blue-600 hover:underline mt-1 block">
                    {detailEntry.url}
                  </a>
                </div>
              )}
              <div>
                <h4 className="text-xs font-semibold text-slate-400 uppercase tracking-wider">{t('detail.dcc')}</h4>
                <div className="flex flex-wrap gap-2 mt-1">
                  {detailEntry.dcc.map((dcc: string) => (
                    <span key={dcc} className="px-2.5 py-0.5 rounded-full text-xs font-semibold bg-blue-50 text-blue-700">{dcc}</span>
                  ))}
                </div>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Installed Detail Modal */}
      {installedDetail && (
        <div className="modal-backdrop" onClick={() => setInstalledDetail(null)}>
          <div className="modal-content" onClick={(e) => e.stopPropagation()}>
            <button className="modal-close" onClick={() => setInstalledDetail(null)}>&times;</button>
            <h2 className="text-2xl font-bold text-slate-900 mb-4">{installedDetail.name}</h2>
            <div className="space-y-4">
              <div className="grid grid-cols-2 gap-4">
                <div>
                  <h4 className="text-xs font-semibold text-slate-400 uppercase tracking-wider">{t('card.dccLabel')}</h4>
                  <p className="text-slate-700 mt-1">{installedDetail.dcc}</p>
                </div>
                <div>
                  <h4 className="text-xs font-semibold text-slate-400 uppercase tracking-wider">{t('detail.version')}</h4>
                  <p className="text-slate-700 mt-1">{installedDetail.version || t('card.noVersion')}</p>
                </div>
              </div>
              <div>
                <h4 className="text-xs font-semibold text-slate-400 uppercase tracking-wider">Path</h4>
                <p className="text-slate-500 font-mono text-xs mt-1 break-all bg-slate-50 p-2 rounded border border-slate-100">{installedDetail.path}</p>
              </div>
              <div className="grid grid-cols-2 gap-4">
                <div>
                  <h4 className="text-xs font-semibold text-slate-400 uppercase tracking-wider">{t('detail.source')}</h4>
                  <p className="text-slate-700 mt-1">{installedDetail.source_name}</p>
                </div>
                <div>
                  <h4 className="text-xs font-semibold text-slate-400 uppercase tracking-wider">{t('detail.installType')}</h4>
                  <p className="text-slate-700 mt-1">{installedDetail.install_type}</p>
                </div>
              </div>
              <div className="pt-4 border-t border-slate-100 flex justify-end gap-2">
                {isPackageOutdated(installedDetail.name, installedDetail.dcc) && (
                  <button
                    onClick={() => handleUpdate(installedDetail.name, installedDetail.dcc)}
                    className="btn-action btn-action-primary"
                  >
                    {t('card.update')}
                  </button>
                )}
                <button
                  onClick={() => handleUninstall(installedDetail)}
                  className="btn-action text-red-600 border-red-200 hover:bg-red-50"
                >
                  {t('card.uninstall')}
                </button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
