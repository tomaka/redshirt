# Note: because of path dependencies in the Cargo.toml, this Docker image needs to be built from the root of the repository

# TODO: could be alpine, but musl doesn't support proc macros https://github.com/rust-lang/rust/issues/40174
FROM rust:1-buster AS builder
LABEL maintainer "Pierre Krieger <pierre.krieger1708@gmail.com>"

COPY . /build
WORKDIR /build/modules/p2p-loader
RUN cargo build --bin passive-node --release --verbose


FROM debian:buster
LABEL maintainer "Pierre Krieger <pierre.krieger1708@gmail.com>"
COPY --from=builder /build/modules/target/release/passive-node /usr/local/bin

EXPOSE 30333
CMD ["/usr/local/bin/passive-node"]
