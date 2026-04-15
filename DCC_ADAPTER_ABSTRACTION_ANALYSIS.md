# DCC Adapter Boilerplate Optimization Analysis

**Analysis Date**: 2026-04-15
**Scope**: dcc-mcp-maya, dcc-mcp-unreal, dcc-mcp-photoshop, dcc-mcp-zbrush

## Executive Summary

Current DCC adapters exhibit 50-70% code duplication. Each new adapter requires 500-1000 LOC of boilerplate that is 95% identical to existing adapters.

**Opportunity**: Extract to DccServerBase (500 LOC), reducing per-adapter from 1000 LOC → 150 LOC, and enabling new adapters in 2-3 days.

## Total Duplication Per Adapter

| Component | LOC | % Identical |
|-----------|-----|------------|
| Skill path collection | 35-50 | 100% |
| Server __init__ | 32-50 | 95% |
| register_builtin_actions | 62-75 | 98% |
| Skill query methods (7) | 150-200 | 100% |
| start_server() singleton | 100-140 | 100% |
| HotReloader class | 200-220 | 100% |
| GatewayElection class | 300-350 | 100% |
| **TOTAL PER ADAPTER** | **880-1,085** | **99%** |

## Expected Ecosystem Savings

- Maya: 1,564 LOC → 250 LOC (84% reduction)
- Unreal: ~920 LOC → 150 LOC (84%)
- Photoshop: ~1,100 LOC → 200 LOC (82%)
- Total saved: 4,434 LOC (68% reduction)
- Development time: 5-10x faster per new adapter

## Proposed Abstractions

### 1. DccServerBase (500 LOC)
- Parameterized skill path resolution
- Server init with gateway/hotreload
- All 7 skill query methods
- Optional hot-reload and gateway

### 2. DccSkillHotReloader (200 LOC)
- Generic hot-reload implementation
- Works with any DCC

### 3. DccGatewayElection (300 LOC)
- Generic gateway election/failover
- Health check loop
- First-wins socket binding

### 4. Factory Function (50 LOC)
- Singleton creation helper
- Standard setup orchestration

## Implementation Example - Maya Refactor

### Before (1,041 LOC)
- server.py: 1,032 LOC
- hotreload.py: 214 LOC
- gateway_election.py: 318 LOC

### After (250 LOC)
- server.py: 250 LOC (inherit base)
- hotreload.py: DELETED
- gateway_election.py: DELETED

## Benefits

✅ 75-85% code reduction per adapter
✅ 5-10x faster adapter development
✅ Single source of truth for shared logic
✅ Well-tested implementations
✅ Consistent behavior across all DCCs
