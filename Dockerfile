FROM rust:latest as builder
COPY . /hematita
WORKDIR /hematita
RUN cargo build

FROM debian:bullseye-slim
COPY --from=builder /hematita/target/debug/hematita_cli .