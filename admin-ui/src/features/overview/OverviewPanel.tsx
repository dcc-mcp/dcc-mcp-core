import type { InterpolationValues, MessageKey } from '../../i18n';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

export type OverviewTab = 'stats' | 'traffic';

export type OverviewPanelProps = {
  active: boolean;
  overviewTab: OverviewTab;
  onTabChange: (tab: OverviewTab) => void;
  t: Translator;
};

const TABS: { id: OverviewTab; labelKey: string }[] = [
  { id: 'stats', labelKey: 'navigation.overviewTab.stats' },
  { id: 'traffic', labelKey: 'navigation.overviewTab.traffic' },
];

/**
 * Thin tab wrapper for the Overview panel.
 *
 * The Overview panel is a composite that delegates to Stats and Traffic
 * content sections. Since Stats and Traffic are rendered inline in
 * App.tsx (they depend on many parent-level variables), we cannot extract
 * them here. Instead this component manages the tab UI and the parent
 * (App.tsx) conditionally renders the correct content section.
 */
export function OverviewPanel({
  active,
  overviewTab,
  onTabChange,
  t,
}: OverviewPanelProps) {
  if (!active) return null;

  return (
    <nav className="overview-tabs" role="tablist" aria-label={t('navigation.overviewTab.meta')}>
      {TABS.map((tab) => (
        <button
          key={tab.id}
          className={overviewTab === tab.id ? 'overview-tab active' : 'overview-tab'}
          role="tab"
          aria-selected={overviewTab === tab.id}
          type="button"
          onClick={() => onTabChange(tab.id)}
        >
          {t(tab.labelKey as MessageKey)}
        </button>
      ))}
    </nav>
  );
}
