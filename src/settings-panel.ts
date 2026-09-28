import { FONT_SIZES, type Settings, type SettingsStore } from './settings';

// Static markup (no user data). Every control names its setting in data-setting,
// so one pair of handlers reads and writes all of them.
const TEMPLATE = `
  <h2 id="settings-title">Settings</h2>
  <fieldset>
    <legend>Theme</legend>
    <label><input type="radio" name="theme" value="system" data-setting="theme"> System</label>
    <label><input type="radio" name="theme" value="light" data-setting="theme"> Light</label>
    <label><input type="radio" name="theme" value="dark" data-setting="theme"> Dark</label>
  </fieldset>
  <label class="row">Editor font size
    <select data-setting="fontSize">${FONT_SIZES.map((size) => `<option value="${size}">${size} px</option>`).join('')}</select>
  </label>
  <label><input type="checkbox" data-setting="wordWrap"> Word wrap</label>
  <label><input type="checkbox" data-setting="autoSave"> Auto-save files after you stop typing</label>
  <fieldset>
    <legend>Line ending for new files</legend>
    <label><input type="radio" name="newFileLineEnding" value="crlf" data-setting="newFileLineEnding"> CRLF (Windows)</label>
    <label><input type="radio" name="newFileLineEnding" value="lf" data-setting="newFileLineEnding"> LF (Unix)</label>
  </fieldset>
  <label><input type="checkbox" data-setting="showPreview"> Show preview</label>
  <label><input type="checkbox" data-setting="showOutline"> Show outline</label>
`;

type Control = HTMLInputElement | HTMLSelectElement;

function controlValue(control: Control): Settings[keyof Settings] {
  if (control instanceof HTMLInputElement && control.type === 'checkbox') return control.checked;
  if (control instanceof HTMLSelectElement) return Number(control.value) as Settings['fontSize'];
  return control.value as Settings['theme'];
}

/** The gear's panel: opens under the gear, closes on Esc, outside click or a second press. */
export function createSettingsPanel(gear: HTMLButtonElement, store: SettingsStore) {
  const panel = document.createElement('div');
  panel.id = 'settings-panel';
  panel.setAttribute('role', 'dialog');
  panel.setAttribute('aria-labelledby', 'settings-title');
  panel.hidden = true;
  panel.innerHTML = TEMPLATE;
  document.body.append(panel);
  gear.setAttribute('aria-haspopup', 'dialog');
  gear.setAttribute('aria-expanded', 'false');

  const controls = [...panel.querySelectorAll<Control>('[data-setting]')];

  function showCurrentValues() {
    const settings = store.get();
    for (const control of controls) {
      const value = settings[control.dataset.setting as keyof Settings];
      if (control instanceof HTMLInputElement && control.type === 'checkbox') control.checked = value === true;
      else if (control instanceof HTMLInputElement && control.type === 'radio') control.checked = control.value === value;
      else control.value = String(value);
    }
  }

  panel.addEventListener('change', (e) => {
    const control = e.target as Control;
    const name = control.dataset.setting as keyof Settings | undefined;
    if (name) store.update({ [name]: controlValue(control) });
  });
  // Shortcuts and menu toggles change settings too; keep the open panel truthful.
  store.onChange(() => {
    if (!panel.hidden) showCurrentValues();
  });

  function open() {
    showCurrentValues();
    panel.hidden = false;
    gear.setAttribute('aria-expanded', 'true');
    controls[0]?.focus();
  }

  function close() {
    if (panel.hidden) return;
    panel.hidden = true;
    gear.setAttribute('aria-expanded', 'false');
    gear.focus();
  }

  function toggle() {
    if (panel.hidden) open();
    else close();
  }

  gear.addEventListener('click', toggle);
  panel.addEventListener('keydown', (e) => {
    if (e.key !== 'Escape') return;
    e.stopPropagation();
    close();
  });
  document.addEventListener('mousedown', (e) => {
    const target = e.target as Node;
    if (!panel.hidden && !panel.contains(target) && !gear.contains(target)) close();
  });

  return { toggle };
}
