// Forms marked `data-keep-scroll` post and redirect back to the same page.
// Remember the scroll position before leaving the page and restore it after the reload.
// Also used for links that open overlays that should keep their scroll position.
const KEY = "keep-scroll";

export default function setupKeepScroll() {
  const path = globalThis.location.pathname;
  const overlay = document.querySelector(".overlay");

  try {
    const stored = sessionStorage.getItem(KEY);
    if (stored) {
      sessionStorage.removeItem(KEY);
      const position = JSON.parse(stored);
      if (overlay) {
        // if on overlay: keep stored position and set restore to true
        sessionStorage.setItem(
          KEY,
          JSON.stringify({ ...position, restore: true }),
        );
      } else if (position.restore && position.path === path) {
        window.scrollTo(0, position.y);
      }
    }
  } catch {
    // storage unavailable or corrupt: load the page as usual
  }

  if (overlay) return;

  let restore = false;
  for (const form of document.querySelectorAll("form[data-keep-scroll]")) {
    form.addEventListener("submit", () => {
      restore = true; // keep-scroll forms should always restore
    });
  }

  // The next page knows whether it renders an overlay, even for links without redirect_to.
  window.addEventListener("pagehide", () => {
    try {
      sessionStorage.setItem(
        KEY,
        JSON.stringify({ path, y: Math.round(window.scrollY), restore }),
      );
    } catch {
      // storage unavailable: fall back to the default jump to the top
    }
  });
}
