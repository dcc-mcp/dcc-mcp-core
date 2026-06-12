import { RiEyeLine } from '@remixicon/react';
import { type InterpolationValues, type MessageKey } from '../../i18n';
import { formatTraceDate, resolveDccIcon } from '../../admin-ui-core';
import { type SkillRow } from '../../admin-types';
import { Button } from '../../components/ui/button';
import { deriveAccentColor, deriveBrandingInitial } from './branding';
import './SkillInventoryList.css';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

export type SkillInventoryListProps = {
  skills: SkillRow[];
  selected: SkillRow | null;
  onOpen: (skill: SkillRow) => void;
  t: Translator;
};

/// Dense operator list for live skill inventory.
///
/// Skills are operational records, not marketplace promos: each row keeps the
/// DCC, load state, adoption signal, tools, and instance coverage visible while
/// preserving the detail drawer for full SKILL.md inspection.
export function SkillInventoryList({ skills, selected, onOpen, t }: SkillInventoryListProps) {
  return (
    <div className="skill-inventory-list" role="list">
      <div className="skill-inventory-list-header" aria-hidden="true">
        <span>{t('skillPaths.table.skill')}</span>
        <span>{t('skillPaths.table.state')}</span>
        <span>{t('skillPaths.table.usage')}</span>
        <span>{t('skillPaths.table.tools')}</span>
        <span>{t('skillPaths.table.instances')}</span>
        <span>{t('action.view')}</span>
      </div>
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
            <SkillStateCell skill={skill} t={t} />
            <SkillUsageCell skill={skill} t={t} />
            <SkillToolsCell skill={skill} t={t} />
            <SkillInstancesCell skill={skill} t={t} />
            <div className="skill-inventory-action-cell">
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="skill-inventory-action"
                onClick={() => onOpen(skill)}
                aria-label={t('skillPaths.action.openDetail', { name: skill.name })}
              >
                <RiEyeLine data-icon="inline-start" aria-hidden="true" />
                {t('action.view')}
              </Button>
            </div>
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
  const accent = branding?.accent_color || deriveAccentColor(skill.dcc_type, skill.name);
  const dccIcon = resolveDccIcon(skill.dcc_type);
  const avatar = branding?.logo_url ? (
    <img src={branding.logo_url} alt="" />
  ) : branding?.emoji ? (
    <span className="skill-inventory-avatar-emoji">{branding.emoji}</span>
  ) : dccIcon ? (
    <img src={dccIcon} alt="" />
  ) : (
    <span>{deriveBrandingInitial(skill.name)}</span>
  );

  return (
    <div className="skill-inventory-skill-cell">
      <div
        className="skill-inventory-avatar"
        style={{ '--skill-accent': accent } as React.CSSProperties}
        aria-hidden
      >
        {avatar}
      </div>
      <div className="skill-inventory-identity">
        <button
          type="button"
          className="skill-inventory-title"
          onClick={() => onOpen(skill)}
        >
          {skill.name}
        </button>
        <p title={skill.summary ?? t('skillPaths.summary.missing')}>
          {skill.summary || t('skillPaths.summary.missing')}
        </p>
        {branding?.tagline ? (
          <small title={branding.tagline}>{branding.tagline}</small>
        ) : null}
      </div>
    </div>
  );
}

function SkillStateCell({ skill, t }: { skill: SkillRow; t: Translator }) {
  const adoption = skill.adoption;
  const adoptionTone =
    adoption.failure_count > 0 || adoption.load_error_count > 0
      ? 'err'
      : adoption.used
        ? 'ok'
        : adoption.low_adoption
          ? 'warn'
          : 'muted';
  const adoptionLabel = adoption.low_adoption
    ? t('skillPaths.state.lowAdoption')
    : adoption.used
      ? t('skillPaths.state.used')
      : t('skillPaths.state.notUsed');

  return (
    <div className="skill-inventory-state-cell">
      <span className="source-pill" data-dcc={skill.dcc_type || 'unknown'}>
        {skill.dcc_type || t('common.status.unknown')}
      </span>
      <span className={`badge ${skill.loaded ? 'badge-ok' : 'badge-muted'}`}>
        {skill.loaded ? t('skillPaths.state.loaded') : t('skillPaths.state.unloaded')}
      </span>
      <span className={`badge badge-${adoptionTone}`}>{adoptionLabel}</span>
      {skill.version ? (
        <code className="skill-inventory-version">v{skill.version}</code>
      ) : null}
    </div>
  );
}

function SkillUsageCell({ skill, t }: { skill: SkillRow; t: Translator }) {
  const adoption = skill.adoption;
  return (
    <div className="skill-inventory-usage-cell">
      <strong>
        {t('skillPaths.usage.callsFailures', {
          calls: adoption.call_count,
          failures: adoption.failure_count,
        })}
      </strong>
      <span>{t('skillPaths.usage.searchHits', { count: adoption.search_hits })}</span>
      <span>
        {adoption.best_rank == null
          ? t('skillPaths.usage.noRank')
          : t('skillPaths.usage.bestRank', { rank: adoption.best_rank })}
      </span>
      <span>
        {adoption.last_used
          ? t('skillPaths.usage.lastUsed', { time: formatTraceDate(adoption.last_used) })
          : t('skillPaths.usage.neverUsed')}
      </span>
    </div>
  );
}

function SkillToolsCell({ skill, t }: { skill: SkillRow; t: Translator }) {
  const tools = skill.tools.slice(0, 4);
  const overflow = skill.tools.length - tools.length;
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
          {overflow > 0 ? <span>+{overflow}</span> : null}
        </div>
      ) : null}
    </div>
  );
}

function SkillInstancesCell({ skill, t }: { skill: SkillRow; t: Translator }) {
  const instances = skill.instances.slice(0, 3);
  const overflow = skill.instances.length - instances.length;
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
          {overflow > 0 ? <span>+{overflow}</span> : null}
        </div>
      ) : null}
    </div>
  );
}
