import { useMemo, useState } from 'react';
import { RiDeleteBinLine, RiRefreshLine } from '@remixicon/react';
import { EmptyRow, MetricTile, PanelHeader, StatusLine, TimeValue } from '../../admin-ui-core';
import type { MemoryFilters, MemoryRow, Translator } from '../../admin-types';
import { Button } from '../../components/ui/button';
import { useForgetMemory, useMemoryQuery } from '../../hooks/queries';
import './memory.css';

const LAYERS = ['all', 'longterm', 'working', 'ephemeral'] as const;
type LayerFilter = typeof LAYERS[number];

function formatRate(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return '-';
  return `${value.toFixed(1)}%`;
}

function createdIso(row: MemoryRow): string {
  const millis = Number(row.created_unix_secs) * 1000;
  return Number.isFinite(millis) ? new Date(millis).toISOString() : '';
}

function payloadText(row: MemoryRow): string {
  return JSON.stringify(row.payload ?? {}, null, 2);
}

function rowHaystack(row: MemoryRow): string {
  return [
    row.layer,
    row.key,
    row.session_id,
    row.dcc_name,
    String(row.score),
    payloadText(row),
  ].join(' ').toLowerCase();
}

export function MemoryPanel({ active, t }: { active: boolean; t: Translator }) {
  const [layer, setLayer] = useState<LayerFilter>('longterm');
  const [dccName, setDccName] = useState('');
  const [keyPrefix, setKeyPrefix] = useState('');
  const [search, setSearch] = useState('');
  const filters: MemoryFilters = useMemo(
    () => ({
      layer,
      dccName: dccName.trim(),
      keyPrefix: keyPrefix.trim(),
      limit: 300,
    }),
    [dccName, keyPrefix, layer],
  );
  const memoryQuery = useMemoryQuery(active, filters);
  const forgetMemory = useForgetMemory();
  const payload = memoryQuery.data ?? null;
  const rows = payload?.memory ?? [];
  const visibleRows = useMemo(() => {
    const needle = search.trim().toLowerCase();
    if (!needle) return rows;
    return rows.filter((row) => rowHaystack(row).includes(needle));
  }, [rows, search]);
  const hasForgetSelector = Boolean(dccName.trim()) || Boolean(keyPrefix.trim());
  const busy = memoryQuery.isLoading || forgetMemory.isPending;
  const error = memoryQuery.error?.message ?? (payload?.enabled === false ? payload.error : undefined);

  if (!active) return null;

  const forgetMatches = () => {
    if (!hasForgetSelector) return;
    const body: { layer?: string; dcc_name?: string; key_prefix?: string } = {};
    if (layer !== 'all') body.layer = layer;
    if (dccName.trim()) body.dcc_name = dccName.trim();
    if (keyPrefix.trim()) body.key_prefix = keyPrefix.trim();
    forgetMemory.mutate(body);
  };

  return (
    <section className="panel active memory-panel" data-panel="memory">
      <PanelHeader
        title={t('memory.title')}
        action={
          <div className="memory-actions">
            <Button type="button" size="sm" disabled={busy} onClick={() => { void memoryQuery.refetch(); }}>
              <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
              {t('memory.action.refresh')}
            </Button>
            <Button
              type="button"
              size="sm"
              variant="destructive"
              disabled={!hasForgetSelector || busy}
              onClick={forgetMatches}
            >
              <RiDeleteBinLine data-icon="inline-start" aria-hidden="true" />
              {forgetMemory.isPending ? t('memory.action.forgetting') : t('memory.action.forget')}
            </Button>
          </div>
        }
      />
      <StatusLine
        text={
          memoryQuery.isLoading
            ? t('memory.status.loading')
            : payload?.enabled === false
              ? t('memory.status.disabled')
              : `${visibleRows.length} / ${rows.length}`
        }
        error={error}
      />

      <div className="memory-filter-bar">
        <select
          className="filter-input memory-filter-input"
          value={layer}
          aria-label={t('memory.filter.layer')}
          onChange={(event) => setLayer(event.target.value as LayerFilter)}
        >
          {LAYERS.map((item) => (
            <option key={item} value={item}>{t(`memory.filter.${item}` as any)}</option>
          ))}
        </select>
        <input
          className="filter-input memory-filter-input"
          value={dccName}
          onChange={(event) => setDccName(event.target.value)}
          placeholder={t('memory.filter.dcc')}
        />
        <input
          className="filter-input memory-filter-input"
          value={keyPrefix}
          onChange={(event) => setKeyPrefix(event.target.value)}
          placeholder={t('memory.filter.keyPrefix')}
        />
        <input
          className="filter-input memory-filter-search"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          placeholder={t('memory.filter.search')}
        />
      </div>

      <div className="metric-grid compact memory-metrics">
        <MetricTile label={t('memory.metric.records')} value={payload?.summary.total ?? 0} />
        <MetricTile label={t('memory.metric.hitRate')} value={formatRate(payload?.summary.hit_rate_pct)} />
        <MetricTile
          label={t('memory.metric.outcomes')}
          value={t('memory.label.okFail', {
            ok: payload?.summary.ok_count ?? 0,
            fail: payload?.summary.fail_count ?? 0,
          })}
          detail={t('memory.label.positiveNegative', {
            positive: payload?.summary.positive ?? 0,
            negative: payload?.summary.negative ?? 0,
          })}
        />
        <MetricTile label={t('memory.metric.dccs')} value={Object.keys(payload?.summary.by_dcc ?? {}).length} />
      </div>

      <div className="table-scroll memory-table-wrap">
        <table className="admin-table memory-table">
          <thead>
            <tr>
              <th>{t('memory.table.created')}</th>
              <th>{t('memory.table.layer')}</th>
              <th>{t('memory.table.key')}</th>
              <th>{t('memory.table.dcc')}</th>
              <th>{t('memory.table.session')}</th>
              <th>{t('memory.table.score')}</th>
              <th>{t('memory.table.payload')}</th>
              <th>{t('memory.table.actions')}</th>
            </tr>
          </thead>
          <tbody>
            {visibleRows.length === 0 ? (
              <EmptyRow columns={8}>{t('memory.empty.noData')}</EmptyRow>
            ) : (
              visibleRows.map((row) => (
                <tr key={row.id}>
                  <td><TimeValue value={createdIso(row)} /></td>
                  <td><span className="source-pill">{row.layer}</span></td>
                  <td><code title={row.key}>{row.key}</code></td>
                  <td>{row.dcc_name || '-'}</td>
                  <td><span className="mono-path">{row.session_id}</span></td>
                  <td className={row.score > 0 ? 'memory-score-ok' : row.score < 0 ? 'memory-score-err' : ''}>
                    {row.score.toFixed(1)}
                  </td>
                  <td><pre className="memory-payload">{payloadText(row)}</pre></td>
                  <td>
                    <Button
                      type="button"
                      size="xs"
                      variant="ghost"
                      disabled={busy}
                      onClick={() => forgetMemory.mutate({ id: row.id })}
                    >
                      <RiDeleteBinLine data-icon="inline-start" aria-hidden="true" />
                      {t('memory.action.forgetRow')}
                    </Button>
                  </td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>
    </section>
  );
}
