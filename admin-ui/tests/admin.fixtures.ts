export const analyticsSeriesFixture = [
  { date: '2025-06-02', calls: 1, failures: 0, tokens_input: 120, tokens_output: 90, avg_duration_ms: '36' },
  { date: '2025-07-08', calls: 2, failures: 0, tokens_input: 210, tokens_output: 140, avg_duration_ms: '48' },
  { date: '2025-08-14', calls: 2, failures: 0, tokens_input: 260, tokens_output: 160, avg_duration_ms: '54' },
  { date: '2025-09-03', calls: 1, failures: 0, tokens_input: 160, tokens_output: 110, avg_duration_ms: '42' },
  { date: '2025-10-20', calls: 3, failures: 0, tokens_input: 410, tokens_output: 260, avg_duration_ms: '62' },
  { date: '2025-11-18', calls: 2, failures: 0, tokens_input: 280, tokens_output: 180, avg_duration_ms: '58' },
  { date: '2025-12-09', calls: 2, failures: 0, tokens_input: 310, tokens_output: 220, avg_duration_ms: '66' },
  { date: '2026-01-22', calls: 3, failures: 0, tokens_input: 460, tokens_output: 330, avg_duration_ms: '72' },
  { date: '2026-02-11', calls: 1, failures: 0, tokens_input: 180, tokens_output: 120, avg_duration_ms: '39' },
  { date: '2026-03-04', calls: 4, failures: 0, tokens_input: 580, tokens_output: 390, avg_duration_ms: '82' },
  { date: '2026-04-15', calls: 3, failures: 0, tokens_input: 520, tokens_output: 360, avg_duration_ms: '76' },
  { date: '2026-05-01', calls: 3, failures: 0, tokens_input: 620, tokens_output: 430, avg_duration_ms: '88' },
  { date: '2026-05-04', calls: 2, failures: 0, tokens_input: 420, tokens_output: 300, avg_duration_ms: '70' },
  { date: '2026-05-06', calls: 3, failures: 1, tokens_input: 760, tokens_output: 520, avg_duration_ms: '210' },
  { date: '2026-05-08', calls: 2, failures: 0, tokens_input: 500, tokens_output: 340, avg_duration_ms: '79' },
  { date: '2026-05-11', calls: 3, failures: 0, tokens_input: 900, tokens_output: 680, avg_duration_ms: '94' },
  { date: '2026-05-13', calls: 2, failures: 0, tokens_input: 620, tokens_output: 440, avg_duration_ms: '84' },
  { date: '2026-05-15', calls: 3, failures: 0, tokens_input: 1100, tokens_output: 820, avg_duration_ms: '98' },
  { date: '2026-05-17', calls: 2, failures: 0, tokens_input: 780, tokens_output: 560, avg_duration_ms: '92' },
  { date: '2026-05-18', calls: 3, failures: 1, tokens_input: 1280, tokens_output: 940, avg_duration_ms: '240' },
];

export const analyticsTotals = analyticsSeriesFixture.reduce(
  (totals, point) => ({
    calls: totals.calls + point.calls,
    failures: totals.failures + point.failures,
    tokensInput: totals.tokensInput + point.tokens_input,
    tokensOutput: totals.tokensOutput + point.tokens_output,
  }),
  { calls: 0, failures: 0, tokensInput: 0, tokensOutput: 0 },
);
