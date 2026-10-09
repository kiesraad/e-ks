import { expect, type Page, type Route, test } from "@playwright/test";

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
            data-expired-url="/login?expired=true">
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
// script schedules anything.
async function openPage(page: Page, server: Server) {
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

  await page.route(`${ORIGIN}/**`, async (route) => {
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

  await page.clock.install({ time: new Date("2026-10-08T10:00:00") });
  await page.goto(`${ORIGIN}/page`);
  await page.evaluate(setupSessionExpiry);
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

  test("stays closed while another tab keeps the session alive", async ({
    page,
  }) => {
    // The server has seen activity from another tab: ten minutes left when
    // this tab asks, one minute left the next time.
    await openPage(page, {
      peeks: [expiry(10 * 60), expiry(WARNING_LEAD)],
      extend: expiry(IDLE_TIMEOUT),
      extendTokens: [],
    });
    const dialog = page.locator("dialog.session-expiry");

    await page.clock.fastForward(minutes(14));
    // give the peek time to answer before asserting the dialog stayed closed
    await page.waitForTimeout(100);
    await expect(dialog).toBeHidden();

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
    await page.waitForTimeout(100);
    await expect(dialog).toBeHidden();
    await page.clock.fastForward(minutes(1));
    await expect(dialog).toBeVisible();
  });

  test("closes when the countdown runs out but another tab extended", async ({
    page,
  }) => {
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
    await expect(page).toHaveURL(`${ORIGIN}/login?expired=true`);
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
