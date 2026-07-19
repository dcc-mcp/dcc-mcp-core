import { useMemo, useState, type ReactNode } from 'react';
import { RiArrowDownSLine, RiArrowRightSLine, RiInformationLine, RiRefreshLine } from '@remixicon/react';
import { useQuery } from '@tanstack/react-query';
import { Button } from '../../components/ui/button';
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../../components/ui/select';
import { EmptyRow, MetricTile, PanelHeader, StatusLine, TimeValue, formatDurationMs } from '../../admin-ui-core';
import { apiJson } from '../../platform';
import type { SessionKpi, SessionRow, SessionsPayload, SessionStatus, Translator } from '../../admin-types';
import type { MessageKey } from '../../i18n';
import { adminKeys } from '../../hooks/queries';
import './sessions.css';

export type SessionsPanelProps = {
  active: boolean;
  updatedAt: string;
  error?: string;
  onRefresh: () => void;
  t: Translator;
};

type SessionsQueryParams = {
  dcc_type?: string;
  status?: string;
  search?: string;
  limit?: number;
};

const EMPTY_KPI: SessionKpi = { total: 0, active: 0, ended: 0, crashed: 0, by_dcc: {} };

const SESSION_STATUSES: SessionStatus[] = ['active', 'ended', 'crashed', 'interrupted', 'timed_out', 'cancelled', 'unknown'];

const STATUS_TONE: Record<SessionStatus, 'ok' | 'warn' | 'err' | 'muted'> = {
  active: 'ok',
  ended: 'muted',
  crashed: 'err',
  interrupted: 'warn',
  timed_out: 'warn',
  cancelled: 'muted',
  unknown: 'muted',
};

// SessionStatus values are snake_case but locale keys are camelCase (e.g. `timed_out` -> `timedOut`).
const STATUS_LOCALE_KEY: Record<SessionStatus, string> = {
  active: 'active',
  ended: 'ended',
  crashed: 'crashed',
  interrupted: 'interrupted',
  timed_out: 'timedOut',
  cancelled: 'cancelled',
  unknown: 'unknown',
};

function statusLabel(t: Translator, status: SessionStatus): string {
  return t(`sessions.status.${STATUS_LOCALE_KEY[status]}` as MessageKey);
}

function SessionStatusBadge({ status, t }: { status: SessionStatus; t: Translator }) {
  return <span className={`badge badge-${STATUS_TONE[status]}`}>{statusLabel(t, status)}</span>;
}

function truncateId(id: string | null | undefined): string {
  if (!id) return '-';
  return id.length > 14 ? `${id.slice(0, 8)}…${id.slice(-4)}` : id;
}

async function fetchSessions(params: SessionsQueryParams): Promise<SessionsPayload> {
  const search = new URLSearchParams();
  if (params.dcc_type) search.set('dcc_type', params.dcc_type);
  if (params.status) search.set('status', params.status);
  if (params.search) search.set('search', params.search);
  if (params.limit) search.set('limit', String(params.limit));
  const qs = search.toString();
  return apiJson<SessionsPayload>(qs ? `/sessions?${qs}` : '/sessions');
}

type SessionNode = SessionRow & { children: SessionNode[] };

function buildSessionTree(sessions: SessionRow[]): SessionNode[] {
  const nodes = new Map<string, SessionNode>();
  for (const row of sessions) {
    nodes.set(row.session_id, { ...row, children: [] });
  }
  const roots: SessionNode[] = [];
  for (const node of nodes.values()) {
    const parent = node.parent_session_id ? nodes.get(node.parent_session_id) : undefined;
    if (parent) {
      parent.children.push(node);
    } else {
      roots.push(node);
    }
  }
  return roots;
}

type RowRenderContext = {
  t: Translator;
  collapsedIds: Set<string>;
  detailIds: Set<string>;
  toggleCollapse: (id: string) => void;
  toggleDetail: (id: string) => void;
};

function renderSessionRows(nodes: SessionNode[], depth: number, ctx: RowRenderContext): ReactNode[] {
  const { t, collapsedIds, detailIds, toggleCollapse, toggleDetail } = ctx;
  const rows: ReactNode[] = [];

  for (const node of nodes) {
    const hasChildren = node.children.length > 0;
    const isCollapsed = collapsedIds.has(node.session_id);
    const showDetail = detailIds.has(node.session_id);

    rows.push(
      <tr key={node.session_id} className="sessions-row">
        <td>
          <span className="sessions-indent" style={{ paddingLeft: `${depth * 20}px` }}>
            {hasChildren ? (
              <button
                type="button"
                className="sessions-tree-btn"
                onClick={() => toggleCollapse(node.session_id)}
                aria-label={isCollapsed ? 'Expand children' : 'Collapse children'}
              >
                {isCollapsed ? <RiArrowRightSLine size={14} /> : <RiArrowDownSLine size={14} />}
              </button>
            ) : (
              <span className="sessions-tree-spacer" />
            )}
            <button
              type="button"
              className="sessions-detail-btn"
              onClick={() => toggleDetail(node.session_id)}
              aria-label="Toggle session detail"
              aria-pressed={showDetail}
            >
              <RiInformationLine size={14} />
            </button>
            <code className="sessions-id" title={node.session_id}>{truncateId(node.session_id)}</code>
          </span>
        </td>
        <td>
          {node.parent_session_id ? (
            <code className="sessions-id sessions-id-muted" title={node.parent_session_id}>
              {truncateId(node.parent_session_id)}
            </code>
          ) : (
            <span className="sessions-id-muted">{t('sessions.detail.noParent')}</span>
          )}
        </td>
        <td><SessionStatusBadge status={node.status} t={t} /></td>
        <td>{node.dcc_type || '-'}</td>
        <td>{node.agent_name || node.agent_id || '-'}</td>
        <td><TimeValue value={node.started_at} /></td>
        <td>{formatDurationMs(node.duration_ms)}</td>
        <td>{node.turn_count}</td>
        <td>{node.tool_call_count}</td>
      </tr>,
    );

    if (showDetail) {
      rows.push(
        <tr key={`${node.session_id}-detail`} className="sessions-detail-row">
          <td colSpan={9}>
            <div className="sessions-detail">
              <div className="sessions-detail-grid">
                <div>
                  <span className="sessions-detail-label">{t('sessions.detail.parentInfo')}</span>
                  <code>{node.parent_session_id ? truncateId(node.parent_session_id) : t('sessions.detail.noParent')}</code>
                </div>
                <div>
                  <span className="sessions-detail-label">{t('sessions.detail.version')}</span>
                  <span>{node.version || '-'}</span>
                </div>
                <div>
                  <span className="sessions-detail-label">{t('sessions.detail.endReason')}</span>
                  <span>{node.end_reason || t('sessions.detail.noEndReason')}</span>
                </div>
              </div>
            </div>
          </td>
        </tr>,
      );
    }

    if (hasChildren && !isCollapsed) {
      rows.push(...renderSessionRows(node.children, depth + 1, ctx));
    }
  }

  return rows;
}

export function SessionsPanel({ active, updatedAt, error, onRefresh, t }: SessionsPanelProps) {
  const [dccType, setDccType] = useState('all');
  const [statusFilter, setStatusFilter] = useState('all');
  const [search, setSearch] = useState('');
  const [collapsedIds, setCollapsedIds] = useState<Set<string>>(new Set());
  const [detailIds, setDetailIds] = useState<Set<string>>(new Set());

  const queryParams: SessionsQueryParams = { limit: 200 };
  if (dccType !== 'all') queryParams.dcc_type = dccType;
  if (statusFilter !== 'all') queryParams.status = statusFilter;
  if (search.trim()) queryParams.search = search.trim();

  const sessionsQuery = useQuery({
    queryKey: [...adminKeys.all, 'sessions', queryParams],
    queryFn: () => fetchSessions(queryParams),
    enabled: active,
    refetchInterval: active ? 5000 : false,
    staleTime: 4000,
    gcTime: 30_000,
  });

  const sessions = sessionsQuery.data?.sessions ?? [];
  const kpi = sessionsQuery.data?.kpi ?? EMPTY_KPI;
  const tree = useMemo(() => buildSessionTree(sessions), [sessions]);
  const dccTypeOptions = useMemo(() => Object.keys(kpi.by_dcc).sort(), [kpi.by_dcc]);

  const toggleCollapse = (id: string) => {
    setCollapsedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const toggleDetail = (id: string) => {
    setDetailIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  if (!active) return null;

  const isLoading = sessionsQuery.isLoading;
  const statusText = updatedAt || (isLoading ? t('common.status.loading') : `${sessions.length} / ${kpi.total}`);
  const errorText = error || sessionsQuery.error?.message;

  return (
    <section className="panel active sessions-panel" data-panel="sessions">
      <PanelHeader
        title={t('sessions.panel.title')}
        meta={t('sessions.panel.description')}
        action={(
          <Button type="button" size="sm" onClick={() => { onRefresh(); void sessionsQuery.refetch(); }}>
            <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
            {t('action.refresh')}
          </Button>
        )}
      />
      <StatusLine text={statusText} error={errorText} />

      <div className="metric-grid compact sessions-kpi-row">
        <MetricTile label={t('sessions.kpi.total')} value={kpi.total} />
        <MetricTile label={t('sessions.kpi.active')} value={kpi.active} tone="ok" />
        <MetricTile label={t('sessions.kpi.ended')} value={kpi.ended} />
        <MetricTile label={t('sessions.kpi.crashed')} value={kpi.crashed} tone={kpi.crashed > 0 ? 'err' : undefined} />
      </div>

      {Object.keys(kpi.by_dcc).length > 0 ? (
        <div className="sessions-dcc-breakdown">
          <span className="sessions-dcc-breakdown-label">{t('sessions.kpi.byDcc')}</span>
          <div className="sessions-dcc-chips">
            {Object.entries(kpi.by_dcc).map(([dcc, count]) => (
              <span key={dcc} className="sessions-dcc-chip">
                <span className="sessions-dcc-chip-name">{dcc}</span>
                <span className="sessions-dcc-chip-count">{count}</span>
              </span>
            ))}
          </div>
        </div>
      ) : null}

      <div className="sessions-filter-bar">
        <Select value={dccType} onValueChange={setDccType}>
          <SelectTrigger className="admin-select-trigger sessions-filter-select" size="sm" aria-label={t('sessions.filter.dccType')}>
            <SelectValue placeholder={t('sessions.filter.dccType')} />
          </SelectTrigger>
          <SelectContent className="admin-select-content" position="popper" align="start">
            <SelectGroup>
              <SelectItem value="all">{t('sessions.filter.all')}</SelectItem>
              {dccTypeOptions.map((dt) => (
                <SelectItem key={dt} value={dt}>{dt}</SelectItem>
              ))}
            </SelectGroup>
          </SelectContent>
        </Select>

        <Select value={statusFilter} onValueChange={setStatusFilter}>
          <SelectTrigger className="admin-select-trigger sessions-filter-select" size="sm" aria-label={t('sessions.filter.status')}>
            <SelectValue placeholder={t('sessions.filter.status')} />
          </SelectTrigger>
          <SelectContent className="admin-select-content" position="popper" align="start">
            <SelectGroup>
              <SelectItem value="all">{t('sessions.filter.all')}</SelectItem>
              {SESSION_STATUSES.map((s) => (
                <SelectItem key={s} value={s}>{statusLabel(t, s)}</SelectItem>
              ))}
            </SelectGroup>
          </SelectContent>
        </Select>

        <input
          type="text"
          className="filter-input sessions-search-input"
          placeholder={t('sessions.filter.search')}
          value={search}
          onChange={(event) => setSearch(event.target.value)}
        />
      </div>

      <div className="table-scroll sessions-table-wrap">
        <table className="admin-table sessions-table">
          <thead>
            <tr>
              <th>{t('sessions.table.sessionId')}</th>
              <th>{t('sessions.table.parent')}</th>
              <th>{t('sessions.table.status')}</th>
              <th>{t('sessions.table.dccType')}</th>
              <th>{t('sessions.table.agent')}</th>
              <th>{t('sessions.table.started')}</th>
              <th>{t('sessions.table.duration')}</th>
              <th>{t('sessions.table.turns')}</th>
              <th>{t('sessions.table.tools')}</th>
            </tr>
          </thead>
          <tbody>
            {tree.length === 0 ? (
              <EmptyRow columns={9}>{isLoading ? t('common.status.loading') : t('sessions.empty.title')}</EmptyRow>
            ) : (
              renderSessionRows(tree, 0, { t, collapsedIds, detailIds, toggleCollapse, toggleDetail })
            )}
          </tbody>
        </table>
      </div>
    </section>
  );
}
