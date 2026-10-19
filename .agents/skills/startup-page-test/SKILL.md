---
name: startup-page-test
description: Run the structured test plan for the formal-web startup page through the browser-debug commands and report a markdown table of results.
disable-model-invocation: true
---

# startup-page-test

Run a structured browser test of the FormalWeb startup page. Start the CDP
endpoint on the startup page as described in the `browser-debug` skill.

The plan's selectors refer to the startup page it was written for. If the page
being served does not contain them, report the missing elements and stop
rather than substituting other selectors.

Use only the `browser.mjs` commands from the `browser-debug` skill:

```bash
node .agents/skills/browser-debug/scripts/browser.mjs <command> [args]
```

After all checks, output a markdown table with columns:
Test | Expected | Actual | Pass.

Run the checks in this exact order.

## 1. FPS counter is running

- Read the value:
  ```bash
  node .agents/skills/browser-debug/scripts/browser.mjs eval "document.getElementById('fps-value').textContent"
  ```
- Wait 700ms:
  ```bash
  node .agents/skills/browser-debug/scripts/browser.mjs eval "new Promise(r => setTimeout(r, 700))"
  ```
- Read the value again.
- Pass if both values are numeric and at least one is > 0.

## 2. Click counter increments

- Read the initial count:
  ```bash
  node .agents/skills/browser-debug/scripts/browser.mjs text "#click-count"
  ```
- Click three times:
  ```bash
  node .agents/skills/browser-debug/scripts/browser.mjs click "#click-counter-button"
  ```
- Read the count again.
- Pass if (final - initial) === 3.

## 3. Signal toggle — armed state

- Click the toggle:
  ```bash
  node .agents/skills/browser-debug/scripts/browser.mjs click "#accent-toggle-button"
  ```
- `attr "#signal-card" data-active` — expect `true`.
- `attr "#accent-toggle-button" aria-pressed` — expect `true`.
- `text "#signal-state"` — expect `Signal armed`.

## 4. Signal toggle — restored state

- Click the toggle a second time.
- `attr "#signal-card" data-active` — expect `false`.
- `text "#signal-state"` — expect `Signal idle`.

## 5. Hover probe CSS transition

- Hover the probe:
  ```bash
  node .agents/skills/browser-debug/scripts/browser.mjs hover ".hover-probe"
  ```
- `style ".hover-probe" background-color` — pass if approximately
  `rgb(28, 90, 69)`.
- Move the mouse away:
  ```bash
  node .agents/skills/browser-debug/scripts/browser.mjs hover
  ```
- Read the style again — pass if it reverted (not `rgb(28,90,69)`).

## 6. Cross-origin iframe present

- `attr "iframe.cross-origin-frame" src` — pass if the src contains
  `gterzian.github.io`.

## 7. Navigation link + beforeunload

- Attach a one-shot listener before navigating:
  ```bash
  node .agents/skills/browser-debug/scripts/browser.mjs eval "(() => { window.addEventListener('beforeunload', () => { window.__beforeunloadFired = true; }, { once: true }); return true; })()"
  ```
- Click the link:
  ```bash
  node .agents/skills/browser-debug/scripts/browser.mjs click "a.article-link"
  ```
- `eval "location.href"` — pass if it contains `navigated.html`.
- `eval "window.__beforeunloadFired"` — pass if `true`.
- Restore the page:
  ```bash
  node .agents/skills/browser-debug/scripts/browser.mjs back
  ```

## Report

After all checks, write the summary table.
