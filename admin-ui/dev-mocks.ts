/// Vite dev-server mock middleware.
///
/// Returns canned fixture data on `/admin/api/*` so the marketplace UI
/// renders end-to-end at `npm run dev` without a live gateway. The
/// plugin is only registered from `vite.config.ts` when the dev server
/// boots, so the production single-file bundle is unaffected.

import type { Plugin } from 'vite';
import type { IncomingMessage, ServerResponse } from 'node:http';

const NOW = new Date().toISOString();

const HEALTH = {
  status: 'ok',
  instances_ready: 3,
  instances_total: 4,
  uptime_secs: 18342,
  version: '0.17.38-dev',
  response_format: { default: 'toon' },
  searched_skills: 5,
  used_skills: 3,
  low_adoption_skills: 1,
  load_error_count: 0,
};

type BrandingMock = { accent_color?: string; emoji?: string; tagline?: string };
type LinksMock = { docs?: string; repo?: string; homepage?: string; issues?: string };

function makeSkill(name: string, dcc: string, summary: string, opts: Partial<{
  loaded: boolean;
  actions: number;
  tools: string[];
  used: boolean;
  lowAdoption: boolean;
  failures: number;
  instances: number;
  branding: BrandingMock;
  links: LinksMock;
  example_prompts: string[];
}> = {}) {
  const loaded = opts.loaded ?? true;
  const id8 = name.padEnd(8, '0').slice(0, 8);
  const fullId = `${id8}-aaaa-bbbb-cccc-${'0'.repeat(12)}`;
  return {
    name,
    dcc_type: dcc,
    loaded,
    action_count: opts.actions ?? (opts.tools?.length ?? 3),
    instance_count: opts.instances ?? 1,
    instances: [id8],
    instance_ids: [fullId],
    instance_details: [{ id: fullId, prefix: id8, dcc_type: dcc }],
    tools: opts.tools ?? ['operate', 'inspect', 'render'],
    summary,
    branding: opts.branding ?? null,
    links: opts.links ?? null,
    example_prompts: opts.example_prompts ?? [],
    adoption: {
      search_hits: 4,
      best_rank: opts.used ? 1 : 7,
      average_rank: 3.2,
      selected_count: opts.used ? 3 : 0,
      call_count: opts.used ? 12 : 0,
      failure_count: opts.failures ?? 0,
      load_error_count: 0,
      last_searched: NOW,
      last_used: opts.used ? NOW : null,
      fallback_displaced_by_scripting: 0,
      searched: true,
      used: !!opts.used,
      low_adoption: !!opts.lowAdoption,
    },
  };
}

const SKILLS_PAYLOAD = {
  total: 7,
  loaded: 6,
  unloaded: 1,
  action_count: 21,
  health: {
    searched_skills: 5,
    used_skills: 3,
    low_adoption_skills: 1,
    load_error_count: 0,
    missing_path_count: 0,
  },
  skills: [
    makeSkill('maya-modeling', 'maya', 'Polygon modeling primitives for Autodesk Maya — bevel, extrude, retopo, mirror, and bridge edges.', {
      tools: ['create_sphere', 'extrude_edges', 'bevel', 'mirror_geometry'],
      used: true,
      actions: 4,
      branding: { accent_color: '#ff7a45', emoji: '🐉', tagline: 'High-impact bevel and retopo flow' },
      links: { docs: 'https://example.com/skills/maya-modeling', repo: 'https://github.com/example/maya-modeling' },
      example_prompts: [
        'Bevel the selected polygon edges with mitred corners',
        'Retopologise the hi-poly mesh down to a quad cage',
        'Mirror geometry across the world X-axis',
      ],
    }),
    makeSkill('blender-lookdev', 'blender', 'Material assignment, environment lighting and beauty preview rendering for Blender Cycles.', {
      tools: ['assign_material', 'set_envlight', 'render_preview'],
      used: true,
      actions: 3,
      branding: { accent_color: '#f5792a', emoji: '🎨', tagline: 'Lookdev fast-path for Cycles' },
      links: { docs: 'https://example.com/skills/blender-lookdev', homepage: 'https://example.com' },
      example_prompts: [
        'Assign a procedural metal material to the selected object',
        'Render a beauty preview at 720p with denoising',
      ],
    }),
    makeSkill('houdini-render', 'houdini', 'Karma / Mantra render submission, AOV control, and crop-region overrides.', {
      tools: ['submit_render', 'aov_setup', 'crop_window'],
      lowAdoption: true,
      actions: 3,
      branding: { accent_color: '#ff8f00', tagline: 'Submit Karma jobs without leaving the agent' },
      links: { docs: 'https://example.com/skills/houdini-render', issues: 'https://example.com/issues' },
      example_prompts: ['Submit a Karma render for shot 010_010 at full quality'],
    }),
    makeSkill('photoshop-comp', 'photoshop', 'Multi-pass compositing, smart object linking, and export bundles for downstream review pipelines.', { tools: ['merge_passes', 'export_review', 'link_smart_object'], used: true, actions: 3 }),
    makeSkill('unity-scene', 'unity', 'Scene graph inspection and asset import diagnostics for Unity Editor projects.', { tools: ['list_objects', 'check_imports'], actions: 2 }),
    makeSkill('unreal-bp', 'unreal', 'Blueprint scaffolding and node connection helpers for Unreal Engine 5.', { tools: ['create_blueprint', 'connect_nodes', 'rebuild_lighting'], failures: 1, actions: 3 }),
    makeSkill('dcc-diagnostics', 'maya', 'Infrastructure-layer diagnostics — screenshot, audit log dump, environment probe.', { loaded: false, tools: ['screenshot', 'audit_log'], actions: 2 }),
  ],
};

const SKILL_PATHS_PAYLOAD = {
  paths: [
    { source: 'env:DCC_MCP_SKILL_PATHS', path: 'G:/studio/skills', status: 'present', exists: true },
    { id: 7, source: 'admin_custom', path: 'G:/custom/admin-skills', status: 'present', exists: true },
    { id: 9, source: 'admin_custom', path: 'D:/old/skills', status: 'missing', exists: false },
  ],
};

function send(res: ServerResponse, status: number, body: unknown) {
  res.statusCode = status;
  res.setHeader('Content-Type', 'application/json');
  res.end(JSON.stringify(body));
}

export function adminApiMockPlugin(): Plugin {
  return {
    name: 'dcc-mcp-admin-api-mock',
    configureServer(server) {
      server.middlewares.use('/admin/api', (req: IncomingMessage, res: ServerResponse, next) => {
        const url = req.url ?? '';
        if (url.startsWith('/health')) return send(res, 200, HEALTH);
        if (url.startsWith('/skills')) return send(res, 200, SKILLS_PAYLOAD);
        if (url.startsWith('/skill-paths')) {
          if (req.method === 'POST' || req.method === 'DELETE') return send(res, 200, { ok: true });
          return send(res, 200, SKILL_PATHS_PAYLOAD);
        }
        if (url.startsWith('/skill-detail')) {
          return send(res, 200, {
            skill: {
              name: 'maya-modeling',
              description: 'Modeling primitives for Maya.',
              dcc_type: 'maya',
              state: 'loaded',
              markdown: '---\nname: maya-modeling\n---\n\n# Modeling\n\nSphere, cube, extrude, bevel.',
              tools: [
                { name: 'create_sphere', summary: 'Create a UV sphere primitive', annotations: { readonly: true } },
                { name: 'extrude_edges', summary: 'Extrude selected edges', annotations: { destructive: true } },
              ],
            },
            instances: [],
          });
        }
        return next();
      });
    },
  };
}
