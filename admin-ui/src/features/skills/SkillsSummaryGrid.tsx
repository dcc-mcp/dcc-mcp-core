import { type InterpolationValues, type MessageKey } from '../../i18n';
import { MetricTile } from '../../admin-ui-core';
import { type SkillTotals } from './hooks/useSkillsInventory';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

export type SkillsSummaryGridProps = {
  totals: SkillTotals;
  pathCount: number;
  t: Translator;
};

/// Six-tile health summary at the top of the Skills panel.
///
/// Mirrors the original inline metric grid; pulled into its own
/// component so the panel orchestrator can stay focused on layout.
export function SkillsSummaryGrid({ totals, pathCount, t }: SkillsSummaryGridProps) {
  return (
    <div className="metric-grid compact skill-summary-grid">
      <MetricTile
        label={t('skillPaths.metric.loadedSkills')}
        value={totals.loaded}
        detail={t('skillPaths.detail.indexed', { count: totals.total })}
      />
      <MetricTile
        label={t('skillPaths.metric.actions')}
        value={totals.action_count}
        detail={t('skillPaths.detail.fromLoadedSkills')}
      />
      <MetricTile
        label={t('skillPaths.metric.searchPaths')}
        value={pathCount}
        detail={t('skillPaths.detail.activeDiscoveryRoots')}
      />
      <MetricTile
        label={t('skillPaths.metric.searchedUsed')}
        value={`${totals.searched} / ${totals.used}`}
        detail={t('skillPaths.detail.searchedUsed')}
      />
      <MetricTile
        tone={totals.low_adoption > 0 ? 'warn' : 'ok'}
        label={t('skillPaths.metric.lowAdoption')}
        value={totals.low_adoption}
        detail={t('skillPaths.detail.lowAdoption')}
      />
      <MetricTile
        tone={totals.load_errors > 0 || totals.missing_paths > 0 ? 'warn' : 'ok'}
        label={t('skillPaths.metric.healthSignals')}
        value={totals.load_errors + totals.missing_paths}
        detail={t('skillPaths.detail.healthSignals', {
          loads: totals.load_errors,
          paths: totals.missing_paths,
        })}
      />
    </div>
  );
}
