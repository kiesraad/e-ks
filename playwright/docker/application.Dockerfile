FROM debian:trixie-slim

# debian:trixie-slim ships without a CA trust store, but eks needs one: it
# builds a TLS-capable reqwest client at startup (for the BRP client), and
# without ca-certificates that initialization panics before eks can bind a
# port. curl is here for the compose healthcheck below.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
CMD ["/app/eks"]
