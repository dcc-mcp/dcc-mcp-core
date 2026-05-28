/// Skills feature barrel — orchestrator + every consumer-facing primitive.
///
/// Importing from `./features/skills` keeps callers off the internal
/// file layout so we can re-organise pieces (e.g. extract a sub-panel)
/// without rippling through `App.tsx`. Hooks remain reachable via
/// `./features/skills/hooks/*` for tests that want to drive them in
/// isolation.
export { SkillsPanel } from './SkillsPanel';
export type { SkillsPanelProps } from './SkillsPanel';
export { SkillCard } from './SkillCard';
export { SkillDetailPanel } from './SkillDetailPanel';
export { deriveAccentColor, deriveBrandingInitial } from './branding';
