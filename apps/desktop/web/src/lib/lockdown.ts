/**
 * Production builds only: remove the ways a user can reload the page by
 * accident. A reload wipes unsaved canvas edits and the undo history without
 * the "save?" prompt that closing the window shows.
 *
 * Development keeps the native menu (Reload, Inspect Element) for debugging.
 */
export function lockDownInProduction() {
  if (!import.meta.env.PROD) return;

  // Right-click menu: the page-level one carries Reload. Text fields keep their
  // own cut / copy / paste menu.
  window.addEventListener('contextmenu', (event) => {
    const target = event.target as HTMLElement | null;
    if (target?.closest('input, textarea, [contenteditable="true"]')) return;
    event.preventDefault();
  });

  // F5, Ctrl/Cmd+R and the hard-reload variants.
  window.addEventListener(
    'keydown',
    (event) => {
      const reload = event.key === 'F5' || ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'r');
      if (reload) event.preventDefault();
    },
    true,
  );
}
