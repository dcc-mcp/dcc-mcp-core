import { QueryClient } from '@tanstack/react-query';

/// Shared QueryClient singleton for the admin UI.
///
/// Conservative defaults for an embedded dashboard that polls a local gateway:
/// - staleTime: 4s — live data stays responsive while panel payloads poll at a slower cadence
/// - gcTime: 30s — keep inactive panel data in cache for quick tab switching
/// - retry: 1 — one retry is enough for a local gateway; avoids thundering retries
/// - refetchOnWindowFocus: false — this is an embedded SPA, not a user-facing site
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 4_000,
      gcTime: 30_000,
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
});
