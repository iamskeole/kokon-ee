# Kokon service image (Ubuntu 22.04, glibc 2.35): the box's own layer, without a window, used
# from a browser. All inference lives on stock engines behind the box's gateway, never in this
# image, so it stays small; libvulkan1 lets the in-process lab models (MobileSAM, BiRefNet) run
# when the box has a GPU. Docs: docs/fresh-box.md.
# To build it yourself, from the repository root:
#
#   docker build -f docker/Dockerfile -t kokon-service .

# The ONNX Runtime (AI watermark, stickers) is built with GCC 13 and links against its libstdc++
# (GLIBCXX_3.4.32), which Ubuntu 22.04 (glibc 2.35, the runtime base) does not have. This stage
# fetches the GCC 13 static libstdc++ from Ubuntu 24.04's *main* repos (24.04 ships GCC 13;
# the ubuntu-toolchain-r PPA that used to serve it for 22.04 lives on Launchpad and flakes with
# 503s) and hands it to the build stage below, which links it in statically.
FROM ubuntu:24.04 AS toolchain
RUN apt-get update \
 && apt-get install -y --no-install-recommends g++-13 \
 && rm -rf /var/lib/apt/lists/*

FROM ubuntu:22.04 AS build
ARG DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
 && apt-get install -y --no-install-recommends build-essential ca-certificates curl pkg-config libssl-dev \
 && curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
 && apt-get install -y --no-install-recommends nodejs \
 && rm -rf /var/lib/apt/lists/*
# The GCC 13 libstdc++ (crates/qls-core/build.rs links it in statically; the runtime stage then needs nothing new).
COPY --from=toolchain /usr/lib/gcc/x86_64-linux-gnu/13/libstdc++.a /opt/gcc13/libstdc++.a
ENV KOKON_LIBSTDCXX_A=/opt/gcc13/libstdc++.a
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
ENV PATH=/root/.cargo/bin:$PATH
WORKDIR /src
COPY package.json package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY . .
# The UI first: kokon-service embeds it (crates/qls-service/src/remote.rs).
RUN npm run build && cargo build -p qls-service --release

FROM ubuntu:22.04
ARG DEBIAN_FRONTEND=noninteractive
# Runtime libraries of the bundled stack (the service and its UI; the inference engine lives on the gateway, not in this image).
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates libgomp1 libssl3 libvulkan1 \
 && rm -rf /var/lib/apt/lists/*
RUN useradd --create-home --uid 1000 kokon \
 && mkdir -p /data /config /gallery \
 && chown kokon:kokon /data /config /gallery
COPY --from=build /src/target/release/kokon-service /usr/local/bin/kokon-service
# Models, engine and gallery database in /data; settings in /config; saved images in /gallery.
ENV QLS_DATA_DIR=/data \
    QLS_CONFIG_DIR=/config \
    QLS_OUTPUT_DIR=/gallery \
    NVIDIA_VISIBLE_DEVICES=all \
    NVIDIA_DRIVER_CAPABILITIES=compute,utility \
    KOKON_CONTAINER=1
USER kokon
VOLUME ["/data", "/config", "/gallery"]
# The browser UI (password required). The token API on 39151 stays inside the container.
EXPOSE 39152
# --remote: open to other devices once a password is set
# (docker exec -it kokon kokon-service --set-password; applies without a restart).
CMD ["kokon-service", "--persistent", "--remote"]
