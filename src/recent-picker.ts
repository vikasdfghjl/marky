/**
 * Keyboard-first recent-files list (F9, Ctrl+R). Resolves with the chosen path,
 * or null when dismissed with Esc or a click outside.
 */
export function pickRecentFile(paths: string[]): Promise<string | null> {
  const previousFocus = document.activeElement as HTMLElement | null;
  const backdrop = document.createElement('div');
  backdrop.className = 'picker-backdrop';
  const panel = document.createElement('div');
  panel.className = 'picker';
  panel.setAttribute('role', 'dialog');
  panel.setAttribute('aria-label', 'Recent files');
  const list = document.createElement('ul');
  list.setAttribute('role', 'listbox');
  panel.append(list);
  backdrop.append(panel);

  if (paths.length === 0) {
    const empty = document.createElement('li');
    empty.className = 'picker-empty';
    empty.textContent = 'No recent files yet';
    list.append(empty);
  }

  const items = paths.map((path, index) => {
    const item = document.createElement('li');
    item.setAttribute('role', 'option');
    item.id = `recent-file-${index}`;
    item.dataset.index = String(index);
    const name = document.createElement('span');
    name.className = 'picker-name';
    name.textContent = path.split(/[\\/]/).pop() ?? path;
    const folder = document.createElement('span');
    folder.className = 'picker-folder';
    folder.textContent = path;
    item.append(name, folder);
    list.append(item);
    return item;
  });

  let selected = 0;
  const highlight = (index: number) => {
    if (items.length === 0) return;
    selected = (index + items.length) % items.length;
    items.forEach((item, i) => item.setAttribute('aria-selected', String(i === selected)));
    // Focus stays on the panel; screen readers follow the highlighted option.
    list.setAttribute('aria-activedescendant', items[selected].id);
    items[selected].scrollIntoView({ block: 'nearest' });
  };
  highlight(0);

  return new Promise((resolve) => {
    const finish = (path: string | null) => {
      document.removeEventListener('keydown', onKey, true);
      backdrop.remove();
      previousFocus?.focus();
      resolve(path);
    };

    const onKey = (e: KeyboardEvent) => {
      e.stopPropagation();
      if (e.key === 'Escape') return finish(null);
      if (e.key === 'Enter') return finish(paths[selected] ?? null);
      if (e.key === 'ArrowDown') highlight(selected + 1);
      else if (e.key === 'ArrowUp') highlight(selected - 1);
      else return;
      e.preventDefault();
    };

    backdrop.addEventListener('mousedown', (e) => {
      const item = (e.target as Element).closest<HTMLElement>('[role="option"]');
      if (item) return finish(paths[Number(item.dataset.index)]);
      if (e.target === backdrop) finish(null);
    });
    document.addEventListener('keydown', onKey, true);
    document.body.append(backdrop);
    list.tabIndex = -1;
    list.focus();
  });
}
