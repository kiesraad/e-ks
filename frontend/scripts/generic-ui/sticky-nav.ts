// Toggle sticky navigation styles when the header is stuck.
export default function setupStickyNav() {
  const sticky = document.querySelector(".sticky-nav");

  if (!sticky) {
    return;
  }

  // The bar sticks below the fixed header, so the viewport edge the observer
  // measures against has to move down by that same height.
  const header = document.querySelector("header nav");
  const offset = header ? Math.round(header.getBoundingClientRect().height) : 0;

  const observer = new IntersectionObserver(
    ([e]) => {
      e.target.classList.toggle("is-stuck", e.intersectionRatio < 1);
    },
    { threshold: [1], rootMargin: `-${offset}px 0px 0px 0px` },
  );

  observer.observe(sticky);
}
