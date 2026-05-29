import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import App from './App';
import { applyTheme, readThemeMode, resolveTheme } from './theme';
import './styles.css';
import './setup.css';
import './styles/skills.css';

// Apply the persisted color scheme before the first paint to avoid a flash.
applyTheme(resolveTheme(readThemeMode()));

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
