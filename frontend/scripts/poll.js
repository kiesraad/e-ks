// setup abort controller to clean up fetches when navigating to another page
const controller = new AbortController();
let esbuildEventSource;

window.addEventListener("beforeunload", () => {
  controller.abort();
  esbuildEventSource?.close();
});

// long polling, when the server is up we get a 200 response every 30s
const longPoll = () => {
  fetch("/livereload/poll", {
    cache: "no-store",
    signal: controller.signal,
  }).then(
    (r) => {
      if (!controller.signal.aborted) {
        if (r.ok) {
          console.log("[livereload] heartbeat");
          longPoll();
        } else {
          console.log("[livereload] disconnected");
          shortPoll();
        }
      }
    },
    () => {
      if (!controller.signal.aborted) {
        console.log("[livereload] disconnected");
        shortPoll();
      }
    },
  );
};

// short polling, when the server is down we check it every 1000ms
const shortPoll = () => {
  fetch("/livereload/healthy", {
    cache: "no-store",
    signal: AbortSignal.timeout(1000),
  })
    .then((r) => {
      if (r?.ok) {
        globalThis.location.reload();
      } else {
        setTimeout(shortPoll, 1000);
      }
    })
    .catch(() => {
      setTimeout(shortPoll, 1000);
    });
};

// Safari keeps the tab in a "loading" state while any fetch/EventSource that
// was started before window 'load' is still pending. Defer both the long poll
// and the esbuild EventSource until after 'load' (plus a setTimeout to push
// them out of the load task entirely) so Safari stops the loading indicator.
window.addEventListener("load", () => {
  setTimeout(() => {
    longPoll();

    esbuildEventSource = new EventSource("/static/esbuild");
    esbuildEventSource.addEventListener("change", () => {
      location.reload();
    });

    console.log("[livereload] running");
  }, 0);
});
