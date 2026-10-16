const INTERVAL = 3000;

// While a BRP check runs, the page carries a hidden notice pointing at a
// status URL that answers `{"running": bool}`. Poll it and show the notice
// once the check is done. The user refreshes when it suits them, so nothing
// moves under their hands, and no HTML has to be injected under the CSP.
export default function setupBrpCheckStatus() {
  const notice = document.querySelector<HTMLElement>("[data-brp-status-url]");
  const statusUrl = notice?.dataset.brpStatusUrl;

  if (!notice || !statusUrl) {
    return;
  }

  const tick = async () => {
    if (!document.hidden) {
      try {
        const response = await fetch(statusUrl, { cache: "no-store" });
        if (response.ok) {
          const status: { running: boolean } = await response.json();
          if (!status.running) {
            notice.hidden = false;
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
