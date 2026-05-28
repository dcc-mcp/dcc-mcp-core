import { type SkillLinks } from '../../admin-types';

type LinkChip = { key: keyof SkillLinks; label: string; url: string };

const LINK_LABELS: Record<keyof SkillLinks, string> = {
  docs: 'Docs',
  repo: 'Repo',
  homepage: 'Home',
  issues: 'Issues',
  chat: 'Chat',
};

/// Compact chip row of author-supplied external links — only rendered
/// when the skill has at least one link configured. Each chip uses
/// `rel="noopener noreferrer"` and stops propagation so a click never
/// escapes the surrounding card-surface button.
export function SkillLinksRow({ links }: { links: SkillLinks | null | undefined }) {
  if (!links) return null;
  const chips: LinkChip[] = (Object.keys(LINK_LABELS) as (keyof SkillLinks)[])
    .map((key) => {
      const url = links[key];
      return url ? { key, label: LINK_LABELS[key], url: String(url) } : null;
    })
    .filter((chip): chip is LinkChip => chip !== null);

  if (chips.length === 0) return null;

  return (
    <div className="skill-card-links">
      {chips.map(({ key, label, url }) => (
        <a
          key={key}
          className="skill-card-link"
          href={url}
          target="_blank"
          rel="noopener noreferrer"
          onClick={(e) => e.stopPropagation()}
          title={url}
        >
          {label}
        </a>
      ))}
    </div>
  );
}
