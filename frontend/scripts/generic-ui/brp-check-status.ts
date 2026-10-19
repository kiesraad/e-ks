const INTERVAL = 3000;

// While a BRP check runs, the page carries a hidden notice pointing at a
// status URL that answers `{"running": bool}`. Poll it and, once the check is
// done, show the notice in place of the strip that still reports the check as
// running. The user refreshes when it suits them, so nothing else moves under
// their hands, and no HTML has to be injected under the CSP.
export default function setupBrpCheckStatus() {
  const notice = document.querySelector<HTMLElement>("[data-brp-status-url]");
  const statusUrl = notice?.dataset.brpStatusUrl;

  if (!notice || !statusUrl) {
    return;
  }

  const finish = () => {
    for (const strip of document.querySelectorAll("[data-brp-strip]")) {
      strip.classList.add("hidden");
    }
    notice.hidden = false;
  };

  const tick = async () => {
    if (!document.hidden) {
      try {
        const response = await fetch(statusUrl, { cache: "no-store" });
        if (response.ok) {
          const status: { running: boolean } = await response.json();
          if (!status.running) {
            finish();
            return;
          }
        }
      } catch {
        // transient failure: try again next tick
      }
    }
    setTimeout(tick, INTERVAL);
  };

  setTimeout(tick, INTERVAL);
}
