import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import App from './App';
import { applyTheme, readThemeMode, resolveTheme } from './theme';
// Stylesheets are imported in cascade order: base/layout/components/panels
// first, then the skills detail view, then responsive overrides (last so
// they win over every base feature stylesheet). styles.css was split into
// these feature files to stay under the admin-ui 3000-line CI cap.
import './styles.css';
import './styles/skill-detail.css';
import './styles/responsive.css';
import './setup.css';
import './styles/skills.css';

// Apply the persisted color scheme before the first paint to avoid a flash.
applyTheme(resolveTheme(readThemeMode()));

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
