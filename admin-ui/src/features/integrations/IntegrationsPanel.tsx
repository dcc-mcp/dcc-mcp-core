import { useCallback, useEffect, useMemo, useState } from 'react';
import type { InterpolationValues, MessageKey } from '../../i18n';
import {
  PanelHeader,
  StatusLine,
  haystack,
  matchesListFilter,
} from '../../admin-ui-core';
import {
  useIntegrationsQuery,
  useUpdateIntegration,
} from '../../hooks/queries';
import type { IntegrationKind } from '../../admin-types';
import { IntegrationCard } from './IntegrationCard';
import { IntegrationEditForm } from './IntegrationEditForm';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

export type IntegrationsPanelProps = {
  active: boolean;
  search: string;
  updatedAt: string;
  error?: string;
  onUpdated: (text: string) => void;
  onError: (error: unknown) => void;
  onCountsChange?: (counts: { total: number; active: number }) => void;
  t: Translator;
};

/// Top-level orchestrator for the `/admin#integrations` panel.
///
/// Displays three integration cards (Sentry, Webhooks, OTLP) in a grid.
/// Each card can open an edit form. Sentry is the only fully editable one;
/// Webhooks and OTLP show placeholder empty states.
export function IntegrationsPanel({
  active,
  search,
  updatedAt,
  error,
  onUpdated,
  onError,
  onCountsChange,
  t,
}: IntegrationsPanelProps) {
  const [editingKind, setEditingKind] = useState<IntegrationKind | null>(null);

  const integrationsQuery = useIntegrationsQuery(active);
  const updateMut = useUpdateIntegration();

  const integrations = useMemo(() => integrationsQuery.data ?? [], [integrationsQuery.data]);

  // Refresh on first show.
  useEffect(() => {
    if (!active) return;
    integrationsQuery.refetch();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active]);

  // Status line updates.
  useEffect(() => {
    if (!active) return;
    if (integrationsQuery.data) {
      const time = new Date().toLocaleTimeString();
      onUpdated(
        t('integrations.detail.count', { count: integrations.length }) +
          ` · ${time}`,
      );
    }
  }, [active, integrations.length, integrationsQuery.data, onUpdated, t]);

  useEffect(() => {
    if (integrationsQuery.error) onError(integrationsQuery.error);
  }, [integrationsQuery.error, onError]);

  // Report counts to parent.
  useEffect(() => {
    const activeCount = integrations.filter((i) => i.status === 'active').length;
    onCountsChange?.({ total: integrations.length, active: activeCount });
  }, [integrations, onCountsChange]);

  // Filter by search.
  const filteredIntegrations = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return integrations;
    return integrations.filter((entry) =>
      matchesListFilter(
        q,
        haystack(entry.kind, entry.label, entry.description, entry.status),
      ),
    );
  }, [integrations, search]);

  const editingEntry = useMemo(
    () => integrations.find((i) => i.kind === editingKind) ?? null,
    [integrations, editingKind],
  );

  const handleEdit = useCallback((kind: IntegrationKind) => {
    setEditingKind(kind);
  }, []);

  const handleCancel = useCallback(() => {
    setEditingKind(null);
  }, []);

  const handleSave = useCallback(
    async (kind: IntegrationKind, config: Record<string, unknown>) => {
      try {
        await updateMut.mutateAsync({ kind, config });
        setEditingKind(null);
        onUpdated(
          t('integrations.toast.saved', { kind }) +
            ` · ${new Date().toLocaleTimeString()}`,
        );
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        onError(message);
        throw err;
      }
    },
    [updateMut, onUpdated, onError, t],
  );

  return (
    <section className={`panel${active ? ' active' : ''} integrations-panel`}>
      <PanelHeader
        title={t('integrations.title')}
        meta={t('integrations.detail.count', { count: integrations.length })}
      />

      <StatusLine
        text={updatedAt || t('integrations.detail.count', { count: integrations.length })}
        error={error}
      />

      {/* Edit form overlay */}
      {editingEntry ? (
        <IntegrationEditForm
          entry={editingEntry}
          saving={updateMut.isPending}
          onSave={handleSave}
          onCancel={handleCancel}
          t={t}
        />
      ) : integrationsQuery.isLoading ? (
        <p className="empty">{t('common.status.loading')}</p>
      ) : integrationsQuery.error ? (
        <p className="empty">{t('integrations.error.fetchFailed')}</p>
      ) : filteredIntegrations.length === 0 ? (
        <p className="empty">
          {search.trim() ? t('integrations.empty.search') : t('integrations.empty.none')}
        </p>
      ) : (
        <div className="integrations-grid">
          {filteredIntegrations.map((entry) => (
            <IntegrationCard
              key={entry.kind}
              entry={entry}
              onEdit={handleEdit}
              t={t}
            />
          ))}
        </div>
      )}
    </section>
  );
}
