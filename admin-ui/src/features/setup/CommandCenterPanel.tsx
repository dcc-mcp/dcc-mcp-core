import { useEffect, useRef, useState } from 'react';
import { RiApps2Line, RiArrowRightLine, RiFileCopyLine, RiPuzzle2Line } from '@remixicon/react';
import type { InterpolationValues, MessageKey } from '../../i18n';
import type { HealthPayload, InstanceSummary } from '../../admin-types';
import { formatBytes, formatUptime, gatewayLabel } from '../../admin-ui-core';
import { Button } from '../../components/ui/button';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

type CommandCenterPanelProps = {
  health: HealthPayload | null;
  instanceSummary: InstanceSummary;
  mcpUrl: string;
  onCopy: (text: string, label: string) => boolean | void | Promise<boolean | void>;
  onOpenInstances: () => void;
  onOpenMarketplace: () => void;
  onOpenSkills: () => void;
  t: Translator;
};

type CommandRow = {
  key: string;
  command: string;
  labelKey: MessageKey;
  detailKey: MessageKey;
};

type CommandSection = {
  key: string;
  titleKey: MessageKey;
  detailKey: MessageKey;
  commands: CommandRow[];
};

type CommandGuide = 'prompt' | 'agent-cli' | 'human-cli';

export function CommandCenterPanel({
  health,
  instanceSummary,
  mcpUrl,
  onCopy,
  onOpenInstances,
  onOpenMarketplace,
  onOpenSkills,
  t,
}: CommandCenterPanelProps) {
  const [copiedCommandKey, setCopiedCommandKey] = useState<string | null>(null);
  const [activeGuide, setActiveGuide] = useState<CommandGuide>('prompt');
  const copyFeedbackTimer = useRef<number | null>(null);
  const gatewayTone: 'ok' | 'warn' | 'muted' = health?.status === 'ok' ? 'ok' : health ? 'warn' : 'muted';
  const agentSections: CommandSection[] = [
    {
      key: 'gateway',
      titleKey: 'setup.section.gateway',
      detailKey: 'setup.section.gatewayDetail',
      commands: [
        {
          key: 'ensure',
          labelKey: 'setup.cli.ensure',
          detailKey: 'setup.cli.ensureDetail',
          command: 'dcc-mcp-cli list',
        },
        {
          key: 'inventory',
          labelKey: 'setup.cli.inventory',
          detailKey: 'setup.cli.inventoryDetail',
          command: 'dcc-mcp-cli health',
        },
      ],
    },
    {
      key: 'execute',
      titleKey: 'setup.section.execute',
      detailKey: 'setup.section.executeDetail',
      commands: [
        {
          key: 'search',
          labelKey: 'setup.cli.search',
          detailKey: 'setup.cli.searchDetail',
          command: 'dcc-mcp-cli search --query "create sphere" --dcc-type <dcc_type> --limit 20',
        },
        {
          key: 'describe',
          labelKey: 'setup.cli.describe',
          detailKey: 'setup.cli.describeDetail',
          command: 'dcc-mcp-cli describe <tool_slug>',
        },
        {
          key: 'call',
          labelKey: 'setup.cli.call',
          detailKey: 'setup.cli.callDetail',
          command: 'dcc-mcp-cli call <tool_slug> --json \'{"radius":2.0}\'',
        },
      ],
    },
    {
      key: 'extend',
      titleKey: 'setup.section.extend',
      detailKey: 'setup.section.extendDetail',
      commands: [
        {
          key: 'load-skill',
          labelKey: 'setup.cli.loadSkill',
          detailKey: 'setup.cli.loadSkillDetail',
          command: 'dcc-mcp-cli load-skill <skill_name> --dcc-type <dcc_type> --instance-id <instance_id>',
        },
        {
          key: 'marketplace',
          labelKey: 'setup.cli.marketplace',
          detailKey: 'setup.cli.marketplaceDetail',
          command: 'dcc-mcp-cli marketplace search --query "rigging" --dcc maya --limit 20',
        },
      ],
    },
  ];
  const humanSections: CommandSection[] = [
    {
      key: 'gateway',
      titleKey: 'setup.section.gateway',
      detailKey: 'setup.section.gatewayDetail',
      commands: [
        {
          key: 'human-ensure',
          labelKey: 'setup.cli.ensure',
          detailKey: 'setup.cli.ensureDetail',
          command: 'dcc-mcp-cli list',
        },
        {
          key: 'human-inventory',
          labelKey: 'setup.cli.inventory',
          detailKey: 'setup.cli.inventoryDetail',
          command: 'dcc-mcp-cli health',
        },
      ],
    },
    {
      key: 'extend',
      titleKey: 'setup.section.extend',
      detailKey: 'setup.section.extendDetail',
      commands: [
        {
          key: 'human-marketplace',
          labelKey: 'setup.cli.marketplace',
          detailKey: 'setup.cli.marketplaceDetail',
          command: 'dcc-mcp-cli marketplace search --query "rigging" --dcc maya --limit 20',
        },
        {
          key: 'marketplace-install',
          labelKey: 'setup.cli.marketplaceInstall',
          detailKey: 'setup.cli.marketplaceInstallDetail',
          command: 'dcc-mcp-cli marketplace install <package_name> --dcc maya',
        },
        {
          key: 'marketplace-update',
          labelKey: 'setup.cli.marketplaceUpdate',
          detailKey: 'setup.cli.marketplaceUpdateDetail',
          command: 'dcc-mcp-cli marketplace update <package_name> --dcc maya',
        },
      ],
    },
    {
      key: 'maintain',
      titleKey: 'setup.section.maintain',
      detailKey: 'setup.section.maintainDetail',
      commands: [
        {
          key: 'server-update-check',
          labelKey: 'setup.cli.updateCheck',
          detailKey: 'setup.cli.updateCheckDetail',
          command: 'dcc-mcp-cli update check --binary dcc-mcp-server --current-version <server_version>',
        },
        {
          key: 'cli-update-apply',
          labelKey: 'setup.cli.cliUpdateApply',
          detailKey: 'setup.cli.cliUpdateApplyDetail',
          command: 'dcc-mcp-cli update apply',
        },
      ],
    },
  ];
  const agentPrompt = [
    t('setup.agentPrompt.line.role'),
    t('setup.agentPrompt.line.gateway', { mcpUrl }),
    t('setup.agentPrompt.line.ensure'),
    t('setup.agentPrompt.line.workflow'),
    t('setup.agentPrompt.line.skills'),
    t('setup.agentPrompt.line.report'),
  ].join('\n');

  useEffect(() => () => {
    if (copyFeedbackTimer.current != null) {
      window.clearTimeout(copyFeedbackTimer.current);
    }
  }, []);

  const markCopied = (commandKey: string) => {
    if (copyFeedbackTimer.current != null) {
      window.clearTimeout(copyFeedbackTimer.current);
    }
    setCopiedCommandKey(commandKey);
    copyFeedbackTimer.current = window.setTimeout(() => {
      setCopiedCommandKey(null);
      copyFeedbackTimer.current = null;
    }, 2000);
  };

  const handleCopyCommand = async (row: CommandRow) => {
    const copied = await onCopy(row.command, t(row.labelKey));
    if (copied !== false) {
      markCopied(row.key);
    }
  };

  const handleCopyPrompt = async () => {
    const copied = await onCopy(agentPrompt, t('setup.agentPrompt.copyLabel'));
    if (copied !== false) {
      markCopied('agent-prompt');
    }
  };

  return (
    <div className="command-center-layout">
      <section className="command-center" aria-labelledby="command-center-title">
        <div className="command-center-head">
          <div>
            <h3 id="command-center-title">{t('setup.command.title')}</h3>
            <p>{t('setup.command.meta')}</p>
          </div>
          <span className={`command-state command-state-${gatewayTone}`}>
            {gatewayLabel(health)}
          </span>
        </div>

        <div className="command-status-grid" aria-label={t('setup.command.statusAria')}>
          <StatusCell
            label={t('setup.metric.gateway')}
            value={health?.status ?? t('common.status.unknown')}
            detail={mcpUrl}
            tone={gatewayTone}
          />
          <StatusCell
            label={t('setup.metric.instances')}
            value={instanceSummary.live}
            detail={t('setup.detail.instanceSummary', {
              stale: instanceSummary.stale,
              unhealthy: instanceSummary.unhealthy,
            })}
            tone={instanceSummary.unhealthy > 0 ? 'warn' : 'ok'}
          />
          <StatusCell
            label={t('setup.metric.uptime')}
            value={formatUptime(health?.uptime_secs)}
            detail={formatBytes(health?.rss_bytes)}
          />
        </div>

        <div className="command-guide-tabs" role="tablist" aria-label={t('setup.guide.tabsAria')}>
          <button
            className={`command-guide-tab${activeGuide === 'prompt' ? ' active' : ''}`}
            type="button"
            role="tab"
            id="command-guide-tab-prompt"
            aria-controls="command-guide-panel-prompt"
            aria-selected={activeGuide === 'prompt'}
            onClick={() => setActiveGuide('prompt')}
          >
            <strong>{t('setup.guide.prompt')}</strong>
            <small>{t('setup.guide.promptMeta')}</small>
          </button>
          <button
            className={`command-guide-tab${activeGuide === 'agent-cli' ? ' active' : ''}`}
            type="button"
            role="tab"
            id="command-guide-tab-agent-cli"
            aria-controls="command-guide-panel-agent-cli"
            aria-selected={activeGuide === 'agent-cli'}
            onClick={() => setActiveGuide('agent-cli')}
          >
            <strong>{t('setup.guide.agentCli')}</strong>
            <small>{t('setup.guide.agentCliMeta')}</small>
          </button>
          <button
            className={`command-guide-tab${activeGuide === 'human-cli' ? ' active' : ''}`}
            type="button"
            role="tab"
            id="command-guide-tab-human-cli"
            aria-controls="command-guide-panel-human-cli"
            aria-selected={activeGuide === 'human-cli'}
            onClick={() => setActiveGuide('human-cli')}
          >
            <strong>{t('setup.guide.humanCli')}</strong>
            <small>{t('setup.guide.humanCliMeta')}</small>
          </button>
        </div>

        {activeGuide === 'prompt' ? (
          <div
            className="command-guide-panel command-agent-prompt"
            role="tabpanel"
            id="command-guide-panel-prompt"
            aria-labelledby="command-guide-tab-prompt"
          >
            <div className="command-agent-prompt-head">
              <div>
                <strong>{t('setup.agentPrompt.title')}</strong>
                <span>{t('setup.agentPrompt.detail')}</span>
              </div>
              <Button
                className="command-copy-action command-prompt-copy"
                type="button"
                variant={copiedCommandKey === 'agent-prompt' ? 'default' : 'secondary'}
                size="sm"
                aria-label={`${t('action.copy')}: ${t('setup.agentPrompt.copyLabel')}`}
                aria-live="polite"
                data-copied={copiedCommandKey === 'agent-prompt' ? 'true' : undefined}
                onClick={() => void handleCopyPrompt()}
              >
                <RiFileCopyLine data-icon="inline-start" aria-hidden="true" />
                {copiedCommandKey === 'agent-prompt' ? t('common.action.copied') : t('action.copy')}
              </Button>
            </div>
            <pre>{agentPrompt}</pre>
          </div>
        ) : (
          <div
            className="cli-command-sections"
            role="tabpanel"
            id={activeGuide === 'agent-cli' ? 'command-guide-panel-agent-cli' : 'command-guide-panel-human-cli'}
            aria-labelledby={activeGuide === 'agent-cli' ? 'command-guide-tab-agent-cli' : 'command-guide-tab-human-cli'}
          >
            {(activeGuide === 'agent-cli' ? agentSections : humanSections).map((section) => (
              <section className={`cli-command-section cli-command-section-${section.key}`} key={section.key}>
                <div className="cli-command-section-head">
                  <strong>{t(section.titleKey)}</strong>
                  <span>{t(section.detailKey)}</span>
                </div>
                <div className="cli-command-list">
                  {section.commands.map((row, index) => (
                    <article className="cli-command-row" data-copied={copiedCommandKey === row.key ? 'true' : undefined} key={row.key}>
                      <span className="cli-command-index">{String(index + 1).padStart(2, '0')}</span>
                      <div className="cli-command-copy">
                        <div className="cli-command-title">
                          <strong>{t(row.labelKey)}</strong>
                          <span>{t(row.detailKey)}</span>
                        </div>
                        <code>{row.command}</code>
                      </div>
                      <Button
                        className="command-copy-action cli-command-action"
                        type="button"
                        variant={copiedCommandKey === row.key ? 'default' : 'secondary'}
                        size="sm"
                        aria-label={`${t('action.copy')}: ${row.command}`}
                        aria-live="polite"
                        data-copied={copiedCommandKey === row.key ? 'true' : undefined}
                        onClick={() => void handleCopyCommand(row)}
                      >
                        <RiFileCopyLine data-icon="inline-start" aria-hidden="true" />
                        {copiedCommandKey === row.key ? t('common.action.copied') : t('action.copy')}
                      </Button>
                    </article>
                  ))}
                </div>
              </section>
            ))}
          </div>
        )}
      </section>

      <aside className="command-center-aside" aria-label={t('setup.command.asideAria')}>
        <div className="command-aside-block command-aside-summary-block">
          <span>{t('setup.aside.context')}</span>
          <div className="command-aside-summary">
            <div>
              <small>{t('setup.metric.gateway')}</small>
              <strong>{gatewayLabel(health)}</strong>
            </div>
            <div>
              <small>{t('setup.metric.instances')}</small>
              <strong>{instanceSummary.live}</strong>
            </div>
          </div>
          <code>{mcpUrl}</code>
        </div>
        <div className="command-aside-block">
          <span>{t('setup.aside.flow')}</span>
          <div className="command-aside-flow">
            <div className="command-aside-flow-step">
              <strong>01</strong>
              <span>
                <b>{t('setup.aside.flow.search')}</b>
                <small>{t('setup.aside.flow.searchDetail')}</small>
              </span>
            </div>
            <div className="command-aside-flow-step">
              <strong>02</strong>
              <span>
                <b>{t('setup.aside.flow.describe')}</b>
                <small>{t('setup.aside.flow.describeDetail')}</small>
              </span>
            </div>
            <div className="command-aside-flow-step">
              <strong>03</strong>
              <span>
                <b>{t('setup.aside.flow.call')}</b>
                <small>{t('setup.aside.flow.callDetail')}</small>
              </span>
            </div>
          </div>
        </div>
        <div className="command-aside-block">
          <span>{t('setup.aside.extend')}</span>
          <div className="command-aside-actions">
            <button type="button" className="command-aside-action" onClick={onOpenSkills}>
              <span className="command-aside-action-icon" aria-hidden="true">
                <RiPuzzle2Line />
              </span>
              <span className="command-aside-action-copy">
                <strong>{t('navigation.panel.skills')}</strong>
                <small>{t('setup.aside.skillsDetail')}</small>
              </span>
              <RiArrowRightLine className="command-aside-action-arrow" aria-hidden="true" />
            </button>
            <button type="button" className="command-aside-action" onClick={onOpenMarketplace}>
              <span className="command-aside-action-icon" aria-hidden="true">
                <RiApps2Line />
              </span>
              <span className="command-aside-action-copy">
                <strong>{t('navigation.panel.marketplace')}</strong>
                <small>{t('setup.aside.marketplaceDetail')}</small>
              </span>
              <RiArrowRightLine className="command-aside-action-arrow" aria-hidden="true" />
            </button>
          </div>
        </div>
        <div className="command-aside-block">
          <span>{t('setup.aside.runtime')}</span>
          <div className="command-aside-runtime">
            <strong>{t('setup.detail.liveInstances', { count: instanceSummary.live })}</strong>
            <small>{t('setup.detail.instanceSummary', {
              stale: instanceSummary.stale,
              unhealthy: instanceSummary.unhealthy,
            })}</small>
          </div>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="command-aside-runtime-link"
            onClick={onOpenInstances}
          >
            {t('navigation.panel.instances')}
            <RiArrowRightLine data-icon="inline-end" aria-hidden="true" />
          </Button>
        </div>
      </aside>
    </div>
  );
}

function StatusCell({
  label,
  value,
  detail,
  tone = 'muted',
}: {
  label: string;
  value: string | number;
  detail?: string;
  tone?: 'ok' | 'warn' | 'muted';
}) {
  return (
    <div className={`command-status-cell command-status-${tone}`}>
      <span>{label}</span>
      <strong>{value}</strong>
      {detail ? <small>{detail}</small> : null}
    </div>
  );
}
