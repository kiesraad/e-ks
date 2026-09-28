// The header is fixed, so it only casts a shadow once content scrolls under it.
export default function setupHeaderShadow() {
  const header = document.querySelector("header");

  if (!header) {
    return;
  }

  let queued = false;

  const update = () => {
    queued = false;
    header.classList.toggle("is-scrolled", window.scrollY > 0);
  };

  update();

  window.addEventListener(
    "scroll",
    () => {
      if (!queued) {
        queued = true;
        requestAnimationFrame(update);
      }
    },
    { passive: true },
  );
}
