FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2

# debian:trixie-slim ships without a CA trust store, but eks needs one: it
# builds a TLS-capable reqwest client at startup (for the BRP client), and
# without ca-certificates that initialization panics before eks can bind a
# port. curl is here for the compose healthcheck below.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
USER nobody
CMD ["/app/eks"]
