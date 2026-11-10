/// Make table rows clickable
export default function setupClickRow() {
  document.querySelectorAll("tr.clickable").forEach((row) => {
    row.addEventListener("click", (event) => {
      // skip if the click originated a cell with class drag-handle
      if (
        event?.target instanceof HTMLElement &&
        (event.target.closest(".drag-handle") ||
          event.target.classList.contains("flash-success"))
      ) {
        return;
      }

      // a click on a link or button already does its own work; a second
      // synthetic click would submit or navigate twice
      if (
        event?.target instanceof HTMLElement &&
        event.target.closest("a,button")
      ) {
        return;
      }

      const links = row.querySelectorAll<HTMLAnchorElement | HTMLButtonElement>(
        "a,button",
      );
      links.item(links.length - 1)?.click();
    });
  });
}
