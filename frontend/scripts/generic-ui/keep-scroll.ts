// Forms marked `data-keep-scroll` post and redirect back to the same page.
// Remember the scroll position on submit and restore it after the reload.
const KEY = "keep-scroll";

export default function setupKeepScroll() {
  const path = globalThis.location.pathname;

  try {
    const stored = sessionStorage.getItem(KEY);
    if (stored) {
      sessionStorage.removeItem(KEY);
      const { path: storedPath, y } = JSON.parse(stored);
      if (storedPath === path) {
        window.scrollTo(0, y);
      }
    }
  } catch {
    // storage unavailable or corrupt: load the page as usual
  }

  for (const form of document.querySelectorAll<HTMLFormElement>(
    "form[data-keep-scroll]",
  )) {
    form.addEventListener("submit", () => {
      try {
        sessionStorage.setItem(
          KEY,
          JSON.stringify({ path, y: Math.round(window.scrollY) }),
        );
      } catch {
        // storage unavailable: fall back to the default jump to the top
      }
    });
  }
}
