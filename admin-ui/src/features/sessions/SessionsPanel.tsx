import { Fragment, useMemo, useState } from 'react';
import { RiArrowDownSLine, RiArrowRightSLine, RiRefreshLine } from '@remixicon/react';
import {
  PanelHeader,
  PanelTabs,
  StatusLine,
  apiJson,
  compactId,
  formatDurationMs,
  haystack,
  matchesListFilter,
} from '../../admin-ui-core';
import { formatTime } from '../../time';
import { Input } from '../../components/ui/input';
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
import type { SessionsTab } from '../../navigation';
import { normalizeSessionsPayload, type SessionRow, type SessionStatus, type SessionsApiPayload } from './session-data';
import './sessions.css';

// ── helpers ────────────────────────────────────────────────────────────────

const POLL_INTERVAL_MS = 5_000;
const ALL_DCC = '__all__';
const ALL_STATUS = '__all__';

function statusBadgeClass(status: SessionStatus): string {
  switch (status) {
    case 'active':
      return 'badge badge-ok';
    case 'ended':
    case 'interrupted':
    case 'cancelled':
      return 'badge badge-muted';
    case 'disconnected':
    case 'timed_out':
      return 'badge badge-warn';
    case 'crashed':
    case 'gpu_crashed':
    case 'thread_affinity_failure':
      return 'badge badge-err';
    case 'unknown':
    default:
      return 'badge badge-muted';
  }
}

const STATUS_LABEL_KEYS: Record<SessionStatus, MessageKey> = {
  active: 'sessions.status.active',
  ended: 'sessions.status.ended',
  interrupted: 'sessions.status.interrupted',
  disconnected: 'sessions.status.disconnected',
  crashed: 'sessions.status.crashed',
  gpu_crashed: 'sessions.status.gpuCrashed',
  timed_out: 'sessions.status.timedOut',
  cancelled: 'sessions.status.cancelled',
  thread_affinity_failure: 'sessions.status.threadAffinityFailure',
  unknown: 'sessions.status.unknown',
};

function sessionDurationMs(session: SessionRow): number | null {
  if (session.duration_ms != null) return session.duration_ms;
  const start = Date.parse(session.started_at);
  const end = session.ended_at ? Date.parse(session.ended_at) : Date.now();
  return Number.isFinite(start) && Number.isFinite(end) && end >= start ? end - start : null;
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

export function SessionsPanel({
  active,
  tab,
  onTabChange,
  onOpenMemory,
  t,
}: {
  active: boolean;
  tab: SessionsTab;
  onTabChange: (tab: SessionsTab) => void;
  onOpenMemory: (sessionId: string) => void;
  t: Translator;
}) {
  const [dccFilter, setDccFilter] = useState(ALL_DCC);
  const [statusFilter, setStatusFilter] = useState(ALL_STATUS);
  const [search, setSearch] = useState('');
  const [collapsedIds, setCollapsedIds] = useState<Set<string>>(new Set());
  const [detailId, setDetailId] = useState<string | null>(null);

  const { data, isLoading, error, refetch } = useQuery({
    queryKey: ['admin', 'sessions'],
    queryFn: async () => normalizeSessionsPayload(await apiJson<SessionsApiPayload>('/sessions')),
    enabled: active,
    refetchInterval: active ? POLL_INTERVAL_MS : false,
  });

  const tree = useMemo(() => {
    if (!data?.sessions) return [];
    return buildTree(data.sessions);
  }, [data]);

  const dccTypes = useMemo(() => {
    if (!data?.sessions) return [];
    return Array.from(new Set(data.sessions.flatMap((s) => s.dcc_type ? [s.dcc_type] : []))).sort();
  }, [data]);

  const allStatuses: SessionStatus[] = [
    'active', 'ended', 'interrupted', 'disconnected', 'crashed',
    'gpu_crashed', 'timed_out', 'cancelled', 'thread_affinity_failure', 'unknown',
  ];

  const flatNodes = useMemo(() => flattenTree(tree), [tree]);

  const filtered = useMemo(() => {
    return flatNodes.filter((node) => {
      const s = node.session;
      if (dccFilter !== ALL_DCC && s.dcc_type !== dccFilter) return false;
      if (statusFilter !== ALL_STATUS && s.status !== statusFilter) return false;
      if (search.trim()) {
        const hay = haystack(s.session_id, s.dcc_type, s.status, s.instance_id, s.agent_name, s.actor_name);
        if (!matchesListFilter(search, hay)) return false;
      }
      return true;
    });
  }, [flatNodes, dccFilter, statusFilter, search]);

  function toggleCollapsed(id: string) {
    setCollapsedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }

  const hasFilters = dccFilter !== ALL_DCC || statusFilter !== ALL_STATUS || Boolean(search.trim());
  const displayNodes = useMemo(() => {
    if (hasFilters) return filtered;
    const hiddenIds = new Set<string>();
    return filtered.filter((node) => {
      const parentId = node.session.parent_session_id;
      if (parentId && (collapsedIds.has(parentId) || hiddenIds.has(parentId))) {
        hiddenIds.add(node.session.session_id);
        return false;
      }
      return true;
    });
  }, [filtered, collapsedIds, hasFilters]);
  if (!active) return null;

  return (
    <section className="panel active sessions-panel" data-panel="sessions">
      <PanelHeader
        title={t('sessions.title')}
        meta={t('sessions.meta')}
        action={
          <div className="table-actions">
            <PanelTabs
              value={tab}
              tabs={[
                { id: 'sessions', label: t('navigation.sessionsTab.sessions') },
                { id: 'memory', label: t('navigation.sessionsTab.memory') },
              ]}
              ariaLabel={t('navigation.sessionsTab.meta')}
              onValueChange={onTabChange}
            />
            <Button type="button" size="sm" disabled={isLoading} onClick={() => refetch()}>
              <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
              {t('sessions.action.refresh')}
            </Button>
          </div>
        }
      />

      <div className="sessions-overview">
        <dl className="sessions-summary" aria-label={t('sessions.section.summary')}>
          <div><dt>{t('sessions.kpi.total')}</dt><dd>{data?.total ?? '—'}</dd></div>
          <div><dt>{t('sessions.kpi.active')}</dt><dd>{data?.kpi.active ?? '—'}</dd></div>
          <div><dt>{t('sessions.kpi.ended')}</dt><dd>{data?.kpi.ended ?? '—'}</dd></div>
          <div><dt>{t('sessions.kpi.byDcc')}</dt><dd>{data?.kpi.by_dcc ? Object.keys(data.kpi.by_dcc).length : '—'}</dd></div>
        </dl>
        {data?.kpi.by_dcc && Object.keys(data.kpi.by_dcc).length > 0 && (
          <details className="sessions-by-dcc">
            <summary>{t('sessions.kpi.byDcc')}</summary>
            <dl>{Object.entries(data.kpi.by_dcc).map(([dcc, count]) => (
              <div key={dcc}><dt>{dcc}</dt><dd>{count}</dd></div>
            ))}</dl>
          </details>
        )}
      </div>
      {/* ── filters ─────────────────────────────────────────────────── */}
      <div className="sessions-filter-bar">
        <Select value={dccFilter} onValueChange={setDccFilter}>
          <SelectTrigger className="admin-select-trigger" aria-label={t('sessions.filter.dccType')}>
            <SelectValue placeholder={t('sessions.filter.dccType')} />
          </SelectTrigger>
          <SelectContent className="admin-select-content">
            <SelectGroup>
              <SelectItem value={ALL_DCC}>{t('sessions.filter.all')}</SelectItem>
              {dccTypes.map((dcc) => (
                <SelectItem key={dcc} value={dcc}>{dcc}</SelectItem>
              ))}
            </SelectGroup>
          </SelectContent>
        </Select>

        <Select value={statusFilter} onValueChange={setStatusFilter}>
          <SelectTrigger className="admin-select-trigger" aria-label={t('sessions.filter.status')}>
            <SelectValue placeholder={t('sessions.filter.status')} />
          </SelectTrigger>
          <SelectContent className="admin-select-content">
            <SelectGroup>
              <SelectItem value={ALL_STATUS}>{t('sessions.filter.all')}</SelectItem>
              {allStatuses.map((status) => (
                <SelectItem key={status} value={status}>{t(STATUS_LABEL_KEYS[status])}</SelectItem>
              ))}
            </SelectGroup>
          </SelectContent>
        </Select>

        <Input
          type="search"
          className="sessions-search-input" aria-label={t('sessions.filter.search')}
          data-testid="sessions-search"
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
        <table className="sessions-tree-table sessions-table">
          <thead>
            <tr>
              <th>{t('sessions.label.sessionId')}</th>
              <th>{t('sessions.label.toolCalls')} / {t('sessions.label.turns')}</th>
              <th>{t('sessions.label.startTime')}</th>
              <th>{t('sessions.label.status')}</th>
            </tr>
          </thead>
          <tbody>
            {displayNodes.map((node) => {
              const s = node.session;
              const isDetailOpen = detailId === s.session_id;
              const isCollapsed = collapsedIds.has(s.session_id);
              const hasChildren = node.children.length > 0;
              const startedAt = s.started_at;

              return (
                <Fragment key={s.session_id}>
                  <tr className={`session-tree-row sessions-row ${node.depth > 0 ? 'child' : ''} ${isDetailOpen ? 'expanded' : ''}`}>
                    <td className="session-identity">
                      <div className="session-tree-indent" style={{ paddingInlineStart: hasFilters ? 0 : Math.min(node.depth, 4) * 12 }}>
                        <Button type="button" variant="ghost" size="sm" className="sessions-detail-btn"
                          aria-expanded={isDetailOpen} aria-controls={`session-detail-${s.session_id}`}
                          onClick={() => setDetailId(isDetailOpen ? null : s.session_id)}>
                          {isDetailOpen ? <RiArrowDownSLine aria-hidden="true" /> : <RiArrowRightSLine aria-hidden="true" />}
                          <span title={s.session_id}>{compactId(s.session_id)}</span>
                        </Button>
                        {hasChildren && !hasFilters && (
                          <Button type="button" variant="ghost" size="sm" className="sessions-tree-btn"
                            aria-label={t('sessions.action.toggleChildren', { id: compactId(s.session_id) })}
                            aria-expanded={!isCollapsed} onClick={() => toggleCollapsed(s.session_id)}>
                            {isCollapsed ? <RiArrowRightSLine aria-hidden="true" /> : <RiArrowDownSLine aria-hidden="true" />}
                            {node.children.length}
                          </Button>
                        )}
                      </div>
                      <div className="session-context">{s.dcc_type || '—'}<span>{s.agent_name || s.actor_name || compactId(s.instance_id)}</span></div>
                    </td>
                    <td className="session-activity">
                      <span>{s.tool_call_count} {t('sessions.label.toolCalls')}</span>
                      <small>{s.turn_count ?? '—'} {t('sessions.label.turns')}</small>
                    </td>
                    <td className="session-time">
                      <time dateTime={startedAt} title={formatTime(startedAt)}>{formatTime(startedAt)}</time>
                      <small>{formatDurationMs(sessionDurationMs(s))}</small>
                    </td>
                    <td className="session-status"><span className={statusBadgeClass(s.status)}>{t(STATUS_LABEL_KEYS[s.status])}</span></td>
                  </tr>
                  {/* ── expanded detail row ─────────────────────────── */}
                  {isDetailOpen && (
                    <tr className="sessions-detail-row">
                      <td colSpan={4} className="session-expand-detail" id={`session-detail-${s.session_id}`}>
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
                            <strong>{t('sessions.label.turns')}</strong>
                            {s.turn_count ?? '—'}
                          </span>
                          <span>
                            <strong>{t('sessions.label.startTime')}</strong>
                            {formatTime(startedAt)}
                          </span>
                          <span>
                            <strong>{t('sessions.label.endedAt')}</strong>
                            {s.ended_at ? formatTime(s.ended_at) : '\u2014'}
                          </span>
                          <span>
                            <strong>{t('sessions.label.duration')}</strong>
                            {formatDurationMs(sessionDurationMs(s))}
                          </span>
                        </div>

                        <div className="detail-section session-detail-actions">
                          <Button
                            type="button"
                            size="sm"
                            variant="secondary"
                            onClick={(event) => {
                              event.stopPropagation();
                              onOpenMemory(s.session_id);
                            }}
                          >
                            {t('sessions.action.viewMemory')}
                          </Button>
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
                              <code>{s.version ?? '\u2014'}</code>
                            </span>
                          </div>
                        </div>

                        {/* end reason */}
                        {s.end_reason && (
                          <div className="detail-section">
                            <div className="detail-section-title">{t('sessions.label.endReason')}</div>
                            <div className="end-reason-detail">
                              <code>{s.end_reason}</code>
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
