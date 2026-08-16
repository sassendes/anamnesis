FROM rust:1.88-bookworm AS builder
WORKDIR /src
COPY Cargo.toml build.rs ./
COPY migrations ./migrations
COPY src ./src
RUN cargo build --release --bin anamnesis

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /src/target/release/anamnesis /usr/local/bin/anamnesis
USER nonroot
EXPOSE 8080 9090
ENTRYPOINT ["/usr/local/bin/anamnesis"]
