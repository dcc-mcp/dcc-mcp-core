/// Marketplace-card branding derivation.
///
/// When `metadata.dcc-mcp.branding` is not authored (most skills today),
/// we still want each card to feel distinct. This module derives a
/// stable accent colour + initial letter from the skill identifier so
/// the grid reads as a marketplace instead of an undifferentiated table.

const FNV_OFFSET = 2166136261;
const FNV_PRIME = 16777619;

/// 32-bit FNV-1a — small, fast, deterministic. Adequate for assigning
/// a hue per skill name; not cryptographic.
function fnv1a(text: string): number {
  let hash = FNV_OFFSET;
  for (let i = 0; i < text.length; i++) {
    hash ^= text.charCodeAt(i);
    hash = Math.imul(hash, FNV_PRIME);
  }
  return hash >>> 0;
}

/// Derive a CSS-friendly accent colour for a card from skill identity.
///
/// Hues are sampled across the full colour wheel, but saturation and
/// lightness are clamped so the result remains readable against both
/// light and dark panels.
export function deriveAccentColor(dccType: string, skillName: string): string {
  const seed = fnv1a(`${dccType}::${skillName}`);
  const hue = seed % 360;
  return `hsl(${hue} 70% 56%)`;
}

/// First grapheme of the skill name, uppercased — used as the avatar
/// fallback when the skill ships without a branding glyph.
export function deriveBrandingInitial(skillName: string): string {
  const trimmed = skillName.trim();
  if (!trimmed) return '?';
  return trimmed.charAt(0).toUpperCase();
}
