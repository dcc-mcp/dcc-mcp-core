import { useState } from 'react';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, expect, it } from 'vitest';
import { MarketplaceDetailModal } from './MarketplaceDetailModal';

afterEach(cleanup);

it('names package details, traps keyboard focus, and restores the opening control', async () => {
  function PackageDetails() {
    const [open, setOpen] = useState(false);
    return (
      <>
        <button onClick={() => setOpen(true)}>View package</button>
        <MarketplaceDetailModal
          entry={open ? {
            name: 'maya-modeling',
            description: 'Modeling tools',
            dcc: ['maya'],
            tags: [],
            url: 'https://example.com/package',
          } : null}
          onClose={() => setOpen(false)}
          t={(key) => key}
        />
      </>
    );
  }

  render(<PackageDetails />);
  const trigger = screen.getByRole('button', { name: 'View package' });
  trigger.focus();
  fireEvent.click(trigger);

  const dialog = await screen.findByRole('dialog', { name: 'maya-modeling' });
  const close = within(dialog).getByRole('button', { name: 'marketplace.detail.close' });
  await waitFor(() => expect(document.activeElement).toBe(close));

  const projectLink = within(dialog).getByRole('link');
  projectLink.focus();
  fireEvent.keyDown(projectLink, { key: 'Tab' });
  expect(document.activeElement).toBe(close);

  trigger.focus();
  expect(dialog.contains(document.activeElement)).toBe(true);

  fireEvent.keyDown(close, { key: 'Escape' });
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
  await waitFor(() => expect(document.activeElement).toBe(trigger));
});
