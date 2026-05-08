// Disable native-OS autocorrect / autocapitalize / autocomplete / smart-quote
// behavior across every text input in the app. Webkit's per-element
// `autocorrect` and `autocapitalize` attributes don't inherit from a
// parent, so we install a MutationObserver that stamps them on every
// input/textarea/[contenteditable] as it lands in the DOM. `spellcheck`
// is handled separately on <html> in index.html (it does inherit).

const TARGET_SELECTOR = "input, textarea, [contenteditable=''], [contenteditable='true']";

function disableFor(el: Element): void {
  if (!(el instanceof HTMLElement)) return;
  el.setAttribute("autocorrect", "off");
  el.setAttribute("autocapitalize", "off");
  el.setAttribute("autocomplete", "off");
  el.setAttribute("spellcheck", "false");
}

function sweep(root: ParentNode): void {
  if (root instanceof Element && root.matches(TARGET_SELECTOR)) {
    disableFor(root);
  }
  root.querySelectorAll(TARGET_SELECTOR).forEach(disableFor);
}

export function installDisableOsPredictions(): void {
  sweep(document);
  const observer = new MutationObserver((mutations) => {
    for (const m of mutations) {
      m.addedNodes.forEach((node) => {
        if (node instanceof Element) sweep(node);
      });
    }
  });
  observer.observe(document.documentElement, { childList: true, subtree: true });
}
