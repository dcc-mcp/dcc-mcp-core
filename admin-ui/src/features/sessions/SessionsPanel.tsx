import { useMemo, useState } from 'react';
import { RiRefreshLine, RiArrowDownSLine, RiArrowRightSLine } from '@remixicon/react';
import { useQuery } from '@tanstack/react-query';
import { Button } from '../../components/ui/button';
import { Badge } from '../../components/ui/badge';
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../../components/ui/select';
import { PanelHeader, StatusLine, EmptyRow } from '../../admin-ui-core';
import { API_BASE } from '../../platform';
import type { SessionRow, SessionsPayload, SessionStatus, Translator } from '../../admin-types';
import { adminKeys } from '../../hooks/queries';
import { adminJsonResponse } from '../../admin-ui-core';
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
  offset?: number;
};

async function fetchSessions(params: SessionsQueryParams): Promise<SessionsPayload> {
  const u = new URL(`${API_BASE}/sessions`, window.location.origin);
  if (params.dcc_type) u.searchParams.set('dcc_type', params.dcc_type);
  if (params.status) u.searchParams.set('status', params.status);
  if (params.search) u.searchParams.set('search', params.search);
  if (params.limit) u.searchParams.set('limit', String(params.limit));
  if (params.offset) u.searchParams.set('offset', String(params.offset));
  const resp = await fetch(u.toString());
  return adminJsonResponse<SessionsPayload>(resp);
}

const STATUS_COLORS: Record<SessionStatus, string> = {
  active: 'ok',
  ended: 'muted',
  crashed: 'err',
  interrupted: 'warn',
  timed_out: 'warn',
  cancelled: 'muted',
  unknown: 'muted',
};

function statusLabel(t: Translator, status: SessionStatus): string {
  return t(`sessions.status.${status}` as Parameters<Translator>[0]);
}

function truncateId(id: string): string {
  return id.length > 12 ? `${id.slice(0, 6)}...${id.slice(-6)}` : id;
}

type TreeNode = SessionRow & { children: TreeNode[]; depth: number };

function buildSessionTree(sessions: SessionRow[]): SessionRow[] {
  const map = new Map<string, TreeNode>();
  const roots: TreeNode[] = [];

  for (const s of sessions) {
    map.set(s.session_id, { ...s, children: [], depth: 0 });
  }
  for (const node of map.values()) {
    if (node.parent_session_id && map.has(node.parent_session_id)) {
      map.get(node.parent_session_id)!.children.push(node);
    } else {
      roots.push(node);
    }
  }

  function flatten(nodes: TreeNode[], depth: number): SessionRow[] {
    const result: SessionRow[] = [];
    for (const node of nodes) {
      node.depth = depth;
      result.push(node);
      if (node.children.length > 0) {
        result.push(...flatten(node.children, depth + 1));
      }
    }
    return result;
  }

  return flatten(roots, 0);
}

function KpiCard({ label, value, tone }: { label: string; value: string | number; tone?: 'ok' | 'warn' | 'err' | 'muted' }) {
  return (
    <div className="sessions-kpi-card">
      <span className={`sessions-kpi-value ${tone ? `sessions-kpi-${tone}` : ''}`}>{value}</span>
      <span className="sessions-kpi-label">{label}</span>
    </div>
  );
}

export function SessionsPanel({
  active,
  updatedAt,
  error,
  onRefresh,
  t,
}: SessionsPanelProps) {
  const [dccType, setDccType] = useState<string>('all');
  const [statusFilter, setStatusFilter] = useState<string>('all');
  const [search, setSearch] = useState('');
  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set());

  const queryParams: SessionsQueryParams = {
    limit: 200,
  };
  if (dccType !== 'all') queryParams.dcc_type = dccType;
  if (statusFilter !== 'all') queryParams.status = statusFilter;
  if (search.trim()) queryParams.search = search.trim();

  const sessionsQuery = useQuery({
    queryKey: [...adminKeys.all, 'sessions', queryParams],
    queryFn: () => fetchSessions(queryParams),
    enabled: active,
    refetchInterval: active ? 5000 : false,
    staleTime: 4000,
    gcTime: 30000,
  });

  const sessions = sessionsQuery.data?.sessions ?? [];
  const kpi = sessionsQuery.data?.kpi ?? { total: 0, active: 0, ended: 0, crashed: 0, by_dcc: {} };
  const tree = useMemo(() => buildSessionTree(sessions), [sessions]);
  const dccTypes = useMemo(() => {
    const types = new Set<string>();
    for (const s of sessions) {
      if (s.dcc_type) types.add(s.dcc_type);
    }
    return Array.from(types).sort();
  }, [sessions]);

  const toggleExpand = (id: string) => {
    setExpandedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  if (!active) return null;

  return (
    <section className="panel active sessions-panel">
      <PanelHeader
        title={t('sessions.panel.title')}
        action={(
          <Button type="button" size="sm" onClick={() => { onRefresh(); sessionsQuery.refetch(); }}>
            <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
            {t('action.refresh')}
          </Button>
        )}
      />
      <StatusLine text={updatedAt || (sessionsQuery.isLoading ? t('common.status.loading') : '')} error={error || sessionsQuery.error?.message} />

      <div className="sessions-kpi-row">
        <KpiCard label={t('sessions.kpi.total')} value={kpi.total} />
        <KpiCard label={t('sessions.kpi.active')} value={kpi.active} tone="ok" />
        <KpiCard label={t('sessions.kpi.ended')} value={kpi.ended} tone="muted" />
        <KpiCard label={t('sessions.kpi.crashed')} value={kpi.crashed} tone="err" />
      </div>

      <div className="sessions-filter-bar">
        <div className="sessions-filter-group">
          <Select value={dccType} onValueChange={setDccType}>
            <SelectTrigger className="admin-select-trigger sessions-filter-select" size="sm">
              <SelectValue placeholder={t('sessions.filter.dccType')} />
            </SelectTrigger>
            <SelectContent className="admin-select-content" position="popper" align="start">
              <SelectGroup>
                <SelectItem value="all">{t('sessions.filter.all')}</SelectItem>
                {dccTypes.map((dt) => (
                  <SelectItem key={dt} value={dt}>{dt}</SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
        </div>
        <div className="sessions-filter-group">
          <Select value={statusFilter} onValueChange={setStatusFilter}>
            <SelectTrigger className="admin-select-trigger sessions-filter-select" size="sm">
              <SelectValue placeholder={t('sessions.filter.status')} />
            </SelectTrigger>
            <SelectContent className="admin-select-content" position="popper" align="start">
              <SelectGroup>
                <SelectItem value="all">{t('sessions.filter.all')}</SelectItem>
                <SelectItem value="active">{statusLabel(t, 'active')}</SelectItem>
                <SelectItem value="ended">{statusLabel(t, 'ended')}</SelectItem>
                <SelectItem value="crashed">{statusLabel(t, 'crashed')}</SelectItem>
              </SelectGroup>
            </SelectContent>
          </Select>
        </div>
        <div className="sessions-search-group">
          <input
            type="text"
            className="sessions-search-input"
            placeholder={t('sessions.filter.search')}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
      </div>

      <div className="sessions-table-wrap">
        <table className="sessions-table">
          <thead>
            <tr>
              <th>{t('sessions.table.sessionId')}</th>
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
              <EmptyRow columns={8}>{sessionsQuery.isLoading ? t('common.status.loading') : t('sessions.empty.title')}</EmptyRow>
            ) : (
              tree.map((row) => {
                const isExpanded = expandedIds.has(row.session_id);
                const indent = (row as any).depth ?? 0;
                return (
                  <>
                    <tr key={row.session_id} className="sessions-row">
                      <td>
                        <span className="sessions-indent" style={{ paddingLeft: `${indent * 20}px` }}>
                          {indent > 0 && (
                            <button
                              type="button"
                              className="sessions-expand-btn"
                              onClick={() => toggleExpand(row.session_id)}
                              aria-label={isExpanded ? 'Collapse' : 'Expand'}
                            >
                              {isExpanded ? <RiArrowDownSLine size={14} /> : <RiArrowRightSLine size={14} />}
                            </button>
                          )}
                          <code className="sessions-id">{truncateId(row.session_id)}</code>
                        </span>
                      </td>
                      <td>
                        <Badge className={`sessions-status-badge sessions-status-${row.status}`}>{statusLabel(t, row.status)}</Badge>
                      </td>
                      <td>{row.dcc_type || '-'}</td>
                      <td>{row.agent_name || row.agent_id || '-'}</td>
                      <td>{row.started_at}</td>
                      <td>{row.duration_ms != null ? `${Math.round(row.duration_ms / 1000)}s` : '-'}</td>
                      <td>{row.turn_count}</td>
                      <td>{row.tool_call_count}</td>
                    </tr>
                    {isExpanded && (
                      <tr key={`${row.session_id}-detail`} className="sessions-detail-row">
                        <td colSpan={8}>
                          <div className="sessions-detail">
                            <div className="sessions-detail-grid">
                              <div>
                                <span className="sessions-detail-label">{t('sessions.detail.parentInfo')}</span>
                                <code>{row.parent_session_id ? truncateId(row.parent_session_id) : t('sessions.detail.noParent')}</code>
                              </div>
                              <div>
                                <span className="sessions-detail-label">{t('sessions.detail.version')}</span>
                                <span>{row.version || '-'}</span>
                              </div>
                              <div>
                                <span className="sessions-detail-label">{t('sessions.detail.endReason')}</span>
                                <span>{row.end_reason || t('sessions.detail.noEndReason')}</span>
                              </div>
                            </div>
                          </div>
                        </td>
                      </tr>
                    )}
                  </>
                );
              })
            )}
          </tbody>
        </table>
      </div>
    </section>
  );
}
