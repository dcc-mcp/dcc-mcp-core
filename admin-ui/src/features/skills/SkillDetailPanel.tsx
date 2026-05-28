import { type InterpolationValues, type MessageKey } from '../../i18n';
import { compactId } from '../../admin-ui-core';
import { type SkillDetailInstance, type SkillDetailPayload, type SkillRow } from '../../admin-types';
import { SkillMarkdownPreview } from '../../components/SkillMarkdownPreview';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

type ToolSummary = { name: string; summary: string; annotations: string[] };

function readAnnotations(raw: unknown): string[] {
  if (!raw || typeof raw !== 'object') return [];
  const out: string[] = [];
  for (const [k, v] of Object.entries(raw as Record<string, unknown>)) {
    if (v === true) out.push(k);
  }
  return out;
}

function skillDetailTools(detail: SkillDetailInstance | null | undefined): ToolSummary[] {
  if (!detail || !Array.isArray(detail.tools)) return [];
  return detail.tools
    .filter((tool): tool is Record<string, unknown> => !!tool && typeof tool === 'object')
    .map((tool) => ({
      name: typeof tool.name === 'string' ? tool.name : '',
      summary: typeof tool.summary === 'string' ? tool.summary : '',
      annotations: readAnnotations(tool.annotations),
    }))
    .filter((tool) => tool.name);
}

export type SkillDetailPanelProps = {
  skill: SkillRow;
  detail: SkillDetailPayload | null;
  busy: boolean;
  onReload: () => void;
  onClose: () => void;
  t: Translator;
};

/// Slide-out pane summarising the SKILL.md of a single inventory row.
///
/// Moved from `admin-ui-core.tsx` so the Skills feature owns its own
/// detail surface end-to-end. Surface is unchanged from the pre-split
/// version — markdown preview, tool list, instance pills.
export function SkillDetailPanel({
  skill,
  detail,
  busy,
  onReload,
  onClose,
  t,
}: SkillDetailPanelProps) {
  const selected = detail?.skill ?? detail?.instances?.[0] ?? null;
  const tools = skillDetailTools(selected);
  const dccLabel = selected?.dcc_type ?? selected?.dcc ?? skill.dcc_type;
  const instanceCount =
    detail?.instances?.length || skill.instance_count || (selected?.instance_id ? 1 : 0);

  return (
    <section className="skill-detail-panel" aria-live="polite">
      <div className="skill-detail-heading">
        <div>
          <h3>{selected?.name ?? skill.name}</h3>
          <div className="skill-detail-meta">
            <span className="source-pill">{dccLabel || t('common.status.unknown')}</span>
            <span className={`badge ${skill.loaded ? 'badge-ok' : 'badge-muted'}`}>
              {selected?.state
                ?? (skill.loaded
                  ? t('skillPaths.state.loaded')
                  : t('skillPaths.state.unloaded'))}
            </span>
            {selected?.instance_short ? (
              <span className="mono-path">
                {t('skillPaths.label.instance', { id: selected.instance_short })}
              </span>
            ) : null}
          </div>
        </div>
        <div className="table-actions">
          <button className="refresh-btn" type="button" disabled={busy} onClick={onReload}>
            {busy ? t('common.status.loading') : t('action.reload')}
          </button>
          <button className="linkish" type="button" onClick={onClose}>
            {t('action.close')}
          </button>
        </div>
      </div>
      {selected?.description ? (
        <p className="skill-detail-description">{selected.description}</p>
      ) : null}
      {selected?.skill_md_path ? (
        <div className="mono-path skill-detail-path">{selected.skill_md_path}</div>
      ) : null}
      <div className="skill-detail-summary-grid">
        <span>
          <strong>{t('skillPaths.table.state')}</strong>
          {selected?.state
            ?? (skill.loaded ? t('skillPaths.state.loaded') : t('skillPaths.state.unloaded'))}
        </span>
        <span>
          <strong>{t('skillPaths.metric.actions')}</strong>
          {tools.length || skill.action_count}
        </span>
        <span>
          <strong>{t('skillPaths.table.instances')}</strong>
          {instanceCount}
        </span>
        <span>
          <strong>DCC</strong>
          {dccLabel || t('common.status.unknown')}
        </span>
      </div>
      {detail?.error || selected?.error ? (
        <p className="empty skill-detail-error">{detail?.error ?? selected?.error}</p>
      ) : null}
      {selected?.message ? <p className="empty">{selected.message}</p> : null}
      {tools.length > 0 ? (
        <div className="skill-tool-list">
          {tools.map((tool) => (
            <div className="skill-tool-row" key={tool.name}>
              <code title={tool.name}>{tool.name}</code>
              {tool.summary ? <span>{tool.summary}</span> : null}
              {tool.annotations.length > 0 ? (
                <div className="skill-tool-annotations">
                  {tool.annotations.map((label) => (
                    <span className="source-pill" key={`${tool.name}-${label}`}>
                      {label}
                    </span>
                  ))}
                </div>
              ) : null}
            </div>
          ))}
        </div>
      ) : null}
      <SkillMarkdownPreview
        markdown={selected?.markdown}
        frontmatterLabel={t('skillPaths.label.frontmatter')}
        noMarkdownLabel={t('skillPaths.detail.noMarkdown')}
        noBodyLabel={t('skillPaths.detail.noBody')}
        copyLabel={t('action.copy')}
        copiedLabel={t('skillPaths.action.copiedCode')}
      />
      {detail?.instances && detail.instances.length > 1 ? (
        <div className="skill-detail-instances">
          {detail.instances.map((instance) => (
            <span
              className="source-pill"
              key={`${instance.instance_id ?? instance.instance_short ?? instance.name}`}
            >
              {instance.dcc_type ?? instance.dcc ?? skill.dcc_type}:
              {instance.instance_short ?? compactId(instance.instance_id)}
            </span>
          ))}
        </div>
      ) : null}
    </section>
  );
}
