import { useCallback, useState } from 'react';
import {
  normalizeSkillRow,
  type SkillPayload,
  type SkillRow,
} from '../../../admin-types';
import { apiJson } from '../../../admin-ui-core';

/// Aggregate counts surfaced by the Skills panel summary grid.
export type SkillTotals = {
  total: number;
  loaded: number;
  unloaded: number;
  action_count: number;
  searched: number;
  used: number;
  low_adoption: number;
  load_errors: number;
  missing_paths: number;
};

const EMPTY_TOTALS: SkillTotals = {
  total: 0,
  loaded: 0,
  unloaded: 0,
  action_count: 0,
  searched: 0,
  used: 0,
  low_adoption: 0,
  load_errors: 0,
  missing_paths: 0,
};

export type UseSkillsInventoryArgs = {
  onUpdated: (loaded: number, actions: number) => void;
  onError: (error: unknown) => void;
};

/// Encapsulate the `/skills` fetch lifecycle and the derived totals.
///
/// The hook returns the immutable inventory snapshot plus a `refresh`
/// callback so panel components can re-trigger fetches without owning
/// the underlying state machine. The shape mirrors what every other
/// panel hook in this module exposes (`data`, `refresh`).
export function useSkillsInventory({ onUpdated, onError }: UseSkillsInventoryArgs) {
  const [skills, setSkills] = useState<SkillRow[]>([]);
  const [totals, setTotals] = useState<SkillTotals>(EMPTY_TOTALS);

  const refresh = useCallback(async () => {
    try {
      const payload = await apiJson<SkillPayload>('/skills');
      const rows = Array.isArray(payload.skills) ? payload.skills.map(normalizeSkillRow) : [];
      const healthSection = payload.health ?? {};
      setSkills(rows);
      const totalsNext: SkillTotals = {
        total: Number(payload.total ?? rows.length),
        loaded: Number(payload.loaded ?? rows.filter((s) => s.loaded).length),
        unloaded: Number(payload.unloaded ?? rows.filter((s) => !s.loaded).length),
        action_count: Number(payload.action_count ?? rows.reduce((sum, s) => sum + s.action_count, 0)),
        searched: Number(healthSection.searched_skills ?? rows.filter((s) => s.adoption.searched).length),
        used: Number(healthSection.used_skills ?? rows.filter((s) => s.adoption.used).length),
        low_adoption: Number(
          healthSection.low_adoption_skills ?? rows.filter((s) => s.adoption.low_adoption).length,
        ),
        load_errors: Number(
          healthSection.load_error_count
            ?? rows.reduce((sum, s) => sum + s.adoption.load_error_count, 0),
        ),
        missing_paths: Number(healthSection.missing_path_count ?? 0),
      };
      setTotals(totalsNext);
      onUpdated(totalsNext.loaded, totalsNext.action_count);
    } catch (error) {
      onError(error);
    }
  }, [onError, onUpdated]);

  return { skills, totals, refresh };
}
