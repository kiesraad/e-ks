const INTERVAL = 3000;

// A page with `data-poll="true"` changes on the server while a background
// check runs. Refetch it and reload as soon as the markup differs; the
// reload renders the next state, which decides whether polling goes on.
// The CSP forbids Trusted Types policies, so the fetched HTML is only
// compared, never parsed.
export default function setupPollFragment() {
  if (!document.querySelector('[data-poll="true"]')) {
    return;
  }

  let last: string | undefined;

  const tick = async () => {
    if (document.hidden) {
      setTimeout(tick, INTERVAL);
      return;
    }

    const response = await fetch(location.href, { cache: "no-store" });
    const html = await response.text();
    if (last !== undefined && html !== last) {
      location.reload();
      return;
    }

    last = html;
    setTimeout(tick, INTERVAL);
  };

  // Baseline right away, so a change shortly after render is not missed.
  tick();
}
