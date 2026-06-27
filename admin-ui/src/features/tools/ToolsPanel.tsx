import { RiRefreshLine } from '@remixicon/react';
import { Button } from '../../components/ui/button';
import { StatusLine, groupRows, toolGroupLabel, toolInstanceLabel } from '../../admin-ui-core';
import type { ToolRow, Translator } from '../../admin-types';

export type ToolsPanelProps = {
  updatedAt: string;
  error?: string;
  tools: ToolRow[];
  filteredTools: ToolRow[];
  onRefresh: () => void;
  t: Translator;
};

export function ToolsPanel({
  updatedAt,
  error,
  tools,
  filteredTools,
  onRefresh,
  t,
}: ToolsPanelProps) {
  return (
    <section className="panel active tools-panel">
      <h2>{t('tools.title')}</h2>
      <StatusLine text={updatedAt} error={error} />
      {tools.length === 0 ? (
        <p className="empty">{t('tools.empty.none')}</p>
      ) : filteredTools.length === 0 ? (
        <p className="empty">{t('tools.empty.search')}</p>
      ) : (
        Array.from(groupRows(filteredTools, toolGroupLabel).entries())
          .sort(([a], [b]) => a.localeCompare(b))
          .map(([group, groupTools]) => (
            <div key={group} className="group-block">
              <h3 className="group-title">{group}</h3>
              <p className="group-meta">{t('tools.group.toolCount', { count: groupTools.length })}</p>
              <table>
                <thead>
                  <tr>
                    <th>{t('tools.table.slug')}</th>
                    <th>{t('common.table.appType')}</th>
                    <th>{t('common.table.instance')}</th>
                    <th>{t('common.table.summary')}</th>
                  </tr>
                </thead>
                <tbody>
                  {groupTools.map((tool) => (
                    <tr key={tool.slug}>
                      <td>{tool.slug}</td>
                      <td>{tool.dcc_type}</td>
                      <td>{toolInstanceLabel(tool)}</td>
                      <td>{tool.summary.slice(0, 120)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ))
      )}
      <Button type="button" size="sm" onClick={onRefresh}>
        <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
        {t('action.refresh')}
      </Button>
    </section>
  );
}
