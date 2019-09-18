FROM alpine:edge as build
    RUN apk add --no-cache cargo clang-dev llvm8-dev
    WORKDIR /app
    COPY . .
    RUN cargo build --release

FROM alpine:edge
    WORKDIR /app
    ENTRYPOINT ["/app/pg-sse"]
    COPY --from=build /app/target/release/pg-sse .
