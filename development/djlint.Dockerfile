FROM python:3.15-slim@sha256:65a65fc7b30b06cb6df4930fd4650697cb23f4c2698ab81fc87e4b031bc36928

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
