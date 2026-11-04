import type { ReactNode } from 'react';
import { RiArrowRightSLine, RiBookOpenLine } from '@remixicon/react';
import type { Panel, Translator } from '../admin-types';
import { NavIcon } from '../admin-ui-core';
import { hrefForAdmin, NAVIGATION, projectDocsHref, type AdminLinkExtra } from '../navigation';
import { BrandLogo } from './BrandLogo';
import { Button } from './ui/button';
import { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectTrigger, SelectValue } from './ui/select';

type DashboardShellProps = {
  activePanel: Panel;
  discoverTab: string;
  overviewTab: string;
  tracesTab: string;
  sessionsTab: string;
  reliabilityTab: string;
  statsRange: string;
  onNavigate: (panel: Panel, extra?: AdminLinkExtra) => void;
  preferences: ReactNode;
  children: ReactNode;
  t: Translator;
};

export function DashboardShell({
  activePanel, discoverTab, overviewTab, tracesTab, sessionsTab, reliabilityTab,
  statsRange, onNavigate, preferences, children, t,
}: DashboardShellProps) {
  const entries = NAVIGATION.map((entry) => ({
    ...entry,
    extra: {
      discoverTab: entry.discoverTab,
      overviewTab: entry.overviewTab,
      tracesTab: entry.tracesTab,
      sessionsTab: entry.sessionsTab,
      reliabilityTab: entry.reliabilityTab,
      range: entry.panel === 'overview' ? statsRange : undefined,
    },
  }));
  const current = entries.find((entry) => entry.panel === activePanel
    && (!entry.discoverTab || entry.discoverTab === discoverTab)
    && (!entry.overviewTab || entry.overviewTab === overviewTab)
    && (!entry.tracesTab || entry.tracesTab === tracesTab));
  const groups = [...new Set(entries.map((entry) => entry.groupKey))];
  const subpage = activePanel === 'sessions' && sessionsTab === 'memory'
    ? t('navigation.sessionsTab.memory')
    : activePanel === 'reliability' && reliabilityTab === 'policy'
      ? t('navigation.reliabilityTab.policy')
      : activePanel === 'overview' ? t(overviewTab === 'traffic' ? 'navigation.overviewTab.traffic' : 'navigation.overviewTab.stats') : null;

  return (
    <div className="app-shell">
      <a className="skip-to-content" href="#main-content">{t('chrome.skipToContent')}</a>
      <aside className="side-rail">
        <div className="brand-lockup">
          <BrandLogo />
          <p className="brand-tag">{t('chrome.app.subtitle')}</p>
        </div>
        <nav className="nav-links" aria-label={t('common.aria.adminNavigation')}>
          {groups.map((group) => (
            <section className="nav-group" aria-labelledby={`nav-${group}`} key={group}>
              <h2 className="nav-section-title" id={`nav-${group}`}>{t(group)}</h2>
              {entries.filter((entry) => entry.groupKey === group).map((entry) => (
                <a
                  key={entry.id}
                  href={hrefForAdmin(entry.panel, entry.extra)}
                  className={entry.id === current?.id ? 'nav-link active' : 'nav-link'}
                  aria-current={entry.id === current?.id ? 'page' : undefined}
                  onClick={(event) => {
                    if (event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
                    event.preventDefault();
                    onNavigate(entry.panel, entry.extra);
                  }}
                >
                  <NavIcon panel={entry.icon} />
                  <span>{t(entry.labelKey)}</span>
                </a>
              ))}
            </section>
          ))}
        </nav>
        <div className="rail-footer">{t('chrome.app.title')}</div>
      </aside>
      <div className="workspace">
        <header className="workspace-bar">
          <div className="workspace-location" role="group" aria-label={t('chrome.currentPage')}>
            {current && <>
              <span className="workspace-group">{t(current.groupKey)}</span>
              <RiArrowRightSLine aria-hidden="true" />
              <h1 className="workspace-page">{t(current.labelKey)}</h1>
              {subpage && <><RiArrowRightSLine aria-hidden="true" /><span>{subpage}</span></>}
            </>}
          </div>
          <span className="mobile-brand">DCC-MCP</span>
          <div className="workspace-preferences">
            <Button asChild variant="ghost" size="icon" className="docs-link">
              <a href={projectDocsHref()} target="_blank" rel="noopener noreferrer" aria-label={t('navigation.docs.title')} title={t('navigation.docs.title')}>
                <RiBookOpenLine aria-hidden="true" />
              </a>
            </Button>
            {preferences}
          </div>
          <div className="mobile-navigation">
            <Select value={current?.id} onValueChange={(id) => {
              const entry = entries.find((item) => item.id === id);
              if (entry) onNavigate(entry.panel, entry.extra);
            }}>
              <SelectTrigger className="admin-select-trigger mobile-navigation-trigger" aria-label={t('chrome.navigate')}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent className="admin-select-content mobile-navigation-content" position="popper" align="start">
                {groups.map((group) => (
                  <SelectGroup key={group}>
                    <SelectLabel>{t(group)}</SelectLabel>
                    {entries.filter((entry) => entry.groupKey === group).map((entry) => (
                      <SelectItem key={entry.id} value={entry.id}>{t(entry.labelKey)}</SelectItem>
                    ))}
                  </SelectGroup>
                ))}
              </SelectContent>
            </Select>
          </div>
        </header>
        <main className="main-stage" id="main-content" tabIndex={-1}>{children}</main>
      </div>
    </div>
  );
}
