import { useCallback, useEffect, useMemo } from 'react';
import { createPortal } from 'react-dom';
import { type InterpolationValues, type MessageKey } from '../../i18n';
import { haystack, matchesListFilter, PanelHeader, StatusLine } from '../../admin-ui-core';
import { SkillCard } from './SkillCard';
import { SkillDetailPanel } from './SkillDetailPanel';
import { SkillSearchPathsTable } from './SkillSearchPathsTable';
import { SkillsSummaryGrid } from './SkillsSummaryGrid';
import { useSkillDetail } from './hooks/useSkillDetail';
import { useSkillPaths } from './hooks/useSkillPaths';
import { useSkillsInventory } from './hooks/useSkillsInventory';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

export type SkillsPanelProps = {
  active: boolean;
  search: string;
  updatedAt: string;
  error?: string;
  onUpdated: (text: string) => void;
  onError: (error: unknown) => void;
  t: Translator;
  /// Exposed so the parent search bar can show "skills X / paths Y".
  /// The orchestrator computes filtered counts and reports them back
  /// through the supplied callback after every recompute.
  onCountsChange?: (counts: { skills: number; paths: number }) => void;
};

/// Top-level orchestrator for the `/admin#skill-paths` panel.
///
/// Owns the three feature hooks (inventory, paths, detail), reacts to
/// `active` to refresh on first show, and composes the marketplace
/// card grid with the search-paths management section. All visual
/// pieces (`SkillCard`, `SkillsSummaryGrid`, `SkillSearchPathsTable`,
/// `SkillDetailPanel`) are kept dumb — they receive plain props.
export function SkillsPanel({
  active,
  search,
  updatedAt,
  error,
  onUpdated,
  onError,
  onCountsChange,
  t,
}: SkillsPanelProps) {
  const inventory = useSkillsInventory({
    onUpdated: (loaded, actions) =>
      onUpdated(
        t('common.updated.skillInventory', {
          loaded,
          actions,
          time: new Date().toLocaleTimeString(),
        }),
      ),
    onError,
  });
  const pathStore = useSkillPaths({
    onUpdated: (count) =>
      onUpdated(t('common.updated.paths', { count, time: new Date().toLocaleTimeString() })),
    onError,
    onMutated: async () => {
      await Promise.allSettled([inventory.refresh(), pathStore.refresh()]);
    },
  });
  const detailStore = useSkillDetail({
    onUpdated: (name) =>
      onUpdated(
        t('common.updated.skillDetail', { name, time: new Date().toLocaleTimeString() }),
      ),
    onError,
  });

  // Refresh on panel show so navigation away/back surfaces fresh data.
  useEffect(() => {
    if (!active) return;
    void Promise.allSettled([inventory.refresh(), pathStore.refresh()]);
    // intentionally exclude refresh callbacks: re-running them only
    // when `active` flips matches the prior fetchSkillInventory()
    // call site that triggered on activePanel transition.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active]);

  const filteredSkills = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return inventory.skills;
    return inventory.skills.filter((skill) =>
      matchesListFilter(
        q,
        haystack(
          skill.name,
          skill.dcc_type,
          skill.loaded ? 'loaded' : 'unloaded',
          skill.summary ?? '',
          skill.instances.join(' '),
          skill.tools.join(' '),
          skill.adoption.low_adoption ? 'low adoption' : '',
          skill.adoption.searched ? 'searched' : '',
          skill.adoption.used ? 'used' : '',
        ),
      ),
    );
  }, [inventory.skills, search]);

  const filteredPaths = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return pathStore.paths;
    return pathStore.paths.filter((r) =>
      matchesListFilter(
        q,
        haystack(
          r.display_path ?? r.path,
          r.path_alias ?? '',
          r.path_hash ?? '',
          r.status ?? '',
          r.source,
          r.id != null ? String(r.id) : '',
        ),
      ),
    );
  }, [pathStore.paths, search]);

  useEffect(() => {
    onCountsChange?.({ skills: filteredSkills.length, paths: filteredPaths.length });
  }, [filteredPaths.length, filteredSkills.length, onCountsChange]);

  if (!active) return null;

  return (
    <section className="panel active skill-paths-panel" data-panel="skill-paths">
      <PanelHeader
        title={t('skillPaths.title')}
        action={
          <button
            className="refresh-btn"
            type="button"
            disabled={pathStore.busy}
            onClick={() => void Promise.allSettled([inventory.refresh(), pathStore.refresh()])}
          >
            {t('action.refresh')}
          </button>
        }
      />
      <StatusLine text={updatedAt} error={error} />
      <p className="empty log-hint">{t('skillPaths.description')}</p>
      <SkillsSummaryGrid totals={inventory.totals} pathCount={pathStore.paths.length} t={t} />
      <div className="skill-inventory-section">
        <h3 className="section-kicker">{t('skillPaths.section.loadedSkills')}</h3>
        {inventory.skills.length === 0 ? (
          <p className="empty">{t('skillPaths.empty.skills')}</p>
        ) : filteredSkills.length === 0 ? (
          <p className="empty">{t('skillPaths.empty.skillsSearch')}</p>
        ) : (
          <div className="skill-card-grid" role="list">
            {filteredSkills.map((skill) => (
              <div role="listitem" key={`${skill.dcc_type}-${skill.name}-${skill.loaded ? 'on' : 'off'}`}>
                <SkillCard
                  skill={skill}
                  selected={
                    detailStore.selected?.name === skill.name
                      && detailStore.selected?.dcc_type === skill.dcc_type
                  }
                  onOpen={(s) => void detailStore.open(s)}
                  t={t}
                />
              </div>
            ))}
          </div>
        )}
        <SkillDetailModal
          skill={detailStore.selected}
          detail={detailStore.detail}
          busy={detailStore.busy}
          onReload={detailStore.reload}
          onClose={detailStore.close}
          t={t}
        />
      </div>
      <SkillSearchPathsTable
        paths={pathStore.paths}
        filtered={filteredPaths}
        input={pathStore.input}
        busy={pathStore.busy}
        onInputChange={pathStore.setInput}
        onAdd={() => void pathStore.addPath()}
        onDelete={(id) => void pathStore.deletePath(id)}
        t={t}
      />
    </section>
  );
}

/// Portal-based modal that renders the detail panel as a slide-in overlay.
/// Handles backdrop dismiss, Escape key close, and body scroll lock.
function SkillDetailModal({
  skill,
  detail,
  busy,
  onReload,
  onClose,
  t,
}: {
  skill: import('../../admin-types').SkillRow | null;
  detail: import('../../admin-types').SkillDetailPayload | null;
  busy: boolean;
  onReload: () => void;
  onClose: () => void;
  t: (key: import('../../i18n').MessageKey, values?: import('../../i18n').InterpolationValues) => string;
}) {
  // Escape key closes the modal.
  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    },
    [onClose],
  );

  useEffect(() => {
    if (!skill) return;
    document.addEventListener('keydown', handleKeyDown);
    document.body.style.overflow = 'hidden';
    return () => {
      document.removeEventListener('keydown', handleKeyDown);
      document.body.style.overflow = '';
    };
  }, [skill, handleKeyDown]);

  if (!skill) return null;

  return createPortal(
    <div
      className="skill-detail-backdrop"
      role="dialog"
      aria-modal="true"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="skill-detail-modal">
        <button
          type="button"
          className="skill-detail-close"
          aria-label={t('action.close')}
          onClick={onClose}
        >
          &times;
        </button>
        <SkillDetailPanel
          skill={skill}
          detail={detail}
          busy={busy}
          onReload={onReload}
          onClose={onClose}
          t={t}
        />
      </div>
    </div>,
    document.body,
  );
}
