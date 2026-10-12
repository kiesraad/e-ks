// Needs the Navigation API; without it, every traversal counts as backwards.
function isForwardTraversal(): boolean {
  const activation = globalThis.navigation?.activation;

  return (
    activation?.navigationType === "traverse" &&
    activation.from !== null &&
    activation.from.index < activation.entry.index
  );
}

// Landing on a create overlay through history navigation invites accidentally
// creating a second entity: step past it when travelling forward, otherwise
// leave it via its close link, replacing its history entry with the
// underlying page.
function skipCreateOverlay(): boolean {
  const closeLink = document.querySelector<HTMLAnchorElement>(
    ".overlay[data-create] .close-overlay",
  );

  if (!closeLink) {
    return false;
  }

  if (isForwardTraversal() && globalThis.navigation.canGoForward) {
    globalThis.history.forward();
  } else {
    globalThis.location.replace(closeLink.href);
  }

  return true;
}

function isBackForwardLoad(): boolean {
  const [entry] = performance.getEntriesByType("navigation");

  return (
    entry instanceof PerformanceNavigationTiming &&
    entry.type === "back_forward"
  );
}

// Pages render per-session data, so a page restored from the browser's
// back/forward cache may be stale (Cache-Control: no-store does not keep
// pages out of that cache). Refetch instead of showing the restored copy.
export default function setupBfCacheReload() {
  if (isBackForwardLoad() && skipCreateOverlay()) {
    return;
  }

  window.addEventListener("pageshow", (event) => {
    if (event.persisted && !skipCreateOverlay()) {
      globalThis.location.reload();
    }
  });
}
