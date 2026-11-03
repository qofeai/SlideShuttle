FROM ubuntu:26.04
RUN apt-get update -qq \
    && apt-get install -y -qq --no-install-recommends libreoffice-impress \
    && rm -rf /var/lib/apt/lists/*
COPY fonts.conf /etc/fonts/local.conf
WORKDIR /work
CMD ["tail", "-f", "/dev/null"]
