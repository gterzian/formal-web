/**
 * Test plan for the FormalWeb startup page.
 *
 * This is just a string - the LLM does the sequencing using the general
 * browser tools. To test a different page, write a new prompt here and
 * register it as a new command in index.ts.
 */
export const FORMALWEB_TEST_PLAN = `
You are running a structured browser test of the FormalWeb startup page.
Use only the browser_* tools. After all checks, output a markdown table
with columns: Test | Expected | Actual | Pass.

Run the checks in this exact order:

---

**1. FPS counter is running**
- Use browser_evaluate to read document.getElementById('fps-value').textContent
- Wait 700ms (browser_evaluate: await new Promise(r => setTimeout(r, 700)))
- Read the value again
- Pass if both values are numeric and at least one is > 0

---

**2. Click counter increments**
- Use browser_get_text with selector "#click-count" to read the initial count
- Use browser_click with selector "#click-counter-button" - repeat 3 times
- Use browser_get_text with selector "#click-count" again
- Pass if (final - initial) === 3

---

**3. Signal toggle - armed state**
- Use browser_click on "#accent-toggle-button"
- Use browser_get_attribute on "#signal-card" for "data-active" - expect "true"
- Use browser_get_attribute on "#accent-toggle-button" for "aria-pressed" - expect "true"
- Use browser_get_text on "#signal-state" - expect "Signal armed"

---

**4. Signal toggle - restored state**
- Use browser_click on "#accent-toggle-button" (second click)
- Use browser_get_attribute on "#signal-card" for "data-active" - expect "false"
- Use browser_get_text on "#signal-state" - expect "Signal idle"

---

**5. Hover probe CSS transition**
- Use browser_hover on ".hover-probe"
- Use browser_get_computed_style on ".hover-probe" for "background-color"
- Pass if the value is approximately rgb(28, 90, 69)
- Use browser_hover with no selector to move the mouse away
- Use browser_get_computed_style again - pass if the value has reverted (not rgb(28,90,69))

---

**6. Cross-origin iframe present**
- Use browser_get_attribute on "iframe.cross-origin-frame" for "src"
- Pass if the src contains "gterzian.github.io"

---

**7. Navigation link + beforeunload**
- Use browser_evaluate to attach a one-shot listener before navigating:
    window.addEventListener('beforeunload', () => { window.__beforeunloadFired = true; }, { once: true })
- Use browser_click on "a.article-link"
- Use browser_evaluate: location.href - pass if contains "navigated.html"
- Use browser_evaluate: window.__beforeunloadFired - pass if true
- Use browser_history_back to restore

---

After all checks, write the summary table.
`.trim();
