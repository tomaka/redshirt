# Note: because of path dependencies in the Cargo.toml, this Docker image needs to be built from the root of the repository

FROM rust:alpine AS builder
LABEL maintainer "Pierre Krieger <pierre.krieger1708@gmail.com>"

COPY . /build
WORKDIR /build/modules/p2p-loader
RUN apk update
RUN apk add libgit2-dev openssl-dev
RUN rustup default nightly  # TODO: proc macros on musl has only been implemented recently
RUN cargo build --bin passive-node --release --verbose --all-features


FROM alpine:latest
LABEL maintainer "Pierre Krieger <pierre.krieger1708@gmail.com>"
COPY --from=builder /build/modules/target/release/passive-node /usr/local/bin

EXPOSE 30333
CMD ["/usr/local/bin/passive-node"]
