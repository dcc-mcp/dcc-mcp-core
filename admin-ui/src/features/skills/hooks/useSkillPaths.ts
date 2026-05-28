import { useCallback, useState } from 'react';
import { normalizeSkillPathRow, type SkillPathRow } from '../../../admin-types';
import { ADMIN_FETCH_TIMEOUT_MS, API_BASE, apiJson } from '../../../admin-ui-core';

export type UseSkillPathsArgs = {
  onUpdated: (count: number) => void;
  onError: (error: unknown) => void;
  /// Fired after a successful add/remove so the parent can re-fetch the
  /// `/skills` inventory and reflect rows that landed via the new path.
  onMutated?: () => Promise<void> | void;
};

/// Skill-search-path CRUD encapsulated for the Skills panel.
///
/// Owns the search-paths list, the "add new path" input buffer, and the
/// busy flag the table uses to disable per-row actions while a write is
/// in flight. The hook stays UI-agnostic — it only exposes data and
/// action handlers; rendering decisions live in the panel components.
export function useSkillPaths({ onUpdated, onError, onMutated }: UseSkillPathsArgs) {
  const [paths, setPaths] = useState<SkillPathRow[]>([]);
  const [input, setInput] = useState('');
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const payload = await apiJson<{ paths: SkillPathRow[] }>('/skill-paths');
      const rows = Array.isArray(payload.paths) ? payload.paths.map(normalizeSkillPathRow) : [];
      setPaths(rows);
      onUpdated(rows.length);
    } catch (error) {
      onError(error);
    }
  }, [onError, onUpdated]);

  const sendWithTimeout = useCallback(async (init: RequestInit, urlSuffix: string) => {
    const ctrl = new AbortController();
    const tid = window.setTimeout(() => ctrl.abort(), ADMIN_FETCH_TIMEOUT_MS);
    try {
      const res = await fetch(`${API_BASE}${urlSuffix}`, { ...init, signal: ctrl.signal });
      if (!res.ok) {
        throw new Error(`${res.status} ${res.statusText}`);
      }
    } catch (err) {
      if (err instanceof DOMException && err.name === 'AbortError') {
        throw new Error(`Request timed out after ${ADMIN_FETCH_TIMEOUT_MS / 1000}s`);
      }
      throw err;
    } finally {
      clearTimeout(tid);
    }
  }, []);

  const addPath = useCallback(async () => {
    const path = input.trim();
    if (!path) return;
    setBusy(true);
    try {
      await sendWithTimeout(
        {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ path }),
        },
        '/skill-paths',
      );
      setInput('');
      await onMutated?.();
    } catch (error) {
      onError(error);
    } finally {
      setBusy(false);
    }
  }, [input, onError, onMutated, sendWithTimeout]);

  const deletePath = useCallback(
    async (id: number) => {
      setBusy(true);
      try {
        await sendWithTimeout({ method: 'DELETE' }, `/skill-paths/${encodeURIComponent(String(id))}`);
        await onMutated?.();
      } catch (error) {
        onError(error);
      } finally {
        setBusy(false);
      }
    },
    [onError, onMutated, sendWithTimeout],
  );

  return { paths, input, setInput, busy, refresh, addPath, deletePath };
}
