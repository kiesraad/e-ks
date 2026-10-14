import {
  type BrowserContext,
  expect,
  type Page,
  type Route,
  test,
} from "@playwright/test";

import setupSessionExpiry from "../../../frontend/scripts/generic-ui/session-expiry";

const ORIGIN = "http://eks.test";
const IDLE_TIMEOUT = 15 * 60;
const WARNING_LEAD = 60;
const CSRF_TOKEN = "csrf-token-for-test";

type Expiry = {
  expires_in_secs: number;
  warning_lead_secs: number;
  extendable: boolean;
};

const expiry = (expiresIn: number, extendable = true): Expiry => ({
  expires_in_secs: expiresIn,
  warning_lead_secs: WARNING_LEAD,
  extendable,
});

const pageMarkup = `
  <!doctype html>
  <html><head><style>.hidden { display: none; }</style></head><body>
    <dialog class="modal session-expiry"
            data-expires-in="${IDLE_TIMEOUT}"
            data-warning-lead="${WARNING_LEAD}"
            data-extendable="true"
            data-status-url="/session"
            data-expired-url="/login?session_expired=true">
      <h3>Uw sessie verloopt bijna</h3>
      <p class="session-expiry-message">U bent een tijd niet actief geweest.</p>
      <p class="session-expiry-message-final hidden">Kan niet worden verlengd.</p>
      <p class="session-expiry-countdown">
        <span>Resterende tijd</span>
        <strong class="session-expiry-timer" role="timer">--:--</strong>
      </p>
      <button type="button" class="session-expiry-extend">Ingelogd blijven</button>
      <form method="post" action="/logout">
        <input type="hidden" name="csrf_token" value="${CSRF_TOKEN}">
        <button type="submit">Nu uitloggen</button>
      </form>
    </dialog>
  </body></html>
`;

type Server = {
  /// Answers for successive GET /session peeks; the last one repeats.
  peeks: (Expiry | "gone")[];
  /// Answer for POST /session.
  extend: Expiry | "gone";
  /// CSRF header values seen on POST /session.
  extendTokens: string[];
};

// Serve the page, the session endpoint and the login page from one origin so
// the script's relative fetches work, and install the fake clock before the
// script schedules anything. Routes and clock are shared by every tab of the
// context.
async function serve(context: BrowserContext, server: Server) {
  const json = (route: Route, body: Expiry) =>
    route.fulfill({
      contentType: "application/json",
      body: JSON.stringify(body),
    });
  // A dead session gets the login page (the middleware's redirect, followed
  // by fetch); Playwright cannot fulfill a redirect status itself, so answer
  // with the page it lands on.
  const gone = (route: Route) =>
    route.fulfill({ contentType: "text/html", body: "<h2>Inloggen</h2>" });

  await context.route(`${ORIGIN}/**`, async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;

    if (path === "/page") {
      return route.fulfill({ contentType: "text/html", body: pageMarkup });
    }
    if (path === "/login") {
      return route.fulfill({
        contentType: "text/html",
        body: "<h2>Inloggen</h2>",
      });
    }
    if (path === "/session" && request.method() === "GET") {
      const answer =
        server.peeks.length > 1 ? server.peeks.shift() : server.peeks[0];
      return answer === "gone" || answer === undefined
        ? gone(route)
        : json(route, answer);
    }
    if (path === "/session" && request.method() === "POST") {
      server.extendTokens.push(request.headers()["x-csrf-token"] ?? "");
      return server.extend === "gone"
        ? gone(route)
        : json(route, server.extend);
    }
    return route.fulfill({ status: 404 });
  });

  await context.clock.install({ time: new Date("2026-10-08T10:00:00") });
}

async function openTab(page: Page) {
  await page.goto(`${ORIGIN}/page`);
  await page.evaluate(setupSessionExpiry);
}

async function openPage(page: Page, server: Server) {
  await serve(page.context(), server);
  await openTab(page);
}

const minutes = (count: number) => count * 60 * 1000;

test.describe("session-expiry", () => {
  test("opens the warning one minute before expiry and counts down", async ({
    page,
  }) => {
    await openPage(page, {
      peeks: [expiry(WARNING_LEAD)],
      extend: expiry(IDLE_TIMEOUT),
      extendTokens: [],
    });
    const dialog = page.locator("dialog.session-expiry");
    const timer = dialog.locator(".session-expiry-timer");

    await page.clock.fastForward(minutes(13));
    await expect(dialog).toBeHidden();

    await page.clock.fastForward(minutes(1));
    await expect(dialog).toBeVisible();
    await expect(timer).toHaveText("1:00");
    await expect(dialog.locator(".session-expiry-extend")).toBeVisible();

    await page.clock.runFor(1000);
    await expect(timer).toHaveText("0:59");
  });

  test("stays closed while the server reports a later expiry", async ({
    page,
  }) => {
    // The server saw activity this tab never heard about (no broadcast reached
    // it): ten minutes left when this tab asks, one minute left the next time.
    await openPage(page, {
      peeks: [expiry(10 * 60), expiry(WARNING_LEAD)],
      extend: expiry(IDLE_TIMEOUT),
      extendTokens: [],
    });
    const dialog = page.locator("dialog.session-expiry");

    await page.clock.fastForward(minutes(14));
    // give the peek time to answer before asserting the dialog stayed closed
    await expect(dialog).toBeHidden({ timeout: 100 });

    await page.clock.fastForward(minutes(9));
    await expect(dialog).toBeVisible();
  });

  test("stay logged in extends the session with the CSRF token", async ({
    page,
  }) => {
    const server: Server = {
      peeks: [expiry(WARNING_LEAD)],
      extend: expiry(IDLE_TIMEOUT),
      extendTokens: [],
    };
    await openPage(page, server);
    const dialog = page.locator("dialog.session-expiry");

    await page.clock.fastForward(minutes(14));
    await expect(dialog).toBeVisible();

    await dialog.locator(".session-expiry-extend").click();
    await expect(dialog).toBeHidden();
    expect(server.extendTokens).toEqual([CSRF_TOKEN]);

    // The warning comes back a full idle timeout later, not earlier.
    await page.clock.fastForward(minutes(13));
    await expect(dialog).toBeHidden({ timeout: 100 });
    await page.clock.fastForward(minutes(1));
    await expect(dialog).toBeVisible();
  });

  test("closes when another tab stays logged in", async ({ context }) => {
    await serve(context, {
      peeks: [expiry(WARNING_LEAD)],
      extend: expiry(IDLE_TIMEOUT),
      extendTokens: [],
    });
    const first = await context.newPage();
    const second = await context.newPage();
    await openTab(first);
    await openTab(second);
    const firstDialog = first.locator("dialog.session-expiry");
    const secondDialog = second.locator("dialog.session-expiry");

    await context.clock.fastForward(minutes(14));
    await expect(firstDialog).toBeVisible();
    await expect(secondDialog).toBeVisible();

    // The second tab's broadcast closes the first tab's warning too.
    await secondDialog.locator(".session-expiry-extend").click();
    await expect(secondDialog).toBeHidden();
    await expect(firstDialog).toBeHidden();
  });

  test("closes when the countdown runs out but the server reports a later expiry", async ({
    page,
  }) => {
    // The session was extended elsewhere without a broadcast reaching this
    // tab: the final peek is the only way it finds out.
    await openPage(page, {
      peeks: [expiry(WARNING_LEAD), expiry(IDLE_TIMEOUT)],
      extend: expiry(IDLE_TIMEOUT),
      extendTokens: [],
    });
    const dialog = page.locator("dialog.session-expiry");

    await page.clock.fastForward(minutes(14));
    await expect(dialog).toBeVisible();

    await page.clock.runFor(minutes(1));
    await expect(dialog).toBeHidden();
    await expect(page).toHaveURL(`${ORIGIN}/page`);
  });

  test("goes to the login page once the session is gone", async ({ page }) => {
    await openPage(page, {
      peeks: [expiry(WARNING_LEAD), "gone"],
      extend: expiry(IDLE_TIMEOUT),
      extendTokens: [],
    });
    const dialog = page.locator("dialog.session-expiry");

    await page.clock.fastForward(minutes(14));
    await expect(dialog).toBeVisible();

    await page.clock.runFor(minutes(1));
    await expect(page).toHaveURL(`${ORIGIN}/login?session_expired=true`);
  });

  test("tells the user to log in again when the session cannot be extended", async ({
    page,
  }) => {
    await openPage(page, {
      peeks: [expiry(WARNING_LEAD, false)],
      extend: expiry(IDLE_TIMEOUT),
      extendTokens: [],
    });
    const dialog = page.locator("dialog.session-expiry");

    await page.clock.fastForward(minutes(14));
    await expect(dialog).toBeVisible();
    await expect(dialog.locator(".session-expiry-extend")).toBeHidden();
    await expect(dialog.locator(".session-expiry-message-final")).toBeVisible();
    await expect(dialog.locator(".session-expiry-message")).toBeHidden();
  });
});
