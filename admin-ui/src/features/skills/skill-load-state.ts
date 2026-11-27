import type { SkillLoadState } from '../../admin-types';
import type { MessageKey } from '../../i18n';

export const skillStateLabel: Record<SkillLoadState, MessageKey> = {
  loaded: 'skillPaths.state.loaded',
  partial: 'skillPaths.state.partial',
  unloaded: 'skillPaths.state.unloaded',
  catalog_only: 'skillPaths.state.catalogAvailable',
  unknown: 'common.status.unknown',
};
