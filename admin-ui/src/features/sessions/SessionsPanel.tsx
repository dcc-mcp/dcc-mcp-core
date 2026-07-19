import { Fragment, useMemo, useState } from 'react';
import { RiRefreshLine } from '@remixicon/react';
import {
  API_BASE,
  PanelHeader,
  StatusLine,
  MetricTile,
  apiJson,
  compactId,
  formatDurationMs,
  haystack,
  matchesListFilter,
} from '../../admin-ui-core';
import { formatTime } from '../../time';
import { Badge } from '../../components/ui/badge';
import { Button } from '../../components/ui/button';
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../../components/ui/select';
import { useQuery } from '@tanstack/react-query';
import type { Translator } from '../../admin-types';
import type { MessageKey } from '../../i18n';
import './sessions.css';

// ── types ──────────────────────────────────────────────────────────────────

type SessionStatus =
  | 'active'
  | 'ended'
  | 'disconnected'
  | 'crashed'
  | 'gpu_crashed'
  | 'timed_out'
  | 'cancelled'
  | 'thread_affinity_failure';

type SessionEndReason =
  | { normal: null }
  | { disconnect: { detail: string } }
  | { host_crash: { detail: string } }
  | { gpu_crash: { detail: string } }
  | { timeout: { detail: string } }
  | { cancelled: { detail: string } }
  | { thread_affinity_failure: { detail: string } };

type SessionRow = {
  session_id: string;
  parent_session_id?: string | null;
  dcc_type: string;
  instance_id?: string | null;
  status: SessionStatus;
  started_at_ms: number;
  last_activity_at_ms: number;
  ended_at_ms?: number | null;
  end_reason?: SessionEndReason | null;
  tool_call_count: number;
  error_count: number;
  core_version: string;
  adapter_version?: string | null;
  build_sha?: string | null;
};

type SessionsPayload = {
  sessions: SessionRow[];
  total: number;
  active: number;
  ended: number;
  by_dcc: Record<string, number>;
  by_status: Record<string, number>;
};

// ── helpers ────────────────────────────────────────────────────────────────

const POLL_INTERVAL_MS = 5_000;
const ALL_DCC = '__all__';
const ALL_STATUS = '__all__';

function statusBadgeClass(status: SessionStatus): string {
  switch (status) {
    case 'active':
      return 'badge badge-ok';
    case 'ended':
    case 'cancelled':
      return 'badge badge-muted';
    case 'disconnected':
    case 'timed_out':
      return 'badge badge-warn';
    case 'crashed':
    case 'gpu_crashed':
    case 'thread_affinity_failure':
      return 'badge badge-err';
    default:
      return 'badge badge-muted';
  }
}

const STATUS_LABEL_KEYS: Record<SessionStatus, MessageKey> = {
  active: 'sessions.status.active',
  ended: 'sessions.status.ended',
  disconnected: 'sessions.status.disconnected',
  crashed: 'sessions.status.crashed',
  gpu_crashed: 'sessions.status.gpuCrashed',
  timed_out: 'sessions.status.timedOut',
  cancelled: 'sessions.status.cancelled',
  thread_affinity_failure: 'sessions.status.threadAffinityFailure',
};

function endReasonLabel(reason: SessionEndReason | null | undefined): string {
  if (!reason) return '-';
  if ('normal' in reason) return 'Normal';
  if ('disconnect' in reason) return `Disconnect: ${reason.disconnect.detail}`;
  if ('host_crash' in reason) return `Host Crash: ${reason.host_crash.detail}`;
  if ('gpu_crash' in reason) return `GPU Crash: ${reason.gpu_crash.detail}`;
  if ('timeout' in reason) return `Timeout: ${reason.timeout.detail}`;
  if ('cancelled' in reason) return `Cancelled: ${reason.cancelled.detail}`;
  if ('thread_affinity_failure' in reason) return `Thread Affinity: ${reason.thread_affinity_failure.detail}`;
  return '-';
}

function endReasonKind(reason: SessionEndReason | null | undefined): string {
  if (!reason) return '-';
  if ('normal' in reason) return 'normal';
  if ('disconnect' in reason) return 'disconnect';
  if ('host_crash' in reason) return 'host_crash';
  if ('gpu_crash' in reason) return 'gpu_crash';
  if ('timeout' in reason) return 'timeout';
  if ('cancelled' in reason) return 'cancelled';
  if ('thread_affinity_failure' in reason) return 'thread_affinity_failure';
  return '-';
}

function sessionDurationMs(session: SessionRow): number | null {
  const end = session.ended_at_ms ?? Date.now();
  if (end < session.started_at_ms) return null;
  return end - session.started_at_ms;
}

type TreeNode = {
  session: SessionRow;
  children: TreeNode[];
  depth: number;
};

function buildTree(sessions: SessionRow[]): TreeNode[] {
  const byId = new Map<string, TreeNode>();
  const roots: TreeNode[] = [];

  for (const s of sessions) {
    byId.set(s.session_id, { session: s, children: [], depth: 0 });
  }

  for (const node of byId.values()) {
    const parentId = node.session.parent_session_id;
    if (parentId && byId.has(parentId)) {
      byId.get(parentId)!.children.push(node);
    } else {
      roots.push(node);
    }
  }

  function assignDepth(nodes: TreeNode[], depth: number) {
    for (const node of nodes) {
      node.depth = depth;
      assignDepth(node.children, depth + 1);
    }
  }

  assignDepth(roots, 0);
  return roots;
}

function flattenTree(nodes: TreeNode[]): TreeNode[] {
  const result: TreeNode[] = [];
  function walk(list: TreeNode[]) {
    for (const node of list) {
      result.push(node);
      walk(node.children);
    }
  }
  walk(nodes);
  return result;
}

// ── component ──────────────────────────────────────────────────────────────

export function SessionsPanel({ active, t }: { active: boolean; t: Translator }) {
  const [dccFilter, setDccFilter] = useState(ALL_DCC);
  const [statusFilter, setStatusFilter] = useState(ALL_STATUS);
  const [search, setSearch] = useState('');
  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set());

  const { data, isLoading, error, refetch } = useQuery({
    queryKey: ['admin', 'sessions'],
    queryFn: () => apiJson<SessionsPayload>(`${API_BASE}/sessions`),
    enabled: active,
    refetchInterval: active ? POLL_INTERVAL_MS : false,
  });

  const tree = useMemo(() => {
    if (!data?.sessions) return [];
    return buildTree(data.sessions);
  }, [data]);

  const dccTypes = useMemo(() => {
    if (!data?.sessions) return [];
    return Array.from(new Set(data.sessions.map((s) => s.dcc_type))).sort();
  }, [data]);

  const allStatuses: SessionStatus[] = [
    'active', 'ended', 'disconnected', 'crashed',
    'gpu_crashed', 'timed_out', 'cancelled', 'thread_affinity_failure',
  ];

  const flatNodes = useMemo(() => flattenTree(tree), [tree]);

  const filtered = useMemo(() => {
    return flatNodes.filter((node) => {
      const s = node.session;
      if (dccFilter !== ALL_DCC && s.dcc_type !== dccFilter) return false;
      if (statusFilter !== ALL_STATUS && s.status !== statusFilter) return false;
      if (search.trim()) {
        const hay = haystack(s.session_id, s.dcc_type, s.status, s.instance_id);
        if (!matchesListFilter(search, hay)) return false;
      }
      return true;
    });
  }, [flatNodes, dccFilter, statusFilter, search]);

  function toggleExpand(id: string) {
    setExpandedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }

  const visibleNodeIds = useMemo(() => {
    const ids = new Set<string>();
    for (const node of filtered) {
      let visible = true;
      if (node.session.parent_session_id) {
        const parent = flatNodes.find((n) => n.session.session_id === node.session.parent_session_id);
        if (!parent || !filtered.includes(parent)) {
          visible = false;
        } else if (!expandedIds.has(parent.session.session_id)) {
          visible = false;
        }
      }
      if (visible) ids.add(node.session.session_id);
    }
    return ids;
  }, [filtered, expandedIds, flatNodes]);

  const displayNodes = useMemo(() => {
    return filtered.filter((node) => visibleNodeIds.has(node.session.session_id));
  }, [filtered, visibleNodeIds]);

  if (!active) return null;

  return (
    <section className="panel active sessions-panel" data-panel="sessions">
      <PanelHeader
        title={t('sessions.title')}
        meta={t('sessions.meta')}
        action={
          <Button type="button" size="sm" disabled={isLoading} onClick={() => refetch()}>
            <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
            {t('sessions.action.refresh')}
          </Button>
        }
      />

      {/* ── KPI metrics ─────────────────────────────────────────────── */}
      <div className="sessions-metrics">
        <MetricTile
          label={t('sessions.kpi.total')}
          value={data?.total ?? '-'}
        />
        <MetricTile
          tone="ok"
          label={t('sessions.kpi.active')}
          value={data?.active ?? '-'}
        />
        <MetricTile
          label={t('sessions.kpi.ended')}
          value={data?.ended ?? '-'}
        />
        <MetricTile
          label={t('sessions.kpi.byDcc')}
          value={data?.by_dcc ? Object.keys(data.by_dcc).length : '-'}
        />
      </div>

      {/* ── DCC chips ───────────────────────────────────────────────── */}
      {data?.by_dcc && Object.keys(data.by_dcc).length > 0 && (
        <div className="sessions-by-dcc">
          {Object.entries(data.by_dcc).map(([dcc, count]) => (
            <span className="dcc-chip" key={dcc}>
              {dcc}: {count}
            </span>
          ))}
        </div>
      )}

      {/* ── filters ─────────────────────────────────────────────────── */}
      <div className="sessions-filter-bar">
        <Select value={dccFilter} onValueChange={setDccFilter}>
          <SelectTrigger className="w-36">
            <SelectValue placeholder={t('sessions.filter.dccType')} />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              <SelectItem value={ALL_DCC}>{t('sessions.filter.all')}</SelectItem>
              {dccTypes.map((dcc) => (
                <SelectItem key={dcc} value={dcc}>{dcc}</SelectItem>
              ))}
            </SelectGroup>
          </SelectContent>
        </Select>

        <Select value={statusFilter} onValueChange={setStatusFilter}>
          <SelectTrigger className="w-40">
            <SelectValue placeholder={t('sessions.filter.status')} />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              <SelectItem value={ALL_STATUS}>{t('sessions.filter.all')}</SelectItem>
              {allStatuses.map((status) => (
                <SelectItem key={status} value={status}>{t(STATUS_LABEL_KEYS[status])}</SelectItem>
              ))}
            </SelectGroup>
          </SelectContent>
        </Select>

        <input
          type="text"
          className="input"
          placeholder={t('sessions.filter.search')}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>

      {/* ── status line ──────────────────────────────────────────────── */}
      <StatusLine error={error instanceof Error ? error.message : undefined} />

      {/* ── loading ──────────────────────────────────────────────────── */}
      {isLoading && (
        <p className="empty">{t('sessions.status.loading')}</p>
      )}

      {/* ── empty ────────────────────────────────────────────────────── */}
      {!isLoading && !error && data && displayNodes.length === 0 && (
        <p className="empty">
          {data.sessions.length === 0 ? t('sessions.empty.noData') : t('sessions.empty.noResults')}
        </p>
      )}

      {/* ── session table ────────────────────────────────────────────── */}
      {!isLoading && !error && displayNodes.length > 0 && (
        <table className="sessions-tree-table">
          <thead>
            <tr>
              <th style={{ width: '22%' }}>{t('sessions.label.sessionId')}</th>
              <th style={{ width: '10%' }}>{t('sessions.label.dccType')}</th>
              <th style={{ width: '12%' }}>{t('sessions.label.status')}</th>
              <th style={{ width: '8%' }}>{t('sessions.label.toolCalls')}</th>
              <th style={{ width: '8%' }}>{t('sessions.label.errors')}</th>
              <th style={{ width: '18%' }}>{t('sessions.label.startTime')}</th>
              <th style={{ width: '12%' }}>{t('sessions.label.duration')}</th>
              <th style={{ width: '10%' }}>{t('sessions.label.instanceId')}</th>
            </tr>
          </thead>
          <tbody>
            {displayNodes.map((node) => {
              const s = node.session;
              const isExpanded = expandedIds.has(s.session_id);
              const hasChildren = node.children.length > 0;
              const indentStr = node.depth > 0 ? '\u00A0\u00A0\u00A0'.repeat(node.depth) : '';
              const branchPrefix = node.depth > 0 ? '\u2514\u2500 ' : '';
              const startedAt = new Date(s.started_at_ms).toISOString();
              const lastActivityAt = new Date(s.last_activity_at_ms).toISOString();

              return (
                <Fragment key={s.session_id}>
                  <tr
                    className={`session-tree-row ${node.depth > 0 ? 'child' : ''} ${isExpanded ? 'expanded' : ''}`}
                    onClick={() => toggleExpand(s.session_id)}
                  >
                    <td>
                      <span className="session-tree-indent">
                        <span className="tree-lines">
                          {indentStr}{branchPrefix}
                        </span>
                        <span className="session-id-cell">
                          <code title={s.session_id}>{compactId(s.session_id)}</code>
                        </span>
                        {hasChildren && (
                          <Badge variant="outline" className="source-pill">
                            {isExpanded ? '\u25BC' : '\u25B6'} {node.children.length}
                          </Badge>
                        )}
                        {!s.parent_session_id ? (
                          <Badge variant="outline" className="badge-muted">{t('sessions.badge.root')}</Badge>
                        ) : null}
                      </span>
                    </td>
                    <td>{s.dcc_type}</td>
                    <td>
                      <span className={statusBadgeClass(s.status)}>
                        {t(STATUS_LABEL_KEYS[s.status])}
                      </span>
                    </td>
                    <td>{s.tool_call_count}</td>
                    <td>
                      <span className={s.error_count > 0 ? 'badge badge-err' : ''}>
                        {s.error_count > 0 ? s.error_count : '\u2014'}
                      </span>
                    </td>
                    <td>
                      <time dateTime={startedAt} title={formatTime(startedAt)}>
                        {formatTime(startedAt)}
                      </time>
                    </td>
                    <td>{formatDurationMs(sessionDurationMs(s))}</td>
                    <td>
                      <code title={s.instance_id ?? ''}>{compactId(s.instance_id)}</code>
                    </td>
                  </tr>

                  {/* ── expanded detail row ─────────────────────────── */}
                  {isExpanded && (
                    <tr>
                      <td colSpan={8} className="session-expand-detail" style={{ padding: 0 }}>
                        <div className="detail-grid">
                          <span>
                            <strong>{t('sessions.label.sessionId')}</strong>
                            <code title={s.session_id}>{s.session_id}</code>
                          </span>
                          <span>
                            <strong>{t('sessions.label.status')}</strong>
                            <span className={statusBadgeClass(s.status)}>
                              {t(STATUS_LABEL_KEYS[s.status])}
                            </span>
                          </span>
                          <span>
                            <strong>{t('sessions.label.dccType')}</strong>
                            {s.dcc_type}
                          </span>
                          <span>
                            <strong>{t('sessions.label.instanceId')}</strong>
                            <code>{s.instance_id ?? '-'}</code>
                          </span>
                          <span>
                            <strong>{t('sessions.label.toolCalls')}</strong>
                            {s.tool_call_count}
                          </span>
                          <span>
                            <strong>{t('sessions.label.errors')}</strong>
                            {s.error_count}
                          </span>
                          <span>
                            <strong>{t('sessions.label.startTime')}</strong>
                            {formatTime(startedAt)}
                          </span>
                          <span>
                            <strong>{t('sessions.label.lastActivity')}</strong>
                            {formatTime(lastActivityAt)}
                          </span>
                          <span>
                            <strong>{t('sessions.label.endedAt')}</strong>
                            {s.ended_at_ms ? formatTime(new Date(s.ended_at_ms).toISOString()) : '\u2014'}
                          </span>
                          <span>
                            <strong>{t('sessions.label.duration')}</strong>
                            {formatDurationMs(sessionDurationMs(s))}
                          </span>
                        </div>

                        {/* parent info */}
                        <div className="detail-section">
                          <div className="detail-section-title">{t('sessions.detail.parentInfo')}</div>
                          <div className="detail-grid">
                            <span>
                              <strong>{t('sessions.label.parentSession')}</strong>
                              <code>{s.parent_session_id ?? t('sessions.label.noParent')}</code>
                            </span>
                            <span>
                              <strong>{t('sessions.label.childCount')}</strong>
                              {node.children.length}
                            </span>
                          </div>
                        </div>

                        {/* versions */}
                        <div className="detail-section">
                          <div className="detail-section-title">{t('sessions.detail.versions')}</div>
                          <div className="detail-grid">
                            <span>
                              <strong>{t('sessions.label.coreVersion')}</strong>
                              <code>{s.core_version}</code>
                            </span>
                            <span>
                              <strong>{t('sessions.label.adapterVersion')}</strong>
                              <code>{s.adapter_version ?? '\u2014'}</code>
                            </span>
                            <span>
                              <strong>{t('sessions.label.buildSha')}</strong>
                              <code>{s.build_sha ?? '\u2014'}</code>
                            </span>
                          </div>
                        </div>

                        {/* end reason */}
                        {s.end_reason && (
                          <div className="detail-section">
                            <div className="detail-section-title">{t('sessions.label.endReason')}</div>
                            <div className="end-reason-detail">
                              <strong>{endReasonKind(s.end_reason)}:</strong>{' '}
                              <code>{endReasonLabel(s.end_reason)}</code>
                            </div>
                          </div>
                        )}
                      </td>
                    </tr>
                  )}
                </Fragment>
              );
            })}
          </tbody>
        </table>
      )}
    </section>
  );
}
