import type { Panel } from '../admin-types';

export type PanelSearchBarProps = {
  panel: Panel;
  discoverTab?: string;
  placeholder: string;
  value: string;
  ariaLabel: string;
  meta?: string;
  showLatencyFilter?: boolean;
  slowOnly?: boolean;
  slowLabel?: string;
  allLabel?: string;
  latencyTitle?: string;
  onChange: (value: string) => void;
  onToggleLatency?: () => void;
};

export function PanelSearchBar({
  panel,
  discoverTab,
  placeholder,
  value,
  ariaLabel,
  meta,
  showLatencyFilter = false,
  slowOnly = false,
  slowLabel,
  allLabel,
  latencyTitle,
  onChange,
  onToggleLatency,
}: PanelSearchBarProps) {
  const canToggleLatency = showLatencyFilter && onToggleLatency && slowLabel && allLabel;
  return (
    <div
      className="list-search-wrap"
      role="search"
      data-panel={panel}
      data-discover-tab={panel === 'discover' ? discoverTab : undefined}
      data-has-filter={canToggleLatency ? 'true' : undefined}
      data-has-meta={meta ? 'true' : undefined}
    >
      <input
        type="search"
        className="list-search-input"
        placeholder={placeholder}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        aria-label={ariaLabel}
      />
      {canToggleLatency ? (
        <button
          className={`filter-chip ${slowOnly ? 'active' : ''}`}
          type="button"
          aria-pressed={slowOnly}
          title={latencyTitle}
          onClick={onToggleLatency}
        >
          {slowOnly ? allLabel : slowLabel}
        </button>
      ) : null}
      {meta ? (
        <span className="list-search-meta" aria-live="polite">
          {meta}
        </span>
      ) : null}
    </div>
  );
}
