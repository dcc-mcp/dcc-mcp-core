import { RiArrowDownSLine, RiArrowRightLine } from '@remixicon/react';
import { type InterpolationValues, type MessageKey } from '../../i18n';
import { formatTraceDate, resolveDccIcon } from '../../admin-ui-core';
import { type SkillRow } from '../../admin-types';
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
            key={`${skill.dcc_type}-${skill.name}-${skill.loaded ? 'on' : 'off'}`}
            className={`skill-inventory-row${selectedRow ? ' is-selected' : ''}${skill.loaded ? '' : ' is-unloaded'}`}
            data-skill-name={skill.name}
            data-dcc={skill.dcc_type || 'unknown'}
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
                <span className="skill-inventory-source">{skill.dcc_type || t('common.status.unknown')}</span>
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
  const dccIcon = resolveDccIcon(skill.dcc_type);

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
          <Badge variant="secondary" className={`skill-inventory-load-state${skill.loaded ? ' is-loaded' : ''}`}>
            {skill.loaded ? t('skillPaths.state.loaded') : t('skillPaths.state.unloaded')}
          </Badge>
        </div>
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
  const instances = skill.instances;
  return (
    <div className="skill-inventory-instances-cell">
      <span className="skill-inventory-count">
        <strong>{skill.instances.length}</strong>
        {t('skillPaths.table.instances').toLowerCase()}
      </span>
      {instances.length > 0 ? (
        <div className="skill-inventory-instances">
          {instances.map((instance) => (
            <code key={instance} title={t('skillPaths.label.instance', { id: instance })}>
              {instance}
            </code>
          ))}
        </div>
      ) : null}
    </div>
  );
}
