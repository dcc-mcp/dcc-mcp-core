import { type InterpolationValues, type MessageKey } from '../../i18n';
import { formatTraceDate } from '../../admin-ui-core';
import { type SkillRow } from '../../admin-types';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

/// Per-card adoption summary — captures the same three signals the
/// pre-redesign inventory table surfaced (state, call/failure counts,
/// last-used timestamp) but renders them as a compact strip suited to
/// the card grid.
export function SkillAdoptionChip({ skill, t }: { skill: SkillRow; t: Translator }) {
  const adoption = skill.adoption;
  const tone =
    adoption.failure_count > 0 || adoption.load_error_count > 0
      ? 'err'
      : adoption.used
        ? 'ok'
        : adoption.low_adoption
          ? 'warn'
          : 'muted';
  const stateLabel = adoption.low_adoption
    ? t('skillPaths.state.lowAdoption')
    : adoption.used
      ? t('skillPaths.state.used')
      : t('skillPaths.state.notUsed');

  return (
    <div className="skill-card-adoption">
      <div className="skill-card-adoption-row">
        <span className={`badge badge-${tone}`}>{stateLabel}</span>
        <span className="skill-card-adoption-counts">
          {t('skillPaths.usage.callsFailures', {
            calls: adoption.call_count,
            failures: adoption.failure_count,
          })}
        </span>
      </div>
      <div className="skill-card-adoption-row muted">
        <span>{t('skillPaths.usage.searchHits', { count: adoption.search_hits })}</span>
        <span>
          {adoption.best_rank == null
            ? t('skillPaths.usage.noRank')
            : t('skillPaths.usage.bestRank', { rank: adoption.best_rank })}
        </span>
      </div>
      <div className="skill-card-adoption-row muted">
        {adoption.last_used
          ? t('skillPaths.usage.lastUsed', {
              time: formatTraceDate(adoption.last_used ?? undefined),
            })
          : t('skillPaths.usage.neverUsed')}
      </div>
    </div>
  );
}
