import { RiAddLine, RiDeleteBinLine } from '@remixicon/react';
import { type InterpolationValues, type MessageKey } from '../../i18n';
import { EmptyRow } from '../../admin-ui-core';
import { type SkillPathRow } from '../../admin-types';
import { Button } from '../../components/ui/button';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

export type SkillSearchPathsTableProps = {
  paths: SkillPathRow[];
  filtered: SkillPathRow[];
  input: string;
  busy: boolean;
  onInputChange: (next: string) => void;
  onAdd: () => void;
  onDelete: (id: number) => void;
  t: Translator;
};

/// Skill-search-path management section.
///
/// Surfaces the "add new path" input above a table of every directory
/// the catalog scans. The component is intentionally dumb — input value
/// + busy flag are owned by the hook; this view only formats them.
export function SkillSearchPathsTable({
  paths,
  filtered,
  input,
  busy,
  onInputChange,
  onAdd,
  onDelete,
  t,
}: SkillSearchPathsTableProps) {
  const canAdd = input.trim().length > 0 && !busy;

  return (
    <div className="skill-inventory-section">
      <h3 className="section-kicker">{t('skillPaths.section.searchPaths')}</h3>
      <div className="skill-path-add">
        <input
          type="text"
          className="list-search-input"
          placeholder={t('skillPaths.placeholder.addDirectoryPath')}
          value={input}
          onChange={(e) => onInputChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && canAdd) {
              e.preventDefault();
              onAdd();
            }
          }}
          aria-label={t('skillPaths.input.newPath')}
        />
        <Button
          type="button"
          size="sm"
          disabled={!canAdd}
          onClick={() => onAdd()}
        >
          <RiAddLine data-icon="inline-start" aria-hidden="true" />
          {t('skillPaths.action.addPath')}
        </Button>
      </div>
      <table>
        <thead>
          <tr>
            <th>{t('skillPaths.table.source')}</th>
            <th>{t('skillPaths.table.pathAlias')}</th>
            <th>{t('skillPaths.table.status')}</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {paths.length === 0 ? (
            <EmptyRow columns={4}>{t('skillPaths.empty.paths')}</EmptyRow>
          ) : filtered.length === 0 ? (
            <EmptyRow columns={4}>{t('skillPaths.empty.pathsSearch')}</EmptyRow>
          ) : (
            filtered.map((row) => (
              <tr key={`${row.source}-${row.path_hash ?? row.path}-${row.id ?? 'x'}`}>
                <td>
                  <span className="source-pill" data-source={row.source} title={row.source}>
                    {row.source_label ?? row.source}
                  </span>
                </td>
                <td>
                  <span className="mono-path">{row.display_path ?? row.path}</span>
                  {row.path_alias ? <div className="muted">{row.path_alias}</div> : null}
                </td>
                <td>
                  <span
                    className={`badge ${
                      row.status === 'present'
                        ? 'badge-ok'
                        : row.status === 'missing'
                          ? 'badge-warn'
                          : 'badge-muted'
                    }`}
                  >
                    {row.status === 'present'
                      ? t('skillPaths.state.present')
                      : row.status === 'missing'
                        ? t('skillPaths.state.missing')
                        : row.status ?? t('common.status.unknown')}
                  </span>
                </td>
                <td>
                  {row.id != null ? (
                    <Button
                      type="button"
                      variant="ghost"
                      size="xs"
                      disabled={busy}
                      onClick={() => onDelete(row.id!)}
                    >
                      <RiDeleteBinLine data-icon="inline-start" aria-hidden="true" />
                      {t('action.remove')}
                    </Button>
                  ) : (
                    '—'
                  )}
                </td>
              </tr>
            ))
          )}
        </tbody>
      </table>
    </div>
  );
}
