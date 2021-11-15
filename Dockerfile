# Guillotine was written from macOS, but it runs on epoll, a linux-specific API.
# So, uh, Docker.

FROM ubuntu:20.04

RUN apt update && apt install -y --no-install-recommends \
       curl \
       ca-certificates \
       build-essential \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

ENV PATH=/root/.cargo/bin:$PATH
