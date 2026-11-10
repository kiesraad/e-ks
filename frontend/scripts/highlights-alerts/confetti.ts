export default function setupConfetti() {
  document
    .querySelectorAll<HTMLElement>("[data-confetti]")
    .forEach((element) => {
      element.addEventListener("click", () => {
        if (
          !globalThis.matchMedia("(prefers-reduced-motion: reduce)").matches
        ) {
          const { left, top, width, height } = element.getBoundingClientRect();
          confetti(undefined, left + width / 2, top + height / 2);
          element.classList.add("disabled");
          globalThis.setTimeout(
            () => element.classList.remove("disabled"),
            500,
          );
        }
      });
    });
}

export function confetti(
  count = 200,
  x = globalThis.innerWidth / 2,
  y = globalThis.innerHeight,
) {
  const canvas = document.body.appendChild(document.createElement("canvas"));
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    canvas.remove();
    return;
  }

  const dpr = globalThis.devicePixelRatio;
  Object.assign(canvas, {
    width: globalThis.innerWidth * dpr,
    height: globalThis.innerHeight * dpr,
  });
  Object.assign(canvas.style, {
    position: "fixed",
    inset: "0",
    width: "100%",
    height: "100%",
    pointerEvents: "none",
  });
  ctx.scale(dpr, dpr);

  const pieces = Array.from({ length: count }, () => {
    const angle = -Math.PI / 2 + (Math.random() - 0.5) * 1.2;
    const speed = 14 + Math.random() * 18;
    return {
      x: x,
      y: y,
      vx: Math.cos(angle) * speed,
      vy: Math.sin(angle) * speed,
      w: 6 + Math.random() * 6,
      h: 4 + Math.random() * 4,
      hue: Math.random() * 360,
      tilt: Math.random() * Math.PI * 2,
      spin: 0.1 + Math.random() * 0.2,
    };
  });

  let lastTime = globalThis.performance.now();

  const tick = (now: number) => {
    const dt = Math.min(now - lastTime, 100) / (1000 / 60);
    lastTime = now;

    ctx.clearRect(0, 0, globalThis.innerWidth, globalThis.innerHeight);
    const drag = 0.96 ** dt;
    for (const p of pieces) {
      p.vy += 0.25 * dt;
      p.vx *= drag;
      p.vy *= drag;
      p.x += (p.vx + Math.sin(p.tilt)) * dt;
      p.y += p.vy * dt;
      p.tilt += p.spin * dt;
      ctx.save();
      ctx.translate(p.x, p.y);
      ctx.rotate(p.tilt);
      ctx.scale(1, Math.cos(p.tilt * 2));
      ctx.fillStyle = `hsl(${p.hue} 90% 60%)`;
      ctx.fillRect(-p.w / 2, -p.h / 2, p.w, p.h);
      ctx.restore();
    }
    pieces.some((p) => p.y < globalThis.innerHeight + 20)
      ? globalThis.requestAnimationFrame(tick)
      : canvas.remove();
  };
  globalThis.requestAnimationFrame(tick);
}
