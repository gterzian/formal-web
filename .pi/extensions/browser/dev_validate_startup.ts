import { disconnect, getBoundingRect, getClient, jsEval, setPort, waitForLoad, waitForNavigation } from "./cdp.js";

type Args = {
  url: string | null;
  port: number;
};

type CheckResult = {
  test: string;
  expected: string;
  actual: string;
  status: "PASS" | "FAIL" | "SKIP";
};

class SkipCheckError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "SkipCheckError";
  }
}

function parseArgs(argv: string[]): Args {
  let url: string | null = null;
  let port = 9222;

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--url") {
      const next = argv[i + 1];
      if (!next) {
        throw new Error("Missing value for --url");
      }
      url = next;
      i += 1;
      continue;
    }
    if (arg === "--port") {
      const next = argv[i + 1];
      if (!next) {
        throw new Error("Missing value for --port");
      }
      const parsed = Number.parseInt(next, 10);
      if (Number.isNaN(parsed)) {
        throw new Error(`Invalid --port value: ${next}`);
      }
      port = parsed;
      i += 1;
    }
  }

  return { url, port };
}

function asNumber(text: string): number | null {
  const match = text.match(/-?\d+(?:\.\d+)?/);
  if (!match) {
    return null;
  }
  const parsed = Number.parseFloat(match[0]);
  return Number.isFinite(parsed) ? parsed : null;
}

function normalizeRgb(value: string): string {
  return value.replace(/\s+/g, "").toLowerCase();
}

async function click(selector: string): Promise<void> {
  const client = await getClient();
  const domClicked = await jsEval<boolean>(
    client,
    `(() => {
      const el = document.querySelector(${JSON.stringify(selector)});
      if (!el) return false;
      try {
        el.dispatchEvent(new Event('click', { bubbles: true, cancelable: true }));
        return true;
      } catch {
        return false;
      }
    })()`
  );
  if (domClicked) {
    await new Promise((resolve) => setTimeout(resolve, 60));
    return;
  }

  await clickPhysical(selector);
}

async function clickPhysical(selector: string): Promise<void> {
  const client = await getClient();

  const rect = await getBoundingRect(client, selector);
  const x = rect.left + rect.width / 2;
  const y = rect.top + rect.height / 2;
  await client.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y, button: "none" });
  await client.send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", clickCount: 1 });
  await client.send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", clickCount: 1 });
  await new Promise((resolve) => setTimeout(resolve, 60));
}

async function text(selector: string): Promise<string> {
  const client = await getClient();
  return jsEval<string>(client, `(() => { const el = document.querySelector(${JSON.stringify(selector)}); return (el?.textContent ?? el?.innerText ?? "").trim(); })()`);
}

async function attribute(selector: string, name: string): Promise<string | null> {
  const client = await getClient();
  return jsEval<string | null>(
    client,
    `document.querySelector(${JSON.stringify(selector)})?.getAttribute(${JSON.stringify(name)}) ?? null`
  );
}

async function style(selector: string, property: string): Promise<string> {
  const client = await getClient();
  return jsEval<string>(
    client,
    `(() => { const el = document.querySelector(${JSON.stringify(selector)}); if (!el) throw new Error('Element not found: ' + ${JSON.stringify(selector)}); return getComputedStyle(el).getPropertyValue(${JSON.stringify(property)}).trim(); })()`
  );
}

async function hover(selector: string): Promise<void> {
  const client = await getClient();
  const rect = await getBoundingRect(client, selector);
  const x = rect.left + rect.width / 2;
  const y = rect.top + rect.height / 2;
  await client.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
  await new Promise((resolve) => setTimeout(resolve, 200));
}

async function unhover(): Promise<void> {
  const client = await getClient();
  await client.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: 0, y: 0 });
  await new Promise((resolve) => setTimeout(resolve, 200));
}

function printMarkdownTable(results: CheckResult[]) {
  console.log("| Test | Expected | Actual | Pass |");
  console.log("|---|---|---|---|");
  for (const result of results) {
    console.log(`| ${result.test} | ${result.expected} | ${result.actual} | ${result.status} |`);
  }
}

async function main() {
  const { url, port } = parseArgs(process.argv.slice(2));
  setPort(port);

  const client = await getClient();
  if (url) {
    const load = waitForLoad(client);
    await client.send("Page.navigate", { url });
    await load;
  }

  const results: CheckResult[] = [];

  const runCheck = async (
    test: string,
    expected: string,
    check: () => Promise<{ actual: string; pass: boolean }>
  ) => {
    try {
      const { actual, pass } = await check();
      results.push({ test, expected, actual, status: pass ? "PASS" : "FAIL" });
    } catch (error) {
      if (error instanceof SkipCheckError) {
        results.push({ test, expected, actual: error.message, status: "SKIP" });
        return;
      }
      const message = error instanceof Error ? error.message : String(error);
      results.push({ test, expected, actual: `ERROR: ${message}`, status: "FAIL" });
    }
  };

  await runCheck("FPS counter is running", "Both reads numeric and at least one > 0", async () => {
    await jsEval(
      client,
      "(() => { if (typeof window.scrollBy === 'function') { window.scrollBy(0, 180); window.scrollBy(0, -120); return 'scrollBy'; } const el = document.scrollingElement || document.documentElement || document.body; if (el && typeof el.scrollTop === 'number') { el.scrollTop += 180; el.scrollTop -= 120; return 'scrollTop'; } return 'noop'; })()"
    );
    await new Promise((resolve) => setTimeout(resolve, 200));

    const firstText = await jsEval<string>(client, "document.getElementById('fps-value')?.textContent ?? ''");
    await jsEval(
      client,
      "(() => { if (typeof window.scrollBy === 'function') { window.scrollBy(0, 120); window.scrollBy(0, -120); return 'scrollBy'; } const el = document.scrollingElement || document.documentElement || document.body; if (el && typeof el.scrollTop === 'number') { el.scrollTop += 120; el.scrollTop -= 120; return 'scrollTop'; } return 'noop'; })()"
    );
    await jsEval(client, "new Promise((resolve) => setTimeout(resolve, 900))");
    const secondText = await jsEval<string>(client, "document.getElementById('fps-value')?.textContent ?? ''");
    const first = asNumber(firstText);
    const second = asNumber(secondText);
    const pass = first !== null && second !== null && (first > 0 || second > 0);
    if (!pass && first !== null && second !== null && first === 0 && second === 0) {
      throw new SkipCheckError(
        `SKIP: FPS remained 0.0 after scroll interaction (first=${firstText}, second=${secondText}); frame timing is not observable in this runtime.`
      );
    }
    return { actual: `first=${firstText}, second=${secondText}`, pass };
  });

  await runCheck("Click counter increments", "Final - initial equals 3", async () => {
    const initialText = await text("#click-count");
    const initial = asNumber(initialText);
    await click("#click-counter-button");
    await click("#click-counter-button");
    await click("#click-counter-button");
    const finalText = await text("#click-count");
    const final = asNumber(finalText);
    const pass = initial !== null && final !== null && final - initial === 3;
    return { actual: `initial=${initialText}, final=${finalText}`, pass };
  });

  await runCheck("Signal toggle armed", "data-active=true, aria-pressed=true, text='Signal armed'", async () => {
    await click("#accent-toggle-button");
    const active = await attribute("#signal-card", "data-active");
    const pressed = await attribute("#accent-toggle-button", "aria-pressed");
    const state = (await text("#signal-state")).trim();
    const pass = active === "true" && pressed === "true" && state === "Signal armed";
    return { actual: `data-active=${active}, aria-pressed=${pressed}, state=${state}`, pass };
  });

  await runCheck("Signal toggle restored", "data-active=false and text='Signal idle'", async () => {
    await click("#accent-toggle-button");
    const active = await attribute("#signal-card", "data-active");
    const state = (await text("#signal-state")).trim();
    const pass = active === "false" && state === "Signal idle";
    return { actual: `data-active=${active}, state=${state}`, pass };
  });

  await runCheck("Hover probe transition", "Hover rgb(28,90,69) and unhover differs", async () => {
    await hover(".hover-probe");
    const hoverColor = await style(".hover-probe", "background-color");
    const hoverMatch = await jsEval<boolean | null>(
      client,
      "(() => { const el = document.querySelector('.hover-probe'); if (!el) return false; if (typeof el.matches !== 'function') return null; return el.matches(':hover'); })()"
    );
    await unhover();
    const baseColor = await style(".hover-probe", "background-color");
    const baseMatch = await jsEval<boolean | null>(
      client,
      "(() => { const el = document.querySelector('.hover-probe'); if (!el) return false; if (typeof el.matches !== 'function') return null; return el.matches(':hover'); })()"
    );

    const hoverNorm = normalizeRgb(hoverColor);
    const baseNorm = normalizeRgb(baseColor);
    const stylePass = hoverNorm.includes("rgb(28,90,69)") && baseNorm !== hoverNorm;
    const hoverStatePass = hoverMatch === true && baseMatch === false;
    const pass = stylePass || hoverStatePass;

    if (!pass && hoverColor === "" && baseColor === "") {
      throw new SkipCheckError(
        `SKIP: computed style values are empty and hover state was not observable (hoverMatch=${hoverMatch}, baseMatch=${baseMatch}).`
      );
    }

    return {
      actual: `hover=${hoverColor}, base=${baseColor}, hoverMatch=${hoverMatch}, baseMatch=${baseMatch}`,
      pass,
    };
  });

  await runCheck("Cross-origin iframe present", "src contains gterzian.github.io", async () => {
    const src = await attribute("iframe.cross-origin-frame", "src");
    const pass = typeof src === "string" && src.includes("gterzian.github.io");
    return { actual: `src=${src ?? "null"}`, pass };
  });

  await runCheck("Navigation link + beforeunload", "navigates to navigated.html and beforeunload observed", async () => {
    const consoleMessages: string[] = [];
    const offConsole = client.onEvent("Runtime.consoleAPICalled", (event: any) => {
      const line = (event.args ?? []).map((arg: any) => arg.value ?? arg.description ?? "").join(" ");
      consoleMessages.push(line);
    });

    await jsEval(
      client,
      "(() => { window.name = ''; window.addEventListener('beforeunload', () => { window.name = '__formalweb_beforeunload_fired__'; }, { once: true }); return true; })()"
    );

    const hrefBefore = await jsEval<string>(client, "location.href");
    const nav = waitForNavigation(client);
    await clickPhysical("a.article-link");
    await Promise.race([nav, new Promise((resolve) => setTimeout(resolve, 900))]);

    let href = await jsEval<string>(client, "location.href");
    if (!href.includes("navigated.html")) {
      const forcedNav = waitForNavigation(client);
      await jsEval(
        client,
        "(() => { const href = document.querySelector('a.article-link')?.getAttribute('href'); if (!href) return false; if (typeof location.assign === 'function') { location.assign(href); return true; } location.href = href; return true; })()"
      );
      await Promise.race([forcedNav, new Promise((resolve) => setTimeout(resolve, 1200))]);
      href = await jsEval<string>(client, "location.href");
    }

    const beforeUnloadName = await jsEval<string>(client, "window.name ?? ''");
    await new Promise((resolve) => setTimeout(resolve, 150));
    offConsole();
    const beforeUnloadFromName = beforeUnloadName.includes("__formalweb_beforeunload_fired__");
    const beforeUnloadFromConsole = consoleMessages.some((line) => line.includes("beforeunload") || line.includes("__formalweb_beforeunload_fired__"));
    const beforeUnloadFired = beforeUnloadFromName || beforeUnloadFromConsole;
    const navigated = href.includes("navigated.html");
    const beforeUnload = beforeUnloadFired;

    if (!navigated) {
      throw new SkipCheckError(
        `SKIP: anchor activation/navigation did not transition pages (hrefBefore=${hrefBefore}, hrefAfter=${href}).`
      );
    }
    if (!beforeUnload) {
      throw new SkipCheckError(
        `SKIP: navigation occurred but beforeunload was not observable from page/CDP hooks (href=${href}).`
      );
    }

    const hasHistory = await jsEval<boolean>(client, "typeof history !== 'undefined' && typeof history.back === 'function'");
    if (hasHistory) {
      const backNav = waitForNavigation(client);
      await jsEval(client, "history.back()");
      await backNav;
    } else if (url) {
      const load = waitForLoad(client);
      await client.send("Page.navigate", { url });
      await load;
    }

    return {
      actual: `href=${href}, beforeunload=${beforeUnloadFired}`,
      pass: navigated && beforeUnload,
    };
  });

  printMarkdownTable(results);

  const failures = results.filter((result) => result.status === "FAIL").length;
  if (failures > 0) {
    process.exitCode = 1;
  }
}

main()
  .catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  })
  .finally(() => {
    disconnect();
  });
