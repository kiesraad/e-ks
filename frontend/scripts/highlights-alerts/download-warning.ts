// The download warning floats over the data-entry pages, so dismissing it must
// not navigate: a reload would discard whatever the user has typed but not
// saved. Post in the background instead and hide the banner in place. Without
// JavaScript the form posts and redirects as before.
export default function setupDownloadWarning() {
  const banner = document.querySelector<HTMLElement>(
    ".download-warning-banner",
  );
  const form = banner?.querySelector("form");

  if (!banner || !form) {
    return;
  }

  // CSRF token from the layout's meta tag; the CSRF guard requires it.
  const csrfToken =
    document
      .querySelector('meta[name="csrf-token"]')
      ?.getAttribute("content") ?? "";

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    banner.classList.add("hidden");

    fetch(form.action, {
      method: "POST",
      headers: { "X-CSRF-Token": csrfToken },
    })
      .then((response) => {
        // Only 204 means the dismissal was recorded; an expired session answers
        // with a redirect to the login page.
        if (response.status !== 204) {
          console.error("Failed to hide download warning", response.status);
          form.submit();
        }
      })
      .catch((error) => {
        console.error("Failed to hide download warning", error);
        form.submit();
      });
  });
}
