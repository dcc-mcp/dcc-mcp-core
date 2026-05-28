import { type InterpolationValues, type MessageKey } from '../../i18n';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

/// Tap-and-copy list of author-supplied example prompts. Only the
/// first two prompts surface on the card; overflow is announced with a
/// chip so users know more exist behind the detail pane.
export function SkillExamplePromptsList({
  prompts,
  t,
  limit = 2,
}: {
  prompts: string[] | undefined;
  t: Translator;
  limit?: number;
}) {
  if (!prompts || prompts.length === 0) return null;
  const shown = prompts.slice(0, limit);
  const overflow = prompts.length - shown.length;

  return (
    <div className="skill-card-prompts">
      <span className="skill-card-prompts-label">
        {t('skillPaths.examplePrompts.label')}
      </span>
      <ul>
        {shown.map((prompt, idx) => (
          <li key={idx} title={prompt}>
            <span aria-hidden>“</span>
            {prompt}
            <span aria-hidden>”</span>
          </li>
        ))}
        {overflow > 0 ? (
          <li className="skill-card-prompts-overflow">
            {t('skillPaths.examplePrompts.more', { count: overflow })}
          </li>
        ) : null}
      </ul>
    </div>
  );
}
