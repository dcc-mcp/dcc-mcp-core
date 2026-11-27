import { RiArrowDownSLine, RiArrowRightLine } from '@remixicon/react';
import { type InterpolationValues, type MessageKey } from '../../i18n';
import { formatTraceDate, resolveDccIcon } from '../../admin-ui-core';
import { type SkillInstanceRef, type SkillRow, type SkillLoadState } from '../../admin-types';
import { Badge } from '../../components/ui/badge';
import { Button } from '../../components/ui/button';
import './SkillInventoryList.css';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

export type SkillInventoryListProps = {
  skills: SkillRow[];
  selected: SkillRow | null;
  onOpen: (skill: SkillRow) => void;
  t: Translator;
};

/// Keep identity and state scannable; expand a row for tool and instance details.
export function SkillInventoryList({ skills, selected, onOpen, t }: SkillInventoryListProps) {
  return (
    <div className="skill-inventory-list" role="list">
      {skills.map((skill) => {
        const selectedRow = selected?.name === skill.name && selected?.dcc_type === skill.dcc_type;
        return (
          <article
            key={`${skill.dcc_type}-${skill.name}-${skill.load_state}`}
            className={`skill-inventory-row${selectedRow ? ' is-selected' : ''}${skill.load_state === 'unloaded' ? ' is-unloaded' : ''}${skill.load_state === 'partial' ? ' is-partial' : ''}`}
            data-skill-name={skill.name}
            data-dcc={skill.dcc_types.join(',') || skill.dcc_type || 'unknown'}
            role="listitem"
          >
            <SkillIdentityCell skill={skill} t={t} onOpen={onOpen} />
            <div className="skill-inventory-action-cell">
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="skill-inventory-action"
                onClick={() => onOpen(skill)}
                aria-label={t('skillPaths.action.openDetail', { name: skill.name })}
              >
                {t('action.view')}
                <RiArrowRightLine data-icon="inline-end" aria-hidden="true" />
              </Button>
            </div>
            <details className="skill-inventory-disclosure">
              <summary>
                <span className="skill-inventory-source">{skill.dcc_types.join(', ') || skill.dcc_type || t('common.status.unknown')}</span>
                <span>{t('skillPaths.usage.callsFailures', {
                  calls: skill.adoption.call_count,
                  failures: skill.adoption.failure_count,
                })}</span>
                <span><strong>{skill.action_count}</strong> {t('skillPaths.metric.actions')}</span>
                <span><strong>{skill.instances.length}</strong> {t('skillPaths.table.instances')}</span>
                <RiArrowDownSLine aria-hidden="true" />
              </summary>
              <div className="skill-inventory-expanded">
                <div className="skill-inventory-description">
                  <p>{skill.summary || t('skillPaths.summary.missing')}</p>
                  {skill.branding?.tagline ? <p>{skill.branding.tagline}</p> : null}
                  {skill.version ? <span className="skill-inventory-version">v{skill.version}</span> : null}
                </div>
                <SkillDccStatesCell skill={skill} t={t} />
                <SkillToolsCell skill={skill} t={t} />
                <SkillInstancesCell skill={skill} t={t} />
                <div className="skill-inventory-usage-detail">
                  <h4>{t('skillPaths.table.usage')}</h4>
                  <SkillStateCell skill={skill} t={t} />
                  <span>{t('skillPaths.usage.searchHits', { count: skill.adoption.search_hits })}</span>
                  <span>{skill.adoption.best_rank == null
                    ? t('skillPaths.usage.noRank')
                    : t('skillPaths.usage.bestRank', { rank: skill.adoption.best_rank })}</span>
                  <span>{skill.adoption.last_used
                    ? t('skillPaths.usage.lastUsed', { time: formatTraceDate(skill.adoption.last_used) })
                    : t('skillPaths.usage.neverUsed')}</span>
                </div>
              </div>
            </details>
          </article>
        );
      })}
    </div>
  );
}

function SkillDccStatesCell({ skill, t }: { skill: SkillRow; t: Translator }) {
  if (skill.dcc_states.length === 0) return null;
  return (
    <div className="skill-inventory-dcc-states">
      <h4>{t('skillPaths.table.dccAvailability')}</h4>
      {skill.dcc_states.map((state) => (
        <div className="skill-inventory-dcc-state" key={state.dcc_type}>
          <strong>{state.dcc_type || t('common.status.unknown')}</strong>
          <span>{skillLoadStateLabel(state.load_state, t)}</span>
          {state.loaded_instance_count > 0 || state.unloaded_instance_count > 0 ? (
            <small>
              {t('skillPaths.dcc.instances', {
                loaded: state.loaded_instance_count,
                unloaded: state.unloaded_instance_count,
              })}
            </small>
          ) : null}
          {state.catalog_available ? (
            <small>{t('skillPaths.state.catalogAvailable')}</small>
          ) : null}
        </div>
      ))}
    </div>
  );
}

function SkillIdentityCell({
  skill,
  t,
  onOpen,
}: {
  skill: SkillRow;
  t: Translator;
  onOpen: (skill: SkillRow) => void;
}) {
  const branding = skill.branding ?? null;
  const dccIcon = resolveDccIcon(skill.dcc_type || skill.dcc_types[0] || '');

  return (
    <div className="skill-inventory-skill-cell">
      <div
        className="skill-inventory-avatar"
        aria-hidden
      >
        <img src={branding?.logo_url || dccIcon} alt="" onError={(event) => {
          if (event.currentTarget.src !== new URL(dccIcon, window.location.href).href) {
            event.currentTarget.src = dccIcon;
          }
        }} />
      </div>
      <div className="skill-inventory-identity">
        <div className="skill-inventory-title-line">
        <button
          type="button"
          className="skill-inventory-title"
          title={skill.name}
          onClick={() => onOpen(skill)}
        >
          {skill.name}
        </button>
          <Badge variant="secondary" className={`skill-inventory-load-state${skill.loaded ? ' is-loaded' : ''}${skill.load_state === 'partial' ? ' is-partial' : ''}`}>
            {skillLoadStateLabel(skill.load_state, t)}
          </Badge>
        </div>
        {skill.dcc_types.length > 0 ? (
          <div className="skill-inventory-scope" title={skill.dcc_types.join(', ')}>
            {skill.dcc_types.join(' · ')}
          </div>
        ) : null}
        <p title={skill.summary ?? t('skillPaths.summary.missing')}>
          {skill.summary || t('skillPaths.summary.missing')}
        </p>
      </div>
    </div>
  );
}

function SkillStateCell({ skill, t }: { skill: SkillRow; t: Translator }) {
  const adoption = skill.adoption;
  const adoptionLabel = adoption.low_adoption
    ? t('skillPaths.state.lowAdoption')
    : adoption.used
      ? t('skillPaths.state.used')
      : t('skillPaths.state.notUsed');

  return (
    <span>{adoptionLabel}</span>
  );
}

function skillLoadStateLabel(state: SkillLoadState, t: Translator): string {
  switch (state) {
    case 'loaded':
      return t('skillPaths.state.loaded');
    case 'partial':
      return t('skillPaths.state.partial');
    case 'unloaded':
      return t('skillPaths.state.unloaded');
    default:
      return t('common.status.unknown');
  }
}

function SkillToolsCell({ skill, t }: { skill: SkillRow; t: Translator }) {
  const tools = skill.tools;
  return (
    <div className="skill-inventory-tools-cell">
      <span className="skill-inventory-count">
        <strong>{skill.action_count}</strong>
        {t('skillPaths.metric.actions').toLowerCase()}
      </span>
      {tools.length > 0 ? (
        <div className="skill-inventory-tools">
          {tools.map((tool) => (
            <code key={tool} title={tool}>{tool}</code>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function SkillInstancesCell({ skill, t }: { skill: SkillRow; t: Translator }) {
  const instances: SkillInstanceRef[] = skill.instance_details.length > 0
    ? skill.instance_details
    : skill.instances.map((prefix, index) => ({
      id: skill.instance_ids[index] ?? prefix,
      prefix,
      dcc_type: skill.dcc_type,
      loaded: undefined,
    }));
  return (
    <div className="skill-inventory-instances-cell">
      <span className="skill-inventory-count">
        <strong>{skill.instances.length}</strong>
        {t('skillPaths.table.instances').toLowerCase()}
      </span>
      {instances.length > 0 ? (
        <div className="skill-inventory-instances">
          {instances.map((instance) => (
            <code
              key={instance.id}
              className={instance.loaded === true ? 'is-loaded' : instance.loaded === false ? 'is-unloaded' : undefined}
              title={`${instance.dcc_type ? `${instance.dcc_type}: ` : ''}${t('skillPaths.label.instance', { id: instance.prefix })}`}
            >
              {instance.prefix}
            </code>
          ))}
        </div>
      ) : null}
    </div>
  );
}
