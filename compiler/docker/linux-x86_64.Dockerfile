FROM ubuntu:22.04 AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        clang-14 \
        curl \
        liblld-14-dev \
        llvm-14-dev \
        zlib1g-dev \
        libzstd-dev \
        libtinfo-dev \
        libxml2-dev \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain 1.94.0

ENV PATH=/root/.cargo/bin:${PATH}
ENV CC=clang-14
ENV CXX=clang++-14
ENV RUSTFLAGS=-Clinker=clang-14
ENV LLVM_CONFIG=llvm-config-14

WORKDIR /workspace
COPY Cargo.toml Cargo.lock ./
COPY compiler/Cargo.toml compiler/build.rs compiler/
COPY compiler/native compiler/native
COPY compiler/src compiler/src

RUN cargo build --locked --release -p land-compiler --features embedded-backend \
    && mkdir /output \
    && cp target/release/landc /output/landc-linux-x86_64

FROM scratch AS artifact
COPY --from=builder /output/ /
