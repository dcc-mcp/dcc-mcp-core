import brandLogoDark from '../assets/brand/dcc-mcp-logo-admin-dark.png';
import brandLogoLight from '../assets/brand/dcc-mcp-logo-admin-light.png';

export function BrandLogo() {
  return (
    <div className="brand-logo" role="img" aria-label="DCC MCP">
      <img
        className="brand-logo-image brand-logo-image-light"
        src={brandLogoLight}
        alt=""
        aria-hidden="true"
      />
      <img
        className="brand-logo-image brand-logo-image-dark"
        src={brandLogoDark}
        alt=""
        aria-hidden="true"
      />
    </div>
  );
}
