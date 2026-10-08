// Warn shortly before the session expires and offer to extend it.
//
// The server renders the remaining lifetime into the dialog's data attributes.
// Timers in background tabs are throttled, so everything is derived from an
// absolute deadline rather than from counted ticks. A user may have several
// tabs open, so before the warning opens, and again when the countdown reaches
// zero, the tab asks the server how much time is really left: another tab may
// have been active or may have extended the session already. That GET does not
// count as activity on the server. Extending POSTs to the same URL with the
// CSRF token, and tells the other tabs over a BroadcastChannel so their
// warnings close too. Once the session is gone, the tab navigates to the login
// page that explains the session expired.
export default function setupSessionExpiry() {
  type Expiry = {
    expires_in_secs: number;
    warning_lead_secs: number;
    extendable: boolean;
  };

  const dialog = document.querySelector<HTMLDialogElement>(
    "dialog.session-expiry",
  );

  if (!dialog) {
    return;
  }

  const statusUrl = dialog.dataset.statusUrl ?? "/session";
  const expiredUrl = dialog.dataset.expiredUrl ?? "/login";
  const timer = dialog.querySelector<HTMLElement>(".session-expiry-timer");
  const message = dialog.querySelector<HTMLElement>(".session-expiry-message");
  const messageFinal = dialog.querySelector<HTMLElement>(
    ".session-expiry-message-final",
  );
  const extendButton = dialog.querySelector<HTMLButtonElement>(
    ".session-expiry-extend",
  );
  // The dialog's logout form carries the session's token; the layout's meta
  // tag is not present on every session page.
  const csrfToken =
    dialog.querySelector<HTMLInputElement>('input[name="csrf_token"]')?.value ??
    "";
  const channel =
    "BroadcastChannel" in globalThis
      ? new BroadcastChannel("eks-session-expiry")
      : null;

  // How long to wait before asking again when the server could not be reached.
  const RETRY_DELAY_MS = 5_000;

  // Epoch milliseconds at which the session expires, and how long before that
  // the warning opens.
  let deadline = 0;
  let warningLeadMs = 0;
  let extendable = true;
  let checkTimeout: ReturnType<typeof setTimeout> | undefined;
  let countdownInterval: ReturnType<typeof setInterval> | undefined;
  let expired = false;

  const clearTimers = () => {
    if (checkTimeout !== undefined) {
      clearTimeout(checkTimeout);
      checkTimeout = undefined;
    }
    if (countdownInterval !== undefined) {
      clearInterval(countdownInterval);
      countdownInterval = undefined;
    }
  };

  const formatRemaining = (ms: number) => {
    const totalSeconds = Math.max(0, Math.ceil(ms / 1000));
    const minutes = Math.floor(totalSeconds / 60);
    const seconds = totalSeconds % 60;
    return `${minutes}:${seconds.toString().padStart(2, "0")}`;
  };

  const sessionGone = () => {
    if (expired) {
      return;
    }
    expired = true;
    clearTimers();
    globalThis.location.assign(expiredUrl);
  };

  // Ask the server for the remaining lifetime; `null` means the session is
  // gone (the middleware answered with the login redirect), `undefined` that
  // the server could not be reached.
  const fetchExpiry = async (
    method: "GET" | "POST",
  ): Promise<Expiry | null | undefined> => {
    try {
      const response = await fetch(statusUrl, {
        method,
        headers: method === "POST" ? { "X-CSRF-Token": csrfToken } : {},
        cache: "no-store",
        redirect: "follow",
      });
      // The middleware answers a dead session with a redirect to the login
      // page, so anything but a JSON answer means the session is gone.
      const isJson = response.headers
        .get("content-type")
        ?.startsWith("application/json");
      if (response.redirected || !response.ok || !isJson) {
        return null;
      }
      return (await response.json()) as Expiry;
    } catch (error) {
      console.error("Failed to check session expiry", error);
      return undefined;
    }
  };

  const closeWarning = () => {
    if (dialog.open) {
      dialog.close();
    }
  };

  const updateCountdown = () => {
    const remaining = deadline - Date.now();
    if (timer) {
      timer.textContent = formatRemaining(remaining);
    }
    if (remaining <= 0) {
      // The countdown ran out here, but another tab may have extended the
      // session in the meantime: let the server decide.
      clearTimers();
      void check();
    }
  };

  const openWarning = () => {
    message?.classList.toggle("hidden", !extendable);
    messageFinal?.classList.toggle("hidden", extendable);
    extendButton?.classList.toggle("hidden", !extendable);
    updateCountdown();
    if (!dialog.open) {
      dialog.showModal();
    }
    if (countdownInterval === undefined) {
      countdownInterval = setInterval(updateCountdown, 1000);
    }
  };

  // Take an expiry answer into account: reset the deadline, and either warn
  // right away or schedule the next check for when the warning is due.
  const applyExpiry = (expiry: Expiry) => {
    clearTimers();
    deadline = Date.now() + expiry.expires_in_secs * 1000;
    warningLeadMs = expiry.warning_lead_secs * 1000;
    extendable = expiry.extendable;

    if (expiry.expires_in_secs <= 0) {
      sessionGone();
      return;
    }

    const untilWarning = deadline - warningLeadMs - Date.now();
    if (untilWarning <= 0) {
      openWarning();
    } else {
      closeWarning();
      checkTimeout = setTimeout(() => void check(), untilWarning);
    }
  };

  // Consult the server before warning or logging out.
  const check = async () => {
    if (expired) {
      return;
    }
    const expiry = await fetchExpiry("GET");
    if (expiry === null) {
      sessionGone();
    } else if (expiry === undefined) {
      // Unreachable server: once the deadline has passed the session is gone
      // for sure; before that, try again shortly.
      if (Date.now() >= deadline) {
        sessionGone();
      } else {
        clearTimers();
        if (dialog.open) {
          countdownInterval = setInterval(updateCountdown, 1000);
        }
        checkTimeout = setTimeout(() => void check(), RETRY_DELAY_MS);
      }
    } else {
      applyExpiry(expiry);
    }
  };

  const extend = async () => {
    if (extendButton) {
      extendButton.disabled = true;
    }
    const expiry = await fetchExpiry("POST");
    if (extendButton) {
      extendButton.disabled = false;
    }
    if (expiry === null) {
      sessionGone();
    } else if (expiry !== undefined) {
      applyExpiry(expiry);
      channel?.postMessage(expiry);
    }
  };

  extendButton?.addEventListener("click", (event) => {
    event.preventDefault();
    void extend();
  });

  // Escape must not dismiss the warning silently: the user has to choose.
  dialog.addEventListener("cancel", (event) => {
    event.preventDefault();
  });

  // Another tab extended the session (or loaded a page, which also counts as
  // activity on the server): adopt its deadline.
  channel?.addEventListener("message", (event: MessageEvent<Expiry>) => {
    const expiry = event.data;
    if (
      expiry &&
      typeof expiry.expires_in_secs === "number" &&
      typeof expiry.warning_lead_secs === "number"
    ) {
      // Only ever move the deadline forward: a late message from a tab that
      // was idle longer must not cut this tab's time short.
      const incomingDeadline = Date.now() + expiry.expires_in_secs * 1000;
      if (incomingDeadline > deadline) {
        applyExpiry(expiry);
      }
    }
  });

  // Throttled timers may have slept through the warning time in a background
  // tab: catch up as soon as the tab is visible again.
  document.addEventListener("visibilitychange", () => {
    if (
      document.visibilityState === "visible" &&
      !expired &&
      Date.now() >= deadline - warningLeadMs
    ) {
      clearTimers();
      void check();
    }
  });

  const initial: Expiry = {
    expires_in_secs: Number(dialog.dataset.expiresIn ?? 0),
    warning_lead_secs: Number(dialog.dataset.warningLead ?? 0),
    extendable: dialog.dataset.extendable !== "false",
  };
  applyExpiry(initial);
  // This page load was activity on the server: let the other tabs know.
  channel?.postMessage(initial);
}
