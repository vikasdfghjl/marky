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

  el.addEventListener('click', (e) => {
    const target = e.target;
    if (!(target instanceof Element)) return;
    const close = target.closest<HTMLElement>('.tab-close');
    if (close) {
      e.stopPropagation();
      handlers.onClose(Number(close.dataset.id));
      return;
    }
    const tab = target.closest<HTMLButtonElement>('[role="tab"]');
    if (tab) handlers.onActivate(Number(tab.dataset.id));
  });

  el.addEventListener('mousedown', (e) => {
    if (e.button !== 1) return;
    const target = e.target;
    if (!(target instanceof Element)) return;
    const item = target.closest<HTMLElement>('.tab-item');
    const tab = item?.querySelector<HTMLButtonElement>('[role="tab"]');
    if (!tab) return;
    e.preventDefault();
    handlers.onClose(Number(tab.dataset.id));
  });

  el.addEventListener('keydown', (e) => {
    const target = e.target;
    if (!(target instanceof HTMLButtonElement) || !target.matches('[role="tab"]')) return;
    const tabs = [...el.querySelectorAll<HTMLButtonElement>('[role="tab"]')];
    const current = tabs.indexOf(target);
    if (current < 0) return;
    if (e.key === 'Delete') {
      e.preventDefault();
      e.stopPropagation();
      handlers.onClose(Number(target.dataset.id));
      return;
    }
    if (tabs.length < 2) return;

    let next: number;
    if (e.key === 'ArrowRight') next = (current + 1) % tabs.length;
    else if (e.key === 'ArrowLeft') next = (current - 1 + tabs.length) % tabs.length;
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = tabs.length - 1;
    else return;

    e.preventDefault();
    const id = tabs[next].dataset.id;
    handlers.onActivate(Number(id));
    el.querySelector<HTMLButtonElement>(`[role="tab"][data-id="${id}"]`)?.focus();
  });

  return {
    render(tabs: TabLabel[], activeId: number) {
      const focusedElement = document.activeElement;
      const focusId = focusedElement instanceof HTMLElement ? focusedElement.dataset.id : undefined;
      const focusWasInTabs = focusedElement instanceof HTMLElement && el.contains(focusedElement);
      const focusedClose = focusedElement instanceof HTMLElement && focusedElement.classList.contains('tab-close');
      const focusedTab = focusedElement instanceof HTMLButtonElement && focusedElement.matches('[role="tab"]');
      el.replaceChildren(
        ...tabs.map((tab) => {
          const item = document.createElement('div');
          item.className = 'tab-item';
          item.setAttribute('role', 'presentation');

          const select = document.createElement('button');
          select.className = 'tab-select';
          select.type = 'button';
          select.setAttribute('role', 'tab');
          select.setAttribute('aria-selected', String(tab.id === activeId));
          select.id = `tab-${tab.id}`;
          select.setAttribute('aria-controls', 'editor');
          if (tab.id === activeId) document.getElementById('editor')?.setAttribute('aria-labelledby', select.id);
          select.setAttribute('aria-label', `${tab.title}${tab.dirty ? ', unsaved changes' : ''}`);
          select.tabIndex = tab.id === activeId ? 0 : -1;
          select.dataset.id = String(tab.id);
          select.title = tab.path ?? tab.title;

          const title = document.createElement('span');
          title.className = 'tab-title';
          title.textContent = tab.title;

          const close = document.createElement('button');
          close.className = 'tab-close';
          close.type = 'button';
          close.dataset.id = String(tab.id);
          close.setAttribute('aria-label', `Close ${tab.title}`);
          close.title = `Close ${tab.title}`;
          // A dot marks unsaved changes; it turns into × on hover or keyboard focus.
          close.textContent = tab.dirty ? '●' : '×';
          close.classList.toggle('dirty', tab.dirty);

          select.append(title);
          item.append(select, close);
          return item;
        }),
      );
      if (focusWasInTabs) {
        const nextFocusId = tabs.some((tab) => String(tab.id) === focusId) ? focusId : String(activeId);
        const target = focusedClose
          ? el.querySelector<HTMLButtonElement>(`.tab-close[data-id="${nextFocusId}"]`)
          : focusedTab
            ? el.querySelector<HTMLButtonElement>(`[role="tab"][data-id="${nextFocusId}"]`)
            : el.querySelector<HTMLButtonElement>(`.tab-close[data-id="${nextFocusId}"]`);
        target?.focus();
      }
      // Next frame: scrolling now would force a layout in the middle of the caller's DOM updates.
      requestAnimationFrame(() => el.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest', inline: 'nearest' }));
    },
  };
}
