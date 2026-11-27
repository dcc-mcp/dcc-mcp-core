import { type InterpolationValues, type MessageKey } from '../../i18n';
import { resolveDccIcon } from '../../admin-ui-core';
import { type SkillRow } from '../../admin-types';
import { Badge } from '../../components/ui/badge';
import { deriveAccentColor, deriveBrandingInitial } from './branding';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

/// Card header — avatar (author logo, DCC icon, or initial), name,
/// summary, and the loaded/unloaded state pill.
///
/// Author-supplied branding (`metadata.dcc-mcp.branding` → `SkillRow.branding`)
/// overrides the fallbacks: a `logo_url` replaces the DCC icon, an
/// `emoji` overrides the initial, and `accent_color` replaces the
/// hash-derived hue passed through the `--skill-accent` custom property.
export function SkillBrandingHeader({ skill, t }: { skill: SkillRow; t: Translator }) {
  const branding = skill.branding ?? null;
  const accent = branding?.accent_color || deriveAccentColor(skill.dcc_type, skill.name);
  const initial = deriveBrandingInitial(skill.name);
  const dccIcon = resolveDccIcon(skill.dcc_type || skill.dcc_types[0] || '');
  const dccLabel = skill.dcc_types.join(', ') || skill.dcc_type || t('common.status.unknown');
  const loadLabel = skill.load_state === 'loaded'
    ? t('skillPaths.state.loaded')
    : skill.load_state === 'partial'
      ? t('skillPaths.state.partial')
      : t('skillPaths.state.unloaded');

  const avatar = branding?.logo_url ? (
    <img src={branding.logo_url} alt="" />
  ) : branding?.emoji ? (
    <span className="skill-card-avatar-emoji">{branding.emoji}</span>
  ) : dccIcon ? (
    <img src={dccIcon} alt="" />
  ) : (
    <span className="skill-card-avatar-initial">{initial}</span>
  );

  return (
    <header className="skill-card-head">
      <div
        className="skill-card-avatar"
        style={{ '--skill-accent': accent } as React.CSSProperties}
        aria-hidden
      >
        {avatar}
      </div>
      <div className="skill-card-identity">
        <h3 className="skill-card-name" title={skill.name}>
          {skill.name}
        </h3>
        {branding?.tagline ? (
          <p className="skill-card-tagline" title={branding.tagline}>
            {branding.tagline}
          </p>
        ) : null}
        <div className="skill-card-meta">
          <Badge variant="outline" className="source-pill" data-dcc={skill.dcc_type || 'multiple'}>
            {dccLabel}
          </Badge>
          <Badge variant={skill.load_state === 'loaded' ? 'default' : 'secondary'} className={skill.load_state === 'loaded' ? 'badge-ok' : 'badge-muted'}>
            {loadLabel}
          </Badge>
          {skill.version ? <span className="skill-card-version">v{skill.version}</span> : null}
        </div>
      </div>
    </header>
  );
}
