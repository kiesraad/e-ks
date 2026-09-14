function isInViewport(element: Element): boolean {
  const { top, bottom } = element.getBoundingClientRect();
  return top >= 0 && bottom <= window.innerHeight;
}

function getRows(personId: string | null, last: number): Element[] | null {
  if (personId) {
    const row = document.querySelector(`[data-id="${personId}"]`);
    if (row) {
      return [row];
    }
  }

  if (last) {
    const rows = document.querySelectorAll(
      `tbody > tr:nth-last-child(-n + ${last})`,
    );
    return [...rows];
  }

  return null;
}

// Highlight a table row when the `highlight` query param is present, then
// remove the param from the URL to avoid persistent state on refresh/share.
export default function highlightRow() {
  const url = new URL(globalThis.location.href);
  const addTable = document.getElementById("add-candidate-table");
  const personId =
    url.searchParams.get("highlight") ?? addTable?.dataset.highlight ?? null;
  const last = Number.parseInt(
    url.searchParams.get("highlight_last") ?? "",
    10,
  );
  const sticky = document.querySelector(".sticky-nav");

  if (!personId && !last) {
    return;
  }

  // Match rows by data-id so deep links can target a specific row.
  const rows = getRows(personId, last);

  // Clean the URL once we've captured the ID.
  url.searchParams.delete("highlight");
  url.searchParams.delete("highlight_last");
  globalThis.history.replaceState({}, "", url.toString());

  if (!rows || rows.length < 1) {
    return;
  }

  // Apply the highlight and bring the row into view, unless a restored
  // scroll position (see keep-scroll) already shows it.
  rows.forEach((row) => {
    row.classList.add("highlighted");
  });
  const lastRow = rows.at(-1);
  if (lastRow && !isInViewport(lastRow)) {
    lastRow.scrollIntoView({ behavior: "auto", block: "center" });
  }

  // Do not animate the sticky nav to avoid glitches on page load
  if (sticky) {
    sticky.classList.add("no-animation");
  }

  // Re-apply highlight after a short delay to ensure animation is visible.
  setTimeout(() => {
    rows.forEach((row) => {
      row.classList.remove("highlighted");
    });

    // After initial page render the sticky-nav can animate again
    if (sticky) {
      sticky.classList.remove("no-animation");
    }
  }, 2000);
}
