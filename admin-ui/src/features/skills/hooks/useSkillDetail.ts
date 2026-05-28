import { useCallback, useState } from 'react';
import {
  normalizeSkillDetailPayload,
  type SkillDetailPayload,
  type SkillRow,
} from '../../../admin-types';
import { apiJson } from '../../../admin-ui-core';

export type UseSkillDetailArgs = {
  onUpdated: (name: string) => void;
  onError: (error: unknown) => void;
};

/// Slide-out detail pane state for a single selected skill.
///
/// Exposes the currently-selected row, the fetched detail payload, and
/// the busy flag the pane uses to show its loading state. Selection
/// clears reset both selected + detail so the pane can render its empty
/// state without leaking the previous skill's content.
export function useSkillDetail({ onUpdated, onError }: UseSkillDetailArgs) {
  const [selected, setSelected] = useState<SkillRow | null>(null);
  const [detail, setDetail] = useState<SkillDetailPayload | null>(null);
  const [busy, setBusy] = useState(false);

  const open = useCallback(
    async (skill: SkillRow) => {
      setSelected(skill);
      setBusy(true);
      setDetail(null);
      try {
        const params = new URLSearchParams({ name: skill.name });
        if (skill.dcc_type) params.set('dcc_type', skill.dcc_type);
        const instanceId = skill.instance_details[0]?.id || skill.instance_ids[0];
        if (instanceId) params.set('instance_id', instanceId);
        const payload = await apiJson<SkillDetailPayload>(`/skill-detail?${params.toString()}`);
        setDetail(normalizeSkillDetailPayload(payload));
        onUpdated(skill.name);
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        setDetail({ skill: null, instances: [], error: message });
        onError(error);
      } finally {
        setBusy(false);
      }
    },
    [onError, onUpdated],
  );

  const close = useCallback(() => {
    setSelected(null);
    setDetail(null);
  }, []);

  const reload = useCallback(() => {
    if (selected) void open(selected);
  }, [open, selected]);

  return { selected, detail, busy, open, close, reload };
}
