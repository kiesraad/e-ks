FROM python:3.12-slim@sha256:5e2bdb1b0d542d5039cffb8181c65930093949194a65c365dfb1b0fe13a4fa5b

ARG UID=1000
ARG GID=1000

ENV PYTHONDONTWRITEBYTECODE=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1

WORKDIR /work/

RUN addgroup --system --gid "${GID}" djlint \
    && adduser --system --uid "${UID}" --ingroup djlint --home /home/djlint djlint \
    && chown -R djlint:djlint /work \
    && pip install --no-cache-dir --only-binary :all: "djlint==1.39.2"

USER djlint

CMD ["djlint", "--ignore=H025", "--reformat", "--lint", "--extension=html", "--indent=2", "templates"]
