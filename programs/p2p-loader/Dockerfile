# Note: because of path dependencies in the Cargo.toml, this Docker image needs to be built from the root of the repository

FROM rust:1 AS builder
LABEL maintainer "Pierre Krieger <pierre.krieger1708@gmail.com>"

COPY . /build
WORKDIR /build/programs/p2p-loader
RUN apt-get update && apt-get install -y musl-tools
RUN rustup target add x86_64-unknown-linux-musl
RUN cargo build --target x86_64-unknown-linux-musl --bin passive-node --release --verbose --all-features


FROM alpine:latest
LABEL maintainer "Pierre Krieger <pierre.krieger1708@gmail.com>"
COPY --from=builder /build/programs/target/x86_64-unknown-linux-musl/release/passive-node /usr/local/bin

EXPOSE 30333
CMD ["/usr/local/bin/passive-node"]
