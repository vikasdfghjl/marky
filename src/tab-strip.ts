/** What the strip needs to draw one tab. */
export interface TabLabel {
  id: number;
  title: string;
  /** Full path, shown as a tooltip; null for untitled documents. */
  path: string | null;
  dirty: boolean;
}

export interface TabStripHandlers {
  onActivate(id: number): void;
  onClose(id: number): void;
}

/**
 * The row of tabs above the editor (F14). Re-rendered in full on every change:
 * there are at most 20 tabs, so diffing would cost more code than it saves.
 */
export function createTabStrip(el: HTMLElement, handlers: TabStripHandlers) {
  el.setAttribute('role', 'tablist');
  el.setAttribute('aria-label', 'Open files');

  el.addEventListener('mousedown', (e) => {
    const tab = (e.target as Element).closest<HTMLElement>('[role="tab"]');
    if (!tab) return;
    const id = Number(tab.dataset.id);
    const clickedClose = (e.target as Element).closest('.tab-close');
    if (e.button === 1 || (e.button === 0 && clickedClose)) {
      e.preventDefault(); // middle-click would otherwise start autoscroll
      return handlers.onClose(id);
    }
    if (e.button === 0) handlers.onActivate(id);
  });

  return {
    render(tabs: TabLabel[], activeId: number) {
      el.replaceChildren(
        ...tabs.map((tab) => {
          const item = document.createElement('div');
          item.setAttribute('role', 'tab');
          item.setAttribute('aria-selected', String(tab.id === activeId));
          item.dataset.id = String(tab.id);
          item.title = tab.path ?? tab.title;

          const title = document.createElement('span');
          title.className = 'tab-title';
          title.textContent = tab.title;

          const close = document.createElement('button');
          close.className = 'tab-close';
          close.tabIndex = -1; // tabs are reached with Ctrl+Tab, closed with Ctrl+W
          close.setAttribute('aria-label', `Close ${tab.title}`);
          // A dot marks unsaved changes; it turns into × on hover, like most editors.
          close.textContent = tab.dirty ? '●' : '×';
          close.classList.toggle('dirty', tab.dirty);

          item.append(title, close);
          return item;
        }),
      );
      el.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    },
  };
}
