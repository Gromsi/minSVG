# Their image — they deploy it (ECS / Cloud Run / Lambda). We do not host this.
# rustc 1.83-safe MSRV; official image includes a C compiler for oxipng/libdeflater.
FROM rust:1.83-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY LICENSE README.md ./
RUN cargo install --path . --locked --features serve --root /out

FROM debian:bookworm-slim
COPY --from=build /out/bin/minsvg /usr/local/bin/minsvg
EXPOSE 8080
USER nobody
CMD ["minsvg", "serve", "--bind", "0.0.0.0:8080"]
