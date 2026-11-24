FROM gcr.io/distroless/static-debian13:nonroot@sha256:8ce6dd87e9823c5b48dcfb2c8604714ccb351ce59607327bcc4957448da279b5
ARG version=dev

COPY --chown=root:root --chmod=755 ./eks /eks

EXPOSE 3000
ENV VERSION=${version}
ENTRYPOINT ["/eks"]
