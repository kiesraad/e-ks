// Toggle sticky navigation styles when the header is stuck.
export default function setupStickyNav() {
  const sticky = document.querySelector(".sticky-nav");

  if (!sticky) {
    return;
  }

  const observer = new IntersectionObserver(
    ([e]) => {
      e.target.classList.toggle("is-stuck", e.intersectionRatio < 1);
    },
    { threshold: [1] },
  );

  observer.observe(sticky);
}
