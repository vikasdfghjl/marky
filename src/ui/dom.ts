/** An element from index.html by id. Throws at once if the markup lacks it, rather than failing later on null. */
export function byId(id: string): HTMLElement {
  const element = document.getElementById(id);
  if (!element) throw new Error(`index.html has no #${id}`);
  return element;
}
