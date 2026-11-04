import { useState } from 'react';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, describe, expect, it } from 'vitest';
import { DetailDialog } from './detail-dialog';

afterEach(cleanup);

function Harness() {
  const [open, setOpen] = useState(false);
  return <>
    <button onClick={() => setOpen(true)}>Open skill</button>
    <button>Background action</button>
    <DetailDialog open={open} onClose={() => setOpen(false)} title="Blender geometry">
      <button onClick={() => setOpen(false)}>Close details</button>
      <a href="#tools">Tools</a>
    </DetailDialog>
  </>;
}

describe('DetailDialog', () => {
  it('names the dialog, contains focus, closes with Escape and restores the opener', async () => {
    render(<Harness />);
    const opener = screen.getByRole('button', { name: 'Open skill' });
    const background = screen.getByRole('button', { name: 'Background action' });
    opener.focus();
    fireEvent.click(opener);

    const dialog = screen.getByRole('dialog', { name: 'Blender geometry' });
    await waitFor(() => expect(screen.getByRole('button', { name: 'Close details' })).toHaveFocus());
    background.focus();
    expect(dialog).toContainElement(document.activeElement as HTMLElement);

    fireEvent.keyDown(dialog, { key: 'Escape' });
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    await waitFor(() => expect(opener).toHaveFocus());
  });

  it('closes with its visible action and allows the same details to open again', async () => {
    render(<Harness />);
    const opener = screen.getByRole('button', { name: 'Open skill' });
    opener.focus();
    fireEvent.click(opener);
    fireEvent.click(screen.getByRole('button', { name: 'Close details' }));
    await waitFor(() => expect(opener).toHaveFocus());
    fireEvent.click(opener);
    expect(screen.getByRole('dialog', { name: 'Blender geometry' })).toBeInTheDocument();
  });
});
