import type { InterpolationValues, MessageKey } from '../../i18n';
import { SkillsPanel } from '../skills';
import { MarketplacePanel } from '../marketplace';
import { IntegrationsPanel } from '../integrations';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

export type DiscoverTab = 'skills' | 'marketplace' | 'integrations';

export type DiscoverPanelProps = {
  active: boolean;
  discoverTab: DiscoverTab;
  search: string;
  onTabChange: (tab: DiscoverTab) => void;
  // SkillsPanel props
  skillUpdatedAt: string;
  skillError?: string;
  onSkillUpdated: (text: string) => void;
  onSkillError: (err: unknown) => void;
  onSkillCountsChange: (counts: { skills: number; paths: number }) => void;
  highlightSkillName?: string | null;
  onHighlightConsumed?: () => void;
  // MarketplacePanel props
  marketplaceUpdatedAt: string;
  marketplaceError?: string;
  onMarketplaceUpdated: (text: string) => void;
  onMarketplaceError: (err: unknown) => void;
  onMarketplaceCountsChange: (counts: { total: number; installed: number }) => void;
  coreVersion?: string | null;
  // IntegrationsPanel props
  integrationsUpdatedAt: string;
  integrationsError?: string;
  onIntegrationsUpdated: (text: string) => void;
  onIntegrationsError: (err: unknown) => void;
  onIntegrationsCountsChange: (counts: { total: number; active: number }) => void;
  /// Navigate to the Skills tab and highlight a skill (marketplace install).
  onNavigateToSkills?: (skillName: string) => void;
  // Shared
  t: Translator;
};

const TABS: { id: DiscoverTab; labelKey: string }[] = [
  { id: 'skills', labelKey: 'navigation.discoverTab.skills' },
  { id: 'marketplace', labelKey: 'navigation.discoverTab.marketplace' },
  { id: 'integrations', labelKey: 'navigation.discoverTab.integrations' },
];

export function DiscoverPanel({
  active,
  discoverTab,
  search,
  onTabChange,
  skillUpdatedAt,
  skillError,
  onSkillUpdated,
  onSkillError,
  onSkillCountsChange,
  highlightSkillName,
  onHighlightConsumed,
  marketplaceUpdatedAt,
  marketplaceError,
  onMarketplaceUpdated,
  onMarketplaceError,
  onMarketplaceCountsChange,
  coreVersion,
  integrationsUpdatedAt,
  integrationsError,
  onIntegrationsUpdated,
  onIntegrationsError,
  onIntegrationsCountsChange,
  onNavigateToSkills,
  t,
}: DiscoverPanelProps) {

  if (!active) return null;

  return (
    <section className="panel active discover-panel" data-panel="discover">
      <nav className="discover-tabs" role="tablist" aria-label={t('navigation.discoverTab.meta')}>
        {TABS.map((tab) => (
          <button
            key={tab.id}
            className={discoverTab === tab.id ? 'discover-tab active' : 'discover-tab'}
            role="tab"
            aria-selected={discoverTab === tab.id}
            type="button"
            onClick={() => onTabChange(tab.id)}
          >
            {t(tab.labelKey as MessageKey)}
          </button>
        ))}
      </nav>
      <SkillsPanel
        active={active && discoverTab === 'skills'}
        search={search}
        updatedAt={skillUpdatedAt}
        error={skillError}
        onUpdated={onSkillUpdated}
        onError={onSkillError}
        onCountsChange={onSkillCountsChange}
        highlightSkillName={highlightSkillName}
        onHighlightConsumed={onHighlightConsumed}
        t={t}
      />
      <MarketplacePanel
        active={active && discoverTab === 'marketplace'}
        search={search}
        updatedAt={marketplaceUpdatedAt}
        error={marketplaceError}
        onUpdated={onMarketplaceUpdated}
        onError={onMarketplaceError}
        onCountsChange={onMarketplaceCountsChange}
        coreVersion={coreVersion}
        onNavigateToSkills={onNavigateToSkills}
        t={t}
      />
      <IntegrationsPanel
        active={active && discoverTab === 'integrations'}
        search={search}
        updatedAt={integrationsUpdatedAt}
        error={integrationsError}
        onUpdated={onIntegrationsUpdated}
        onError={onIntegrationsError}
        onCountsChange={onIntegrationsCountsChange}
        t={t}
      />
    </section>
  );
}
