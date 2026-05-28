import { type InterpolationValues, type MessageKey } from '../../i18n';
import { type SkillRow } from '../../admin-types';
import { SkillAdoptionChip } from './SkillAdoptionChip';
import { SkillBrandingHeader } from './SkillBrandingHeader';
import { SkillExamplePromptsList } from './SkillExamplePromptsList';
import { SkillLinksRow } from './SkillLinksRow';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

export type SkillCardProps = {
  skill: SkillRow;
  selected: boolean;
  onOpen: (skill: SkillRow) => void;
  t: Translator;
};

/// Marketplace card — one per skill in the inventory grid.
///
/// Renders branding header + summary + adoption strip + instance/tool
/// chips. Clicking anywhere on the card opens the detail pane via
/// `onOpen`; the underlying button is the only focusable element so
/// keyboard navigation lands on the entire card surface.
export function SkillCard({ skill, selected, onOpen, t }: SkillCardProps) {
  const toolChips = skill.tools.slice(0, 6);
  const toolOverflow = skill.tools.length - toolChips.length;

  return (
    <article
      className={`skill-card${selected ? ' skill-card-selected' : ''}${
        skill.loaded ? '' : ' skill-card-unloaded'
      }`}
      data-skill-name={skill.name}
      data-dcc={skill.dcc_type || 'unknown'}
    >
      <button
        type="button"
        className="skill-card-surface"
        onClick={() => onOpen(skill)}
        aria-pressed={selected}
        aria-label={t('skillPaths.action.openDetail', { name: skill.name })}
      >
        <SkillBrandingHeader skill={skill} t={t} />
        {skill.summary ? (
          <p className="skill-card-summary" title={skill.summary}>
            {skill.summary}
          </p>
        ) : (
          <p className="skill-card-summary skill-card-summary-empty">
            {t('skillPaths.summary.missing')}
          </p>
        )}
        <SkillAdoptionChip skill={skill} t={t} />
        <footer className="skill-card-footer">
          <div className="skill-card-chiprow">
            <span className="skill-card-stat">
              <strong>{skill.action_count}</strong>
              <span>{t('skillPaths.metric.actions').toLowerCase()}</span>
            </span>
            <span className="skill-card-stat">
              <strong>{skill.instances.length}</strong>
              <span>{t('skillPaths.table.instances').toLowerCase()}</span>
            </span>
          </div>
          {toolChips.length > 0 ? (
            <div className="skill-card-toolchips">
              {toolChips.map((tool) => (
                <code key={tool} className="skill-card-toolchip" title={tool}>
                  {tool}
                </code>
              ))}
              {toolOverflow > 0 ? (
                <span className="skill-card-toolchip skill-card-toolchip-overflow">
                  +{toolOverflow}
                </span>
              ) : null}
            </div>
          ) : null}
          <SkillExamplePromptsList prompts={skill.example_prompts} t={t} />
          <SkillLinksRow links={skill.links ?? null} />
        </footer>
      </button>
    </article>
  );
}
