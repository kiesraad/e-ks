// Landing on a create overlay through history navigation invites accidentally
// creating a second entity: leave the overlay via its close link instead,
// replacing the overlay's history entry with the underlying page.
function skipCreateOverlay(): boolean {
  const closeLink = document.querySelector<HTMLAnchorElement>(
    ".overlay[data-create] .close-overlay",
  );

  if (closeLink) {
    globalThis.location.replace(closeLink.href);
    return true;
  }

  return false;
}

// Pages render per-session data, so a page restored from the browser's
// back/forward cache may be stale (Cache-Control: no-store does not keep
// pages out of that cache). Refetch instead of showing the restored copy.
export default function setupBfcacheReload() {
  const [navigation] = performance.getEntriesByType("navigation");

  if (
    navigation instanceof PerformanceNavigationTiming &&
    navigation.type === "back_forward" &&
    skipCreateOverlay()
  ) {
    return;
  }

  window.addEventListener("pageshow", (event) => {
    if (event.persisted && !skipCreateOverlay()) {
      globalThis.location.reload();
    }
  });
}
